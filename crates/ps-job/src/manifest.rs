//! The `.psjob` manifest: job identity, per-mat tool tables and estimates, and the
//! sha256 of every other file. A published JSON Schema describes it ([`schema`]).

use std::collections::BTreeMap;
use std::fmt::Write;

use ps_core::LibRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::ir::{Estimate, ToolEntry};

/// The manifest layout this build writes and reads.
pub const SCHEMA_VERSION: u32 = 1;

/// A pause the user will see, with the G-code line that causes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PauseRow {
    /// 1-based line number in the mat's G-code file.
    pub line: u32,
    /// The message.
    pub text: String,
}

/// One mat of the job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MatEntry {
    /// 1-based position in the run order.
    pub index: u32,
    /// The mat's name.
    pub name: String,
    /// Path of its G-code file in the bundle.
    pub gcode: String,
    /// Path of its toolpath preview, if any.
    pub preview: Option<String>,
    /// The material used.
    #[schemars(with = "Option<String>")]
    pub material: Option<LibRef>,
    /// Tools by slot.
    pub tools: Vec<ToolEntry>,
    /// Pauses in the program.
    pub pauses: Vec<PauseRow>,
    /// Estimates.
    pub estimate: Estimate,
}

/// The manifest of a `.psjob` bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Manifest {
    /// Layout version of this manifest.
    pub schema_version: u32,
    /// Unique id of the job.
    pub job_id: String,
    /// The app version that wrote it.
    pub app_version: String,
    /// When it was exported (RFC 3339). The only non-deterministic field.
    pub export_time: String,
    /// The G-code dialect, such as `ps-gcode/1`.
    pub dialect: String,
    /// The machine profile used.
    #[schemars(with = "String")]
    pub profile: LibRef,
    /// The mats, in run order.
    pub mats: Vec<MatEntry>,
    /// sha256 (lowercase hex) of every other file in the bundle, by path.
    pub files: BTreeMap<String, String>,
}

/// Why a manifest was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// The manifest was written by a newer app.
    #[error(
        "job file schema {found} is newer than this app supports ({supported}); update the app"
    )]
    TooNew {
        /// The version in the file.
        found: u32,
        /// The newest version this build reads.
        supported: u32,
    },
    /// The job id is empty.
    #[error("manifest has no job id")]
    MissingJobId,
    /// There are no mats.
    #[error("manifest lists no mats")]
    NoMats,
    /// Mats are not numbered 1, 2, 3 … in order.
    #[error("mat {found} appears where mat {expected} is expected")]
    MatOrder {
        /// The index expected at this position.
        expected: u32,
        /// The index found.
        found: u32,
    },
    /// A needed file has no hash entry.
    #[error("manifest has no entry for {0}")]
    MissingFileEntry(String),
    /// A hash is not 64 lowercase hexadecimal digits.
    #[error("bad sha256 for {0}")]
    BadHash(String),
    /// A listed file is not part of the bundle layout.
    #[error("file {0} is not part of the job layout")]
    UnexpectedFile(String),
}

/// Returns the lowercase hex sha256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        // Writing to a String cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// The G-code path of mat `index` (1-based, at least two digits).
#[must_use]
pub fn gcode_path(index: u32) -> String {
    format!("mats/{index:02}.gcode")
}

/// The preview path of mat `index`.
#[must_use]
pub fn preview_path(index: u32) -> String {
    format!("previews/{index:02}.svg")
}

fn numbered(name: &str, dir: &str, ext: &str) -> bool {
    name.strip_prefix(dir)
        .and_then(|rest| rest.strip_suffix(ext))
        .is_some_and(|digits| digits.len() >= 2 && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// True for the file names a bundle may contain.
#[must_use]
pub fn is_bundle_file(name: &str) -> bool {
    matches!(name, "manifest.json" | "profile.json")
        || numbered(name, "mats/", ".gcode")
        || numbered(name, "previews/", ".svg")
}

fn is_hash(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl Manifest {
    /// Checks the manifest's own consistency (not the files it describes).
    ///
    /// # Errors
    /// The first [`ManifestError`] found.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema_version > SCHEMA_VERSION {
            return Err(ManifestError::TooNew {
                found: self.schema_version,
                supported: SCHEMA_VERSION,
            });
        }
        if self.job_id.is_empty() {
            return Err(ManifestError::MissingJobId);
        }
        if self.mats.is_empty() {
            return Err(ManifestError::NoMats);
        }
        for (position, mat) in (1_u32..).zip(&self.mats) {
            if mat.index != position {
                return Err(ManifestError::MatOrder {
                    expected: position,
                    found: mat.index,
                });
            }
        }
        for (path, hash) in &self.files {
            if path == "manifest.json" || !is_bundle_file(path) {
                return Err(ManifestError::UnexpectedFile(path.clone()));
            }
            if !is_hash(hash) {
                return Err(ManifestError::BadHash(path.clone()));
            }
        }
        let needed = std::iter::once("profile.json".to_owned()).chain(
            self.mats
                .iter()
                .flat_map(|m| std::iter::once(m.gcode.clone()).chain(m.preview.clone())),
        );
        for path in needed {
            if !self.files.contains_key(&path) {
                return Err(ManifestError::MissingFileEntry(path));
            }
        }
        Ok(())
    }
}

/// The published JSON Schema (draft 2020-12) of the manifest.
#[must_use]
pub fn schema() -> schemars::Schema {
    schemars::schema_for!(Manifest)
}

#[cfg(test)]
mod tests;
