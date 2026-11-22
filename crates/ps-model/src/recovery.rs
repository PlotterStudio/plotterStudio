//! Autosave and crash recovery.
//!
//! Autosave runs every 30 s when the document changed, and on every mode switch, into
//! `<data>/autosave/<project-id>/`, keeping the newest five. A session lock file holds the process
//! id and a clean-exit flag; on start, a lock without the flag and with a dead process means a
//! crash, and the app offers the newest autosave. A panic writes a local crash report (backtrace,
//! app version, OS, recent log lines); nothing is uploaded.
//!
//! Everything here is plain file and clock logic with the time and the process check injected, so
//! it is deterministic to test. The app supplies the real clock and process probe.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::save::{SaveOptions, save_bytes};

const AUTOSAVE_EXT: &str = "pstudio";

/// When an autosave is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutosaveClock {
    /// When the last autosave happened, in ms.
    pub last_ms: u64,
    /// Whether the document changed since then.
    pub dirty: bool,
}

impl AutosaveClock {
    /// True when a changed document should be saved: the interval has passed, or the mode just
    /// switched. An unchanged document is never saved.
    #[must_use]
    pub fn due(&self, now_ms: u64, interval_ms: u64, mode_switched: bool) -> bool {
        self.dirty && (mode_switched || now_ms.saturating_sub(self.last_ms) >= interval_ms)
    }

    /// Records a completed autosave.
    pub fn saved(&mut self, now_ms: u64) {
        self.last_ms = now_ms;
        self.dirty = false;
    }

    /// Records an edit.
    pub fn edited(&mut self) {
        self.dirty = true;
    }
}

/// One autosave on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutosaveEntry {
    /// The file.
    pub path: PathBuf,
    /// When it was written, in ms since the epoch.
    pub time_ms: u64,
}

/// Why an autosave operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecoveryError {
    /// The project id is not a safe folder name.
    #[error("{0:?} is not a valid project id")]
    BadProjectId(String),
    /// A file operation failed.
    #[error("file error: {0}")]
    Io(String),
}

/// Writes and prunes autosaves under `<data>/autosave/<project-id>/`.
#[derive(Debug, Clone)]
pub struct Autosaver {
    root: PathBuf,
    keep: usize,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Autosaver {
    /// An autosaver under `data_dir` that keeps the newest `keep` files per project.
    #[must_use]
    pub fn new(data_dir: &Path, keep: usize) -> Self {
        Self {
            root: data_dir.join("autosave"),
            keep: keep.max(1),
        }
    }

    fn folder(&self, project_id: &str) -> Result<PathBuf, RecoveryError> {
        if valid_id(project_id) {
            Ok(self.root.join(project_id))
        } else {
            Err(RecoveryError::BadProjectId(project_id.to_owned()))
        }
    }

    /// Writes an autosave stamped `now_ms` and prunes the oldest beyond the limit.
    ///
    /// # Errors
    ///
    /// [`RecoveryError`] for an unsafe project id or a failed write.
    pub fn write(
        &self,
        project_id: &str,
        now_ms: u64,
        bytes: &[u8],
    ) -> Result<PathBuf, RecoveryError> {
        let folder = self.folder(project_id)?;
        fs::create_dir_all(&folder).map_err(|e| RecoveryError::Io(e.to_string()))?;
        let path = folder.join(format!("{now_ms:016}.{AUTOSAVE_EXT}"));
        save_bytes(&path, bytes, SaveOptions { keep_backup: false })
            .map_err(|e| RecoveryError::Io(e.to_string()))?;
        for old in self.list(project_id).into_iter().skip(self.keep) {
            let _ = fs::remove_file(old.path);
        }
        Ok(path)
    }

    /// The project's autosaves, newest first. Files that are not autosaves are ignored.
    #[must_use]
    pub fn list(&self, project_id: &str) -> Vec<AutosaveEntry> {
        let Ok(folder) = self.folder(project_id) else {
            return Vec::new();
        };
        let Ok(read) = fs::read_dir(folder) else {
            return Vec::new();
        };
        let mut entries: Vec<AutosaveEntry> = read
            .filter_map(Result::ok)
            .filter_map(|e| {
                let path = e.path();
                if path.extension()? != AUTOSAVE_EXT {
                    return None;
                }
                let time_ms = path.file_stem()?.to_str()?.parse::<u64>().ok()?;
                Some(AutosaveEntry { path, time_ms })
            })
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.time_ms));
        entries
    }

    /// The newest autosave of a project.
    #[must_use]
    pub fn latest(&self, project_id: &str) -> Option<AutosaveEntry> {
        self.list(project_id).into_iter().next()
    }

    /// Removes every autosave of a project (after a clean save or an explicit discard).
    ///
    /// # Errors
    ///
    /// [`RecoveryError`] for an unsafe project id or a failed removal.
    pub fn clear(&self, project_id: &str) -> Result<(), RecoveryError> {
        let folder = self.folder(project_id)?;
        match fs::remove_dir_all(folder) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(RecoveryError::Io(e.to_string())),
        }
    }
}

// ---- session lock ----

/// What the session lock file says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockInfo {
    /// The process that holds (or held) the session.
    pub pid: u32,
    /// When the session started, in ms since the epoch.
    pub started_ms: u64,
    /// Set when the app exited normally.
    pub clean_exit: bool,
}

/// How a previous session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    /// There was no previous session.
    Fresh,
    /// The previous session exited normally.
    CleanExit,
    /// The previous session died without a clean exit: offer the newest autosave.
    Crashed(LockInfo),
    /// The lock's process is still running: another instance of the app.
    Running(LockInfo),
}

/// Asks whether a process is alive (the app supplies a real implementation).
pub trait ProcessProbe {
    /// True when a process with this id is running.
    fn is_alive(&self, pid: u32) -> bool;
}

/// Reads the lock file to see how the previous session ended.
#[must_use]
pub fn inspect(lock: &Path, probe: &dyn ProcessProbe) -> Recovery {
    let Ok(text) = fs::read_to_string(lock) else {
        return Recovery::Fresh;
    };
    let Ok(info) = serde_json::from_str::<LockInfo>(&text) else {
        return Recovery::Crashed(LockInfo::default());
    };
    if info.clean_exit {
        Recovery::CleanExit
    } else if probe.is_alive(info.pid) {
        Recovery::Running(info)
    } else {
        Recovery::Crashed(info)
    }
}

fn write_lock(lock: &Path, info: &LockInfo) -> std::io::Result<()> {
    let json = serde_json::to_vec(info).map_err(std::io::Error::other)?;
    save_bytes(lock, &json, SaveOptions { keep_backup: false })
        .map_err(|e| std::io::Error::other(e.to_string()))
}

/// Writes the lock for a new session with the clean-exit flag cleared.
///
/// # Errors
///
/// An I/O error when the lock cannot be written.
pub fn acquire(lock: &Path, pid: u32, now_ms: u64) -> std::io::Result<()> {
    write_lock(
        lock,
        &LockInfo {
            pid,
            started_ms: now_ms,
            clean_exit: false,
        },
    )
}

/// Marks the session as ended normally. A missing lock is not an error.
///
/// # Errors
///
/// An I/O error when the lock exists but cannot be rewritten.
pub fn mark_clean_exit(lock: &Path) -> std::io::Result<()> {
    let Ok(text) = fs::read_to_string(lock) else {
        return Ok(());
    };
    let mut info = serde_json::from_str::<LockInfo>(&text).unwrap_or_default();
    info.clean_exit = true;
    write_lock(lock, &info)
}

// ---- crash reports ----

/// What a crash report records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashReport {
    /// The app version.
    pub app_version: String,
    /// The operating system.
    pub os: String,
    /// When it happened, in ms since the epoch.
    pub time_ms: u64,
    /// The panic message.
    pub message: String,
    /// The backtrace.
    pub backtrace: String,
    /// Recent log lines, oldest first.
    pub log_lines: Vec<String>,
}

/// Formats a crash report as plain text, with the last `max_log_lines` log lines.
#[must_use]
pub fn format_report(report: &CrashReport, max_log_lines: usize) -> String {
    let skip = report.log_lines.len().saturating_sub(max_log_lines);
    let log = report.log_lines[skip..].join("\n");
    format!(
        "Plotter Studio crash report\n\
         This report stays on this computer and is never uploaded; attach it to an issue if you wish.\n\
         \n\
         version: {}\n\
         os: {}\n\
         time_ms: {}\n\
         message: {}\n\
         \n\
         backtrace:\n{}\n\
         \n\
         recent log:\n{log}\n",
        report.app_version, report.os, report.time_ms, report.message, report.backtrace
    )
}

/// Writes a crash report into `dir` (created if needed) without overwriting earlier ones, and
/// returns its path.
///
/// # Errors
///
/// An I/O error when the folder or file cannot be written.
pub fn write_report(
    dir: &Path,
    report: &CrashReport,
    max_log_lines: usize,
) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let text = format_report(report, max_log_lines);
    let mut n = 1_u32;
    loop {
        let name = if n == 1 {
            format!("crash-{}.txt", report.time_ms)
        } else {
            format!("crash-{}-{n}.txt", report.time_ms)
        };
        let path = dir.join(name);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(text.as_bytes())?;
                file.sync_all()?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests;
