//! Reading `.psjob` bundles: a zip with a fixed layout whose manifest lists the
//! sha256 of every other file. Opening verifies all of it, so a bundle that opens is complete,
//! untampered and safe to stream to a machine.

use std::collections::BTreeMap;

use ps_core::{ZipError, ZipLimits, read_archive};

use crate::manifest::{Manifest, ManifestError, SCHEMA_VERSION, is_bundle_file, sha256_hex};

/// Why a bundle was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BundleError {
    /// The zip itself was refused (bad archive, unsafe path, bomb, file outside the layout).
    #[error(transparent)]
    Zip(#[from] ZipError),
    /// There is no `manifest.json`.
    #[error("bundle has no manifest.json")]
    MissingManifest,
    /// `manifest.json` is not valid JSON of the right shape.
    #[error("manifest.json is malformed: {0}")]
    Manifest(String),
    /// The manifest breaks its own rules.
    #[error(transparent)]
    Invalid(#[from] ManifestError),
    /// A listed file is not in the archive.
    #[error("bundle is missing {0}")]
    MissingFile(String),
    /// A file's sha256 differs from the manifest.
    #[error("{0} has been modified (sha256 mismatch)")]
    HashMismatch(String),
    /// A file is in the archive but not listed in the manifest.
    #[error("{0} is not listed in the manifest")]
    UnlistedFile(String),
    /// A G-code file is not valid UTF-8.
    #[error("{0} is not valid UTF-8")]
    NotUtf8(String),
}

/// An opened, verified bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// The manifest.
    pub manifest: Manifest,
    files: BTreeMap<String, Vec<u8>>,
}

impl Bundle {
    /// Opens and fully verifies a bundle.
    ///
    /// # Errors
    /// [`BundleError`] for the first problem found; nothing is returned for a bundle that is
    /// incomplete, tampered with, or built outside the layout.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BundleError> {
        let mut files = read_archive(bytes, &ZipLimits::default(), &is_bundle_file)?;
        let manifest_bytes = files
            .remove("manifest.json")
            .ok_or(BundleError::MissingManifest)?;

        // Check the schema version on the raw JSON first, so a newer file whose shape we cannot
        // parse still gets the "update the app" message.
        let raw: serde_json::Value = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| BundleError::Manifest(e.to_string()))?;
        if let Some(found) = raw
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            && found > u64::from(SCHEMA_VERSION)
        {
            return Err(ManifestError::TooNew {
                found: u32::try_from(found).unwrap_or(u32::MAX),
                supported: SCHEMA_VERSION,
            }
            .into());
        }
        let manifest: Manifest =
            serde_json::from_value(raw).map_err(|e| BundleError::Manifest(e.to_string()))?;
        manifest.validate()?;

        for (path, expected) in &manifest.files {
            let data = files
                .get(path)
                .ok_or_else(|| BundleError::MissingFile(path.clone()))?;
            if sha256_hex(data) != *expected {
                return Err(BundleError::HashMismatch(path.clone()));
            }
        }
        if let Some(extra) = files.keys().find(|p| !manifest.files.contains_key(*p)) {
            return Err(BundleError::UnlistedFile(extra.clone()));
        }
        for mat in &manifest.mats {
            let utf8 = files
                .get(&mat.gcode)
                .is_some_and(|data| std::str::from_utf8(data).is_ok());
            if !utf8 {
                return Err(BundleError::NotUtf8(mat.gcode.clone()));
            }
        }
        Ok(Self { manifest, files })
    }

    /// The G-code of mat `index` (1-based).
    #[must_use]
    pub fn gcode(&self, index: u32) -> Option<&str> {
        let mat = self.manifest.mats.iter().find(|m| m.index == index)?;
        std::str::from_utf8(self.files.get(&mat.gcode)?).ok()
    }

    /// The copy of the machine profile used.
    #[must_use]
    pub fn profile_json(&self) -> Option<&[u8]> {
        self.file("profile.json")
    }

    /// Any file of the bundle by its path.
    #[must_use]
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }
}

#[cfg(test)]
mod tests;
