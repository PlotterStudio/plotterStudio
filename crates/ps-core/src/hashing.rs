//! Content hashing: BLAKE3 digests and a canonical feed for composite keys.
//!
//! [`StableHasher`] fixes how every primitive is turned into bytes (little-endian integers,
//! length-prefixed strings, normalised floats) so memo keys and job input hashes are identical
//! on every operating system.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A 32-byte BLAKE3 digest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentHash([u8; 32]);

/// The text was not 64 hexadecimal digits.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid content hash: expected 64 hexadecimal digits")]
pub struct ParseHashError;

impl ContentHash {
    /// Wraps raw digest bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the raw digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", blake3::Hash::from_bytes(self.0).to_hex())
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentHash({self})")
    }
}

impl FromStr for ContentHash {
    type Err = ParseHashError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hash = blake3::Hash::from_hex(s).map_err(|_| ParseHashError)?;
        Ok(Self(*hash.as_bytes()))
    }
}

impl Serialize for ContentHash {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// Hashes a byte slice.
#[must_use]
pub fn hash_bytes(bytes: &[u8]) -> ContentHash {
    ContentHash(*blake3::hash(bytes).as_bytes())
}

/// The canonical quiet NaN used when hashing any NaN.
const CANONICAL_NAN_BITS: u64 = 0x7ff8_0000_0000_0000;

/// Feeds typed values to BLAKE3 in a platform-independent, unambiguous encoding.
#[derive(Debug, Default)]
pub struct StableHasher(blake3::Hasher);

impl StableHasher {
    /// Starts an empty feed.
    #[must_use]
    pub fn new() -> Self {
        Self(blake3::Hasher::new())
    }

    fn raw(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    /// Feeds one byte.
    pub fn write_u8(&mut self, v: u8) {
        self.raw(&[v]);
    }

    /// Feeds a `u32`, little-endian.
    pub fn write_u32(&mut self, v: u32) {
        self.raw(&v.to_le_bytes());
    }

    /// Feeds a `u64`, little-endian.
    pub fn write_u64(&mut self, v: u64) {
        self.raw(&v.to_le_bytes());
    }

    /// Feeds an `i64`, little-endian two's complement.
    pub fn write_i64(&mut self, v: i64) {
        self.raw(&v.to_le_bytes());
    }

    /// Feeds a bool as one byte (0 or 1).
    pub fn write_bool(&mut self, v: bool) {
        self.write_u8(u8::from(v));
    }

    /// Feeds an optional `u32`: a 0 tag, or a 1 tag followed by the value.
    pub fn write_option_u32(&mut self, v: Option<u32>) {
        match v {
            None => self.write_u8(0),
            Some(x) => {
                self.write_u8(1);
                self.write_u32(x);
            }
        }
    }

    /// Feeds an `f64` by its bits, with `-0.0` treated as `0.0` and every NaN as one NaN.
    pub fn write_f64(&mut self, v: f64) {
        let bits = if v.is_nan() {
            CANONICAL_NAN_BITS
        } else if v == 0.0 {
            0
        } else {
            v.to_bits()
        };
        self.write_u64(bits);
    }

    /// Feeds a length-prefixed byte string.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        self.write_u64(len);
        self.raw(bytes);
    }

    /// Feeds a length-prefixed UTF-8 string.
    pub fn write_str(&mut self, s: &str) {
        self.write_bytes(s.as_bytes());
    }

    /// Feeds another digest (fixed length, so no prefix).
    pub fn write_hash(&mut self, h: &ContentHash) {
        self.raw(h.as_bytes());
    }

    /// Finishes the feed and returns the digest.
    #[must_use]
    pub fn finish(&self) -> ContentHash {
        ContentHash(*self.0.finalize().as_bytes())
    }
}

#[cfg(test)]
mod tests;
