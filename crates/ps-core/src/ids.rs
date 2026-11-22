//! Identifiers: `UUIDv7` newtypes that are stable across saves, plus [`PathKey`].
//!
//! Each id kind is its own type so an object id can never be passed where a mat id is expected.
//! `UUIDv7` puts a millisecond timestamp in the high bits, so ids sort in creation order, which
//! keeps ordered maps keyed by id stable.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::{NoContext, Timestamp, Uuid};

use crate::hashing::ContentHash;

/// The text was not a valid identifier.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid id: {0}")]
pub struct ParseIdError(String);

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates a fresh id stamped with the current time.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Creates an id stamped with the given Unix time in milliseconds.
            #[must_use]
            pub fn at_unix_ms(ms: u64) -> Self {
                let ts = Timestamp::from_unix(NoContext, ms / 1_000, u32::try_from(ms % 1_000).unwrap_or(0) * 1_000_000);
                Self(Uuid::new_v7(ts))
            }

            /// Wraps an existing UUID.
            #[must_use]
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Returns the underlying UUID.
            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// Returns the creation time in Unix milliseconds, if this is a v7 id.
            #[must_use]
            pub fn unix_ms(&self) -> Option<u64> {
                let (secs, nanos) = self.0.get_timestamp()?.to_unix();
                secs.checked_mul(1_000)?.checked_add(u64::from(nanos / 1_000_000))
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0.hyphenated(), f)
            }
        }

        impl FromStr for $name {
            type Err = ParseIdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s)
                    .map(Self)
                    .map_err(|e| ParseIdError(e.to_string()))
            }
        }
    };
}

id_type!(
    /// Identifies a placed object.
    ObjectId
);
id_type!(
    /// Identifies a mat.
    MatId
);
id_type!(
    /// Identifies an operation.
    OpId
);
id_type!(
    /// Identifies an imported asset (an original source file).
    AssetId
);
id_type!(
    /// Identifies a row of the operations panel.
    OpRowId
);
id_type!(
    /// Identifies a group of objects edited together.
    GroupId
);
id_type!(
    /// Identifies a set of objects whose registration is locked across tools.
    AttachSetId
);

/// The stable identity of a path, used to match paths again on re-import.
///
/// It is the source file's own id when there is one, otherwise a structural fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PathKey {
    /// An id taken from the source file (an SVG element id, a DXF handle).
    Source(String),
    /// A hash of the path's structure.
    Fingerprint(ContentHash),
}

/// The text was not a valid path key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid path key: {0}")]
pub struct ParseKeyError(String);

impl PathKey {
    /// Creates a key from a source-file id.
    #[must_use]
    pub fn source(id: impl Into<String>) -> Self {
        Self::Source(id.into())
    }

    /// Creates a key from a structural fingerprint.
    #[must_use]
    pub const fn fingerprint(hash: ContentHash) -> Self {
        Self::Fingerprint(hash)
    }
}

impl fmt::Display for PathKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(id) => write!(f, "src:{id}"),
            Self::Fingerprint(h) => write!(f, "fp:{h}"),
        }
    }
}

impl FromStr for PathKey {
    type Err = ParseKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(id) = s.strip_prefix("src:") {
            if id.is_empty() {
                return Err(ParseKeyError("empty source id".into()));
            }
            Ok(Self::Source(id.to_owned()))
        } else if let Some(hex) = s.strip_prefix("fp:") {
            hex.parse()
                .map(Self::Fingerprint)
                .map_err(|e| ParseKeyError(format!("{e}")))
        } else {
            Err(ParseKeyError(format!("unknown prefix in {s:?}")))
        }
    }
}

impl Serialize for PathKey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PathKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;
