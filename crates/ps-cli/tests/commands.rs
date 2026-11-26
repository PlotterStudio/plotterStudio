//! Ground truth for the `pstudio` command line: `export`, `validate` and
//! `inspect` run the same code as the app. Exit codes: 0 success, 1 the work was refused (the
//! design or a file is invalid), 2 usage or file-system problems. A refused export writes nothing.

use std::fs;
use std::path::Path;

use ps_core::{AssetId, MatId, ObjectId, PathKey};
use ps_geometry::{Affine, FillRule, Path as GeoPath};
use ps_job::Bundle;
use ps_model::{
    Document, Mat, MatSize, OpKind, Operation, ParamOverrides, PlacedObject, Project, ProjectMeta,
    ShapePath, SourceStyle, ToolSlotRef,
};
use std::sync::{Arc, OnceLock};

const MACHINE: &str = "generic.cutter-small";
const TIME: &str = "2026-01-01T00:00:00Z";

struct Outcome {
    code: i32,
    out: String,
    err: String,
}

fn pstudio(args: &[&str]) -> Outcome {
    let mut command: Vec<String> = vec!["pstudio".to_owned()];
    command.extend(args.iter().map(|a| (*a).to_owned()));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = {
        let mut io = ps_cli::Io {
            out: &mut out,
            err: &mut err,
            now_unix_secs: 1_700_000_000,
        };
        ps_cli::run(&command, &mut io)
    };
    Outcome {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

fn p(dir: &Path, name: &str) -> String {
    dir.join(name).to_string_lossy().into_owned()
}

const RULES: &str = r##"[
  {"match": {"stroke": "#FF0000"}, "assign": {"op": "cut", "tool": "generic.drag-blade"}},
  {"match": {"stroke": "#0000FF"}, "assign": {"op": "draw", "tool": "pen.fineliner-black-0.4"}}
]"##;

fn square_svg(x: f64, stroke: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200mm\" height=\"200mm\" viewBox=\"0 0 200 200\">\
         <rect id=\"r1\" x=\"{x}\" y=\"10\" width=\"40\" height=\"30\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"0.5\"/></svg>"
    )
}

fn setup(svg: &str) -> (tempfile::TempDir, String, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let input = p(dir.path(), "card.svg");
    fs::write(&input, svg).unwrap();
    let rules = p(dir.path(), "rules.json");
    fs::write(&rules, RULES).unwrap();
    let out = p(dir.path(), "out");
    fs::create_dir(&out).unwrap();
    (dir, input, rules, out)
}

fn export_args<'a>(
    input: &'a str,
    rules: &'a str,
    out: &'a str,
    extra: &[&'a str],
) -> Vec<&'a str> {
    let mut a = vec![
        "export",
        input,
        "--machine",
        MACHINE,
        "--rules",
        rules,
        "--out",
        out,
        "--export-time",
        TIME,
    ];
    a.extend_from_slice(extra);
    a
}

fn files(dir: &str) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

// ---- the tool itself ----

#[test]
fn the_version_flag_prints_the_version() {
    let r = pstudio(&["--version"]);
    assert_eq!(r.code, 0);
    assert!(r.out.contains(env!("CARGO_PKG_VERSION")), "{}", r.out);
}

#[test]
fn help_lists_the_commands() {
    let r = pstudio(&["--help"]);
    assert_eq!(r.code, 0);
    for cmd in ["export", "validate", "inspect"] {
        assert!(r.out.contains(cmd), "{}", r.out);
    }
}

#[test]
fn unknown_options_and_commands_are_refused_as_usage_errors() {
    for args in [
        &["frobnicate"][..],
        &["export", "x.svg", "--no-such-flag"],
        &[],
    ] {
        let r = pstudio(args);
        assert_eq!(r.code, 2, "{args:?}: {}", r.err);
        assert_ne!(r.err.len(), 0);
    }
}

// ---- export ----

#[test]
fn exporting_an_svg_writes_a_bundle_that_opens() {
    let (_d, input, rules, out) = setup(&square_svg(10.0, "#ff0000"));
    let r = pstudio(&export_args(&input, &rules, &out, &[]));
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(files(&out), vec!["card.psjob"]);
    let bundle =
        Bundle::from_bytes(&fs::read(Path::new(&out).join("card.psjob")).unwrap()).unwrap();
    assert_eq!(bundle.manifest.mats.len(), 1);
    assert_eq!(bundle.manifest.export_time, TIME);
    assert_eq!(
        bundle.manifest.profile.to_string(),
        "generic.cutter-small@1.0.0"
    );
    let tools: Vec<&str> = bundle.manifest.mats[0]
        .tools
        .iter()
        .map(|t| t.tool.id())
        .collect();
    assert_eq!(tools, vec!["generic.drag-blade"]);
    assert!(r.out.contains("card.psjob"), "{}", r.out);
}

#[test]
fn the_gcode_format_writes_one_file_per_mat() {
    let (_d, input, rules, out) = setup(&square_svg(10.0, "#ff0000"));
    let r = pstudio(&export_args(&input, &rules, &out, &["--format", "gcode"]));
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(files(&out), vec!["card-01.gcode"]);
    let text = fs::read_to_string(Path::new(&out).join("card-01.gcode")).unwrap();
    assert!(text.starts_with("; plotter-studio "));
    assert!(text.contains("T1 M6") && text.ends_with("M2\n"));
}

#[test]
fn the_both_format_writes_both() {
    let (_d, input, rules, out) = setup(&square_svg(10.0, "#ff0000"));
    let r = pstudio(&export_args(&input, &rules, &out, &["--format", "both"]));
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(files(&out), vec!["card-01.gcode", "card.psjob"]);
}

#[test]
fn exporting_twice_gives_identical_files_and_only_the_time_differs_with_it() {
    let (_d, input, rules, out) = setup(&square_svg(10.0, "#ff0000"));
    pstudio(&export_args(&input, &rules, &out, &[]));
    let first = fs::read(Path::new(&out).join("card.psjob")).unwrap();
    pstudio(&export_args(&input, &rules, &out, &[]));
    assert_eq!(fs::read(Path::new(&out).join("card.psjob")).unwrap(), first);

    let later = pstudio(&[
        "export",
        &input,
        "--machine",
        MACHINE,
        "--rules",
        &rules,
        "--out",
        &out,
        "--export-time",
        "2027-02-02T00:00:00Z",
    ]);
    assert_eq!(later.code, 0);
    let second = fs::read(Path::new(&out).join("card.psjob")).unwrap();
    assert_ne!(second, first);
    let (a, b) = (
        Bundle::from_bytes(&first).unwrap(),
        Bundle::from_bytes(&second).unwrap(),
    );
    assert_eq!(
        a.gcode(1),
        b.gcode(1),
        "the G-code does not depend on the export time"
    );
    assert_eq!(
        a.manifest.job_id, b.manifest.job_id,
        "the job id comes from the inputs"
    );
}

#[test]
fn without_a_time_the_clock_supplies_one() {
    let (_d, input, rules, out) = setup(&square_svg(10.0, "#ff0000"));
    let r = pstudio(&[
        "export",
        &input,
        "--machine",
        MACHINE,
        "--rules",
        &rules,
        "--out",
        &out,
    ]);
    assert_eq!(r.code, 0, "{}", r.err);
    let bundle =
        Bundle::from_bytes(&fs::read(Path::new(&out).join("card.psjob")).unwrap()).unwrap();
    assert_eq!(bundle.manifest.export_time, "2023-11-14T22:13:20Z");
}

fn sample_project() -> Vec<u8> {
    let blade: ps_core::LibRef = "generic.drag-blade@1.0.0".parse().unwrap();
    let object = PlacedObject {
        id: ObjectId::new(),
        source: None,
        transform: Affine::IDENTITY,
        group: None,
        attach: None,
        paths: Arc::new(vec![Arc::new(ShapePath {
            key: PathKey::source("p"),
            geom: GeoPath::rect(10.0, 10.0, 40.0, 30.0),
            fill_rule: FillRule::NonZero,
            style: SourceStyle::default(),
            ops: vec![Operation {
                id: ps_core::OpId::new(),
                kind: OpKind::Cut,
                tool: ToolSlotRef {
                    tool: blade,
                    pen_colour: None,
                },
                overrides: ParamOverrides::default(),
                fill: None,
                enabled: true,
                row: ps_core::OpRowId::new(),
            }],
            start_locked: false,
        })]),
        locked: false,
        hidden: false,
    };
    let mut mat = Mat::new(MatId::new(), "Mat 1", MatSize::inches(12.0, 12.0));
    Arc::make_mut(&mut mat.objects).insert(object.id, Arc::new(object));
    let mut doc = Document::new(ProjectMeta::new("Card", "2026-01-01T00:00:00Z", "0.1.0"));
    Arc::make_mut(&mut doc.mats).insert(mat.id, Arc::new(mat));
    let svg = ps_model::Asset::new(AssetId::new(), "art.svg", b"<svg/>".to_vec());
    Arc::make_mut(&mut doc.assets).insert(svg.id, Arc::new(svg));
    Project::new(doc).to_bytes().unwrap()
}

#[test]
fn a_project_file_exports_without_import_or_rules() {
    let dir = tempfile::tempdir().unwrap();
    let input = p(dir.path(), "card.pstudio");
    fs::write(&input, sample_project()).unwrap();
    let out = p(dir.path(), "out");
    fs::create_dir(&out).unwrap();
    let r = pstudio(&[
        "export",
        &input,
        "--machine",
        MACHINE,
        "--out",
        &out,
        "--export-time",
        TIME,
    ]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(files(&out), vec!["card.psjob"]);
}

#[test]
fn a_machine_is_required_and_must_exist() {
    let (_d, input, rules, out) = setup(&square_svg(10.0, "#ff0000"));
    let missing = pstudio(&["export", &input, "--rules", &rules, "--out", &out]);
    assert_eq!(missing.code, 2);
    let unknown = pstudio(&[
        "export",
        &input,
        "--machine",
        "no.such",
        "--rules",
        &rules,
        "--out",
        &out,
    ]);
    assert_eq!(unknown.code, 2);
    assert!(
        unknown.err.contains("no.such") && unknown.err.contains(MACHINE),
        "{}",
        unknown.err
    );
    assert_eq!(files(&out), Vec::<String>::new());
}

#[test]
fn a_design_outside_the_work_area_is_refused_and_nothing_is_written() {
    // The Air 2's work area is 292 mm wide; this square runs to x = 170 + 40 = 210, so move it out.
    let (_d, input, rules, out) = setup(
        &square_svg(280.0, "#ff0000")
            .replace("200mm", "400mm")
            .replace("0 0 200 200", "0 0 400 400"),
    );
    let r = pstudio(&export_args(&input, &rules, &out, &["--format", "both"]));
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.err.contains("diag-move-out-of-bounds"), "{}", r.err);
    assert_eq!(
        files(&out),
        Vec::<String>::new(),
        "a refused export leaves nothing behind"
    );
}

#[test]
fn paths_no_rule_matches_leave_nothing_to_export() {
    let (_d, input, rules, out) = setup(&square_svg(10.0, "#00ff00"));
    let r = pstudio(&export_args(&input, &rules, &out, &[]));
    assert_eq!(r.code, 1);
    assert!(r.err.contains("diag-import-unmapped"), "{}", r.err);
    assert!(r.err.contains("nothing to export"), "{}", r.err);
    assert_eq!(files(&out), Vec::<String>::new());
}

#[test]
fn importing_needs_rules() {
    let (_d, input, _rules, out) = setup(&square_svg(10.0, "#ff0000"));
    let r = pstudio(&["export", &input, "--machine", MACHINE, "--out", &out]);
    assert_eq!(r.code, 2);
    assert!(r.err.contains("--rules"), "{}", r.err);
}

#[test]
fn missing_and_unsupported_inputs_are_usage_errors() {
    let dir = tempfile::tempdir().unwrap();
    let missing = p(dir.path(), "nope.svg");
    let r = pstudio(&["export", &missing, "--machine", MACHINE]);
    assert_eq!(r.code, 2);
    assert!(r.err.contains("nope.svg"), "{}", r.err);
    let odd = p(dir.path(), "a.xyz");
    fs::write(&odd, "x").unwrap();
    assert_eq!(pstudio(&["export", &odd, "--machine", MACHINE]).code, 2);
}

#[test]
fn a_calibration_file_changes_the_toolpath() {
    let (d, input, rules, out) = setup(&square_svg(10.0, "#ff0000"));
    pstudio(&export_args(&input, &rules, &out, &["--format", "gcode"]));
    let plain = fs::read_to_string(Path::new(&out).join("card-01.gcode")).unwrap();
    let cal = p(d.path(), "cal.json");
    fs::write(&cal, r#"{"blade_offsets_mm": {"generic.drag-blade": 0.5}}"#).unwrap();
    pstudio(&export_args(
        &input,
        &rules,
        &out,
        &["--format", "gcode", "--calibration", &cal],
    ));
    let calibrated = fs::read_to_string(Path::new(&out).join("card-01.gcode")).unwrap();
    assert_ne!(plain, calibrated);
}

// ---- validate ----

#[test]
fn a_good_project_validates() {
    let dir = tempfile::tempdir().unwrap();
    let input = p(dir.path(), "card.pstudio");
    fs::write(&input, sample_project()).unwrap();
    let r = pstudio(&["validate", &input, "--machine", MACHINE]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    assert!(r.out.contains("0 errors"), "{}", r.out);
}

#[test]
fn validation_reports_problems_with_their_keys() {
    let dir = tempfile::tempdir().unwrap();
    let input = p(dir.path(), "card.svg");
    fs::write(&input, square_svg(10.0, "#ff0000")).unwrap();
    // Rules that assign a tool the library does not have: the path is left without an operation,
    // which is reported as a warning.
    let rules = p(dir.path(), "rules.json");
    fs::write(
        &rules,
        r#"[{"match": {}, "assign": {"op": "cut", "tool": "no.such-tool"}}]"#,
    )
    .unwrap();
    let r = pstudio(&["validate", &input, "--machine", MACHINE, "--rules", &rules]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    assert!(r.out.contains("diag-import-unknown-tool"), "{}", r.out);
    assert!(r.out.contains("diag-missing-operation"), "{}", r.out);
    let strict = pstudio(&[
        "validate",
        &input,
        "--machine",
        MACHINE,
        "--rules",
        &rules,
        "--deny-warnings",
    ]);
    assert_eq!(strict.code, 1);
}

#[test]
fn warnings_only_fail_validation_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let input = p(dir.path(), "card.pstudio");
    // A path with no operation draws a "missing operation" warning.
    let mut project = Project::from_bytes(&sample_project(), &semver::Version::new(9, 9, 9))
        .unwrap()
        .project;
    let mat_id = *project.document.mats.keys().next().unwrap();
    let mut mat = (**project.document.mats.get(&mat_id).unwrap()).clone();
    let oid = *mat.objects.keys().next().unwrap();
    let mut obj = (**mat.objects.get(&oid).unwrap()).clone();
    let mut shape = (*obj.paths[0]).clone();
    shape.ops.clear();
    obj.paths = Arc::new(vec![Arc::new(shape)]);
    Arc::make_mut(&mut mat.objects).insert(oid, Arc::new(obj));
    Arc::make_mut(&mut project.document.mats).insert(mat_id, Arc::new(mat));
    fs::write(&input, project.to_bytes().unwrap()).unwrap();

    let lenient = pstudio(&["validate", &input, "--machine", MACHINE]);
    assert_eq!(lenient.code, 0, "{}{}", lenient.out, lenient.err);
    assert!(
        lenient.out.contains("diag-missing-operation"),
        "{}",
        lenient.out
    );
    let strict = pstudio(&["validate", &input, "--machine", MACHINE, "--deny-warnings"]);
    assert_eq!(strict.code, 1);
}

#[test]
fn the_json_flag_prints_machine_readable_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let input = p(dir.path(), "card.pstudio");
    fs::write(&input, sample_project()).unwrap();
    let r = pstudio(&["validate", &input, "--machine", MACHINE, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap();
    assert!(v["diagnostics"].is_array());
    assert_eq!(v["errors"], 0);
}

#[test]
fn library_files_are_validated_against_their_schema() {
    let dir = tempfile::tempdir().unwrap();
    let good = p(dir.path(), "tool.json");
    fs::write(
        &good,
        r##"{"schema_version":1,"version":"1.0.0","license":"CC0-1.0","id":"my.pen","name":"My pen","family":"pen","drive":"fixed_point","geometry":{"tip_width_mm":0.4},"operations":["draw"],"holders":["generic.holder-a"],"parameters":{"force":{"type":"number","unit":"gf","min":20,"max":300,"default":120}},"render":{"effect":"draw"},"pen":{"colour":{"hex":"#231F20","name":"Black"},"tip_type":"fibre"}}"##,
    )
    .unwrap();
    let ok = pstudio(&["validate", &good]);
    assert_eq!(ok.code, 0, "{}{}", ok.out, ok.err);

    let bad = p(dir.path(), "bad.json");
    fs::write(&bad, r#"{"schema_version":1,"family":"pen"}"#).unwrap();
    let r = pstudio(&["validate", &bad]);
    assert_eq!(r.code, 1);
    assert!(r.out.contains("bad.json") || r.err.contains("bad.json"));
}

#[test]
fn a_bundle_validates_and_a_tampered_one_does_not() {
    let (d, input, rules, out) = setup(&square_svg(10.0, "#ff0000"));
    pstudio(&export_args(&input, &rules, &out, &[]));
    let bundle = p(Path::new(&out), "card.psjob");
    assert_eq!(pstudio(&["validate", &bundle]).code, 0);

    // Change the G-code but leave the manifest, whose sha256 no longer matches.
    let bad = p(d.path(), "bad.psjob");
    fs::write(
        &bad,
        replace_entry(&fs::read(&bundle).unwrap(), "mats/01.gcode", b"G0 X0 Y0\n"),
    )
    .unwrap();
    let r = pstudio(&["validate", &bad]);
    assert_eq!(r.code, 1, "{}{}", r.out, r.err);
    assert!(
        r.out.contains("modified") || r.out.contains("sha256"),
        "{}",
        r.out
    );
}

/// Rebuilds a zip with one entry's content replaced.
fn replace_entry(zip_bytes: &[u8], name: &str, content: &[u8]) -> Vec<u8> {
    use std::io::{Cursor, Read, Write};
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let entry_name = entry.name().unwrap().into_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        out.start_file(entry_name.as_str(), options).unwrap();
        out.write_all(if entry_name == name { content } else { &data })
            .unwrap();
    }
    out.finish().unwrap().into_inner()
}

// ---- inspect ----

#[test]
fn inspecting_a_project_summarises_it() {
    let dir = tempfile::tempdir().unwrap();
    let input = p(dir.path(), "card.pstudio");
    fs::write(&input, sample_project()).unwrap();
    let r = pstudio(&["inspect", &input]);
    assert_eq!(r.code, 0, "{}", r.err);
    for needle in [
        "Card",
        "mats: 1",
        "objects: 1",
        "assets: 1",
        "generic.drag-blade",
    ] {
        assert!(r.out.contains(needle), "{needle} not in {}", r.out);
    }
    let j = pstudio(&["inspect", &input, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&j.out).unwrap();
    assert_eq!(v["name"], "Card");
    assert_eq!(v["mats"].as_array().unwrap().len(), 1);
}

#[test]
fn inspecting_a_bundle_lists_its_tools_and_estimates() {
    let (_d, input, rules, out) = setup(&square_svg(10.0, "#ff0000"));
    pstudio(&export_args(&input, &rules, &out, &[]));
    let bundle = p(Path::new(&out), "card.psjob");
    let r = pstudio(&["inspect", &bundle]);
    assert_eq!(r.code, 0, "{}", r.err);
    for needle in [
        "generic.cutter-small",
        "mat 1",
        "generic.drag-blade",
        "estimate",
    ] {
        assert!(r.out.contains(needle), "{needle} not in {}", r.out);
    }
    let v: serde_json::Value =
        serde_json::from_str(&pstudio(&["inspect", &bundle, "--json"]).out).unwrap();
    assert_eq!(v["mats"].as_array().unwrap().len(), 1);
    assert_eq!(v["export_time"], TIME);
}

#[test]
fn inspecting_something_unreadable_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let bad = p(dir.path(), "junk.pstudio");
    fs::write(&bad, "not a zip").unwrap();
    assert_eq!(pstudio(&["inspect", &bad]).code, 1);
    assert_eq!(
        pstudio(&["inspect", &p(dir.path(), "missing.pstudio")]).code,
        2
    );
}

// ---- the real binary ----

#[test]
fn the_installed_binary_runs_and_sets_its_exit_code() {
    let exe = env!("CARGO_BIN_EXE_pstudio");
    let version = std::process::Command::new(exe)
        .arg("--version")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).contains(env!("CARGO_PKG_VERSION")));
    let bad = std::process::Command::new(exe)
        .arg("frobnicate")
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
    assert_ne!(bad.stderr.len(), 0);
}

// ---- devices and run ----

fn exported_job() -> (tempfile::TempDir, String) {
    let (dir, input, rules, out) = setup(&square_svg(10.0, "#FF0000"));
    let r = pstudio(&export_args(&input, &rules, &out, &[]));
    assert_eq!(r.code, 0, "{}", r.err);
    let job = p(Path::new(&out), &files(&out)[0]);
    (dir, job)
}

#[test]
fn devices_lists_the_simulated_machine_when_asked() {
    let r = pstudio(&["devices", "--simulate"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("sim"), "{}", r.out);
    assert!(r.out.contains("Simulated"), "{}", r.out);
}

#[test]
fn devices_says_so_when_there_is_nothing_to_list() {
    let r = pstudio(&["devices"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("no devices"), "{}", r.out);
}

#[test]
fn devices_can_print_json() {
    let r = pstudio(&["devices", "--simulate", "--json"]);
    assert_eq!(r.code, 0, "{}", r.err);
    let v: serde_json::Value = serde_json::from_str(r.out.trim()).unwrap();
    assert_eq!(v["devices"][0]["id"], "sim");
}

#[test]
fn run_sends_a_bundle_to_the_simulated_machine_and_reports_completion() {
    let (_dir, job) = exported_job();
    let r = pstudio(&["run", &job, "--device", "sim", "--yes"]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    assert!(r.out.contains("completed"), "{}", r.out);
}

#[test]
fn run_stops_at_a_prompt_unless_told_to_confirm_prompts() {
    let (_dir, job) = exported_job();
    let r = pstudio(&["run", &job, "--device", "sim"]);
    // The job may or may not contain a pause; when it does the run is cancelled with exit 1.
    assert!(r.code == 0 || r.code == 1, "{}", r.err);
    if r.code == 1 {
        assert!(
            r.err.contains("--yes") || r.out.contains("cancelled"),
            "{}{}",
            r.out,
            r.err
        );
    }
}

#[test]
fn run_refuses_a_device_that_does_not_exist() {
    let (_dir, job) = exported_job();
    let r = pstudio(&["run", &job, "--device", "usb:nothing"]);
    assert_eq!(r.code, 2, "{}", r.err);
    assert!(r.err.contains("usb:nothing"), "{}", r.err);
}

#[test]
fn run_refuses_a_file_that_is_not_a_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let bad = p(dir.path(), "x.psjob");
    fs::write(&bad, b"not a zip").unwrap();
    let r = pstudio(&["run", &bad, "--device", "sim"]);
    assert_eq!(r.code, 1, "{}", r.err);
}

#[test]
fn run_can_pick_one_mat() {
    let (_dir, job) = exported_job();
    let r = pstudio(&["run", &job, "--device", "sim", "--mat", "1", "--yes"]);
    assert_eq!(r.code, 0, "{}", r.err);
    let none = pstudio(&["run", &job, "--device", "sim", "--mat", "9", "--yes"]);
    assert_eq!(none.code, 2, "{}", none.err);
}

// ---- device test ----

/// The GRBL component, built and read once for all tests.
///
/// Cargo replaces the output file even when it is up to date, so a test that builds while another
/// reads can find no file.
fn grbl_wasm() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    WASM.get_or_init(build_grbl_wasm)
}

fn build_grbl_wasm() -> Vec<u8> {
    let status = std::process::Command::new(env!("CARGO"))
        .args([
            "build",
            "-p",
            "grbl-plugin",
            "--target",
            "wasm32-wasip2",
            "--release",
        ])
        .status()
        .expect("cargo runs");
    assert!(status.success());
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"),
        std::path::PathBuf::from,
    );
    fs::read(target.join("wasm32-wasip2/release/grbl_plugin.wasm")).unwrap()
}

fn grbl_package(dir: &Path) -> String {
    use ps_device::{DeviceFilter, PluginManifest, TransportKind, pack};
    let manifest = PluginManifest {
        id: "acme.grbl".into(),
        name: "Acme GRBL".into(),
        filters: vec![DeviceFilter {
            transport: TransportKind::Serial,
            usb: None,
        }],
        internet_hosts: Vec::new(),
    };
    let path = dir.join("grbl.psdevice");
    fs::write(&path, pack(&manifest, grbl_wasm(), "1.0.0").unwrap()).unwrap();
    path.to_string_lossy().into_owned()
}

fn recording(dir: &Path, banner: &[u8]) -> String {
    use ps_device::{Direction, Transcript};
    let mut t = Transcript::default();
    t.record(Direction::Out, 0, &[0x18]);
    t.record(Direction::In, 10, banner);
    let path = dir.join("session.json");
    t.save(&path).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn device_test_runs_a_plugin_in_simulation() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = grbl_package(dir.path());
    let r = pstudio(&["device", "test", &plugin, "--simulate"]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    assert!(r.out.contains("passed"), "{}", r.out);
    assert!(r.out.contains("Acme GRBL"), "{}", r.out);
}

#[test]
fn device_test_replays_a_recorded_session() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = grbl_package(dir.path());
    let session = recording(dir.path(), b"\r\nGrbl 1.1h ['$' for help]\r\n");
    let r = pstudio(&["device", "test", &plugin, "--transcript", &session]);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    assert!(r.out.contains("GRBL 1.1h"), "{}", r.out);
}

#[test]
fn device_test_fails_when_the_plugin_does_not_recognise_the_recording() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = grbl_package(dir.path());
    let session = recording(dir.path(), b"Marlin 2.1\n");
    let r = pstudio(&["device", "test", &plugin, "--transcript", &session]);
    assert_eq!(r.code, 1, "{}\n{}", r.out, r.err);
    assert!(r.out.contains("failed"), "{}", r.out);
}

#[test]
fn device_test_needs_something_to_test_against() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = grbl_package(dir.path());
    let r = pstudio(&["device", "test", &plugin]);
    assert_eq!(r.code, 2, "{}", r.err);
}

#[test]
fn device_test_refuses_a_file_that_is_not_a_package() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.psdevice");
    fs::write(&bad, b"nope").unwrap();
    let r = pstudio(&["device", "test", &bad.to_string_lossy(), "--simulate"]);
    assert_eq!(r.code, 1, "{}", r.err);
    let missing = dir.path().join("missing.psdevice");
    let r = pstudio(&["device", "test", &missing.to_string_lossy(), "--simulate"]);
    assert_eq!(r.code, 2, "{}", r.err);
}

// ---- PDF input ----

#[test]
fn a_pdf_exports_like_an_svg() {
    use pdf_writer::{Content, Finish, Pdf, Rect, Ref};
    let (catalog, tree, page, stream) = (Ref::new(1), Ref::new(2), Ref::new(3), Ref::new(4));
    let mut pdf = Pdf::new();
    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree).kids([page]).count(1);
    let mut sheet = pdf.page(page);
    sheet.media_box(Rect::new(0.0, 0.0, 400.0, 400.0));
    sheet.parent(tree);
    sheet.contents(stream);
    sheet.finish();
    let mut c = Content::new();
    c.set_stroke_rgb(1.0, 0.0, 0.0).set_line_width(1.0);
    c.rect(50.0, 50.0, 100.0, 100.0).stroke();
    pdf.stream(stream, &Content::finish(c));
    let (dir, _, rules, out) = setup(&square_svg(10.0, "#FF0000"));
    let input = p(dir.path(), "card.pdf");
    fs::write(&input, pdf.finish()).unwrap();
    let r = pstudio(&export_args(&input, &rules, &out, &["--format", "gcode"]));
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(files(&out), vec!["card-01.gcode"]);
}
