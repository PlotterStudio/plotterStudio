//! Opening and saving projects from the app: the `.pstudio` format with the safe save
//! (temporary file, sync, rename, optional `.bak`). The opened project's shell (thumbnails,
//! embedded files, unknown fields) is kept and written back, and a file that must not be saved
//! over is refused.

use std::path::Path;

use ps_model::{Document, Project, ReadOnly, SaveOptions, save_bytes};
use semver::Version;

use crate::state::AppState;

/// Why a project could not be opened or saved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectIoError {
    /// The file could not be read or written.
    #[error("file error: {0}")]
    Io(String),
    /// The file is not a valid project.
    #[error("not a valid project: {0}")]
    Invalid(String),
    /// The project was opened read-only and must not be saved over.
    #[error("this project is read-only because it was made by a newer version")]
    ReadOnly,
}

/// A project read from a file.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenProject {
    /// The document.
    pub document: Document,
    /// The rest of the file (thumbnails, embedded files, unknown fields), kept for saving.
    pub shell: Project,
    /// Set when the file must not be saved over.
    pub read_only: Option<ReadOnly>,
}

fn app_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0))
}

/// Reads a `.pstudio` project.
///
/// # Errors
///
/// [`ProjectIoError`] when the file cannot be read or is not a valid project.
pub fn open_project(path: &Path) -> Result<OpenProject, ProjectIoError> {
    let bytes =
        std::fs::read(path).map_err(|e| ProjectIoError::Io(format!("{}: {e}", path.display())))?;
    let loaded = Project::from_bytes(&bytes, &app_version())
        .map_err(|e| ProjectIoError::Invalid(e.to_string()))?;
    Ok(OpenProject {
        document: loaded.project.document.clone(),
        shell: loaded.project,
        read_only: loaded.read_only,
    })
}

/// Saves the project to `path` and records it as the project's file.
///
/// # Errors
///
/// [`ProjectIoError`] when the project is read-only or the file cannot be written; the previous
/// file is left intact.
pub fn save_project(state: &mut AppState, path: &Path) -> Result<(), ProjectIoError> {
    if state.read_only().is_some() {
        return Err(ProjectIoError::ReadOnly);
    }
    let mut shell = state
        .shell()
        .cloned()
        .unwrap_or_else(|| Project::new(state.document().as_ref().clone()));
    shell.document = state.document().as_ref().clone();
    let bytes = shell
        .to_bytes()
        .map_err(|e| ProjectIoError::Invalid(e.to_string()))?;
    let options = SaveOptions {
        keep_backup: state.settings().keep_backup,
    };
    save_bytes(path, &bytes, options).map_err(|e| ProjectIoError::Io(e.to_string()))?;
    state.finish_save(path, shell);
    Ok(())
}

#[cfg(test)]
mod tests;
