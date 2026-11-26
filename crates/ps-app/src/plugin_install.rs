//! Putting a device plugin where the app looks for them.
//!
//! The app loads every `*.psdevice` file in one folder. Installing a plugin is therefore copying a
//! package there, and this module does that part safely: the file is named from the plugin's id
//! (cleaned, because the id comes from a file the user did not write), written beside its final
//! place and renamed into it so a crash never leaves half a plugin, and any older file for the
//! same plugin under another name is removed so the next start does not load two.
//!
//! Checking that the bytes are a plugin at all is the caller's job and comes first: nothing here
//! looks inside the component.

use std::path::{Path, PathBuf};

/// The extension of a plugin package.
const EXTENSION: &str = "psdevice";

/// Why a package could not be put in the plugins folder.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum InstallError {
    /// The plugin's id has no character that can be used in a file name.
    #[error("the plugin's id {0:?} cannot be used as a file name")]
    BadId(String),
    /// The folder or file could not be written.
    #[error("{0}")]
    Io(String),
}

/// A file name for a plugin id: letters, digits, `.`, `-` and `_` are kept, anything else becomes
/// `_`, and leading dots go so the name can be neither hidden nor `..`.
fn stem(id: &str) -> Option<String> {
    let cleaned: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_start_matches('.');
    // An id that is only separators would become a name of underscores; that says nothing.
    if trimmed.is_empty() || trimmed.chars().all(|c| c == '_') {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Copies a plugin package, already checked by the caller, into `dir` as `<id>.psdevice`,
/// creating the folder if needed and replacing any earlier file for the same plugin id.
///
/// # Errors
///
/// [`InstallError::BadId`] when `id` gives no usable file name, [`InstallError::Io`] when the
/// folder or file cannot be written.
pub(crate) fn store(dir: &Path, id: &str, bytes: &[u8]) -> Result<PathBuf, InstallError> {
    let stem = stem(id).ok_or_else(|| InstallError::BadId(id.to_owned()))?;
    let io = |e: std::io::Error| InstallError::Io(e.to_string());
    std::fs::create_dir_all(dir).map_err(io)?;
    let target = dir.join(format!("{stem}.{EXTENSION}"));
    // The extension is not `psdevice`, so a scan that runs meanwhile never sees the partial file.
    let partial = dir.join(format!("{stem}.{EXTENSION}.part"));
    std::fs::write(&partial, bytes).map_err(io)?;
    if let Err(e) = std::fs::rename(&partial, &target) {
        let _ = std::fs::remove_file(&partial);
        return Err(io(e));
    }
    remove_older_copies(dir, &target, id);
    Ok(target)
}

/// Removes every other package in `dir` that holds the plugin `id`, so it loads once.
fn remove_older_copies(dir: &Path, keep: &Path, id: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.filter_map(Result::ok).map(|e| e.path()) {
        if path == keep || path.extension().is_none_or(|x| x != EXTENSION) {
            continue;
        }
        let same = std::fs::read(&path)
            .ok()
            .and_then(|bytes| ps_device::unpack(&bytes).ok())
            .is_some_and(|found| found.manifest.id == id);
        if same && let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(file = %path.display(), error = %e, "an older copy of a plugin could not be removed");
        }
    }
}

#[cfg(test)]
mod tests;
