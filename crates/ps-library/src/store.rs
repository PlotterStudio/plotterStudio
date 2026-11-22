//! The library on disk.
//!
//! ```text
//! <root>/library/builtin/<kind>/<id>.json          read-only snapshot shipped with the app
//! <root>/library/installed/<kind>/<id>/<ver>.json  packages the user imported
//! <root>/library/overlays/<kind>/<id>.json         user changes (merge patch + history)
//! <root>/library/rules/                            colour/layer mapping rule sets
//! <root>/library/calibration/<machine>.json        per-machine calibration
//! <root>/cache/textures/                           GPU-ready mip chains
//! ```
//!
//! An effective entry is a builtin or installed package with its overlay applied. Every write is
//! atomic: a temporary file in the same folder is renamed over the target.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use semver::Version;
use serde_json::Value;

use crate::LibraryKind;
use crate::calibration::{Calibration, file_name};
use crate::conflict::{Action, Choice, action, keep_both_id, resolve};
use crate::overlay::Overlay;
use crate::schema::{Entry, Issue};

/// What an install did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A new entry was installed.
    Installed,
    /// A newer version replaced the old one; the overlay was kept.
    Updated,
    /// The user chose to replace an equal or older version.
    Replaced,
    /// The entry was installed as a copy under this new id.
    InstalledCopy(String),
    /// Nothing was changed.
    Skipped,
}

/// Why a library operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// The same or an older version is installed and the user has not said what to do.
    #[error(
        "an entry with this id and version is already installed; choose replace, keep both or skip"
    )]
    NeedsAnswer,
    /// Built-in entries cannot be removed or changed.
    #[error("built-in entries are read-only")]
    ReadOnly,
    /// The id would not be safe to use in a file name.
    #[error("invalid library id {0:?}")]
    BadId(String),
    /// The entry or change is not valid.
    #[error("invalid library data")]
    Invalid(Vec<Issue>),
    /// A file operation failed.
    #[error("library file error: {0}")]
    Io(String),
}

fn io(e: impl std::fmt::Display) -> StoreError {
    StoreError::Io(e.to_string())
}

/// One row of a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Entry id.
    pub id: String,
    /// Package version.
    pub version: Version,
    /// Display name.
    pub name: String,
    /// True when the package is built in.
    pub builtin: bool,
    /// True when the user has changes on top.
    pub has_overlay: bool,
}

/// A library rooted in a data directory.
#[derive(Debug, Clone)]
pub struct Library {
    root: PathBuf,
}

fn check_id(id: &str) -> Result<(), StoreError> {
    let ok = !id.is_empty()
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if ok {
        Ok(())
    } else {
        Err(StoreError::BadId(id.to_owned()))
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), StoreError> {
    let dir = path.parent().ok_or_else(|| io("path has no parent"))?;
    fs::create_dir_all(dir).map_err(io)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(io)?;
    tmp.write_all(data).map_err(io)?;
    tmp.as_file().sync_all().map_err(io)?;
    tmp.persist(path).map_err(|e| io(e.error))?;
    Ok(())
}

fn entry_value(entry: &Entry) -> Value {
    let v = match entry {
        Entry::Tool(t) => serde_json::to_value(t),
        Entry::Material(m) => serde_json::to_value(m),
        Entry::Profile(p) => serde_json::to_value(p),
    };
    v.unwrap_or(Value::Null)
}

/// Where a package came from.
struct Package {
    value: Value,
    version: Version,
    builtin: bool,
}

impl Library {
    /// Opens (creating if needed) a library under `root`.
    ///
    /// # Errors
    /// [`StoreError::Io`] if the folders cannot be created.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        for sub in [
            "library/builtin",
            "library/installed",
            "library/overlays",
            "library/rules",
            "library/calibration",
            "cache/textures",
        ] {
            fs::create_dir_all(root.join(sub)).map_err(io)?;
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    fn lib(&self, part: &str, kind: LibraryKind) -> PathBuf {
        self.root.join("library").join(part).join(kind.folder())
    }

    fn builtin_path(&self, kind: LibraryKind, id: &str) -> PathBuf {
        self.lib("builtin", kind).join(format!("{id}.json"))
    }

    fn installed_dir(&self, kind: LibraryKind, id: &str) -> PathBuf {
        self.lib("installed", kind).join(id)
    }

    fn overlay_path(&self, kind: LibraryKind, id: &str) -> PathBuf {
        self.lib("overlays", kind).join(format!("{id}.json"))
    }

    fn installed_versions(&self, kind: LibraryKind, id: &str) -> Vec<Version> {
        let Ok(rd) = fs::read_dir(self.installed_dir(kind, id)) else {
            return Vec::new();
        };
        let mut out: Vec<Version> = rd
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".json")?.parse().ok()
            })
            .collect();
        out.sort();
        out
    }

    fn read_value(path: &Path) -> Result<Value, StoreError> {
        serde_json::from_slice(&fs::read(path).map_err(io)?).map_err(io)
    }

    /// The best package for an id: the newest version, an installed one winning a tie.
    fn package_of(&self, kind: LibraryKind, id: &str) -> Result<Option<Package>, StoreError> {
        check_id(id)?;
        let mut best: Option<Package> = None;
        let builtin = self.builtin_path(kind, id);
        if builtin.is_file() {
            let value = Self::read_value(&builtin)?;
            let version = value
                .get("version")
                .and_then(Value::as_str)
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| io("builtin entry has no valid version"))?;
            best = Some(Package {
                value,
                version,
                builtin: true,
            });
        }
        if let Some(newest) = self.installed_versions(kind, id).pop() {
            let wins = best.as_ref().is_none_or(|b| newest >= b.version);
            if wins {
                let path = self.installed_dir(kind, id).join(format!("{newest}.json"));
                best = Some(Package {
                    value: Self::read_value(&path)?,
                    version: newest,
                    builtin: false,
                });
            }
        }
        Ok(best)
    }

    /// Adds a built-in entry (used when the app prepares its shipped snapshot).
    ///
    /// # Errors
    /// [`StoreError`] for unsafe ids or I/O errors.
    pub fn add_builtin(&mut self, entry: &Entry) -> Result<(), StoreError> {
        check_id(entry.id())?;
        let text = serde_json::to_vec_pretty(&entry_value(entry)).map_err(io)?;
        write_atomic(&self.builtin_path(entry.kind(), entry.id()), &text)
    }

    /// Installs an entry, applying the conflict rules.
    ///
    /// # Errors
    /// [`StoreError::NeedsAnswer`] when an equal or older version is installed and `choice` is
    /// `None`; other [`StoreError`]s for unsafe ids or I/O errors.
    pub fn install(
        &mut self,
        entry: &Entry,
        choice: Option<Choice>,
    ) -> Result<Outcome, StoreError> {
        let (kind, id) = (entry.kind(), entry.id().to_owned());
        check_id(&id)?;
        let existing = self.installed_versions(kind, &id).pop();
        let act = action(resolve(existing.as_ref(), entry.version()), choice);
        let write = |lib: &Self, id: &str, value: &Value, version: &Version| {
            let path = lib.installed_dir(kind, id).join(format!("{version}.json"));
            write_atomic(&path, &serde_json::to_vec_pretty(value).map_err(io)?)
        };
        let value = entry_value(entry);
        match act {
            Action::NeedsAnswer => Err(StoreError::NeedsAnswer),
            Action::Skip => Ok(Outcome::Skipped),
            Action::Install => {
                write(self, &id, &value, entry.version())?;
                Ok(Outcome::Installed)
            }
            Action::UpdateKeepingOverlay | Action::ReplaceKeepingOverlay => {
                let replaced = act == Action::ReplaceKeepingOverlay;
                let old = self.installed_versions(kind, &id);
                write(self, &id, &value, entry.version())?;
                for v in old.into_iter().filter(|v| v != entry.version()) {
                    let _ =
                        fs::remove_file(self.installed_dir(kind, &id).join(format!("{v}.json")));
                }
                Ok(if replaced {
                    Outcome::Replaced
                } else {
                    Outcome::Updated
                })
            }
            Action::InstallCopy => {
                let taken: std::collections::BTreeSet<String> =
                    self.list(kind)?.into_iter().map(|s| s.id).collect();
                let new_id = keep_both_id(&id, &taken);
                let mut copy = value;
                if let Some(obj) = copy.as_object_mut() {
                    obj.insert("id".to_owned(), Value::from(new_id.clone()));
                }
                write(self, &new_id, &copy, entry.version())?;
                Ok(Outcome::InstalledCopy(new_id))
            }
        }
    }

    /// Removes all installed versions of an entry (its overlay is kept).
    ///
    /// # Errors
    /// [`StoreError::ReadOnly`] for built-in entries.
    pub fn uninstall(&mut self, kind: LibraryKind, id: &str) -> Result<(), StoreError> {
        check_id(id)?;
        let dir = self.installed_dir(kind, id);
        if dir.is_dir() {
            return fs::remove_dir_all(dir).map_err(io);
        }
        if self.builtin_path(kind, id).is_file() {
            return Err(StoreError::ReadOnly);
        }
        Ok(())
    }

    /// The package as shipped or imported, without the overlay.
    ///
    /// # Errors
    /// [`StoreError`] for unsafe ids, I/O errors or an invalid stored file.
    pub fn package(&self, kind: LibraryKind, id: &str) -> Result<Option<Entry>, StoreError> {
        self.package_of(kind, id)?
            .map(|p| Entry::from_json(kind, &p.value.to_string()).map_err(StoreError::Invalid))
            .transpose()
    }

    fn overlay(&self, kind: LibraryKind, id: &str) -> Result<Overlay, StoreError> {
        let path = self.overlay_path(kind, id);
        if path.is_file() {
            serde_json::from_slice(&fs::read(path).map_err(io)?).map_err(io)
        } else {
            Ok(Overlay::new())
        }
    }

    fn save_overlay(&self, kind: LibraryKind, id: &str, o: &Overlay) -> Result<(), StoreError> {
        write_atomic(
            &self.overlay_path(kind, id),
            &serde_json::to_vec_pretty(o).map_err(io)?,
        )
    }

    /// The package with the user's overlay applied.
    ///
    /// # Errors
    /// [`StoreError`] for unsafe ids, I/O errors, or an overlay that no longer validates.
    pub fn effective(&self, kind: LibraryKind, id: &str) -> Result<Option<Entry>, StoreError> {
        let Some(pkg) = self.package_of(kind, id)? else {
            return Ok(None);
        };
        let merged = self.overlay(kind, id)?.effective(&pkg.value);
        Entry::from_json(kind, &merged.to_string())
            .map(Some)
            .map_err(StoreError::Invalid)
    }

    /// Records that the entry now reads `edited`, as an overlay over its package.
    ///
    /// # Errors
    /// [`StoreError::Invalid`] if the edited entry does not validate (nothing is saved).
    pub fn edit(
        &mut self,
        kind: LibraryKind,
        id: &str,
        edited: &Value,
        when: &str,
        who: &str,
    ) -> Result<(), StoreError> {
        let pkg = self
            .package_of(kind, id)?
            .ok_or_else(|| StoreError::BadId(id.to_owned()))?;
        Entry::from_json(kind, &edited.to_string()).map_err(StoreError::Invalid)?;
        let mut overlay = self.overlay(kind, id)?;
        overlay.edit(&pkg.value, edited, when, who);
        self.save_overlay(kind, id, &overlay)
    }

    /// Drops all of the user's changes to an entry.
    ///
    /// # Errors
    /// [`StoreError`] for I/O errors.
    pub fn revert(
        &mut self,
        kind: LibraryKind,
        id: &str,
        when: &str,
        who: &str,
    ) -> Result<(), StoreError> {
        check_id(id)?;
        let mut overlay = self.overlay(kind, id)?;
        overlay.revert(when, who);
        self.save_overlay(kind, id, &overlay)
    }

    /// Undoes the last change to an entry's overlay; false when there is nothing to undo.
    ///
    /// # Errors
    /// [`StoreError`] for I/O errors.
    pub fn undo(&mut self, kind: LibraryKind, id: &str) -> Result<bool, StoreError> {
        check_id(id)?;
        let mut overlay = self.overlay(kind, id)?;
        let undone = overlay.undo();
        if undone {
            self.save_overlay(kind, id, &overlay)?;
        }
        Ok(undone)
    }

    /// True when the user has changes on top of an entry.
    #[must_use]
    pub fn has_overlay(&self, kind: LibraryKind, id: &str) -> bool {
        self.overlay(kind, id).is_ok_and(|o| !o.is_empty())
    }

    /// Lists every entry of a kind, sorted by id.
    ///
    /// # Errors
    /// [`StoreError`] for I/O errors.
    pub fn list(&self, kind: LibraryKind) -> Result<Vec<Summary>, StoreError> {
        let mut ids = std::collections::BTreeSet::new();
        if let Ok(rd) = fs::read_dir(self.lib("builtin", kind)) {
            for e in rd.filter_map(Result::ok) {
                if let Some(id) = e.file_name().to_string_lossy().strip_suffix(".json") {
                    ids.insert(id.to_owned());
                }
            }
        }
        if let Ok(rd) = fs::read_dir(self.lib("installed", kind)) {
            for e in rd.filter_map(Result::ok) {
                ids.insert(e.file_name().to_string_lossy().into_owned());
            }
        }
        let mut out = Vec::new();
        for id in ids {
            if let Some(pkg) = self.package_of(kind, &id)? {
                let name = pkg
                    .value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_owned();
                out.push(Summary {
                    has_overlay: self.has_overlay(kind, &id),
                    id,
                    version: pkg.version,
                    name,
                    builtin: pkg.builtin,
                });
            }
        }
        Ok(out)
    }

    /// The calibration of a machine (the defaults when never calibrated).
    ///
    /// # Errors
    /// [`StoreError`] for unsafe ids or unreadable files.
    pub fn calibration(&self, machine_id: &str) -> Result<Calibration, StoreError> {
        check_id(machine_id)?;
        let path = self
            .root
            .join("library/calibration")
            .join(file_name(machine_id));
        if path.is_file() {
            serde_json::from_slice(&fs::read(path).map_err(io)?).map_err(io)
        } else {
            Ok(Calibration::default())
        }
    }

    /// Saves a machine's calibration after checking it.
    ///
    /// # Errors
    /// [`StoreError::Invalid`] for nonsense values.
    pub fn save_calibration(&self, machine_id: &str, c: &Calibration) -> Result<(), StoreError> {
        check_id(machine_id)?;
        let issues = c.validate();
        if !issues.is_empty() {
            return Err(StoreError::Invalid(issues));
        }
        let path = self
            .root
            .join("library/calibration")
            .join(file_name(machine_id));
        write_atomic(&path, &serde_json::to_vec_pretty(c).map_err(io)?)
    }

    /// The folder for derived caches (textures).
    #[must_use]
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// The folder for colour/layer mapping rule sets.
    #[must_use]
    pub fn rules_dir(&self) -> PathBuf {
        self.root.join("library/rules")
    }
}

#[cfg(test)]
mod tests;
