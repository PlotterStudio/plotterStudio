//! The three commands: export, validate, inspect.

use std::io::Write;
use std::path::{Path, PathBuf};

use ps_compiler::{CompileInput, MatSelection, compile};
use ps_core::{Diagnostics, hash_bytes};
use ps_gcode::{
    BundleInput, BundleMat, BundleWriteError, WriteContext, export_gcode, write_bundle, write_mat,
};
use ps_job::{Bundle, Job};
use ps_library::{Entry, LibraryKind, read_package};
use ps_model::{Document, Mat, Project, SaveOptions, save_bytes};
use semver::Version;
use serde_json::{Value, json};

use crate::context::Context;
use crate::report;
use crate::source::{self, Loaded};
use crate::time::rfc3339_utc;
use crate::{ExportArgs, Failure, Format, InspectArgs, Io, ValidateArgs};

fn read(path: &Path) -> Result<Vec<u8>, Failure> {
    std::fs::read(path).map_err(|e| Failure::usage(format!("cannot read {}: {e}", path.display())))
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| "job".to_owned(), |s| s.to_string_lossy().into_owned())
}

fn app_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0))
}

/// A short description of a mat for the G-code header, such as `12x12in standard`.
fn mat_label(mat: &Mat) -> String {
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

fn compile_document(ctx: &Context, loaded: &Loaded) -> Result<(Job, Diagnostics), Failure> {
    let job = compile(&CompileInput {
        doc: &loaded.document,
        libs: &ctx.libs,
        profile: &ctx.profile,
        calibration: &ctx.calibration,
        mats: MatSelection::All,
    })
    .map_err(|e| Failure::refused(format!("cannot compile {}: {e}", loaded.name)))?;
    let mut all = loaded.diagnostics.clone();
    for d in &job.diagnostics {
        all.push(d.clone());
    }
    all.sort();
    Ok((job, all))
}

// ---- export ----

/// A job id derived from the generated G-code, so identical inputs give identical bundles even
/// when a document's object ids are freshly generated.
fn job_id(ctx: &Context, doc: &Document, job: &Job) -> String {
    let mut text = String::new();
    for (i, mat_job) in job.mats.iter().enumerate() {
        let Some(mat) = doc.mat(mat_job.mat) else {
            continue;
        };
        let g = write_mat(
            mat_job,
            &WriteContext {
                app_version: env!("CARGO_PKG_VERSION"),
                job_id: "",
                profile: &ctx.profile,
                mat_index: u32::try_from(i + 1).unwrap_or(u32::MAX),
                mat_count: u32::try_from(job.mats.len()).unwrap_or(u32::MAX),
                mat_label: &mat_label(mat),
                material: mat.material.as_ref(),
            },
        );
        text.push_str(&g.text);
    }
    let hex = hash_bytes(text.as_bytes()).to_string();
    format!("job-{}", &hex[..16])
}

fn describe(e: &BundleWriteError, err: &mut dyn Write) -> Failure {
    if let BundleWriteError::Invalid { mat_index, error } = e {
        for v in &error.violations {
            let _ = writeln!(err, "error mat {mat_index} step {}: {:?}", v.step, v.kind);
        }
    }
    Failure::refused(format!("export refused: {e}"))
}

pub(crate) fn export(args: &ExportArgs, io: &mut Io<'_>) -> Result<(), Failure> {
    let ctx = Context::from_setup(&args.setup)?;
    let export_time = args
        .export_time
        .clone()
        .unwrap_or_else(|| rfc3339_utc(io.now_unix_secs));
    let loaded = source::load(
        &args.input,
        args.setup.rules.as_deref(),
        &ctx.libs,
        &export_time,
    )?;
    let (job, diagnostics) = compile_document(&ctx, &loaded)?;
    report::print(io.err, &diagnostics);
    let (errors, _) = report::counts(&diagnostics);
    if job.is_blocked() || errors > 0 {
        return Err(Failure::refused(format!(
            "export refused: {errors} error(s) in {}",
            args.input.display()
        )));
    }
    if job.mats.is_empty() {
        return Err(Failure::refused(
            "nothing to export: no operation is assigned to any path",
        ));
    }

    let out_dir = args.out.clone().unwrap_or_else(|| PathBuf::from("."));
    if !out_dir.is_dir() {
        return Err(Failure::usage(format!(
            "{} is not a folder",
            out_dir.display()
        )));
    }
    let doc = &loaded.document;
    let labels: Vec<String> = job
        .mats
        .iter()
        .map(|m| {
            doc.mat(m.mat)
                .map_or_else(String::new, |mat| mat_label(mat))
        })
        .collect();
    let mats: Vec<BundleMat<'_>> = job
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
    let id = job_id(&ctx, doc, &job);
    let input = BundleInput {
        job_id: &id,
        app_version: env!("CARGO_PKG_VERSION"),
        export_time: &export_time,
        profile: &ctx.profile,
        mats: &mats,
    };
    let stem = stem(&args.input);

    // Validate everything and build the bundle in memory before any file is written.
    let bundle = if matches!(args.format, Format::Psjob | Format::Both) {
        Some(write_bundle(&input).map_err(|e| describe(&e, io.err))?)
    } else {
        None
    };
    let mut written: Vec<PathBuf> = Vec::new();
    if matches!(args.format, Format::Gcode | Format::Both) {
        written.extend(export_gcode(&out_dir, &stem, &input).map_err(|e| describe(&e, io.err))?);
    }
    if let Some(bytes) = bundle {
        let path = out_dir.join(format!("{stem}.psjob"));
        save_bytes(&path, &bytes, SaveOptions { keep_backup: false })
            .map_err(|e| Failure::usage(format!("{e}")))?;
        written.push(path);
    }

    let _ = writeln!(
        io.out,
        "exported {} mat(s) from {} for {}",
        job.mats.len(),
        args.input.display(),
        ctx.profile.id
    );
    for path in &written {
        let _ = writeln!(io.out, "  wrote {}", path.display());
    }
    for (i, m) in job.mats.iter().enumerate() {
        let _ = writeln!(
            io.out,
            "  mat {}: {} tool(s), estimate {} s",
            i + 1,
            m.tool_table.len(),
            (m.estimate.machine_time_ms + 500) / 1_000
        );
    }
    Ok(())
}

// ---- validate ----

struct Findings {
    errors: usize,
    warnings: usize,
    lines: Vec<String>,
    diagnostics: Option<Diagnostics>,
}

fn validate_library_file(path: &Path, bytes: &[u8]) -> Result<Findings, Failure> {
    let text = String::from_utf8_lossy(bytes);
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| Failure::refused(format!("{}: not valid JSON: {e}", path.display())))?;
    let kind = if value.get("family").is_some() {
        LibraryKind::Tool
    } else if value.get("presets").is_some() {
        LibraryKind::Material
    } else if value.get("work_area").is_some() {
        LibraryKind::Profile
    } else {
        return Err(Failure::refused(format!(
            "{}: cannot tell whether this is a tool, material or machine profile",
            path.display()
        )));
    };
    let issues = match Entry::from_json(kind, &text) {
        Ok(_) => Vec::new(),
        Err(issues) => issues,
    };
    let lines = issues
        .iter()
        .map(|i| format!("{}: {}: {}", path.display(), i.pointer, i.message))
        .collect();
    Ok(Findings {
        errors: issues.len(),
        warnings: 0,
        lines,
        diagnostics: None,
    })
}

fn validate_package(path: &Path, bytes: &[u8]) -> Result<Findings, Failure> {
    let report =
        read_package(bytes).map_err(|e| Failure::refused(format!("{}: {e}", path.display())))?;
    let lines: Vec<String> = report
        .issues
        .iter()
        .map(|(file, i)| format!("{file}: {}: {}", i.pointer, i.message))
        .collect();
    Ok(Findings {
        errors: report.issues.len(),
        warnings: 0,
        lines,
        diagnostics: None,
    })
}

fn validate_bundle(path: &Path, bytes: &[u8]) -> Findings {
    match Bundle::from_bytes(bytes) {
        Ok(_) => Findings {
            errors: 0,
            warnings: 0,
            lines: Vec::new(),
            diagnostics: None,
        },
        Err(e) => Findings {
            errors: 1,
            warnings: 0,
            lines: vec![format!("{}: {e}", path.display())],
            diagnostics: None,
        },
    }
}

fn validate_design(args: &ValidateArgs, io: &Io<'_>) -> Result<Findings, Failure> {
    let ctx = Context::from_setup(&args.setup)?;
    let created = rfc3339_utc(io.now_unix_secs);
    let loaded = source::load(
        &args.input,
        args.setup.rules.as_deref(),
        &ctx.libs,
        &created,
    )?;
    let (_, diagnostics) = compile_document(&ctx, &loaded)?;
    let (errors, warnings) = report::counts(&diagnostics);
    Ok(Findings {
        errors,
        warnings,
        lines: diagnostics.iter().map(report::line).collect(),
        diagnostics: Some(diagnostics),
    })
}

pub(crate) fn validate(args: &ValidateArgs, io: &mut Io<'_>) -> Result<(), Failure> {
    let ext = args
        .input
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let findings = match ext.as_str() {
        "json" => validate_library_file(&args.input, &read(&args.input)?)?,
        "zip" => validate_package(&args.input, &read(&args.input)?)?,
        "psjob" => validate_bundle(&args.input, &read(&args.input)?),
        _ => validate_design(args, io)?,
    };

    if args.json {
        let diagnostics = findings.diagnostics.as_ref().map_or_else(
            || Value::Array(findings.lines.iter().map(|l| json!(l)).collect()),
            report::to_json,
        );
        let doc = json!({
            "input": args.input.display().to_string(),
            "errors": findings.errors,
            "warnings": findings.warnings,
            "diagnostics": diagnostics,
        });
        let _ = writeln!(io.out, "{doc}");
    } else {
        for line in &findings.lines {
            let _ = writeln!(io.out, "{line}");
        }
        let _ = writeln!(
            io.out,
            "{}: {} errors, {} warnings",
            args.input.display(),
            findings.errors,
            findings.warnings
        );
    }
    if findings.errors > 0 || (args.deny_warnings && findings.warnings > 0) {
        return Err(Failure::refused(format!(
            "validation failed: {} errors, {} warnings",
            findings.errors, findings.warnings
        )));
    }
    Ok(())
}

// ---- inspect ----

fn tools_used(doc: &Document) -> Vec<String> {
    let mut ids: Vec<String> = doc
        .mats
        .values()
        .flat_map(|m| m.objects.values())
        .flat_map(|o| o.paths.iter())
        .flat_map(|p| p.ops.iter())
        .map(|op| op.tool.tool.id().to_owned())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

fn inspect_project(
    path: &Path,
    bytes: &[u8],
    json_out: bool,
    io: &mut Io<'_>,
) -> Result<(), Failure> {
    let loaded = Project::from_bytes(bytes, &app_version())
        .map_err(|e| Failure::refused(format!("{}: {e}", path.display())))?;
    let doc = &loaded.project.document;
    let tools = tools_used(doc);
    let objects: usize = doc.mats.values().map(|m| m.objects.len()).sum();
    if json_out {
        let mats: Vec<Value> = doc
            .mats
            .values()
            .map(|m| {
                json!({
                    "name": m.name,
                    "width_mm": m.size.width.get(),
                    "height_mm": m.size.height.get(),
                    "objects": m.objects.len(),
                })
            })
            .collect();
        let v = json!({
            "name": doc.meta.name,
            "template": loaded.project.template,
            "read_only": loaded.read_only.is_some(),
            "mats": mats,
            "objects": objects,
            "assets": doc.assets.len(),
            "tools": tools,
        });
        let _ = writeln!(io.out, "{v}");
        return Ok(());
    }
    let _ = writeln!(io.out, "{}", doc.meta.name);
    if loaded.read_only.is_some() {
        let _ = writeln!(io.out, "read only: written by a newer version");
    }
    let _ = writeln!(io.out, "mats: {}", doc.mats.len());
    let _ = writeln!(io.out, "objects: {objects}");
    let _ = writeln!(io.out, "assets: {}", doc.assets.len());
    let _ = writeln!(io.out, "tools: {}", tools.join(", "));
    for (i, m) in doc.mats.values().enumerate() {
        let _ = writeln!(
            io.out,
            "  mat {} {:?}: {} x {} mm, {} object(s)",
            i + 1,
            m.name,
            m.size.width.get(),
            m.size.height.get(),
            m.objects.len()
        );
    }
    Ok(())
}

fn inspect_bundle(
    path: &Path,
    bytes: &[u8],
    json_out: bool,
    io: &mut Io<'_>,
) -> Result<(), Failure> {
    let bundle = Bundle::from_bytes(bytes)
        .map_err(|e| Failure::refused(format!("{}: {e}", path.display())))?;
    let m = &bundle.manifest;
    if json_out {
        let mats: Vec<Value> = m
            .mats
            .iter()
            .map(|mat| {
                json!({
                    "index": mat.index,
                    "name": mat.name,
                    "gcode": mat.gcode,
                    "tools": mat.tools.iter().map(|t| t.tool.id()).collect::<Vec<_>>(),
                    "machine_time_ms": mat.estimate.machine_time_ms,
                    "with_changes_ms": mat.estimate.with_changes_ms,
                    "tool_changes": mat.estimate.tool_changes,
                    "pauses": mat.pauses.len(),
                })
            })
            .collect();
        let v = json!({
            "job_id": m.job_id,
            "app_version": m.app_version,
            "export_time": m.export_time,
            "dialect": m.dialect,
            "profile": m.profile.to_string(),
            "mats": mats,
        });
        let _ = writeln!(io.out, "{v}");
        return Ok(());
    }
    let _ = writeln!(io.out, "job: {}", m.job_id);
    let _ = writeln!(io.out, "profile: {}", m.profile);
    let _ = writeln!(io.out, "exported: {} by {}", m.export_time, m.app_version);
    for mat in &m.mats {
        let _ = writeln!(io.out, "mat {} {:?}: {}", mat.index, mat.name, mat.gcode);
        for t in &mat.tools {
            let _ = writeln!(
                io.out,
                "  T{} {} ({})",
                t.slot,
                t.tool.id(),
                t.holder.as_str()
            );
        }
        let _ = writeln!(
            io.out,
            "  estimate: {} s ({} s with tool changes), {} tool change(s), {} pause(s)",
            (mat.estimate.machine_time_ms + 500) / 1_000,
            (mat.estimate.with_changes_ms + 500) / 1_000,
            mat.estimate.tool_changes,
            mat.pauses.len()
        );
    }
    Ok(())
}

pub(crate) fn inspect(args: &InspectArgs, io: &mut Io<'_>) -> Result<(), Failure> {
    let bytes = read(&args.input)?;
    match args
        .input
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("pstudio") => inspect_project(&args.input, &bytes, args.json, io),
        Some("psjob") => inspect_bundle(&args.input, &bytes, args.json, io),
        _ => Err(Failure::usage(format!(
            "{}: inspect reads .pstudio projects and .psjob bundles",
            args.input.display()
        ))),
    }
}
