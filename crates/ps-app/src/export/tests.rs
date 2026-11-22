//! Ground truth for exporting from the app: the same validated writers as
//! `pstudio export`. The bundle is built in memory before anything is written, so a refused job
//! leaves no files; the job id is derived from the G-code so identical inputs give identical
//! bundles.

use std::path::Path;
use std::time::Duration;

use ps_geometry::Point;
use ps_job::Bundle;
use ps_library::Settings;
use ps_model::OpKind;

use super::*;
use crate::commands::Mods;
use crate::state::AppState;
use crate::tools::CanvasEvent;

const NONE: Mods = Mods {
    command: false,
    shift: false,
    alt: false,
};
const BLADE: &str = "generic.drag-blade@1.0.0";
const TIME: &str = "2026-01-01T00:00:00Z";

fn compiled_state() -> AppState {
    let mut s = AppState::new(Settings::default());
    s.run_command("tool.rectangle", 0);
    s.canvas_event(CanvasEvent::Press(Point::new(20.0, 20.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(Point::new(80.0, 60.0)), NONE, 0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    s.tick(100);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    s
}

fn is_gcode(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gcode"))
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn exporting_writes_the_bundle_and_one_gcode_file_per_mat() {
    let s = compiled_state();
    let dir = tempfile::tempdir().unwrap();
    let written = export_job(&s, dir.path(), "card", ExportFormat::Both, TIME).unwrap();
    assert_eq!(written.len(), 2);
    let files = names(dir.path());
    assert!(files.contains(&"card.psjob".to_owned()), "{files:?}");
    assert!(files.iter().any(|f| is_gcode(f)), "{files:?}");
}

#[test]
fn the_exported_bundle_opens_and_verifies() {
    let s = compiled_state();
    let dir = tempfile::tempdir().unwrap();
    export_job(&s, dir.path(), "card", ExportFormat::Psjob, TIME).unwrap();
    let bytes = std::fs::read(dir.path().join("card.psjob")).unwrap();
    let bundle = Bundle::from_bytes(&bytes).unwrap();
    assert_eq!(bundle.manifest.mats.len(), 1);
    assert_eq!(bundle.manifest.export_time, TIME);
    let g = bundle.gcode(bundle.manifest.mats[0].index).unwrap();
    assert!(g.contains("G21"), "millimetres: {g}");
    assert!(g.lines().any(|l| l.starts_with("M2")), "ends the program");
}

#[test]
fn format_selects_what_is_written() {
    let s = compiled_state();
    let dir = tempfile::tempdir().unwrap();
    export_job(&s, dir.path(), "a", ExportFormat::Psjob, TIME).unwrap();
    assert_eq!(names(dir.path()), vec!["a.psjob"]);
    let dir = tempfile::tempdir().unwrap();
    export_job(&s, dir.path(), "a", ExportFormat::Gcode, TIME).unwrap();
    let files = names(dir.path());
    assert_eq!(files.len(), 1);
    assert!(is_gcode(&files[0]));
}

#[test]
fn the_same_job_exports_to_identical_bytes() {
    let s = compiled_state();
    let (d1, d2) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    export_job(&s, d1.path(), "x", ExportFormat::Psjob, TIME).unwrap();
    export_job(&s, d2.path(), "x", ExportFormat::Psjob, TIME).unwrap();
    assert_eq!(
        std::fs::read(d1.path().join("x.psjob")).unwrap(),
        std::fs::read(d2.path().join("x.psjob")).unwrap()
    );
}

#[test]
fn the_job_id_comes_from_the_gcode_not_from_random_ids() {
    let a = compiled_state();
    let b = compiled_state();
    assert_eq!(
        job_id_of(&a),
        job_id_of(&b),
        "different object ids, same program"
    );
    assert!(job_id_of(&a).unwrap().starts_with("job-"));
}

#[test]
fn without_a_fresh_compile_nothing_is_written() {
    let mut s = compiled_state();
    s.run_command("nudge.right", 500);
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        export_job(&s, dir.path(), "x", ExportFormat::Both, TIME),
        Err(ExportError::NoJob)
    );
    assert_eq!(names(dir.path()).len(), 0);
}

#[test]
fn a_missing_folder_is_reported_and_nothing_is_written() {
    let s = compiled_state();
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("nope");
    assert!(matches!(
        export_job(&s, &gone, "x", ExportFormat::Both, TIME),
        Err(ExportError::Folder(_))
    ));
}

#[test]
fn the_gcode_view_shows_the_program_of_a_mat() {
    let s = compiled_state();
    let text = gcode_for_mat(&s, 0).unwrap();
    assert!(text.contains("G21"));
    assert!(gcode_for_mat(&s, 5).is_none());
}

#[test]
fn a_mat_label_reads_size_and_grip() {
    let s = AppState::new(Settings::default());
    assert_eq!(mat_label(&s.current_mat()), "12x12in standard");
}

#[test]
fn syntax_colouring_classes_every_gcode_line() {
    use LineClass::*;
    assert_eq!(classify_gcode_line("; comment"), Comment);
    assert_eq!(classify_gcode_line("G0 X1.000 Y2.000"), Travel);
    assert_eq!(classify_gcode_line("G1 X1.000 Y2.000 F6000"), Draw);
    assert_eq!(classify_gcode_line("G3 X1 Y2 I3 J4"), Draw);
    assert_eq!(classify_gcode_line("M3 S150"), Tool);
    assert_eq!(classify_gcode_line("M5"), Tool);
    assert_eq!(classify_gcode_line("T1 M6"), Tool);
    assert_eq!(classify_gcode_line("M0 (Load pen)"), Pause);
    assert_eq!(classify_gcode_line("G4 P0.080"), Other);
    assert_eq!(classify_gcode_line(""), Other);
}
