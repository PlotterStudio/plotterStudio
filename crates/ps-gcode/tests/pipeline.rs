//! End to end: a document compiles to a job, the job validates, writes as G-code and as a
//! `.psjob` bundle, and the bundle opens in the verifying reader. This checks that the compiler's
//! output is always something the writer accepts.

use std::sync::Arc;

use ps_compiler::{CompileInput, MatSelection, compile};
use ps_core::{MatId, ObjectId, OpId, OpRowId, PathKey};
use ps_gcode::{BundleInput, BundleMat, WriteContext, ensure_valid, write_bundle, write_mat};
use ps_geometry::{Affine, FillRule, Path, Point};
use ps_job::Bundle;
use ps_library::{Calibration, LibrarySnapshot, MachineProfile, builtin_entries};
use ps_model::{
    Document, Mat, MatSize, OpKind, Operation, ParamOverrides, PlacedObject, ProjectMeta,
    ShapePath, SourceStyle, ToolSlotRef,
};

const BLADE: &str = "generic.drag-blade@1.0.0";
const PEN: &str = "pen.fineliner-black-0.4@1.0.0";

fn libs() -> LibrarySnapshot {
    LibrarySnapshot::from_entries(builtin_entries().unwrap())
}

fn air2() -> MachineProfile {
    let r: ps_core::LibRef = "generic.cutter-small@1.0.0".parse().unwrap();
    (*libs().resolve_profile(&r).unwrap().item).clone()
}

fn op(tool: &str, kind: OpKind, row: OpRowId) -> Operation {
    Operation {
        id: OpId::new(),
        kind,
        tool: ToolSlotRef {
            tool: tool.parse().unwrap(),
            pen_colour: None,
        },
        overrides: ParamOverrides::default(),
        fill: None,
        enabled: true,
        row,
    }
}

fn document() -> Document {
    let (cut_row, draw_row) = (OpRowId::new(), OpRowId::new());
    let shapes = vec![
        (
            Path::rect(10.0, 10.0, 60.0, 40.0),
            vec![
                op(BLADE, OpKind::Cut, cut_row),
                op(PEN, OpKind::Draw, draw_row),
            ],
        ),
        (
            Path::circle(Point::new(120.0, 40.0), 15.0),
            vec![op(BLADE, OpKind::Cut, cut_row)],
        ),
    ];
    let mut mat = Mat::new(MatId::new(), "Mat 1", MatSize::inches(12.0, 12.0));
    for (i, (geom, ops)) in shapes.into_iter().enumerate() {
        let object = PlacedObject {
            id: ObjectId::new(),
            source: None,
            transform: Affine::IDENTITY,
            group: None,
            attach: None,
            paths: Arc::new(vec![Arc::new(ShapePath {
                key: PathKey::source(format!("p{i}")),
                geom,
                fill_rule: FillRule::NonZero,
                style: SourceStyle::default(),
                ops,
                start_locked: false,
            })]),
            locked: false,
            hidden: false,
        };
        Arc::make_mut(&mut mat.objects).insert(object.id, Arc::new(object));
    }
    let mut doc = Document::new(ProjectMeta::new("t", "t", "v"));
    Arc::make_mut(&mut doc.mats).insert(mat.id, Arc::new(mat));
    doc
}

fn compiled() -> (ps_job::Job, MachineProfile) {
    let (libraries, profile) = (libs(), air2());
    let job = compile(&CompileInput {
        doc: &document(),
        libs: &libraries,
        profile: &profile,
        calibration: &Calibration::default(),
        mats: MatSelection::All,
    })
    .unwrap();
    (job, profile)
}

#[test]
fn a_compiled_job_passes_the_validator() {
    let (job, profile) = compiled();
    assert!(!job.is_blocked(), "{:?}", job.diagnostics);
    assert_eq!(job.mats.len(), 1);
    ensure_valid(&job.mats[0], &profile).unwrap();
}

#[test]
fn a_compiled_job_writes_well_formed_gcode() {
    let (job, profile) = compiled();
    let g = write_mat(
        &job.mats[0],
        &WriteContext {
            app_version: "0.1.0",
            job_id: "job",
            profile: &profile,
            mat_index: 1,
            mat_count: 1,
            mat_label: "12x12in standard",
            material: None,
        },
    );
    assert!(g.text.starts_with("; plotter-studio 0.1.0\n"));
    assert!(g.text.ends_with("M0 (Unload mat)\nM2\n"));
    assert!(g.text.contains("\nT1 M6\n") && g.text.contains("\nT2 M6\n"));
    assert!(
        g.text.contains("\nG2 ") || g.text.contains("\nG3 "),
        "the circle keeps its arcs"
    );
    // Every drawn line of the program maps to a source object.
    let drawn = g.lines.iter().filter(|l| l.src.is_some()).count();
    assert!(drawn > 4, "{drawn}");
    // No line is empty or has trailing space.
    assert!(g.text.lines().all(|l| !l.is_empty() && l == l.trim_end()));
}

#[test]
fn a_compiled_job_exports_as_a_bundle_that_opens() {
    let (job, profile) = compiled();
    let mats = [BundleMat {
        job: &job.mats[0],
        name: "Mat 1",
        label: "12x12in standard",
        material: None,
    }];
    let bytes = write_bundle(&BundleInput {
        job_id: "job",
        app_version: "0.1.0",
        export_time: "2026-11-18T00:00:00Z",
        profile: &profile,
        mats: &mats,
    })
    .unwrap();
    let bundle = Bundle::from_bytes(&bytes).unwrap();
    assert_eq!(bundle.manifest.mats[0].tools.len(), 2);
    assert!(bundle.gcode(1).unwrap().contains("G21\nG90\n"));
}

#[test]
fn what_the_writer_emits_the_parser_reads_back_move_for_move() {
    use ps_gcode::{Command, parse_program};
    use ps_job::Step;

    let (job, profile) = compiled();
    let mat = &job.mats[0];
    let g = write_mat(
        mat,
        &WriteContext {
            app_version: "0.1.0",
            job_id: "job",
            profile: &profile,
            mat_index: 1,
            mat_count: 1,
            mat_label: "12x12in standard",
            material: None,
        },
    );
    let parsed = parse_program(&g.text).expect("every written line parses");
    assert_eq!(parsed.len(), g.lines.len(), "one parsed line per file line");

    // Every drawn step reads back as a Linear or Arc with the same end point and feed.
    let drawn_steps: Vec<&Step> = mat.steps.iter().filter(|s| s.is_drawn()).collect();
    let drawn_cmds: Vec<&Command> = parsed
        .iter()
        .map(|l| &l.command)
        .filter(|c| matches!(c, Command::Linear { .. } | Command::Arc { .. }))
        .collect();
    assert_eq!(drawn_steps.len(), drawn_cmds.len());
    for (step, cmd) in drawn_steps.iter().zip(&drawn_cmds) {
        match (step, cmd) {
            (
                Step::Line {
                    to, feed_mm_min, ..
                },
                Command::Linear {
                    to: t,
                    feed_mm_min: f,
                    ..
                },
            ) => {
                assert_eq!((to, feed_mm_min), (t, f));
            }
            (
                Step::Arc {
                    to,
                    ccw,
                    feed_mm_min,
                    ..
                },
                Command::Arc {
                    to: t,
                    ccw: c,
                    feed_mm_min: f,
                    ..
                },
            ) => {
                assert_eq!((to, ccw, feed_mm_min), (t, c, f));
            }
            other => panic!("step and command disagree: {other:?}"),
        }
    }

    // The tool commands match one for one too.
    let count = |f: &dyn Fn(&Command) -> bool| parsed.iter().filter(|l| f(&l.command)).count();
    assert_eq!(
        count(&|c| matches!(c, Command::ToolDown { .. })),
        mat.steps
            .iter()
            .filter(|s| matches!(s, Step::ToolDown { .. }))
            .count()
    );
    assert_eq!(
        count(&|c| matches!(c, Command::ToolChange { .. })),
        mat.tool_table.len()
    );
    assert!(matches!(parsed.last().unwrap().command, Command::End));
}
