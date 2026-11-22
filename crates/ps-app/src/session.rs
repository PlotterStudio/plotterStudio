//! Autosave and crash recovery.
//!
//! [`SessionGuard`] holds the session lock and writes autosaves: every `autosave_interval_s` when
//! the document changed, and at once on a mode switch, keeping the newest five per project. On
//! start it reads the previous lock: a lock without a clean exit and with a dead process means a
//! crash, and the newest autosave is offered. [`write_panic_files`] is what the panic hook runs:
//! the unsaved work as an autosave and a local crash report, never uploaded.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ps_core::hash_bytes;
use ps_model::{
    AutosaveClock, AutosaveEntry, Autosaver, CrashReport, Document, ProcessProbe, Project,
    Recovery, RecoveryError, acquire, inspect, mark_clean_exit, write_report,
};

use crate::state::AppState;

/// Autosaves kept per project.
const KEEP: usize = 5;
/// Recent log lines kept in a crash report.
const REPORT_LOG_LINES: usize = 200;

/// How the previous session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Startup {
    /// A normal start: there was no previous session, or it exited cleanly.
    Fresh,
    /// The previous session died; the newest autosave, if any, can be restored.
    Crashed {
        /// The autosave to offer.
        autosave: Option<AutosaveEntry>,
    },
    /// Another copy of the app is running with this data folder.
    AnotherInstance,
}

/// The autosave folder name for a project: stable for one file, different for different files.
#[must_use]
pub fn project_id(path: Option<&Path>) -> String {
    match path {
        None => "untitled".to_owned(),
        Some(p) => {
            let hex = hash_bytes(p.to_string_lossy().as_bytes()).to_string();
            format!("p-{}", &hex[..16])
        }
    }
}

/// The newest autosave of any project.
fn newest_autosave(autosaver: &Autosaver, state_dir: &Path) -> Option<AutosaveEntry> {
    let root = state_dir.join("autosave");
    std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter_map(|id| autosaver.latest(&id))
        .max_by_key(|entry| entry.time_ms)
}

/// Holds the session lock and the autosave schedule.
#[derive(Debug)]
pub struct SessionGuard {
    state_dir: PathBuf,
    lock: PathBuf,
    autosaver: Autosaver,
    clock: AutosaveClock,
    last_saved_generation: Option<u64>,
}

impl SessionGuard {
    /// Starts a session under `state_dir`: reads how the last one ended, then takes the lock
    /// (unless another instance holds it).
    pub fn start(
        state_dir: &Path,
        probe: &dyn ProcessProbe,
        pid: u32,
        now_ms: u64,
    ) -> (Self, Startup) {
        let lock = state_dir.join("session.lock");
        let autosaver = Autosaver::new(state_dir, KEEP);
        let startup = match inspect(&lock, probe) {
            Recovery::Fresh | Recovery::CleanExit => Startup::Fresh,
            Recovery::Crashed(_) => Startup::Crashed {
                autosave: newest_autosave(&autosaver, state_dir),
            },
            Recovery::Running(_) => Startup::AnotherInstance,
        };
        if startup != Startup::AnotherInstance {
            // A lock that cannot be written only costs crash detection next time.
            let _ = std::fs::create_dir_all(state_dir);
            let _ = acquire(&lock, pid, now_ms);
        }
        let guard = Self {
            state_dir: state_dir.to_path_buf(),
            lock,
            autosaver,
            clock: AutosaveClock {
                last_ms: now_ms,
                dirty: false,
            },
            last_saved_generation: None,
        };
        (guard, startup)
    }

    /// Autosaves if the document changed and the interval has passed (or the mode just
    /// switched). Returns whether a file was written.
    ///
    /// # Errors
    ///
    /// [`RecoveryError`] when the autosave cannot be written.
    pub fn tick(
        &mut self,
        state: &AppState,
        every_s: u32,
        now_ms: u64,
        switched_mode: bool,
    ) -> Result<bool, RecoveryError> {
        self.clock.dirty =
            state.is_dirty() && self.last_saved_generation != Some(state.generation());
        let interval_ms = u64::from(every_s).saturating_mul(1_000);
        if !self.clock.due(now_ms, interval_ms, switched_mode) {
            return Ok(false);
        }
        let project = Project::new(state.document().as_ref().clone());
        let bytes = project
            .to_bytes()
            .map_err(|e| RecoveryError::Io(e.to_string()))?;
        self.autosaver
            .write(&project_id(state.project_path()), now_ms, &bytes)?;
        self.clock.saved(now_ms);
        self.last_saved_generation = Some(state.generation());
        Ok(true)
    }

    /// Records a real save: the project's autosaves are no longer needed.
    pub fn saved(&mut self, state: &AppState) {
        let _ = self.autosaver.clear(&project_id(state.project_path()));
        let _ = self.autosaver.clear(&project_id(None));
        self.last_saved_generation = Some(state.generation());
    }

    /// Marks the session as ended normally.
    pub fn finish(&self) {
        let _ = mark_clean_exit(&self.lock);
    }

    /// The folder this session keeps its state in.
    #[must_use]
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }
}

/// A document and the file it belongs to, if any.
type Latest = Option<(Option<PathBuf>, Arc<Document>)>;

/// The latest document, shared with the panic hook so it can save unsaved work.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    inner: Arc<Mutex<Latest>>,
}

impl Snapshot {
    /// Records the state's current document (a cheap reference copy).
    pub fn update(&self, state: &AppState) {
        let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        *guard = Some((
            state.project_path().map(Path::to_path_buf),
            state.document(),
        ));
    }

    fn get(&self) -> Latest {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// What the panic hook does: saves the unsaved work as an autosave and writes a local crash
/// report into `<state_dir>/crash/`. Returns the report's path.
///
/// # Errors
///
/// An I/O error when the report cannot be written.
#[expect(
    clippy::too_many_arguments,
    reason = "everything a crash report records is handed over by the hook in one call"
)]
pub fn write_panic_files(
    state_dir: &Path,
    snapshot: &Snapshot,
    message: &str,
    backtrace: &str,
    log_lines: &[String],
    app_version: &str,
    os: &str,
    now_ms: u64,
) -> std::io::Result<PathBuf> {
    if let Some((path, doc)) = snapshot.get() {
        // Saving the work matters more than the report, but both are tried.
        if let Ok(bytes) = Project::new(doc.as_ref().clone()).to_bytes() {
            let _ =
                Autosaver::new(state_dir, KEEP).write(&project_id(path.as_deref()), now_ms, &bytes);
        }
    }
    write_report(
        &state_dir.join("crash"),
        &CrashReport {
            app_version: app_version.to_owned(),
            os: os.to_owned(),
            time_ms: now_ms,
            message: message.to_owned(),
            backtrace: backtrace.to_owned(),
            log_lines: log_lines.to_vec(),
        },
        REPORT_LOG_LINES,
    )
}

#[cfg(test)]
mod tests;
