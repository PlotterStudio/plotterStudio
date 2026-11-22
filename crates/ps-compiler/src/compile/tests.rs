//! Ground truth for the compiler: document + libraries + profile + calibration in, a job
//! out. Every expected coordinate is hand-computed from the layout and the built-in library
//! (a small cutter, fine-point blade with a 0.25 mm swivel offset and 1 mm default overcut,
//! fineliner pen in clamp A).

use std::sync::Arc;

use ps_core::{MatId, ObjectId, OpId, OpRowId, PathKey, PtUm, Severity};
use ps_geometry::{Affine, FillRule, FillSpec, Path, Pattern};
use ps_library::{Calibration, LibrarySnapshot, MachineProfile, Xy, builtin_entries};
use ps_model::{
    Document, Mat, MatSize, OpKind, Operation, ParamOverrides, PlacedObject, ProjectMeta,
    ShapePath, SourceStyle, ToolSlotRef,
};

use super::*;

const BLADE: &str = "generic.drag-blade@1.0.0";
const PEN: &str = "pen.fineliner-black-0.4@1.0.0";
const MM: i64 = 1_000;

fn libs() -> LibrarySnapshot {
    LibrarySnapshot::from_entries(builtin_entries().unwrap())
}

fn air2() -> MachineProfile {
    let r: ps_core::LibRef = "generic.cutter-small@1.0.0".parse().unwrap();
    (*libs().resolve_profile(&r).unwrap().item).clone()
}

fn op(tool: &str, kind: OpKind, row: u64, id: u64) -> Operation {
    Operation {
        id: OpId::at_unix_ms(id),
        kind,
        tool: ToolSlotRef {
            tool: tool.parse().unwrap(),
            pen_colour: None,
        },
        overrides: ParamOverrides::default(),
        fill: None,
        enabled: true,
        row: OpRowId::at_unix_ms(row),
    }
}

fn path(geom: Path, key: &str, ops: Vec<Operation>) -> Arc<ShapePath> {
    Arc::new(ShapePath {
        key: PathKey::source(key),
        geom,
        fill_rule: FillRule::NonZero,
        style: SourceStyle::default(),
        ops,
        start_locked: false,
    })
}

fn object(id: u64, paths: Vec<Arc<ShapePath>>) -> PlacedObject {
    PlacedObject {
        id: ObjectId::at_unix_ms(id),
        source: None,
        transform: Affine::IDENTITY,
        group: None,
        attach: None,
        paths: Arc::new(paths),
        locked: false,
        hidden: false,
    }
}

fn rect_object(id: u64, x: f64, y: f64, w: f64, h: f64, ops: Vec<Operation>) -> PlacedObject {
    object(id, vec![path(Path::rect(x, y, w, h), "p", ops)])
}

fn mat(id: u64) -> Mat {
    Mat::new(MatId::at_unix_ms(id), "Mat", MatSize::inches(12.0, 12.0))
}

fn doc(mats: Vec<(Mat, Vec<PlacedObject>)>) -> Document {
    let mut d = Document::new(ProjectMeta::new("t", "t", "v"));
    for (mut m, objects) in mats {
        for o in objects {
            Arc::make_mut(&mut m.objects).insert(o.id, Arc::new(o));
        }
        Arc::make_mut(&mut d.mats).insert(m.id, Arc::new(m));
    }
    d
}

fn run(d: &Document) -> Job {
    run_with(d, &Calibration::default())
}

fn run_with(d: &Document, cal: &Calibration) -> Job {
    let l = libs();
    let p = air2();
    compile(&CompileInput {
        doc: d,
        libs: &l,
        profile: &p,
        calibration: cal,
        mats: MatSelection::All,
    })
    .unwrap()
}

fn keys(j: &Job) -> Vec<String> {
    j.diagnostics
        .iter()
        .map(|d| d.key.as_str().to_owned())
        .collect()
}

fn pt(x: f64, y: f64) -> PtUm {
    PtUm::from_mm(x, y).unwrap()
}

fn first_travel(steps: &[Step]) -> PtUm {
    steps
        .iter()
        .find_map(|s| {
            if let Step::Travel { to } = s {
                Some(*to)
            } else {
                None
            }
        })
        .unwrap()
}

fn last_drawn_end(steps: &[Step]) -> PtUm {
    steps
        .iter()
        .rev()
        .find(|s| s.is_drawn())
        .and_then(Step::end_point)
        .unwrap()
}

fn drawn(steps: &[Step]) -> usize {
    steps.iter().filter(|s| s.is_drawn()).count()
}

#[test]
fn an_empty_document_compiles_to_nothing() {
    let j = run(&doc(vec![(mat(1), vec![])]));
    assert_eq!(j.mats.len(), 0);
    assert_eq!(keys(&j), Vec::<String>::new());
    assert!(!j.is_blocked());
}

#[test]
fn a_single_cut_becomes_a_program_for_the_blade() {
    let d = doc(vec![(
        mat(1),
        vec![rect_object(
            10,
            10.0,
            10.0,
            50.0,
            30.0,
            vec![op(BLADE, OpKind::Cut, 1, 100)],
        )],
    )]);
    let j = run(&d);
    assert_eq!(keys(&j), Vec::<String>::new(), "{:?}", j.diagnostics);
    assert_eq!(j.mats.len(), 1);
    let m = &j.mats[0];
    assert_eq!(m.tool_table.len(), 1);
    assert_eq!(m.tool_table[0].slot, 1);
    assert_eq!(m.tool_table[0].holder.as_str(), "generic.holder-b");
    // The job starts by telling the user what to load.
    let Step::Pause(msg) = &m.steps[0] else {
        panic!("{:?}", m.steps[0])
    };
    assert_eq!(
        msg.text(),
        "Load blade: Fine-point blade (45°) into Clamp B"
    );
    assert!(matches!(
        m.steps[1],
        Step::LoadTool {
            slot: 1,
            pause: None,
            ..
        }
    ));
    assert_eq!(
        m.steps
            .iter()
            .filter(|s| matches!(s, Step::ToolDown { .. }))
            .count(),
        1
    );
    assert!(matches!(m.steps.last(), Some(Step::ToolUp)));
}

#[test]
fn drag_compensation_offsets_the_holder_and_overcuts_the_start() {
    let d = doc(vec![(
        mat(1),
        vec![rect_object(
            10,
            10.0,
            10.0,
            50.0,
            30.0,
            vec![op(BLADE, OpKind::Cut, 1, 100)],
        )],
    )]);
    let steps = &run(&d).mats[0].steps;
    // The longest edge is the top one (x 10..60), the tip starts at its middle (35, 10) and the
    // holder leads it by 0.25 mm; the 1 mm overcut ends the tip at (36, 10), the holder at 36.25.
    assert_eq!(first_travel(steps), pt(35.25, 10.0));
    assert_eq!(last_drawn_end(steps), pt(36.25, 10.0));
}

#[test]
fn a_pen_draws_exactly_on_the_path() {
    let d = doc(vec![(
        mat(1),
        vec![rect_object(
            10,
            10.0,
            10.0,
            50.0,
            30.0,
            vec![op(PEN, OpKind::Draw, 1, 100)],
        )],
    )]);
    let m = &run(&d).mats[0];
    assert_eq!(m.tool_table[0].holder.as_str(), "generic.holder-a");
    assert_eq!(first_travel(&m.steps), pt(10.0, 10.0));
}

#[test]
fn drawing_comes_before_cutting_whatever_the_row_order() {
    let o = rect_object(
        10,
        10.0,
        10.0,
        50.0,
        30.0,
        vec![
            op(BLADE, OpKind::Cut, 1, 100),
            op(PEN, OpKind::Draw, 2, 101),
        ],
    );
    let m = &run(&doc(vec![(mat(1), vec![o])])).mats[0];
    let order: Vec<&str> = m.tool_table.iter().map(|t| t.tool.id()).collect();
    assert_eq!(order, vec!["pen.fineliner-black-0.4", "generic.drag-blade"]);
    let loads: Vec<u8> = m
        .steps
        .iter()
        .filter_map(|s| {
            if let Step::LoadTool { slot, .. } = s {
                Some(*slot)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(loads, vec![1, 2]);
}

#[test]
fn a_pen_and_a_blade_in_different_clamps_need_no_mid_job_swap() {
    let o = rect_object(
        10,
        10.0,
        10.0,
        50.0,
        30.0,
        vec![
            op(BLADE, OpKind::Cut, 1, 100),
            op(PEN, OpKind::Draw, 2, 101),
        ],
    );
    let m = &run(&doc(vec![(mat(1), vec![o])])).mats[0];
    assert_eq!(m.estimate.tool_changes, 0);
    let Step::Pause(msg) = &m.steps[0] else {
        panic!()
    };
    assert_eq!(
        msg.text(),
        "Load pen: Fine liner, black 0.4 into Clamp A\nLoad blade: Fine-point blade (45°) into Clamp B"
    );
}

#[test]
fn the_holders_calibrated_offset_shifts_every_move() {
    let d = doc(vec![(
        mat(1),
        vec![rect_object(
            10,
            10.0,
            10.0,
            50.0,
            30.0,
            vec![op(PEN, OpKind::Draw, 1, 100)],
        )],
    )]);
    let mut cal = Calibration::default();
    cal.holder_offsets_mm.insert(
        "generic.holder-a".into(),
        Xy {
            x_mm: 1.0,
            y_mm: -2.0,
        },
    );
    let steps = &run_with(&d, &cal).mats[0].steps;
    assert_eq!(first_travel(steps), pt(11.0, 8.0));
}

#[test]
fn a_measured_blade_offset_replaces_the_library_value() {
    let d = doc(vec![(
        mat(1),
        vec![rect_object(
            10,
            10.0,
            10.0,
            50.0,
            30.0,
            vec![op(BLADE, OpKind::Cut, 1, 100)],
        )],
    )]);
    let mut cal = Calibration::default();
    cal.blade_offsets_mm
        .insert("generic.drag-blade".into(), 0.5);
    let steps = &run_with(&d, &cal).mats[0].steps;
    assert_eq!(first_travel(steps), pt(35.5, 10.0));
}

#[test]
fn a_mirrored_mat_reflects_x_across_the_mat_width() {
    let mut m = mat(1);
    m.mirror = true;
    let d = doc(vec![(
        m,
        vec![rect_object(
            10,
            250.0,
            10.0,
            10.0,
            10.0,
            vec![op(PEN, OpKind::Draw, 1, 100)],
        )],
    )]);
    let steps = &run(&d).mats[0].steps;
    // The square spans 304.8 - 260 = 44.8 to 54.8 once mirrored. A pen starts a closed shape where
    // the path starts (the marker in Prepare), which is x = 250 mirrored to 304.8 - 250 = 54.8.
    assert_eq!(first_travel(steps), pt(54.8, 10.0));
}

#[test]
fn the_object_transform_is_applied() {
    let mut o = rect_object(
        10,
        0.0,
        0.0,
        10.0,
        10.0,
        vec![op(PEN, OpKind::Draw, 1, 100)],
    );
    o.transform = Affine::translate((20.0, 30.0));
    let steps = &run(&doc(vec![(mat(1), vec![o])])).mats[0].steps;
    assert_eq!(first_travel(steps), pt(20.0, 30.0));
}

#[test]
fn moves_outside_the_work_area_block_the_job() {
    // The Air 2's work area is 292 mm wide.
    let d = doc(vec![(
        mat(1),
        vec![rect_object(
            10,
            285.0,
            10.0,
            20.0,
            10.0,
            vec![op(PEN, OpKind::Draw, 1, 100)],
        )],
    )]);
    let j = run(&d);
    assert!(j.is_blocked());
    assert!(
        keys(&j).contains(&"diag-move-out-of-bounds".to_owned()),
        "{:?}",
        keys(&j)
    );
    assert!(j.diagnostics.iter().any(|d| d.severity == Severity::Error));
}

#[test]
fn a_missing_tool_stops_before_any_geometry_work() {
    let d = doc(vec![(
        mat(1),
        vec![rect_object(
            10,
            10.0,
            10.0,
            10.0,
            10.0,
            vec![op("no.such-tool@1.0.0", OpKind::Draw, 1, 100)],
        )],
    )]);
    let j = run(&d);
    assert!(j.is_blocked());
    assert!(keys(&j).contains(&"diag-missing-library-entry".to_owned()));
    assert_eq!(j.mats.len(), 0);
}

#[test]
fn hidden_objects_and_disabled_operations_are_skipped() {
    let mut hidden = rect_object(
        10,
        10.0,
        10.0,
        10.0,
        10.0,
        vec![op(PEN, OpKind::Draw, 1, 100)],
    );
    hidden.hidden = true;
    let mut off = op(PEN, OpKind::Draw, 1, 101);
    off.enabled = false;
    let disabled = rect_object(11, 30.0, 10.0, 10.0, 10.0, vec![off]);
    let j = run(&doc(vec![(mat(1), vec![hidden, disabled])]));
    assert_eq!(j.mats.len(), 0);
}

#[test]
fn a_fill_hatches_with_many_pen_strokes() {
    let mut fill = op(PEN, OpKind::Fill, 1, 100);
    fill.fill = Some(FillSpec {
        pattern: Pattern::Parallel,
        angle_deg: 0.0,
        overlap: 0.15,
        outline_pass: false,
    });
    let d = doc(vec![(
        mat(1),
        vec![rect_object(10, 10.0, 10.0, 20.0, 20.0, vec![fill])],
    )]);
    let m = &run(&d).mats[0];
    assert!(drawn(&m.steps) >= 55, "{}", drawn(&m.steps));
    assert!(m.estimate.cut_length_um[&1] > 1_000_000);
}

#[test]
fn inner_cuts_run_before_the_outer_cut_that_frees_them() {
    let outer = path(
        Path::rect(10.0, 10.0, 100.0, 100.0),
        "outer",
        vec![op(BLADE, OpKind::Cut, 1, 100)],
    );
    let inner = path(
        Path::rect(30.0, 30.0, 20.0, 20.0),
        "inner",
        vec![op(BLADE, OpKind::Cut, 1, 100)],
    );
    // The outer path is listed first on purpose.
    let m = &run(&doc(vec![(mat(1), vec![object(10, vec![outer, inner])])])).mats[0];
    let start = first_travel(&m.steps);
    assert!(
        start.x > 30 * MM
            && start.x < 50 * MM + 500
            && start.y > 30 * MM - 500
            && start.y < 50 * MM,
        "the first cut should be the inner square, started at {start:?}"
    );
}

#[test]
fn every_drawn_move_links_back_to_its_source() {
    let the_op = op(PEN, OpKind::Draw, 1, 99);
    let op_id = the_op.id;
    let obj = rect_object(42, 10.0, 10.0, 20.0, 20.0, vec![the_op]);
    let object_id = obj.id;
    let the_mat = mat(7);
    let mat_id = the_mat.id;
    let d = doc(vec![(the_mat, vec![obj])]);
    let m = &run(&d).mats[0];
    let mut n = 0;
    for s in &m.steps {
        if let Step::Line { src, .. } | Step::Arc { src, .. } = s {
            assert_eq!(src.mat, mat_id);
            assert_eq!(src.object, object_id);
            assert_eq!(src.op, op_id);
            assert_eq!(src.move_index, n);
            n += 1;
        }
    }
    assert!(n > 0);
}

#[test]
fn feed_and_force_come_from_the_resolved_parameters() {
    // Cardstock presets are not selected (the mat has no material), so the tool defaults apply:
    // blade force 350 gf clamped to the machine's 400, speed 100 mm/s = 6000 mm/min.
    let d = doc(vec![(
        mat(1),
        vec![rect_object(
            10,
            10.0,
            10.0,
            20.0,
            20.0,
            vec![op(BLADE, OpKind::Cut, 1, 100)],
        )],
    )]);
    let m = &run(&d).mats[0];
    assert!(
        m.steps
            .iter()
            .any(|s| matches!(s, Step::ToolDown { force_gf: 350 })),
        "{:?}",
        m.steps
    );
    assert!(m.steps.iter().any(|s| matches!(
        s,
        Step::Line {
            feed_mm_min: 6_000,
            ..
        } | Step::Arc {
            feed_mm_min: 6_000,
            ..
        }
    )));
}

#[test]
fn the_force_scale_is_applied_and_clamping_is_reported() {
    let d = doc(vec![(
        mat(1),
        vec![rect_object(
            10,
            10.0,
            10.0,
            20.0,
            20.0,
            vec![op(BLADE, OpKind::Cut, 1, 100)],
        )],
    )]);
    let cal = Calibration {
        force_scale: 10.0,
        ..Calibration::default()
    };
    let j = run_with(&d, &cal);
    assert!(
        j.mats[0]
            .steps
            .iter()
            .any(|s| matches!(s, Step::ToolDown { force_gf: 400 }))
    );
    assert!(
        keys(&j).contains(&"diag-force-clamped".to_owned()),
        "{:?}",
        keys(&j)
    );
    assert!(!j.is_blocked(), "clamping is a warning, never an error");
}

#[test]
fn the_estimate_is_filled_in() {
    let d = doc(vec![(
        mat(1),
        vec![rect_object(
            10,
            10.0,
            10.0,
            50.0,
            30.0,
            vec![op(BLADE, OpKind::Cut, 1, 100)],
        )],
    )]);
    let e = &run(&d).mats[0].estimate;
    assert!(e.machine_time_ms > 1_000);
    // A 160 mm perimeter plus the overcut.
    assert!(
        e.cut_length_um[&1] > 160_000 && e.cut_length_um[&1] < 170_000,
        "{:?}",
        e.cut_length_um
    );
    // 50 x 30 mm of a 304.8 mm square mat.
    let expected = 50.0 * 30.0 / (304.8 * 304.8) * 1e6;
    assert!(
        (f64::from(e.utilisation_ppm) - expected).abs() < 100.0,
        "{}",
        e.utilisation_ppm
    );
}

#[test]
fn mats_compile_in_document_order_and_can_be_selected() {
    let (first, second) = (mat(1), mat(2));
    let (first_id, second_id) = (first.id, second.id);
    let a = (
        first,
        vec![rect_object(
            10,
            10.0,
            10.0,
            10.0,
            10.0,
            vec![op(PEN, OpKind::Draw, 1, 100)],
        )],
    );
    let b = (
        second,
        vec![rect_object(
            11,
            20.0,
            20.0,
            10.0,
            10.0,
            vec![op(PEN, OpKind::Draw, 1, 101)],
        )],
    );
    let d = doc(vec![a, b]);
    let j = run(&d);
    let ids: Vec<MatId> = j.mats.iter().map(|m| m.mat).collect();
    assert_eq!(ids, vec![first_id, second_id]);

    let (libraries, profile, cal) = (libs(), air2(), Calibration::default());
    let one = compile(&CompileInput {
        doc: &d,
        libs: &libraries,
        profile: &profile,
        calibration: &cal,
        mats: MatSelection::One(second_id),
    })
    .unwrap();
    assert_eq!(one.mats.len(), 1);
    assert_eq!(one.mats[0].mat, second_id);
}

#[test]
fn selecting_a_mat_that_does_not_exist_is_an_error() {
    let d = doc(vec![(mat(1), vec![])]);
    let missing = MatId::at_unix_ms(9);
    let (libraries, profile, cal) = (libs(), air2(), Calibration::default());
    let r = compile(&CompileInput {
        doc: &d,
        libs: &libraries,
        profile: &profile,
        calibration: &cal,
        mats: MatSelection::One(missing),
    });
    assert_eq!(r.unwrap_err(), CompileError::UnknownMat(missing));
}

#[test]
fn compiling_twice_gives_identical_jobs() {
    let d = doc(vec![(
        mat(1),
        vec![
            rect_object(
                10,
                10.0,
                10.0,
                50.0,
                30.0,
                vec![
                    op(BLADE, OpKind::Cut, 1, 100),
                    op(PEN, OpKind::Draw, 2, 101),
                ],
            ),
            rect_object(
                11,
                80.0,
                40.0,
                30.0,
                30.0,
                vec![op(PEN, OpKind::Draw, 2, 102)],
            ),
        ],
    )]);
    assert_eq!(run(&d), run(&d));
}

#[test]
fn the_input_hash_follows_what_the_compile_read() {
    let a = doc(vec![(
        mat(1),
        vec![rect_object(
            10,
            10.0,
            10.0,
            20.0,
            20.0,
            vec![op(PEN, OpKind::Draw, 1, 100)],
        )],
    )]);
    let mut moved = rect_object(
        10,
        10.0,
        10.0,
        20.0,
        20.0,
        vec![op(PEN, OpKind::Draw, 1, 100)],
    );
    moved.transform = Affine::translate((1.0, 0.0));
    let b = doc(vec![(mat(1), vec![moved])]);
    assert_eq!(run(&a).input_hash, run(&a).input_hash);
    assert_ne!(run(&a).input_hash, run(&b).input_hash);
    let cal = Calibration {
        time_scale: 1.2,
        ..Calibration::default()
    };
    assert_ne!(run(&a).input_hash, run_with(&a, &cal).input_hash);
}

#[test]
fn every_program_move_stays_on_the_grid_inside_the_work_area() {
    let d = doc(vec![(
        mat(1),
        vec![rect_object(
            10,
            10.0,
            10.0,
            50.0,
            30.0,
            vec![op(BLADE, OpKind::Cut, 1, 100)],
        )],
    )]);
    for s in &run(&d).mats[0].steps {
        if let Some(p) = s.end_point() {
            assert!(
                p.x >= 0 && p.x <= 292 * MM && p.y >= 0 && p.y <= 597 * MM,
                "{p:?}"
            );
        }
    }
}
// ---- branches that the built-in library does not exercise ----

mod accuracy;

mod variants {
    use ps_geometry::Point;
    use ps_library::{
        DriveType, Entry, Holder, Origin, ParamSpec, ParamType, RectMm, Tool, ToolFamily,
    };
    use ps_model::OrderingMode;

    use super::*;

    fn tool_entry(base: &str, edit: impl FnOnce(&mut Tool)) -> Entry {
        let reference: ps_core::LibRef = base.parse().unwrap();
        let mut tool = (*libs().resolve_tool(&reference).unwrap().item).clone();
        edit(&mut tool);
        Entry::Tool(tool)
    }

    fn libs_with(extra: Vec<Entry>) -> LibrarySnapshot {
        let mut entries = builtin_entries().unwrap();
        entries.extend(extra);
        LibrarySnapshot::from_entries(entries)
    }

    fn knife() -> Entry {
        tool_entry(BLADE, |t| {
            t.id = "test.knife".into();
            t.drive = DriveType::Tangential;
            t.geometry.blade_offset_mm = None;
        })
    }

    fn heater() -> Entry {
        tool_entry(PEN, |t| {
            t.id = "test.heater".into();
            t.family = ToolFamily::Heated;
            t.drive = DriveType::Heated;
            t.holders = vec!["test.heated-holder".into()];
            t.operations = vec!["engrave".into()];
            let spec = |unit: &str, min, max, default| ParamSpec {
                kind: ParamType::Number,
                unit: Some(unit.into()),
                min,
                max,
                default,
            };
            t.parameters
                .insert("temperature".into(), spec("celsius", 50.0, 300.0, 200.0));
            t.parameters
                .insert("warmup_ms".into(), spec("ms", 0.0, 60_000.0, 1_000.0));
        })
    }

    fn heated_profile() -> MachineProfile {
        let mut p = air2();
        p.heated = true;
        p.heating_rate_c_s = Some(5.0);
        p.holders.push(Holder {
            id: "test.heated-holder".into(),
            label: "Heater".into(),
            accepts: vec![ToolFamily::Heated],
        });
        p
    }

    fn compile_with(d: &Document, libraries: &LibrarySnapshot, profile: &MachineProfile) -> Job {
        compile(&CompileInput {
            doc: d,
            libs: libraries,
            profile,
            calibration: &Calibration::default(),
            mats: MatSelection::All,
        })
        .unwrap()
    }

    fn one(ops: Vec<Operation>, geom: Path) -> Document {
        let o = object(10, vec![path(geom, "p", ops)]);
        doc(vec![(mat(1), vec![o])])
    }

    #[test]
    fn a_tangential_knife_lifts_at_corners_and_reports_the_tool_angle() {
        let libraries = libs_with(vec![knife()]);
        let corner = Path::polyline(
            &[
                Point::new(10.0, 10.0),
                Point::new(40.0, 10.0),
                Point::new(40.0, 40.0),
            ],
            false,
        );
        let d = one(vec![op("test.knife@1.0.0", OpKind::Cut, 1, 100)], corner);
        let j = compile_with(&d, &libraries, &air2());
        assert_eq!(keys(&j), Vec::<String>::new(), "{:?}", j.diagnostics);
        let steps = &j.mats[0].steps;
        assert_eq!(
            steps
                .iter()
                .filter(|s| matches!(s, Step::ToolDown { .. }))
                .count(),
            2
        );
        let angles: Vec<i64> = steps
            .iter()
            .filter_map(|s| {
                if let Step::Line { a_mdeg, .. } = s {
                    *a_mdeg
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(angles, vec![0, 90_000]);
    }

    #[test]
    fn a_heated_tool_preheats_and_cools_down() {
        let libraries = libs_with(vec![heater()]);
        let d = one(
            vec![op("test.heater@1.0.0", OpKind::Engrave, 1, 100)],
            Path::rect(10.0, 10.0, 20.0, 20.0),
        );
        let j = compile_with(&d, &libraries, &heated_profile());
        assert_eq!(keys(&j), Vec::<String>::new(), "{:?}", j.diagnostics);
        let m = &j.mats[0];
        let temp = m
            .steps
            .iter()
            .position(|s| {
                matches!(
                    s,
                    Step::Temperature {
                        celsius: 200,
                        wait: true
                    }
                )
            })
            .unwrap();
        let warm = m
            .steps
            .iter()
            .position(|s| matches!(s, Step::Dwell { ms: 1_000 }))
            .unwrap();
        let down = m
            .steps
            .iter()
            .position(|s| matches!(s, Step::ToolDown { .. }))
            .unwrap();
        assert!(temp < warm && warm < down, "pre-heat, warm-up, then draw");
        assert!(matches!(
            m.steps.last(),
            Some(Step::Temperature {
                celsius: 0,
                wait: false
            })
        ));
        // Heating from 25 to 200 degrees at 5 degrees per second alone takes 35 s.
        assert!(
            m.estimate.machine_time_ms >= 36_000,
            "{}",
            m.estimate.machine_time_ms
        );
    }

    #[test]
    fn a_foil_overlay_is_wrapped_in_place_and_remove_pauses() {
        let o = rect_object(
            10,
            10.0,
            10.0,
            30.0,
            30.0,
            vec![
                op(BLADE, OpKind::Cut, 1, 100),
                op("generic.foil-transfer@1.0.0", OpKind::Foil, 2, 101),
            ],
        );
        let j = run(&doc(vec![(mat(1), vec![o])]));
        let steps = &j.mats[0].steps;
        let at = |text: &str| {
            steps
                .iter()
                .position(|s| matches!(s, Step::Pause(m) if m.text() == text))
                .unwrap_or_else(|| panic!("no pause {text:?} in {steps:?}"))
        };
        let (place, remove) = (
            at("Place the overlay sheet"),
            at("Remove the overlay sheet"),
        );
        let downs: Vec<usize> = steps
            .iter()
            .enumerate()
            .filter_map(|(i, s)| matches!(s, Step::ToolDown { .. }).then_some(i))
            .collect();
        assert_eq!(downs.len(), 2);
        assert!(
            place < downs[0] && downs[0] < remove && remove < downs[1],
            "{place} {downs:?} {remove}"
        );
        // Foil and blade share clamp B, so the swap needs a pause naming the tool.
        assert_eq!(j.mats[0].estimate.tool_changes, 1);
    }

    #[test]
    fn a_wavy_cut_is_drawn_as_a_stroke_and_said_so() {
        let d = one(
            vec![op(BLADE, OpKind::WavyCut, 1, 100)],
            Path::rect(10.0, 10.0, 30.0, 30.0),
        );
        let j = run(&d);
        assert!(
            keys(&j).contains(&"diag-wavy-cut-as-stroke".to_owned()),
            "{:?}",
            keys(&j)
        );
        assert!(!j.is_blocked());
    }

    fn strict_document(objects: Vec<PlacedObject>) -> Document {
        let mut d = doc(vec![(mat(1), objects)]);
        let mut m = (**d.mats.values().next().unwrap()).clone();
        let rows: Vec<OpRowId> = m
            .objects
            .values()
            .flat_map(|o| o.paths.iter().flat_map(|p| p.ops.iter().map(|op| op.row)))
            .collect();
        m.op_order = Arc::new(rows);
        let id = m.id;
        Arc::make_mut(&mut d.mats).insert(id, Arc::new(m));
        Arc::make_mut(&mut d.settings).ordering = OrderingMode::StrictOrder;
        d
    }

    #[test]
    fn strict_order_follows_the_rows_when_nothing_forces_otherwise() {
        // Disjoint shapes: the cut row comes first and stays first.
        let cut = rect_object(
            10,
            10.0,
            10.0,
            30.0,
            30.0,
            vec![op(BLADE, OpKind::Cut, 1, 100)],
        );
        let draw = rect_object(
            11,
            100.0,
            10.0,
            30.0,
            30.0,
            vec![op(PEN, OpKind::Draw, 2, 101)],
        );
        let j = run(&strict_document(vec![cut, draw]));
        let order: Vec<&str> = j.mats[0].tool_table.iter().map(|t| t.tool.id()).collect();
        assert_eq!(order, vec!["generic.drag-blade", "pen.fineliner-black-0.4"]);
        assert!(!keys(&j).contains(&"diag-release-order".to_owned()));
    }

    #[test]
    fn release_safety_overrides_strict_order_and_says_so() {
        // The pen outlines the very piece the blade frees, so drawing must come first;
        // only a user lock could keep the cut first.
        let o = rect_object(
            10,
            10.0,
            10.0,
            30.0,
            30.0,
            vec![
                op(BLADE, OpKind::Cut, 1, 100),
                op(PEN, OpKind::Draw, 2, 101),
            ],
        );
        let j = run(&strict_document(vec![o]));
        let order: Vec<&str> = j.mats[0].tool_table.iter().map(|t| t.tool.id()).collect();
        assert_eq!(order, vec!["pen.fineliner-black-0.4", "generic.drag-blade"]);
        assert!(
            keys(&j).contains(&"diag-release-order".to_owned()),
            "{:?}",
            keys(&j)
        );
        assert!(!j.is_blocked(), "a reorder is a warning");
    }

    /// The rows of a mat's operations, in the order the operations appear.
    fn rows_of(d: &Document) -> Vec<OpRowId> {
        let mat = d.mats.values().next().unwrap();
        mat.objects
            .values()
            .flat_map(|o| o.paths.iter().flat_map(|p| p.ops.iter().map(|op| op.row)))
            .collect()
    }

    /// Locks the `first`-th row (by position) to run before the `later`-th.
    fn with_lock(mut d: Document, first: usize, later: usize) -> Document {
        let rows = rows_of(&d);
        let mut m = (**d.mats.values().next().unwrap()).clone();
        m.run_after = Arc::new(vec![(rows[first], rows[later])]);
        let id = m.id;
        Arc::make_mut(&mut d.mats).insert(id, Arc::new(m));
        d
    }

    fn two_disjoint() -> Vec<PlacedObject> {
        vec![
            rect_object(
                10,
                10.0,
                10.0,
                30.0,
                30.0,
                vec![op(BLADE, OpKind::Cut, 1, 100)],
            ),
            rect_object(
                11,
                100.0,
                10.0,
                30.0,
                30.0,
                vec![op(PEN, OpKind::Draw, 2, 101)],
            ),
        ]
    }

    #[test]
    fn a_run_after_lock_moves_a_row_against_the_default_stage_order() {
        // Drawing normally comes first; the lock says the cut row runs before the draw row.
        let plain = run(&doc(vec![(mat(1), two_disjoint())]));
        let plain_order: Vec<&str> = plain.mats[0]
            .tool_table
            .iter()
            .map(|t| t.tool.id())
            .collect();
        assert_eq!(
            plain_order,
            vec!["pen.fineliner-black-0.4", "generic.drag-blade"]
        );
        let locked = run(&with_lock(doc(vec![(mat(1), two_disjoint())]), 0, 1));
        let order: Vec<&str> = locked.mats[0]
            .tool_table
            .iter()
            .map(|t| t.tool.id())
            .collect();
        assert_eq!(order, vec!["generic.drag-blade", "pen.fineliner-black-0.4"]);
    }

    #[test]
    fn a_lock_wins_over_release_safety_and_the_conflict_is_reported() {
        let o = rect_object(
            10,
            10.0,
            10.0,
            30.0,
            30.0,
            vec![
                op(BLADE, OpKind::Cut, 1, 100),
                op(PEN, OpKind::Draw, 2, 101),
            ],
        );
        let j = run(&with_lock(doc(vec![(mat(1), vec![o])]), 0, 1));
        let order: Vec<&str> = j.mats[0].tool_table.iter().map(|t| t.tool.id()).collect();
        assert_eq!(order, vec!["generic.drag-blade", "pen.fineliner-black-0.4"]);
        assert!(!j.is_blocked(), "reported, not blocked: {:?}", keys(&j));
    }

    #[test]
    fn locks_that_contradict_each_other_are_ignored_rather_than_failing() {
        let mut d = with_lock(doc(vec![(mat(1), two_disjoint())]), 0, 1);
        let mut m = (**d.mats.values().next().unwrap()).clone();
        m.run_after = Arc::new(vec![
            (OpRowId::at_unix_ms(1), OpRowId::at_unix_ms(2)),
            (OpRowId::at_unix_ms(2), OpRowId::at_unix_ms(1)),
        ]);
        let id = m.id;
        Arc::make_mut(&mut d.mats).insert(id, Arc::new(m));
        let j = run(&d);
        assert_eq!(j.mats[0].tool_table.len(), 2, "still compiles");
    }

    fn with_overrides(mut o: Operation, pairs: &[(&str, f64)]) -> Operation {
        for (k, v) in pairs {
            o.overrides.set(
                ps_model::ParamKey::new(*k),
                ps_model::ParamValue::Number(*v),
            );
        }
        o
    }

    fn tool_downs(j: &Job) -> Vec<u32> {
        j.mats[0]
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::ToolDown { force_gf } => Some(*force_gf),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn tabs_set_on_an_operation_lift_the_blade_in_the_program() {
        let plain = run(&one(
            vec![op(BLADE, OpKind::Cut, 1, 100)],
            Path::rect(10.0, 10.0, 80.0, 40.0),
        ));
        let tabbed = run(&one(
            vec![with_overrides(
                op(BLADE, OpKind::Cut, 1, 100),
                &[("tab_count", 4.0), ("tab_width_mm", 2.0)],
            )],
            Path::rect(10.0, 10.0, 80.0, 40.0),
        ));
        assert_eq!(tool_downs(&plain).len(), 1);
        assert_eq!(
            tool_downs(&tabbed).len(),
            4,
            "one cut piece between each pair of tabs"
        );
    }

    #[test]
    fn a_force_ramp_makes_each_pass_press_harder_up_to_the_full_force() {
        let j = run(&one(
            vec![with_overrides(
                op(BLADE, OpKind::Cut, 1, 100),
                &[("passes", 3.0), ("force_ramp", 0.6)],
            )],
            Path::polyline(&[Point::new(10.0, 10.0), Point::new(60.0, 10.0)], false),
        ));
        let forces = tool_downs(&j);
        assert_eq!(forces.len(), 3);
        assert!(forces[0] < forces[1] && forces[1] < forces[2], "{forces:?}");
        assert_eq!(forces[2], 350, "the last pass is at the tool's full force");
    }

    fn even_odd_path(geom: Path, key: &str, ops: Vec<Operation>) -> Arc<ShapePath> {
        Arc::new(ShapePath {
            fill_rule: FillRule::EvenOdd,
            ..(*path(geom, key, ops)).clone()
        })
    }

    fn fill_length(paths: Vec<Arc<ShapePath>>) -> u64 {
        let j = run(&doc(vec![(mat(1), vec![object(10, paths)])]));
        j.mats[0].estimate.cut_length_um.values().sum()
    }

    #[test]
    fn a_ring_inside_a_ring_of_the_same_object_is_a_hole_in_the_fill() {
        let outer = || Path::rect(20.0, 20.0, 60.0, 60.0);
        let alone = fill_length(vec![even_odd_path(
            outer(),
            "outer",
            vec![op(PEN, OpKind::Fill, 1, 100)],
        )]);
        let with_hole = fill_length(vec![
            even_odd_path(outer(), "outer", vec![op(PEN, OpKind::Fill, 1, 100)]),
            even_odd_path(
                Path::rect(35.0, 35.0, 30.0, 30.0),
                "inner",
                vec![op(PEN, OpKind::Fill, 1, 101)],
            ),
        ]);
        assert!(alone > 0);
        assert!(
            with_hole < alone,
            "the counter is not drawn: {with_hole} vs {alone}"
        );
    }

    fn with_pen_check(mut d: Document) -> Document {
        Arc::make_mut(&mut d.settings).pen_check = true;
        d
    }

    fn travels_to(j: &Job, x_um: i64, y_um: i64) -> usize {
        j.mats[0]
            .steps
            .iter()
            .filter(|s| matches!(s, Step::Travel { to } if to.x == x_um && to.y == y_um))
            .count()
    }

    #[test]
    fn the_pen_check_draws_a_two_millimetre_mark_in_the_corner_before_each_pen() {
        let d = || {
            doc(vec![(
                mat(1),
                vec![rect_object(
                    10,
                    50.0,
                    50.0,
                    30.0,
                    30.0,
                    vec![op(PEN, OpKind::Draw, 1, 100)],
                )],
            )])
        };
        let plain = run(&d());
        let checked = run(&with_pen_check(d()));
        assert_eq!(travels_to(&plain, 1_000, 1_000), 0);
        assert_eq!(travels_to(&checked, 1_000, 1_000), 1, "one pen, one mark");
        assert_eq!(tool_downs(&checked).len(), tool_downs(&plain).len() + 1);
        // The mark is 2 mm long: it ends at x = 3 mm.
        assert!(
            checked.mats[0]
                .steps
                .iter()
                .any(|s| matches!(s, Step::Line { to, .. } if to.x == 3_000 && to.y == 1_000))
        );
    }

    #[test]
    fn the_pen_check_is_not_drawn_for_blades_or_when_off() {
        let blade_only = doc(vec![(
            mat(1),
            vec![rect_object(
                10,
                50.0,
                50.0,
                30.0,
                30.0,
                vec![op(BLADE, OpKind::Cut, 1, 100)],
            )],
        )]);
        let j = run(&with_pen_check(blade_only));
        assert_eq!(travels_to(&j, 1_000, 1_000), 0);
    }

    #[test]
    fn each_pen_block_gets_a_mark() {
        let d = doc(vec![(
            mat(1),
            vec![
                rect_object(
                    10,
                    50.0,
                    50.0,
                    30.0,
                    30.0,
                    vec![op(PEN, OpKind::Draw, 1, 100)],
                ),
                rect_object(
                    11,
                    120.0,
                    50.0,
                    30.0,
                    30.0,
                    vec![op(PEN, OpKind::Draw, 2, 101)],
                ),
            ],
        )]);
        let j = run(&with_pen_check(d));
        assert_eq!(travels_to(&j, 1_000, 1_000), 2);
    }

    #[test]
    fn a_pen_starts_each_closed_shape_where_its_path_starts() {
        let d = doc(vec![(
            mat(1),
            vec![
                rect_object(
                    10,
                    20.0,
                    10.0,
                    10.0,
                    10.0,
                    vec![op(PEN, OpKind::Draw, 1, 100)],
                ),
                rect_object(
                    11,
                    5.0,
                    40.0,
                    10.0,
                    10.0,
                    vec![op(PEN, OpKind::Draw, 1, 101)],
                ),
            ],
        )]);
        let j = run(&d);
        // Each square starts at its own start, so the one the Prepare marker shows is the real one,
        // even though the second's nearest corner to the first would be (15, 40).
        assert_eq!(travels_to(&j, 5_000, 40_000), 1);
        assert_eq!(travels_to(&j, 15_000, 40_000), 0);
    }

    #[test]
    fn a_blade_arc_tighter_than_its_offset_is_warned_about() {
        let tiny = Path::circle(Point::new(30.0, 30.0), 0.2);
        let j = run(&one(vec![op(BLADE, OpKind::Cut, 1, 100)], tiny));
        assert!(
            keys(&j).contains(&"diag-min-radius".to_owned()),
            "{:?}",
            keys(&j)
        );
    }

    #[test]
    fn a_machine_with_a_bottom_left_origin_flips_y() {
        let mut p = air2();
        p.origin = Origin::BottomLeft;
        let d = one(
            vec![op(PEN, OpKind::Draw, 1, 100)],
            Path::rect(10.0, 10.0, 20.0, 20.0),
        );
        let j = compile_with(&d, &libs(), &p);
        // y = work height (597) - 10 = 587 for the top edge and 597 - 30 = 567 for the bottom one;
        // the pen starts at the path's own start (10, 10), which is y = 587.
        assert_eq!(first_travel(&j.mats[0].steps), pt(10.0, 587.0));
    }

    #[test]
    fn a_machine_without_arcs_gets_only_lines() {
        let mut p = air2();
        p.arcs = false;
        let d = one(
            vec![op(PEN, OpKind::Draw, 1, 100)],
            Path::circle(Point::new(50.0, 50.0), 20.0),
        );
        let steps = &compile_with(&d, &libs(), &p).mats[0].steps;
        assert!(steps.iter().all(|s| !matches!(s, Step::Arc { .. })));
        assert!(
            steps
                .iter()
                .filter(|s| matches!(s, Step::Line { .. }))
                .count()
                > 30
        );
    }

    #[test]
    fn a_machine_with_arcs_keeps_a_circle_as_two_arcs() {
        let d = one(
            vec![op(PEN, OpKind::Draw, 1, 100)],
            Path::circle(Point::new(50.0, 50.0), 20.0),
        );
        let steps = &run(&d).mats[0].steps;
        assert_eq!(
            steps
                .iter()
                .filter(|s| matches!(s, Step::Arc { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn a_keep_out_blocks_a_move_through_it() {
        let mut p = air2();
        p.keep_outs.push(RectMm {
            x_mm: 40.0,
            y_mm: 0.0,
            width_mm: 10.0,
            height_mm: 200.0,
        });
        let line = Path::polyline(&[Point::new(10.0, 50.0), Point::new(100.0, 50.0)], false);
        let j = compile_with(&one(vec![op(PEN, OpKind::Draw, 1, 100)], line), &libs(), &p);
        assert!(
            keys(&j).contains(&"diag-move-in-keep-out".to_owned()),
            "{:?}",
            keys(&j)
        );
        assert!(j.is_blocked());
    }
}
// ---- tool-down and tool-up dwells per preset ----

mod dwells {
    use ps_library::{Entry, ParamSpec, ParamType};

    use super::*;

    fn dwelling_pen() -> Entry {
        let reference: ps_core::LibRef = PEN.parse().unwrap();
        let mut tool = (*libs().resolve_tool(&reference).unwrap().item).clone();
        tool.id = "test.dwelling-pen".into();
        let spec = |default| ParamSpec {
            kind: ParamType::Number,
            unit: Some("ms".into()),
            min: 0.0,
            max: 2_000.0,
            default,
        };
        tool.parameters.insert("down_dwell_ms".into(), spec(80.0));
        tool.parameters.insert("up_dwell_ms".into(), spec(30.0));
        Entry::Tool(tool)
    }

    #[test]
    fn a_pen_with_dwell_presets_waits_after_going_down_and_after_lifting() {
        let mut entries = builtin_entries().unwrap();
        entries.push(dwelling_pen());
        let libraries = LibrarySnapshot::from_entries(entries);
        let d = doc(vec![(
            mat(1),
            vec![rect_object(
                10,
                10.0,
                10.0,
                20.0,
                20.0,
                vec![op("test.dwelling-pen@1.0.0", OpKind::Draw, 1, 100)],
            )],
        )]);
        let j = compile(&CompileInput {
            doc: &d,
            libs: &libraries,
            profile: &air2(),
            calibration: &Calibration::default(),
            mats: MatSelection::All,
        })
        .unwrap();
        let steps = &j.mats[0].steps;
        let down = steps
            .iter()
            .position(|s| matches!(s, Step::ToolDown { .. }))
            .unwrap();
        assert!(
            matches!(steps[down + 1], Step::Dwell { ms: 80 }),
            "{:?}",
            &steps[down..=down + 1]
        );
        let up = steps
            .iter()
            .rposition(|s| matches!(s, Step::ToolUp))
            .unwrap();
        assert!(
            matches!(steps[up + 1], Step::Dwell { ms: 30 }),
            "{:?}",
            &steps[up..]
        );
    }

    #[test]
    fn tools_without_dwell_presets_have_no_dwell_steps() {
        let d = doc(vec![(
            mat(1),
            vec![rect_object(
                10,
                10.0,
                10.0,
                20.0,
                20.0,
                vec![op(PEN, OpKind::Draw, 1, 100)],
            )],
        )]);
        let j = run(&d);
        assert!(
            !j.mats[0]
                .steps
                .iter()
                .any(|s| matches!(s, Step::Dwell { .. }))
        );
    }
}

#[test]
fn a_locked_start_point_beats_the_nearest_vertex_for_a_pen() {
    let mut o = rect_object(
        10,
        100.0,
        100.0,
        20.0,
        20.0,
        vec![op(PEN, OpKind::Draw, 1, 100)],
    );
    {
        let paths = Arc::make_mut(&mut o.paths);
        let p = Arc::make_mut(&mut paths[0]);
        p.geom = crate::order::start_nearest(&p.geom, Point::new(120.0, 120.0));
        p.start_locked = true;
    }
    let d = doc(vec![(mat(1), vec![o])]);
    let steps = &run(&d).mats[0].steps;
    assert_eq!(first_travel(steps), pt(120.0, 120.0));
}

#[test]
fn without_a_lock_the_pen_starts_at_the_paths_own_start() {
    let o = rect_object(
        10,
        100.0,
        100.0,
        20.0,
        20.0,
        vec![op(PEN, OpKind::Draw, 1, 100)],
    );
    let d = doc(vec![(mat(1), vec![o])]);
    let steps = &run(&d).mats[0].steps;
    assert_eq!(first_travel(steps), pt(100.0, 100.0));
}

#[test]
fn pens_of_different_colours_are_different_slots_and_say_which_ink() {
    let mut red = op(PEN, OpKind::Draw, 1, 100);
    red.tool.pen_colour = Some("#C42B2B".to_owned());
    let black = op(PEN, OpKind::Draw, 2, 101);
    let d = doc(vec![(
        mat(1),
        vec![
            rect_object(10, 10.0, 10.0, 20.0, 20.0, vec![red]),
            rect_object(11, 50.0, 10.0, 20.0, 20.0, vec![black]),
        ],
    )]);
    let table = &run(&d).mats[0].tool_table;
    assert_eq!(table.len(), 2, "{table:?}");
    assert_ne!(table[0].slot, table[1].slot);
    let coloured: Vec<_> = table.iter().filter_map(|t| t.pen.as_ref()).collect();
    assert_eq!(coloured.len(), 1, "only the changed ink is a variant");
    assert_eq!(coloured[0].colour.as_deref(), Some("#C42B2B"));
    assert!(
        table.iter().any(|t| t.label.contains("#C42B2B")),
        "{table:?}"
    );
}

#[test]
fn a_wider_pen_fills_with_fewer_strokes_and_is_reported_in_the_tool_table() {
    let fill_with = |tip: Option<f64>| {
        let mut fill = op(PEN, OpKind::Fill, 1, 100);
        fill.fill = Some(FillSpec {
            pattern: Pattern::Parallel,
            angle_deg: 0.0,
            overlap: 0.15,
            outline_pass: false,
        });
        if let Some(tip) = tip {
            fill.overrides.set(
                ps_model::ParamKey::new("tip_width"),
                ps_model::ParamValue::Number(tip),
            );
        }
        let d = doc(vec![(
            mat(1),
            vec![rect_object(10, 10.0, 10.0, 20.0, 20.0, vec![fill])],
        )]);
        let job = run(&d);
        (
            drawn(&job.mats[0].steps),
            job.mats[0].tool_table[0].pen.clone(),
        )
    };
    let (thin, thin_pen) = fill_with(None);
    let (wide, wide_pen) = fill_with(Some(1.6));
    // Rows are four times fewer with a four times wider pen (spacing 0.34 to 1.36 mm). The moves fall
    // by less: each join of two rows is a semicircle of chords on this machine, the same number of
    // vectors however wide the pen.
    assert!(wide * 2 < thin, "thin {thin}, wide {wide}");
    assert!(thin_pen.is_none());
    assert_eq!(wide_pen.and_then(|p| p.tip_width_um), Some(1_600));
}

#[test]
fn a_part_thinner_than_the_pen_is_still_inked_along_its_centre() {
    let mut fill = op(PEN, OpKind::Fill, 1, 100);
    fill.fill = Some(FillSpec {
        pattern: Pattern::Parallel,
        angle_deg: 0.0,
        overlap: 0.15,
        outline_pass: false,
    });
    // 20 mm long and 0.3 mm wide: narrower than the 0.4 mm tip, so the hatch alone draws nothing.
    let d = doc(vec![(
        mat(1),
        vec![rect_object(10, 10.0, 10.0, 20.0, 0.3, vec![fill])],
    )]);
    let m = &run(&d).mats[0];
    assert!(
        drawn(&m.steps) >= 1,
        "a thin sliver is drawn as a centre line"
    );
}

#[test]
fn a_fill_finishes_each_separate_piece_before_starting_the_next() {
    let mut fill = op(PEN, OpKind::Fill, 1, 100);
    fill.fill = Some(FillSpec {
        pattern: Pattern::Parallel,
        angle_deg: 0.0,
        overlap: 0.15,
        outline_pass: false,
    });
    // Four 8 mm squares in a row, 30 mm apart, all one object like the letters of a word.
    let paths = (0..4)
        .map(|i| {
            path(
                Path::rect(10.0 + 30.0 * f64::from(i), 10.0, 8.0, 8.0),
                &format!("p{i}"),
                vec![fill.clone()],
            )
        })
        .collect();
    let d = doc(vec![(mat(1), vec![object(10, paths)])]);
    let job = run(&d);
    let mut at: Option<i64> = None;
    let mut jumps = 0;
    for step in &job.mats[0].steps {
        if let Some(to) = step.end_point() {
            if let Some(from) = at
                && matches!(step, Step::Travel { .. })
                && (to.x - from).abs() > 15_000
            {
                jumps += 1;
            }
            at = Some(to.x);
        }
    }
    assert_eq!(jumps, 3, "one jump between neighbouring squares each");
}

#[test]
fn points_on_a_straight_edge_do_not_become_extra_moves() {
    // The same 40 x 20 mm rectangle with and without three extra points on each edge: the pen
    // draws four corners either way, because the machine spends a fixed time on every vector.
    let plain = rect_object(
        10,
        10.0,
        10.0,
        40.0,
        20.0,
        vec![op(PEN, OpKind::Draw, 1, 100)],
    );
    let mut dense = Vec::new();
    for (from, to) in [
        ((10.0, 10.0), (50.0, 10.0)),
        ((50.0, 10.0), (50.0, 30.0)),
        ((50.0, 30.0), (10.0, 30.0)),
        ((10.0, 30.0), (10.0, 10.0)),
    ] {
        for i in 0..4 {
            let t = f64::from(i) / 4.0;
            dense.push(Point::new(
                from.0 + (to.0 - from.0) * t,
                from.1 + (to.1 - from.1) * t,
            ));
        }
    }
    let busy = object(
        11,
        vec![path(
            Path::polyline(&dense, true),
            "p",
            vec![op(PEN, OpKind::Draw, 1, 101)],
        )],
    );
    let plain_steps = run(&doc(vec![(mat(1), vec![plain])])).mats[0].steps.clone();
    let busy_steps = run(&doc(vec![(mat(1), vec![busy])])).mats[0].steps.clone();
    assert_eq!(drawn(&busy_steps), drawn(&plain_steps));
    assert_eq!(drawn(&busy_steps), 4);
}

#[test]
fn a_finely_flattened_circle_is_drawn_with_only_the_vectors_it_needs() {
    // A 20 mm radius circle as a 360-gon: its edges bulge only 0.8 um from their chords. Within the
    // profile's 10 um chord tolerance a chord may span at most 2 acos(1 - 0.010 / 20) = 3.62
    // degrees, so a full turn needs at least 360 / 3.62 = 100 vectors, and the pen is not given
    // anything like the 360 there were.
    let points: Vec<Point> = (0..360)
        .map(|i| {
            let a = f64::from(i).to_radians();
            Point::new(60.0 + 20.0 * libm::cos(a), 60.0 + 20.0 * libm::sin(a))
        })
        .collect();
    let object = object(
        12,
        vec![path(
            Path::polyline(&points, true),
            "c",
            vec![op(PEN, OpKind::Draw, 1, 102)],
        )],
    );
    let moves = drawn(&run(&doc(vec![(mat(1), vec![object])])).mats[0].steps);
    assert!((100..=160).contains(&moves), "{moves}");
}
