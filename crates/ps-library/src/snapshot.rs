//! A read-only, cheaply shared view of the effective library.
//!
//! The compiler and the UI resolve references against a [`LibrarySnapshot`]. A reference to a
//! version that is not installed resolves to the newest installed version, with a warning unless
//! that version is a newer, compatible one; a reference to an id that is not installed is an error
//! that blocks export.

use std::collections::BTreeMap;
use std::sync::Arc;

use ps_core::LibRef;
use semver::Version;

use crate::schema::Entry;
use crate::store::{Library, StoreError};
use crate::{LibraryKind, MachineProfile, Material, Tool};

/// Something worth telling the user about a resolved reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveWarning {
    /// The wanted version is not installed; another version of the same id is used.
    VersionSubstituted {
        /// The reference in the project.
        wanted: LibRef,
        /// The reference actually used.
        used: LibRef,
    },
}

/// A reference that cannot be resolved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    /// No version of this id is installed; export is blocked.
    #[error("library entry {0} is not installed")]
    Missing(LibRef),
}

/// A resolved entry and any warning.
#[derive(Debug, Clone)]
pub struct Resolved<T> {
    /// The entry.
    pub item: Arc<T>,
    /// A warning, if the exact version was not available.
    pub warning: Option<ResolveWarning>,
}

type Versions<T> = BTreeMap<String, BTreeMap<Version, Arc<T>>>;

/// The effective tools, materials and profiles at one moment.
#[derive(Debug, Clone, Default)]
pub struct LibrarySnapshot {
    tools: Versions<Tool>,
    materials: Versions<Material>,
    profiles: Versions<MachineProfile>,
}

fn resolve_in<T>(
    map: &Versions<T>,
    wanted: &LibRef,
    lib_ref_of: impl Fn(&T) -> LibRef,
) -> Result<Resolved<T>, ResolveError> {
    let versions = map
        .get(wanted.id())
        .ok_or_else(|| ResolveError::Missing(wanted.clone()))?;
    if let Some(exact) = versions.get(wanted.version()) {
        return Ok(Resolved {
            item: Arc::clone(exact),
            warning: None,
        });
    }
    let (version, newest) = versions
        .iter()
        .next_back()
        .ok_or_else(|| ResolveError::Missing(wanted.clone()))?;
    // A newer version that is compatible stands in quietly: a library update should not put a
    // warning on every project made before it.
    let warning = if is_compatible_update(wanted.version(), version) {
        None
    } else {
        Some(ResolveWarning::VersionSubstituted {
            wanted: wanted.clone(),
            used: lib_ref_of(newest),
        })
    };
    Ok(Resolved {
        item: Arc::clone(newest),
        warning,
    })
}

/// Whether `used` is a newer version that should work wherever `wanted` did: the same major
/// version, or for a 0.x entry the same minor version, since 0.x makes no promise across minors.
fn is_compatible_update(wanted: &Version, used: &Version) -> bool {
    if used <= wanted || used.major != wanted.major {
        return false;
    }
    wanted.major != 0 || used.minor == wanted.minor
}

fn newest<T>(map: &Versions<T>) -> Vec<Arc<T>> {
    map.values()
        .filter_map(|v| v.values().next_back().cloned())
        .collect()
}

impl LibrarySnapshot {
    /// Builds a snapshot from entries (all versions given are kept).
    #[must_use]
    pub fn from_entries(entries: impl IntoIterator<Item = Entry>) -> Self {
        let mut s = Self::default();
        for entry in entries {
            match entry {
                Entry::Tool(t) => {
                    s.tools
                        .entry(t.id.clone())
                        .or_default()
                        .insert(t.version.clone(), Arc::new(t));
                }
                Entry::Material(m) => {
                    s.materials
                        .entry(m.id.clone())
                        .or_default()
                        .insert(m.version.clone(), Arc::new(m));
                }
                Entry::Profile(p) => {
                    s.profiles
                        .entry(p.id.clone())
                        .or_default()
                        .insert(p.version.clone(), Arc::new(p));
                }
            }
        }
        s
    }

    /// Resolves a tool reference.
    ///
    /// # Errors
    /// [`ResolveError::Missing`] when no version of the id is installed.
    pub fn resolve_tool(&self, r: &LibRef) -> Result<Resolved<Tool>, ResolveError> {
        resolve_in(&self.tools, r, Tool::lib_ref)
    }

    /// Resolves a material reference.
    ///
    /// # Errors
    /// [`ResolveError::Missing`] when no version of the id is installed.
    pub fn resolve_material(&self, r: &LibRef) -> Result<Resolved<Material>, ResolveError> {
        resolve_in(&self.materials, r, Material::lib_ref)
    }

    /// Resolves a machine profile reference.
    ///
    /// # Errors
    /// [`ResolveError::Missing`] when no version of the id is installed.
    pub fn resolve_profile(&self, r: &LibRef) -> Result<Resolved<MachineProfile>, ResolveError> {
        resolve_in(&self.profiles, r, MachineProfile::lib_ref)
    }

    /// The newest version of every tool, by id.
    #[must_use]
    pub fn tools(&self) -> Vec<Arc<Tool>> {
        newest(&self.tools)
    }

    /// The newest version of every material, by id.
    #[must_use]
    pub fn materials(&self) -> Vec<Arc<Material>> {
        newest(&self.materials)
    }

    /// The newest version of every profile, by id.
    #[must_use]
    pub fn profiles(&self) -> Vec<Arc<MachineProfile>> {
        newest(&self.profiles)
    }

    /// Pens the user marked as owned.
    #[must_use]
    pub fn owned_pens(&self) -> Vec<Arc<Tool>> {
        self.tools()
            .into_iter()
            .filter(|t| t.pen.as_ref().is_some_and(|p| p.owned))
            .collect()
    }

    /// Ids of entries whose id, name, brand, category or translated names contain the query
    /// (case-insensitive). A blank query matches everything.
    #[must_use]
    pub fn search(&self, kind: LibraryKind, query: &str) -> Vec<String> {
        let q = query.trim().to_lowercase();
        let hit =
            |fields: &[&str]| q.is_empty() || fields.iter().any(|f| f.to_lowercase().contains(&q));
        match kind {
            LibraryKind::Tool => self
                .tools()
                .iter()
                .filter(|t| hit(&[&t.id, &t.name]))
                .map(|t| t.id.clone())
                .collect(),
            LibraryKind::Material => self
                .materials()
                .iter()
                .filter(|m| {
                    let mut fields: Vec<&str> = vec![&m.id, &m.name, &m.category];
                    fields.extend(m.brand.as_deref());
                    fields.extend(m.names.values().map(String::as_str));
                    hit(&fields)
                })
                .map(|m| m.id.clone())
                .collect(),
            LibraryKind::Profile => self
                .profiles()
                .iter()
                .filter(|p| hit(&[&p.id, &p.name]))
                .map(|p| p.id.clone())
                .collect(),
        }
    }
}

impl Library {
    /// Builds a snapshot of the effective library (overlays applied).
    ///
    /// # Errors
    /// [`StoreError`] when an entry cannot be read or no longer validates.
    pub fn snapshot(&self) -> Result<LibrarySnapshot, StoreError> {
        let mut entries = Vec::new();
        for kind in LibraryKind::ALL {
            for summary in self.list(kind)? {
                if let Some(e) = self.effective(kind, &summary.id)? {
                    entries.push(e);
                }
            }
        }
        Ok(LibrarySnapshot::from_entries(entries))
    }
}

#[cfg(test)]
mod tests;
