//! The on-disk library the app works with: the shipped entries as read-only
//! built-ins, plus whatever the user installs from a material package. Packages are validated
//! before anything is written, conflicts with installed entries are put to the user, and the
//! snapshot the compiler resolves against is rebuilt from what is on disk.

use std::path::Path;

use ps_library::{
    Choice, Entry, Library, LibraryKind, LibrarySnapshot, Outcome, StoreError, install_builtin,
    read_package,
};
use semver::Version;

/// One line of a library listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryRow {
    /// The entry id.
    pub id: String,
    /// The name shown to the user.
    pub name: String,
    /// The installed version.
    pub version: Version,
    /// True for entries that ship with the app and cannot be removed.
    pub builtin: bool,
    /// True when the user has changes on top.
    pub edited: bool,
}

/// What installing a package did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallReport {
    /// The ids now installed (a kept copy has its new id).
    pub installed: Vec<String>,
    /// Ids that are already installed and need the user to choose replace, keep both or skip.
    pub needs_answer: Vec<String>,
    /// Everything that went wrong, one message each.
    pub problems: Vec<String>,
}

/// The library on disk.
#[derive(Debug, Clone)]
pub struct LibraryManager {
    library: Library,
}

fn message(e: &StoreError) -> String {
    e.to_string()
}

impl LibraryManager {
    /// Opens (creating if needed) the library under `root` and makes sure the built-in entries are
    /// there.
    ///
    /// # Errors
    ///
    /// A message when the folders cannot be created or the shipped data is broken.
    pub fn open(root: &Path) -> Result<Self, String> {
        let mut library = Library::open(root).map_err(|e| message(&e))?;
        install_builtin(&mut library).map_err(|e| message(&e))?;
        Ok(Self { library })
    }

    /// The entries of one kind, sorted by name.
    #[must_use]
    pub fn rows(&self, kind: LibraryKind) -> Vec<LibraryRow> {
        let mut rows: Vec<LibraryRow> = self
            .library
            .list(kind)
            .unwrap_or_default()
            .into_iter()
            .map(|s| LibraryRow {
                id: s.id,
                name: s.name,
                version: s.version,
                builtin: s.builtin,
                edited: s.has_overlay,
            })
            .collect();
        rows.sort_by_key(|r| (r.name.to_lowercase(), r.id.clone()));
        rows
    }

    /// The effective library, for compiling against.
    ///
    /// # Errors
    ///
    /// A message when an entry cannot be read or no longer validates.
    pub fn snapshot(&self) -> Result<LibrarySnapshot, String> {
        self.library.snapshot().map_err(|e| message(&e))
    }

    /// Installs the materials in a package. `choice` answers an earlier "already installed"
    /// question; `allow_partial` installs the valid materials of a package that has invalid ones.
    pub fn install_package(
        &mut self,
        bytes: &[u8],
        choice: Option<Choice>,
        allow_partial: bool,
    ) -> InstallReport {
        let mut report = InstallReport::default();
        let imported = match read_package(bytes) {
            Ok(r) => r,
            Err(e) => {
                report.problems.push(e.to_string());
                return report;
            }
        };
        for (file, issue) in &imported.issues {
            report.problems.push(format!("{file}: {issue:?}"));
        }
        for item in imported.importable(allow_partial) {
            let entry = Entry::Material(item.material.clone());
            match self.library.install(&entry, choice) {
                Ok(Outcome::Installed | Outcome::Updated | Outcome::Replaced) => {
                    report.installed.push(item.material.id.clone());
                }
                Ok(Outcome::InstalledCopy(id)) => report.installed.push(id),
                Ok(Outcome::Skipped) => {}
                Err(StoreError::NeedsAnswer) => report.needs_answer.push(item.material.id.clone()),
                Err(e) => report.problems.push(format!("{}: {e}", item.material.id)),
            }
        }
        report
    }

    /// Removes an installed entry.
    ///
    /// # Errors
    ///
    /// A message when the entry is built in or the files cannot be removed.
    pub fn uninstall(&mut self, kind: LibraryKind, id: &str) -> Result<(), String> {
        self.library.uninstall(kind, id).map_err(|e| message(&e))
    }
}

#[cfg(test)]
mod tests;
