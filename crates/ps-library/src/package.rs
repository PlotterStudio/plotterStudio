//! Material packages: a `.zip` with `library.json` and one folder per material
//! holding `material.json` and its textures.
//!
//! Reading goes through the safe zip reader (path rules, size and ratio limits, name allowlist).
//! Every material is validated and every problem is collected, grouped by file, so the import
//! dialog can show them all at once. By default nothing is imported if anything is invalid.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;

use ps_core::{ZipError, ZipLimits, read_archive};

use crate::schema::{Entry, Issue, group_by_file};
use crate::texture::{MAX_DIMENSION, TextureError, decode};
use crate::{LibraryKind, Material};

/// Why a package was refused outright (as opposed to individual invalid materials).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackageError {
    /// The zip was refused (unsafe path, bomb, name outside the allowlist, bad archive).
    #[error(transparent)]
    Zip(#[from] ZipError),
    /// There is no `library.json`.
    #[error("package has no library.json")]
    MissingLibrary,
    /// `library.json` could not be read at all.
    #[error("library.json is unreadable: {0}")]
    BadLibrary(String),
    /// The package holds no materials.
    #[error("package contains no materials")]
    NoMaterials,
    /// The package declares no licence.
    #[error("package declares no licence")]
    NoLicense,
    /// The archive could not be written.
    #[error("cannot write package: {0}")]
    Write(String),
}

/// The contents of `library.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryInfo {
    /// Layout version.
    pub schema_version: u32,
    /// Package id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Package version.
    pub version: String,
    /// Licence of the package; packages that declare none are refused.
    #[serde(default)]
    pub license: String,
}

/// One material found in a package.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedMaterial {
    /// The folder it lives in.
    pub folder: String,
    /// The validated material.
    pub material: Material,
    /// Its texture files by name.
    pub textures: BTreeMap<String, Vec<u8>>,
}

/// Everything learned about a package.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportReport {
    /// The package description.
    pub library: LibraryInfo,
    /// Materials that validated, in folder order.
    pub materials: Vec<ImportedMaterial>,
    /// Every problem, with the file it belongs to.
    pub issues: Vec<(String, Issue)>,
}

impl ImportReport {
    /// True when nothing is wrong with the package.
    #[must_use]
    pub fn all_valid(&self) -> bool {
        self.issues.is_empty()
    }

    /// The problems grouped by file.
    #[must_use]
    pub fn issues_by_file(&self) -> BTreeMap<String, Vec<Issue>> {
        group_by_file(self.issues.clone())
    }

    /// The materials to import: all of them when the package is fully valid; otherwise none, or
    /// the valid ones when `allow_partial` is set ("import the valid ones").
    #[must_use]
    pub fn importable(&self, allow_partial: bool) -> Vec<&ImportedMaterial> {
        if self.all_valid() || allow_partial {
            self.materials.iter().collect()
        } else {
            Vec::new()
        }
    }
}

fn is_image(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".webp"]
        .iter()
        .any(|e| lower.ends_with(e))
}

/// The names a material package may contain.
fn allowed(name: &str) -> bool {
    if name == "library.json" {
        return true;
    }
    match name.split('/').collect::<Vec<_>>().as_slice() {
        [_, "material.json"] => true,
        [_, file] => is_image(file),
        _ => false,
    }
}

/// Reads and validates a package.
///
/// # Errors
/// [`PackageError`] when the package as a whole is unusable; problems with individual materials
/// are returned in the [`ImportReport`] instead.
pub fn read_package(bytes: &[u8]) -> Result<ImportReport, PackageError> {
    let files = read_archive(bytes, &ZipLimits::default(), &allowed)?;
    let lib_bytes = files
        .get("library.json")
        .ok_or(PackageError::MissingLibrary)?;
    let library: LibraryInfo =
        serde_json::from_slice(lib_bytes).map_err(|e| PackageError::BadLibrary(e.to_string()))?;

    let mut issues: Vec<(String, Issue)> = Vec::new();
    if library.license.trim().is_empty() {
        issues.push((
            "library.json".to_owned(),
            Issue::new("/license", "the package declares no licence"),
        ));
    }

    let folders: BTreeSet<&str> = files
        .keys()
        .filter_map(|n| n.strip_suffix("/material.json"))
        .collect();
    if folders.is_empty() {
        return Err(PackageError::NoMaterials);
    }

    let mut materials = Vec::new();
    for folder in folders {
        let path = format!("{folder}/material.json");
        let prefix = format!("{folder}/");
        let textures: BTreeMap<String, Vec<u8>> = files
            .iter()
            .filter_map(|(n, d)| Some((n.strip_prefix(&prefix)?.to_owned(), d.clone())))
            .filter(|(n, _)| is_image(n))
            .collect();
        let present: BTreeSet<String> = textures.keys().cloned().collect();
        let text = String::from_utf8_lossy(&files[&path]).into_owned();
        match Entry::from_json_with_files(LibraryKind::Material, &text, &present) {
            Ok(Entry::Material(material)) => {
                let before = issues.len();
                for (name, data) in &textures {
                    if let Err(e) = decode(data) {
                        let message = match e {
                            TextureError::TooLarge { .. } => {
                                format!("{e} (limit is {MAX_DIMENSION} x {MAX_DIMENSION})")
                            }
                            other => other.to_string(),
                        };
                        issues.push((format!("{folder}/{name}"), Issue::new("", message)));
                    }
                }
                if issues.len() == before {
                    materials.push(ImportedMaterial {
                        folder: folder.to_owned(),
                        material,
                        textures,
                    });
                }
            }
            Ok(_) => {}
            Err(found) => issues.extend(found.into_iter().map(|i| (path.clone(), i))),
        }
    }
    Ok(ImportReport {
        library,
        materials,
        issues,
    })
}

/// Writes a package from effective materials (package values plus overlay) and their textures.
///
/// The archive is deterministic: entries are sorted and carry no timestamps.
///
/// # Errors
/// [`PackageError::NoLicense`] when no licence is declared; [`PackageError::Write`] on I/O errors.
pub fn export_package(
    info: &LibraryInfo,
    materials: &[(Material, BTreeMap<String, Vec<u8>>)],
) -> Result<Vec<u8>, PackageError> {
    if info.license.trim().is_empty() {
        return Err(PackageError::NoLicense);
    }
    let werr = |e: &dyn std::fmt::Display| PackageError::Write(e.to_string());
    let opts = SimpleFileOptions::default().last_modified_time(zip::DateTime::default());
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut put = |name: &str, data: &[u8]| -> Result<(), PackageError> {
        w.start_file(name, opts).map_err(|e| werr(&e))?;
        w.write_all(data).map_err(|e| werr(&e))
    };
    put(
        "library.json",
        &serde_json::to_vec_pretty(info).map_err(|e| werr(&e))?,
    )?;
    let mut sorted: Vec<&(Material, BTreeMap<String, Vec<u8>>)> = materials.iter().collect();
    sorted.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    for (material, textures) in sorted {
        let folder = &material.id;
        put(
            &format!("{folder}/material.json"),
            &serde_json::to_vec_pretty(material).map_err(|e| werr(&e))?,
        )?;
        for (name, data) in textures {
            put(&format!("{folder}/{name}"), data)?;
        }
    }
    Ok(w.finish().map_err(|e| werr(&e))?.into_inner())
}

#[cfg(test)]
mod tests;
