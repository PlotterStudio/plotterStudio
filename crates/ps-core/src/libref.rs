//! References to library entries: an id plus a semantic version.
//!
//! Projects store these instead of copies of tools, materials and machine profiles. The text
//! form `id@version` also appears in G-code headers and job manifests.

use std::fmt;
use std::str::FromStr;

use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A reference was malformed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LibRefError {
    /// The id is empty or contains characters other than letters, digits, `.`, `-` and `_`.
    #[error("invalid library id {0:?}")]
    BadId(String),
    /// The text has no `@version` part.
    #[error("library reference {0:?} must look like id@major.minor.patch")]
    MissingVersion(String),
    /// The version is not a semantic version.
    #[error("invalid version in library reference: {0}")]
    BadVersion(String),
}

/// An id and version identifying one library entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LibRef {
    id: String,
    version: Version,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

impl LibRef {
    /// Creates a reference.
    ///
    /// # Errors
    /// [`LibRefError::BadId`] when the id is empty or contains other than letters, digits, `.`,
    /// `-` and `_`.
    pub fn new(id: impl Into<String>, version: Version) -> Result<Self, LibRefError> {
        let id = id.into();
        if valid_id(&id) {
            Ok(Self { id, version })
        } else {
            Err(LibRefError::BadId(id))
        }
    }

    /// The entry's id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The entry's version.
    #[must_use]
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// True when both refer to the same entry, whatever the version.
    #[must_use]
    pub fn same_id(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl PartialOrd for LibRef {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LibRef {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (&self.id, &self.version).cmp(&(&other.id, &other.version))
    }
}

impl fmt::Display for LibRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.id, self.version)
    }
}

impl FromStr for LibRef {
    type Err = LibRefError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (id, version) = s
            .split_once('@')
            .ok_or_else(|| LibRefError::MissingVersion(s.to_owned()))?;
        let version =
            Version::parse(version).map_err(|e| LibRefError::BadVersion(e.to_string()))?;
        Self::new(id, version)
    }
}

impl Serialize for LibRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for LibRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;
