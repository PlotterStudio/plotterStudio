//! Saving safely.
//!
//! Serialise first (the caller does that on a background thread from a document snapshot), then
//! write to a temporary file in the same directory, fsync, and rename over the target. A crash
//! during the save leaves the previous file intact. The previous version is kept as
//! `<name>.pstudio.bak` unless turned off.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

/// Save settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveOptions {
    /// Keep the previous version as `<name>.bak`.
    pub keep_backup: bool,
}

impl Default for SaveOptions {
    fn default() -> Self {
        Self { keep_backup: true }
    }
}

/// Why a save failed. The previous file is intact in every case.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SaveError {
    /// A file-system operation failed.
    #[error("could not save: {0}")]
    Io(String),
}

fn io(e: &std::io::Error) -> SaveError {
    SaveError::Io(e.to_string())
}

/// The backup path for a project file: the whole name plus `.bak`.
#[must_use]
pub fn backup_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    target.with_file_name(name)
}

/// Writes through `write` into a temporary file beside `target`, syncs it, and moves it into
/// place. If `write` fails nothing changes and the temporary file is removed.
///
/// # Errors
///
/// [`SaveError::Io`] when the temporary file cannot be made, written, synced, or moved.
pub fn save_with(
    target: &Path,
    options: SaveOptions,
    write: impl FnOnce(&mut File) -> std::io::Result<()>,
) -> Result<(), SaveError> {
    let dir = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp = NamedTempFile::new_in(dir).map_err(|e| io(&e))?;
    write(temp.as_file_mut()).map_err(|e| io(&e))?;
    temp.as_file_mut().flush().map_err(|e| io(&e))?;
    temp.as_file().sync_all().map_err(|e| io(&e))?;

    if options.keep_backup && target.exists() {
        std::fs::copy(target, backup_path(target)).map_err(|e| io(&e))?;
    }
    temp.persist(target).map_err(|e| io(&e.error))?;
    Ok(())
}

/// Saves `bytes` as the file at `target`.
///
/// # Errors
///
/// As for [`save_with`].
pub fn save_bytes(target: &Path, bytes: &[u8], options: SaveOptions) -> Result<(), SaveError> {
    save_with(target, options, |file| file.write_all(bytes))
}

#[cfg(test)]
mod tests;
