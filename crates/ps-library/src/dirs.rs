//! Where the library lives on disk: the platform data directory.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::store::{Library, StoreError};

/// The platform's data folder for Plotter Studio, if the platform has one.
#[must_use]
pub fn default_root() -> Option<PathBuf> {
    ProjectDirs::from("", "", "PlotterStudio").map(|d| d.data_dir().to_path_buf())
}

/// The settings file (`settings.toml`) in the platform config folder.
#[must_use]
pub fn default_settings_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "PlotterStudio").map(|d| d.config_dir().join("settings.toml"))
}

/// The folder for runtime state under the data folder: autosaves, the session lock and local
/// crash reports.
#[must_use]
pub fn default_state_dir() -> Option<PathBuf> {
    default_root().map(|root| root.join("state"))
}

/// The folder for installed device plugins (`*.psdevice`) under the data folder.
#[must_use]
pub fn default_plugins_dir() -> Option<PathBuf> {
    default_root().map(|root| root.join("plugins"))
}

/// Opens (creating if needed) the library under an explicit root, such as a portable install or
/// a test directory.
///
/// # Errors
/// [`StoreError::Io`] if the folders cannot be created.
pub fn open_at(root: &Path) -> Result<Library, StoreError> {
    Library::open(root)
}

#[cfg(test)]
mod tests;
