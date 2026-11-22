//! Ground truth for the pre-export validator. It reads the IR, not the text, and
//! checks every move against the profile: work area, keep-outs, force range, speed limits, arc
//! support, A-axis support and heater support. Any violation blocks export and lists the
//! offending steps with their source references for canvas links.

use ps_core::{LibRef, MatId, ObjectId, OpId, PathKey, PtUm};
use ps_job::{Estimate, HolderId, MatJob, SrcRef, Step, ToolEntry};
use ps_library::{LibrarySnapshot, MachineProfile, RectMm, builtin_entries};

use super::*;

fn profile() -> MachineProfile {
    let libs = LibrarySnapshot::from_entries(builtin_entries().unwrap());
    let r: LibRef = "generic.cutter-small@1.0.0".parse().unwrap();
    // Work area 292 x 597 mm, force 0..=400 gf, max speed 101.6 mm/s (measured), arcs yes, no tangential axis.
    (*libs.resolve_profile(&r).unwrap().item).clone()
}

fn p(x: f64, y: f64) -> PtUm {
    PtUm::from_mm(x, y).unwrap()
}

fn src() -> SrcRef {
    SrcRef {
        mat: MatId::at_unix_ms(1),
        object: ObjectId::at_unix_ms(2),
        path: PathKey::source("p"),
        op: OpId::at_unix_ms(3),
        move_index: 0,
    }
}

fn line(x: f64, y: f64) -> Step {
    Step::Line {
        to: p(x, y),
        feed_mm_min: 6_000,
        a_mdeg: None,
        src: src(),
    }
}

fn job(steps: Vec<Step>) -> MatJob {
    MatJob {
        mat: MatId::at_unix_ms(1),
        tool_table: vec![ToolEntry {
            slot: 1,
            tool: "pen.fineliner-black-0.4@1.0.0".parse().unwrap(),
            holder: HolderId::new("generic.holder-a"),
            label: "Pen".into(),
            pen: None,
        }],
        steps,
        estimate: Estimate::default(),
    }
}

fn kinds(steps: Vec<Step>, profile: &MachineProfile) -> Vec<(usize, ViolationKind)> {
    check(&job(steps), profile)
        .into_iter()
        .map(|v| (v.step, v.kind))
        .collect()
}

fn load() -> Step {
    Step::LoadTool {
        slot: 1,
        holder: HolderId::new("generic.holder-a"),
        pause: None,
    }
}

#[test]
fn a_clean_program_has_no_violations() {
    let steps = vec![
        load(),
        Step::Travel { to: p(10.0, 10.0) },
        Step::ToolDown { force_gf: 150 },
        line(100.0, 10.0),
        line(100.0, 100.0),
        Step::ToolUp,
    ];
    assert_eq!(kinds(steps, &profile()), vec![]);
}

#[test]
fn moves_outside_the_work_area_are_reported_with_their_step() {
    let steps = vec![
        Step::Travel { to: p(300.0, 10.0) },
        Step::Travel { to: p(10.0, 600.0) },
        Step::Travel {
            to: PtUm::new(-1, 0),
        },
    ];
    assert_eq!(
        kinds(steps, &profile()),
        vec![
            (0, ViolationKind::OutsideWorkArea),
            (1, ViolationKind::OutsideWorkArea),
            (2, ViolationKind::OutsideWorkArea),
        ]
    );
}

#[test]
fn the_far_corner_of_the_work_area_is_still_inside() {
    let steps = vec![
        Step::Travel {
            to: p(292.0, 597.0),
        },
        Step::Travel { to: p(0.0, 0.0) },
    ];
    assert_eq!(kinds(steps, &profile()), vec![]);
}

fn with_keep_out() -> MachineProfile {
    let mut pr = profile();
    pr.keep_outs.push(RectMm {
        x_mm: 100.0,
        y_mm: 100.0,
        width_mm: 50.0,
        height_mm: 50.0,
    });
    pr
}

#[test]
fn a_move_ending_inside_a_keep_out_is_reported() {
    assert_eq!(
        kinds(
            vec![Step::Travel {
                to: p(120.0, 120.0)
            }],
            &with_keep_out()
        ),
        vec![(0, ViolationKind::InKeepOut)]
    );
}

#[test]
fn touching_a_keep_out_edge_is_allowed() {
    assert_eq!(
        kinds(
            vec![Step::Travel {
                to: p(100.0, 100.0)
            }],
            &with_keep_out()
        ),
        vec![]
    );
}

#[test]
fn a_move_through_a_keep_out_is_reported_even_if_both_ends_are_clear() {
    let steps = vec![
        Step::Travel { to: p(50.0, 125.0) },
        Step::Travel {
            to: p(200.0, 125.0),
        },
    ];
    assert_eq!(
        kinds(steps, &with_keep_out()),
        vec![(1, ViolationKind::InKeepOut)]
    );
}

#[test]
fn force_outside_the_profile_range_is_reported() {
    let steps = vec![
        Step::ToolDown { force_gf: 401 },
        Step::ToolDown { force_gf: 400 },
    ];
    let found = kinds(steps, &profile());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, 0);
    assert!(matches!(
        found[0].1,
        ViolationKind::ForceOutOfRange { gf: 401, .. }
    ));
}

#[test]
fn speed_above_the_machine_maximum_is_reported() {
    let fast = Step::Line {
        to: p(10.0, 10.0),
        feed_mm_min: 6_097, // just over 101.6 mm/s (6096 mm/min)
        a_mdeg: None,
        src: src(),
    };
    let found = kinds(vec![fast], &profile());
    assert_eq!(found.len(), 1);
    assert!(matches!(found[0].1, ViolationKind::SpeedTooHigh { .. }));
    let ok = Step::Line {
        to: p(10.0, 10.0),
        feed_mm_min: 6_096,
        a_mdeg: None,
        src: src(),
    };
    assert_eq!(kinds(vec![ok], &profile()), vec![]);
}

fn arc(to: (f64, f64), center: (f64, f64)) -> Step {
    Step::Arc {
        to: p(to.0, to.1),
        center: p(center.0, center.1),
        ccw: true,
        feed_mm_min: 6_000,
        src: src(),
    }
}

#[test]
fn arcs_are_reported_on_a_machine_that_cannot_do_them() {
    let mut pr = profile();
    pr.arcs = false;
    let steps = vec![
        Step::Travel { to: p(60.0, 50.0) },
        arc((50.0, 60.0), (50.0, 50.0)),
    ];
    assert_eq!(kinds(steps, &pr), vec![(1, ViolationKind::ArcsUnsupported)]);
}

#[test]
fn an_arc_that_bulges_outside_the_work_area_is_reported() {
    // Both ends are inside the 292 mm width, but the circle of radius 10 about (285, 50)
    // reaches x = 295.
    let steps = vec![
        Step::Travel { to: p(285.0, 40.0) },
        arc((285.0, 60.0), (285.0, 50.0)),
    ];
    // From (285, 40) counter-clockwise to (285, 60) about (285, 50) passes through x = 295.
    assert_eq!(
        kinds(steps, &profile()),
        vec![(1, ViolationKind::OutsideWorkArea)]
    );
}

#[test]
fn a_tangent_angle_needs_a_tangential_axis() {
    let step = Step::Line {
        to: p(10.0, 10.0),
        feed_mm_min: 6_000,
        a_mdeg: Some(45_000),
        src: src(),
    };
    assert_eq!(
        kinds(vec![step.clone()], &profile()),
        vec![(0, ViolationKind::TangentUnsupported)]
    );
    let mut pr = profile();
    pr.tangential_axis = true;
    assert_eq!(kinds(vec![step], &pr), vec![]);
}

#[test]
fn a_temperature_step_needs_a_heater() {
    let steps = vec![Step::Temperature {
        celsius: 180,
        wait: true,
    }];
    assert_eq!(
        kinds(steps.clone(), &profile()),
        vec![(0, ViolationKind::HeaterUnsupported)]
    );
    let mut pr = profile();
    pr.heated = true;
    assert_eq!(kinds(steps, &pr), vec![]);
}

#[test]
fn a_tool_slot_missing_from_the_table_is_reported() {
    let steps = vec![Step::LoadTool {
        slot: 7,
        holder: HolderId::new("generic.holder-a"),
        pause: None,
    }];
    assert_eq!(
        kinds(steps, &profile()),
        vec![(0, ViolationKind::UnknownTool { slot: 7 })]
    );
}

#[test]
fn drawn_violations_carry_their_source_reference() {
    let found = check(&job(vec![line(400.0, 10.0)]), &profile());
    assert_eq!(found.len(), 1);
    assert!(found[0].src.is_some());
    let travel = check(&job(vec![Step::Travel { to: p(400.0, 10.0) }]), &profile());
    assert!(travel[0].src.is_none());
}

#[test]
fn violations_are_listed_in_step_order() {
    let steps = vec![
        Step::Travel { to: p(500.0, 0.0) },
        Step::ToolDown { force_gf: 999 },
        line(600.0, 0.0),
    ];
    let order: Vec<usize> = check(&job(steps), &profile())
        .iter()
        .map(|v| v.step)
        .collect();
    assert_eq!(order, vec![0, 1, 2]);
}

#[test]
fn the_summary_names_every_violation_kind_once() {
    let steps = vec![
        Step::Travel { to: p(500.0, 0.0) },
        Step::ToolDown { force_gf: 999 },
        line(600.0, 0.0),
    ];
    let err = ensure_valid(&job(steps), &profile()).unwrap_err();
    assert_eq!(err.violations.len(), 3);
    assert!(err.to_string().contains("3 problems"), "{err}");
}

#[test]
fn a_valid_job_passes_ensure_valid() {
    assert!(ensure_valid(&job(vec![Step::Travel { to: p(1.0, 1.0) }]), &profile()).is_ok());
}
