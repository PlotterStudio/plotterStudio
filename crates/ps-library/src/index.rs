//! The community materials index.
//!
//! `index.json` lists packages with their sha256 and download URL. The app searches it locally
//! and checks each download against its hash. Fetching and the minisign signature check belong
//! to `ps-net`, the only crate allowed on the network.

use std::collections::BTreeMap;
use std::fmt::Write;

use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The index layout this build reads.
pub const INDEX_SCHEMA: u32 = 1;

/// Lowercase hex sha256.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        // Writing to a String cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// One downloadable package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    /// Package id.
    pub id: String,
    /// Package version.
    pub version: Version,
    /// Display name.
    pub name: String,
    /// Category, such as `paper`.
    pub category: String,
    /// Brand.
    pub brand: String,
    /// Machine ids the package has settings for.
    pub machines: Vec<String>,
    /// Tool ids the package has settings for.
    pub tools: Vec<String>,
    /// Lowercase hex sha256 of the package file.
    pub sha256: String,
    /// Where to download it.
    pub url: String,
}

impl IndexEntry {
    /// True when downloaded bytes match the listed hash.
    #[must_use]
    pub fn verify(&self, package: &[u8]) -> bool {
        sha256_hex(package) == self.sha256
    }

    /// True for `https://` URLs only.
    #[must_use]
    pub fn has_safe_url(&self) -> bool {
        self.url.starts_with("https://")
    }
}

/// A local search.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// Text to look for in id, name, brand and category.
    pub text: String,
    /// Only packages with settings for this machine.
    pub machine: Option<String>,
    /// Only this category.
    pub category: Option<String>,
}

impl Query {
    /// A text-only query.
    #[must_use]
    pub fn text(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            ..Self::default()
        }
    }
}

/// The index file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    /// Layout version.
    pub schema_version: u32,
    /// The packages.
    pub entries: Vec<IndexEntry>,
}

impl Index {
    /// Parses an index, refusing newer layouts.
    ///
    /// # Errors
    /// A message for invalid JSON or an index from a newer app.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let index: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if index.schema_version > INDEX_SCHEMA {
            return Err(format!(
                "index schema {} is newer than the {INDEX_SCHEMA} this app supports; update the app",
                index.schema_version
            ));
        }
        Ok(index)
    }

    /// Entries matching a query, sorted by id.
    #[must_use]
    pub fn search(&self, q: &Query) -> Vec<&IndexEntry> {
        let text = q.text.trim().to_lowercase();
        let mut out: Vec<&IndexEntry> = self
            .entries
            .iter()
            .filter(|e| {
                let text_ok = text.is_empty()
                    || [&e.id, &e.name, &e.brand, &e.category]
                        .iter()
                        .any(|f| f.to_lowercase().contains(&text));
                let machine_ok = q.machine.as_ref().is_none_or(|m| e.machines.contains(m));
                let category_ok = q.category.as_ref().is_none_or(|c| &e.category == c);
                text_ok && machine_ok && category_ok
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// Entries newer than the installed version of the same id (not-installed ids are ignored).
    #[must_use]
    pub fn updates(&self, installed: &BTreeMap<String, Version>) -> Vec<&IndexEntry> {
        let mut out: Vec<&IndexEntry> = self
            .entries
            .iter()
            .filter(|e| installed.get(&e.id).is_some_and(|have| &e.version > have))
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }
}

#[cfg(test)]
mod tests;
