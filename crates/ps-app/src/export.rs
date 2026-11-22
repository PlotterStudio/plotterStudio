//! Exporting a compiled job from the app, with the same validated writers as
//! `pstudio export`: the IR is checked, one G-code file is written per mat, and the `.psjob`
//! bundle is written atomically. The bundle is built in memory first, so a refused job leaves no
//! files behind.

use std::path::{Path, PathBuf};

use ps_core::hash_bytes;
use ps_gcode::{BundleInput, BundleMat, WriteContext, export_gcode, write_bundle, write_mat};
use ps_model::{Mat, SaveOptions, save_bytes};

use crate::state::AppState;

/// What an export writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// A `.psjob` bundle.
    Psjob,
    /// One `.gcode` file per mat.
    Gcode,
    /// Both.
    Both,
}

/// Why an export did not happen.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportError {
    /// There is no up-to-date compiled job with something to run.
    #[error("there is no up-to-date job to export")]
    NoJob,
    /// The destination is not a folder.
    #[error("{0} is not a folder")]
    Folder(String),
    /// The job failed validation or a file could not be written.
    #[error("export refused: {0}")]
    Refused(String),
}

/// A short description of a mat for the G-code header, such as `12x12in standard`.
#[must_use]
pub fn mat_label(mat: &Mat) -> String {
    let trim = |mm: f64| {
        let inches = format!("{:.1}", mm / 25.4);
        inches
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    };
    let grip = serde_json::to_value(mat.mat_type)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    format!(
        "{}x{}in {grip}",
        trim(mat.size.width.get()),
        trim(mat.size.height.get())
    )
}

fn write_context<'a>(
    state: &'a AppState,
    mat_index: usize,
    mat_count: usize,
    label: &'a str,
    id: &'a str,
    mat: Option<&'a Mat>,
) -> Option<WriteContext<'a>> {
    Some(WriteContext {
        app_version: env!("CARGO_PKG_VERSION"),
        job_id: id,
        profile: state.profile()?,
        mat_index: u32::try_from(mat_index + 1).unwrap_or(u32::MAX),
        mat_count: u32::try_from(mat_count).unwrap_or(u32::MAX),
        mat_label: label,
        material: mat.and_then(|m| m.material.as_ref()),
    })
}

/// A job id derived from the generated G-code, so identical inputs give identical bundles even
/// when a document's object ids are freshly generated.
#[must_use]
pub fn job_id_of(state: &AppState) -> Option<String> {
    let compiled = state.fresh_compile()?;
    let doc = state.document();
    let count = compiled.job.mats.len();
    let mut text = String::new();
    for (i, mat_job) in compiled.job.mats.iter().enumerate() {
        let mat = doc.mat(mat_job.mat)?;
        let label = mat_label(mat);
        let ctx = write_context(state, i, count, &label, "", Some(mat))?;
        text.push_str(&write_mat(mat_job, &ctx).text);
    }
    let hex = hash_bytes(text.as_bytes()).to_string();
    Some(format!("job-{}", &hex[..16]))
}

/// The G-code of one mat (0-based) as it would be exported, for the read-only G-code view.
#[must_use]
pub fn gcode_for_mat(state: &AppState, index: usize) -> Option<String> {
    let compiled = state.fresh_compile()?;
    let id = job_id_of(state)?;
    let doc = state.document();
    let mat_job = compiled.job.mats.get(index)?;
    let mat = doc.mat(mat_job.mat)?;
    let label = mat_label(mat);
    let ctx = write_context(
        state,
        index,
        compiled.job.mats.len(),
        &label,
        &id,
        Some(mat),
    )?;
    Some(write_mat(mat_job, &ctx).text)
}

/// Writes the current job into `dir` as `<stem>.psjob` and/or `<stem>-matNN.gcode`.
///
/// # Errors
///
/// [`ExportError`] when there is no fresh job, the folder is missing, validation refuses the
/// job, or a file cannot be written. Nothing is written in the first three cases.
pub fn export_job(
    state: &AppState,
    dir: &Path,
    stem: &str,
    format: ExportFormat,
    export_time: &str,
) -> Result<Vec<PathBuf>, ExportError> {
    let compiled = state.fresh_compile().ok_or(ExportError::NoJob)?;
    let profile = state.profile().ok_or(ExportError::NoJob)?;
    let id = job_id_of(state).ok_or(ExportError::NoJob)?;
    if !dir.is_dir() {
        return Err(ExportError::Folder(dir.display().to_string()));
    }
    let doc = state.document();
    let labels: Vec<String> = compiled
        .job
        .mats
        .iter()
        .map(|m| {
            doc.mat(m.mat)
                .map_or_else(String::new, |mat| mat_label(mat))
        })
        .collect();
    let mats: Vec<BundleMat<'_>> = compiled
        .job
        .mats
        .iter()
        .zip(&labels)
        .map(|(m, label)| {
            let mat = doc.mat(m.mat);
            BundleMat {
                job: m,
                name: mat.map_or("Mat", |x| x.name.as_str()),
                label,
                material: mat.and_then(|x| x.material.as_ref()),
            }
        })
        .collect();
    let input = BundleInput {
        job_id: &id,
        app_version: env!("CARGO_PKG_VERSION"),
        export_time,
        profile,
        mats: &mats,
    };
    let refuse = |e: &dyn std::fmt::Display| ExportError::Refused(e.to_string());
    let bundle = if matches!(format, ExportFormat::Psjob | ExportFormat::Both) {
        Some(write_bundle(&input).map_err(|e| refuse(&e))?)
    } else {
        None
    };
    let mut written = Vec::new();
    if matches!(format, ExportFormat::Gcode | ExportFormat::Both) {
        written.extend(export_gcode(dir, stem, &input).map_err(|e| refuse(&e))?);
    }
    if let Some(bytes) = bundle {
        let path = dir.join(format!("{stem}.psjob"));
        save_bytes(&path, &bytes, SaveOptions { keep_backup: false }).map_err(|e| refuse(&e))?;
        written.push(path);
    }
    Ok(written)
}

/// How a G-code line is coloured in the read-only view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineClass {
    /// A comment.
    Comment,
    /// A rapid move.
    Travel,
    /// A drawn line or arc.
    Draw,
    /// A tool change, lower or raise.
    Tool,
    /// A pause for the user.
    Pause,
    /// Anything else.
    Other,
}

/// Classifies one line of G-code for syntax colouring.
#[must_use]
pub fn classify_gcode_line(line: &str) -> LineClass {
    let t = line.trim_start();
    let word = t.split_whitespace().next().unwrap_or("");
    if t.starts_with(';') || t.starts_with('(') {
        LineClass::Comment
    } else if word == "G0" {
        LineClass::Travel
    } else if matches!(word, "G1" | "G2" | "G3") {
        LineClass::Draw
    } else if word == "M0" {
        LineClass::Pause
    } else if matches!(word, "M3" | "M5") || (word.starts_with('T') && t.contains("M6")) {
        LineClass::Tool
    } else {
        LineClass::Other
    }
}

#[cfg(test)]
mod tests;
