//! The shell layer: what happens when a command asks for a dialog or a file.
//!
//! File and folder choosers are behind [`Dialogs`], so the window binary uses native dialogs while
//! tests answer from a script. Errors are shown as a notice in the status bar rather than
//! interrupting the user.

use std::path::{Path, PathBuf};

use ps_model::{Document, ProjectMeta};

use crate::clock::rfc3339_utc;
use crate::export::{ExportError, ExportFormat, export_job, gcode_for_mat};
use crate::import::import_file;
use crate::l10n::Arg;
use crate::machine::job_header;
use crate::project_io::{open_project, save_project};
use crate::state::Effect;
use crate::ui::App;

/// The questions the shell asks the user.
pub trait Dialogs {
    /// Choose a project to open; `None` if cancelled.
    fn open_project(&mut self) -> Option<PathBuf>;
    /// Choose a drawing (SVG, SVGZ or DXF) to import; `None` if cancelled.
    fn import_drawing(&mut self) -> Option<PathBuf>;
    /// Choose where to save, starting from a suggested file name; `None` if cancelled.
    fn save_project(&mut self, suggested: &str) -> Option<PathBuf>;
    /// Choose a folder to export into; `None` if cancelled.
    fn pick_folder(&mut self) -> Option<PathBuf>;
    /// Choose a material package to install; `None` if cancelled.
    fn pick_package(&mut self) -> Option<PathBuf>;
    /// Choose a device plugin (`.psdevice`) to install; `None` if cancelled.
    fn pick_plugin(&mut self) -> Option<PathBuf>;
}

/// Carries out the effect a command left for the shell.
pub fn handle_effect(app: &mut App, effect: Effect, dialogs: &mut dyn Dialogs, now_unix_secs: u64) {
    match effect {
        Effect::None => {}
        Effect::NewProject => {
            let meta = ProjectMeta::new(
                "Untitled",
                &rfc3339_utc(now_unix_secs),
                env!("CARGO_PKG_VERSION"),
            );
            app.state_mut().load_document(Document::new(meta));
            app.notice = None;
        }
        Effect::OpenProject => {
            if let Some(path) = dialogs.open_project() {
                match open_project(&path) {
                    Ok(opened) => {
                        app.state_mut().adopt_project(opened, &path);
                        app.notice = None;
                    }
                    Err(e) => app.notice = Some(notice(app, "notice-open-failed", &e.to_string())),
                }
            }
        }
        Effect::Import => {
            if let Some(path) = dialogs.import_drawing() {
                import(app, &path, now_unix_secs);
            }
        }
        Effect::Save => save(app, dialogs, false),
        Effect::SaveAs => save(app, dialogs, true),
        Effect::Export => {
            if let Some(dir) = dialogs.pick_folder() {
                let _ = export_to_folder(app, &dir, now_unix_secs);
            }
        }
        Effect::SendToMachine => send_to_machine(app),
        Effect::InstallPlugin => {
            if let Some(path) = dialogs.pick_plugin() {
                match std::fs::read(&path) {
                    Ok(bytes) => app.install_plugin_bytes(&bytes),
                    Err(e) => app.report_plugin_failure(&format!("{}: {e}", path.display())),
                }
            }
        }
        Effect::InstallPackage => {
            if let Some(path) = dialogs.pick_package() {
                match std::fs::read(&path) {
                    Ok(bytes) => app.install_package(&bytes, None),
                    Err(e) => app.notice = Some(format!("{}: {e}", path.display())),
                }
            }
        }
    }
}

/// Sends the first compiled mat to the connected machine.
fn send_to_machine(app: &mut App) {
    if !app.machine.is_ready() {
        app.notice = Some(app.loc.tr("notice-no-machine"));
        return;
    }
    let (Some(program), Some(header)) = (gcode_for_mat(app.state(), 0), job_header(app.state(), 0))
    else {
        app.notice = Some(app.loc.tr("export-nothing"));
        return;
    };
    match app.machine.send(program, header) {
        Ok(()) => app.notice = None,
        Err(e) => app.notice = Some(notice(app, "notice-send-failed", &e.to_string())),
    }
}

/// Imports a drawing and tells the user what happened.
fn import(app: &mut App, path: &Path, now_unix_secs: u64) {
    match import_file(app.state_mut(), path, now_unix_secs * 1_000) {
        Ok(report) => {
            let count = f64::from(u32::try_from(report.objects).unwrap_or(u32::MAX));
            app.notice = Some(app.loc.tr_args(
                "notice-imported",
                &[
                    ("name", Arg::Text(&report.name)),
                    ("count", Arg::Number(count)),
                ],
            ));
        }
        Err(e) => app.notice = Some(notice(app, "notice-import-failed", &e.to_string())),
    }
}

fn notice(app: &App, key: &str, reason: &str) -> String {
    app.loc.tr_args(key, &[("reason", Arg::Text(reason))])
}

fn save(app: &mut App, dialogs: &mut dyn Dialogs, always_ask: bool) {
    let existing = app.state().project_path().map(Path::to_path_buf);
    let target = match existing {
        Some(path) if !always_ask => Some(path),
        _ => {
            let suggested = format!("{}.pstudio", project_stem(app));
            dialogs.save_project(&suggested)
        }
    };
    let Some(path) = target else {
        return;
    };
    match save_project(app.state_mut(), &path) {
        Ok(()) => app.notice = None,
        Err(e) => app.notice = Some(notice(app, "notice-save-failed", &e.to_string())),
    }
}

fn project_stem(app: &App) -> String {
    app.state()
        .project_path()
        .and_then(Path::file_stem)
        .map_or_else(|| "job".to_owned(), |s| s.to_string_lossy().into_owned())
}

/// Exports the current job into `dir` and records the outcome in the app's export status.
///
/// # Errors
///
/// The [`ExportError`] when nothing could be written; the status then says why.
pub fn export_to_folder(
    app: &mut App,
    dir: &Path,
    now_unix_secs: u64,
) -> Result<Vec<PathBuf>, ExportError> {
    let stem = project_stem(app);
    let time = rfc3339_utc(now_unix_secs);
    match export_job(app.state(), dir, &stem, ExportFormat::Both, &time) {
        Ok(files) => {
            let count = f64::from(u32::try_from(files.len()).unwrap_or(u32::MAX));
            app.export_status = Some(
                app.loc
                    .tr_args("export-done", &[("count", Arg::Number(count))]),
            );
            Ok(files)
        }
        Err(e) => {
            app.export_status = Some(notice(app, "export-failed", &e.to_string()));
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests;
