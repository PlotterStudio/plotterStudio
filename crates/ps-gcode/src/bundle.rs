//! Writing `.psjob` bundles and plain G-code files.
//!
//! Every mat is validated against the profile before anything is written. A bundle is a zip with
//! a fixed layout (`manifest.json`, `profile.json`, `mats/NN.gcode`, `previews/NN.svg`) whose
//! manifest lists the sha256 of every other file; the zip uses fixed timestamps and a sorted
//! entry order, so the bytes are deterministic except for `export_time` in the manifest. Plain
//! G-code files are written to a temporary directory and moved into place only when every mat
//! succeeded, so a failed export never leaves partial output.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use ps_core::LibRef;
use ps_job::{Manifest, MatEntry, MatJob, SCHEMA_VERSION, gcode_path, preview_path, sha256_hex};
use ps_library::MachineProfile;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::check::{ValidationError, ensure_valid};
use crate::preview::svg_preview;
use crate::write::{Gcode, WriteContext, write_mat};

/// One mat to export.
#[derive(Debug, Clone, Copy)]
pub struct BundleMat<'a> {
    /// The compiled program.
    pub job: &'a MatJob,
    /// The mat's name, for the manifest.
    pub name: &'a str,
    /// A short description for the G-code header, such as `12x12in standard`.
    pub label: &'a str,
    /// The material, when one is set.
    pub material: Option<&'a LibRef>,
}

/// Everything an export needs.
#[derive(Debug, Clone, Copy)]
pub struct BundleInput<'a> {
    /// The job id.
    pub job_id: &'a str,
    /// The app version.
    pub app_version: &'a str,
    /// When the export happens (RFC 3339); the only non-deterministic input.
    pub export_time: &'a str,
    /// The profile the job was compiled for.
    pub profile: &'a MachineProfile,
    /// The mats, in run order.
    pub mats: &'a [BundleMat<'a>],
}

/// Why an export failed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BundleWriteError {
    /// There was nothing to export.
    #[error("a job needs at least one mat")]
    NoMats,
    /// A mat breaks the machine's limits.
    #[error("mat {mat_index} cannot be exported: {error}")]
    Invalid {
        /// The 1-based mat position.
        mat_index: u32,
        /// What is wrong.
        error: ValidationError,
    },
    /// The profile could not be serialised.
    #[error("could not serialise the profile or manifest: {0}")]
    Serialise(String),
    /// The zip could not be built.
    #[error("could not build the bundle: {0}")]
    Zip(String),
    /// A file or directory operation failed.
    #[error("file error: {0}")]
    Io(String),
}

struct Rendered {
    gcode: Gcode,
    preview: String,
}

fn one_based(i: usize) -> u32 {
    u32::try_from(i + 1).unwrap_or(u32::MAX)
}

fn render(input: &BundleInput<'_>) -> Result<Vec<Rendered>, BundleWriteError> {
    if input.mats.is_empty() {
        return Err(BundleWriteError::NoMats);
    }
    let count = one_based(input.mats.len() - 1);
    let width = ps_core::um_from_mm(input.profile.work_area.width_mm).unwrap_or(0);
    let height = ps_core::um_from_mm(input.profile.work_area.height_mm).unwrap_or(0);
    let mut out = Vec::with_capacity(input.mats.len());
    for (i, mat) in input.mats.iter().enumerate() {
        let mat_index = one_based(i);
        ensure_valid(mat.job, input.profile)
            .map_err(|error| BundleWriteError::Invalid { mat_index, error })?;
        let gcode = write_mat(
            mat.job,
            &WriteContext {
                app_version: input.app_version,
                job_id: input.job_id,
                profile: input.profile,
                mat_index,
                mat_count: count,
                mat_label: mat.label,
                material: mat.material,
            },
        );
        out.push(Rendered {
            gcode,
            preview: svg_preview(mat.job, width, height),
        });
    }
    Ok(out)
}

/// Builds the `.psjob` bundle bytes.
///
/// # Errors
///
/// [`BundleWriteError`] when there are no mats, a mat breaks the machine's limits, or the
/// archive cannot be built. Nothing is produced in that case.
pub fn write_bundle(input: &BundleInput<'_>) -> Result<Vec<u8>, BundleWriteError> {
    let rendered = render(input)?;
    let profile_json = serde_json::to_vec_pretty(input.profile)
        .map_err(|e| BundleWriteError::Serialise(e.to_string()))?;

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    files.insert("profile.json".to_owned(), profile_json);
    let mut mats = Vec::new();
    for (i, (mat, r)) in input.mats.iter().zip(&rendered).enumerate() {
        let index = one_based(i);
        let (gcode, preview) = (gcode_path(index), preview_path(index));
        files.insert(gcode.clone(), r.gcode.text.clone().into_bytes());
        files.insert(preview.clone(), r.preview.clone().into_bytes());
        mats.push(MatEntry {
            index,
            name: mat.name.to_owned(),
            gcode,
            preview: Some(preview),
            material: mat.material.cloned(),
            tools: mat.job.tool_table.clone(),
            pauses: r.gcode.pauses.clone(),
            estimate: mat.job.estimate.clone(),
        });
    }

    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        job_id: input.job_id.to_owned(),
        app_version: input.app_version.to_owned(),
        export_time: input.export_time.to_owned(),
        dialect: input.profile.gcode.dialect.clone(),
        profile: input.profile.lib_ref(),
        mats,
        files: files
            .iter()
            .map(|(path, bytes)| (path.clone(), sha256_hex(bytes)))
            .collect(),
    };
    let mut manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| BundleWriteError::Serialise(e.to_string()))?;
    manifest_json.push(b'\n');

    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    let zip_err = |e: zip::result::ZipError| BundleWriteError::Zip(e.to_string());
    let io_err = |e: std::io::Error| BundleWriteError::Zip(e.to_string());
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("manifest.json", options).map_err(zip_err)?;
    zip.write_all(&manifest_json).map_err(io_err)?;
    for (path, bytes) in &files {
        zip.start_file(path.as_str(), options).map_err(zip_err)?;
        zip.write_all(bytes).map_err(io_err)?;
    }
    Ok(zip.finish().map_err(zip_err)?.into_inner())
}

/// Writes one `{stem}-NN.gcode` file per mat into `dir`.
///
/// Every mat is validated and rendered first; the files are then written to a temporary
/// directory inside `dir` and moved into place, so a failure never leaves partial output or
/// damages existing files.
///
/// # Errors
///
/// [`BundleWriteError`] for an invalid mat or a file-system problem.
pub fn export_gcode(
    dir: &Path,
    stem: &str,
    input: &BundleInput<'_>,
) -> Result<Vec<PathBuf>, BundleWriteError> {
    let rendered = render(input)?;
    let io = |e: std::io::Error| BundleWriteError::Io(e.to_string());
    if !dir.is_dir() {
        return Err(BundleWriteError::Io(format!(
            "{} is not a directory",
            dir.display()
        )));
    }
    let tmp = dir.join(format!(".{stem}.export-tmp"));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(io)?;
    }
    std::fs::create_dir(&tmp).map_err(io)?;

    let result = (|| {
        let mut staged = Vec::new();
        for (i, r) in rendered.iter().enumerate() {
            let name = format!("{stem}-{:02}.gcode", one_based(i));
            let from = tmp.join(&name);
            std::fs::write(&from, r.gcode.text.as_bytes()).map_err(io)?;
            staged.push((from, dir.join(name)));
        }
        let mut finished = Vec::new();
        for (from, to) in staged {
            std::fs::rename(&from, &to).map_err(io)?;
            finished.push(to);
        }
        Ok(finished)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

#[cfg(test)]
mod tests;
