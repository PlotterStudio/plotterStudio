//! A zip reader that refuses hostile archives.
//!
//! Used for material packages, project files and device plugin packages. It reads only the
//! central directory first, rejects unsafe entries before decompressing anything, and
//! stream-decompresses with hard limits so a zip bomb cannot exhaust memory.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

/// Entries smaller than this are exempt from the ratio check: tiny files legitimately
/// compress very well.
const RATIO_MIN_BYTES: u64 = 64 * 1024;

/// Limits applied while reading an archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZipLimits {
    /// Most entries allowed.
    pub max_entries: usize,
    /// Most uncompressed bytes, summed over all entries as the archive declares them.
    pub max_total_bytes: u64,
    /// Highest uncompressed-to-compressed ratio for an entry of at least 64 KiB.
    pub max_ratio: u64,
}

impl Default for ZipLimits {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_total_bytes: 500 * 1024 * 1024,
            max_ratio: 200,
        }
    }
}

/// Why an archive was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ZipError {
    /// The bytes are not a readable zip archive.
    #[error("not a valid zip archive: {0}")]
    Invalid(String),
    /// An entry name is absolute, contains `..`, or is otherwise unsafe.
    #[error("unsafe entry path {0:?}")]
    UnsafePath(String),
    /// An entry is a symbolic link.
    #[error("entry {0:?} is a symbolic link")]
    Symlink(String),
    /// An entry name is not on the caller's allowlist.
    #[error("entry {0:?} is not allowed in this kind of archive")]
    NotAllowed(String),
    /// The archive has more entries than the limit.
    #[error("archive has too many entries")]
    TooManyEntries,
    /// The archive declares more uncompressed data than the limit.
    #[error("archive is too large when uncompressed")]
    TooLarge,
    /// An entry expands far beyond its compressed size, or beyond its declared size.
    #[error("entry {0:?} looks like a zip bomb")]
    ZipBomb(String),
    /// Two entries have the same name.
    #[error("duplicate entry {0:?}")]
    Duplicate(String),
}

/// Returns true when `name` is a safe relative archive path: no empty, `.` or `..` components,
/// no leading `/`, no backslashes and no drive letters.
#[must_use]
pub fn is_safe_path(name: &str) -> bool {
    if name.is_empty() || name.starts_with('/') || name.contains('\\') || name.contains(':') {
        return false;
    }
    name.split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// File-type bits of a Unix mode, and the value meaning "symbolic link".
const S_IFMT: u32 = 0o170_000;
const S_IFLNK: u32 = 0o120_000;

/// Reads every file of a zip archive into memory, enforcing the limits and the allowlist.
///
/// Directory entries are skipped. Entry names are returned as stored.
///
/// # Errors
/// [`ZipError`] describing the first rule the archive breaks.
pub fn read_archive(
    bytes: &[u8],
    limits: &ZipLimits,
    allow: &dyn Fn(&str) -> bool,
) -> Result<BTreeMap<String, Vec<u8>>, ZipError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| ZipError::Invalid(e.to_string()))?;
    if archive.len() > limits.max_entries {
        return Err(ZipError::TooManyEntries);
    }

    // First pass: look at every entry's metadata only.
    let mut declared_total: u64 = 0;
    for i in 0..archive.len() {
        let file = archive
            .by_index_raw(i)
            .map_err(|e| ZipError::Invalid(e.to_string()))?;
        if file.is_dir() {
            continue;
        }
        let name = file
            .name()
            .map_err(|e| ZipError::Invalid(e.to_string()))?
            .into_owned();
        if !is_safe_path(&name) {
            return Err(ZipError::UnsafePath(name));
        }
        if file.is_symlink() || file.unix_mode().is_some_and(|m| m & S_IFMT == S_IFLNK) {
            return Err(ZipError::Symlink(name));
        }
        if !allow(&name) {
            return Err(ZipError::NotAllowed(name));
        }
        declared_total = declared_total.saturating_add(file.size());
        if declared_total > limits.max_total_bytes {
            return Err(ZipError::TooLarge);
        }
    }

    // Second pass: decompress through a counting reader.
    let mut out = BTreeMap::new();
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| ZipError::Invalid(e.to_string()))?;
        if file.is_dir() {
            continue;
        }
        let name = file
            .name()
            .map_err(|e| ZipError::Invalid(e.to_string()))?
            .into_owned();
        let declared = file.size();
        let compressed = file.compressed_size().max(1);
        if declared >= RATIO_MIN_BYTES && declared / compressed > limits.max_ratio {
            return Err(ZipError::ZipBomb(name));
        }
        let mut data = Vec::with_capacity(usize::try_from(declared.min(1 << 24)).unwrap_or(0));
        // One byte past the declared size is enough to prove the entry lies about it.
        let mut reader = (&mut file).take(declared.saturating_add(1));
        reader
            .read_to_end(&mut data)
            .map_err(|e| ZipError::Invalid(e.to_string()))?;
        if data.len() as u64 > declared {
            return Err(ZipError::ZipBomb(name));
        }
        if out.insert(name.clone(), data).is_some() {
            return Err(ZipError::Duplicate(name));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
