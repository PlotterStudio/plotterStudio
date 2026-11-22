//! Ground truth for the job IR: ordered steps per mat with source links. Lengths are
//! closed-form (3-4-5 triangles, quarter circles), not taken from the implementation.

use std::f64::consts::FRAC_PI_2;

use ps_core::{MatId, ObjectId, OpId, PathKey, PtUm};

use super::*;

fn src(move_index: u32) -> SrcRef {
    SrcRef {
        mat: MatId::at_unix_ms(1),
        object: ObjectId::at_unix_ms(2),
        path: PathKey::source("p1"),
        op: OpId::at_unix_ms(3),
        move_index,
    }
}

fn line(x: i64, y: i64, i: u32) -> Step {
    Step::Line {
        to: PtUm::new(x, y),
        feed_mm_min: 6_000,
        a_mdeg: None,
        src: src(i),
    }
}

fn mat_job(steps: Vec<Step>) -> MatJob {
    MatJob {
        mat: MatId::at_unix_ms(1),
        tool_table: vec![],
        steps,
        estimate: Estimate::default(),
    }
}

#[test]
fn only_moves_have_end_points() {
    assert_eq!(
        Step::Travel {
            to: PtUm::new(1, 2)
        }
        .end_point(),
        Some(PtUm::new(1, 2))
    );
    assert_eq!(line(3, 4, 0).end_point(), Some(PtUm::new(3, 4)));
    let arc = Step::Arc {
        to: PtUm::new(0, 1_000),
        center: PtUm::new(0, 0),
        ccw: true,
        feed_mm_min: 3_000,
        src: src(1),
    };
    assert_eq!(arc.end_point(), Some(PtUm::new(0, 1_000)));
    for s in [
        Step::ToolUp,
        Step::ToolDown { force_gf: 350 },
        Step::Dwell { ms: 80 },
        Step::Temperature {
            celsius: 120,
            wait: true,
        },
        Step::Pause(PauseMsg::new("Unload mat")),
    ] {
        assert_eq!(s.end_point(), None, "{s:?}");
    }
}

#[test]
fn drawn_moves_are_lines_and_arcs() {
    assert!(line(1, 1, 0).is_drawn());
    assert!(
        !Step::Travel {
            to: PtUm::new(1, 1)
        }
        .is_drawn()
    );
    assert!(!Step::ToolUp.is_drawn());
}

#[test]
fn drawn_length_of_a_345_triangle() {
    let job = mat_job(vec![
        Step::Travel {
            to: PtUm::new(0, 0),
        },
        Step::ToolDown { force_gf: 100 },
        line(3_000, 0, 0),
        line(3_000, 4_000, 1),
        line(0, 0, 2), // the hypotenuse, 5000
        Step::ToolUp,
    ]);
    assert!((job.drawn_length_um() - 12_000.0).abs() < 1e-9);
}

#[test]
fn travel_length_is_counted_separately() {
    let job = mat_job(vec![
        Step::Travel {
            to: PtUm::new(0, 0),
        },
        Step::Travel {
            to: PtUm::new(3_000, 4_000),
        }, // 5000
        line(3_000, 4_000, 0),
        Step::Travel {
            to: PtUm::new(3_000, 0),
        }, // 4000
    ]);
    assert!((job.travel_length_um() - 9_000.0).abs() < 1e-9);
    assert!(job.drawn_length_um().abs() < 1e-9);
}

#[test]
fn drawn_length_of_a_quarter_circle_arc() {
    let job = mat_job(vec![
        Step::Travel {
            to: PtUm::new(1_000, 0),
        },
        Step::Arc {
            to: PtUm::new(0, 1_000),
            center: PtUm::new(0, 0),
            ccw: true,
            feed_mm_min: 3_000,
            src: src(0),
        },
    ]);
    assert!((job.drawn_length_um() - 1_000.0 * FRAC_PI_2).abs() < 1e-6);
}

#[test]
fn a_clockwise_arc_the_long_way_round() {
    // Clockwise from (1000,0) to (0,1000) is three quarters of the circle.
    let job = mat_job(vec![
        Step::Travel {
            to: PtUm::new(1_000, 0),
        },
        Step::Arc {
            to: PtUm::new(0, 1_000),
            center: PtUm::new(0, 0),
            ccw: false,
            feed_mm_min: 3_000,
            src: src(0),
        },
    ]);
    assert!((job.drawn_length_um() - 1_000.0 * 3.0 * FRAC_PI_2).abs() < 1e-6);
}

#[test]
fn bounds_cover_every_point_and_arc_extremes() {
    let job = mat_job(vec![
        Step::Travel {
            to: PtUm::new(1_000, 0),
        },
        // Counter-clockwise half circle through (0, 1000) to (-1000, 0): bulges to y = 1000.
        Step::Arc {
            to: PtUm::new(-1_000, 0),
            center: PtUm::new(0, 0),
            ccw: true,
            feed_mm_min: 3_000,
            src: src(0),
        },
        line(-1_000, -500, 1),
    ]);
    let b = job.bounds().unwrap();
    assert_eq!(
        (b.min, b.max),
        (PtUm::new(-1_000, -500), PtUm::new(1_000, 1_000))
    );
}

#[test]
fn bounds_of_a_job_with_no_moves_is_none() {
    assert_eq!(mat_job(vec![Step::ToolUp]).bounds(), None);
}

#[test]
fn moves_are_counted() {
    let job = mat_job(vec![
        Step::Travel {
            to: PtUm::new(0, 0),
        },
        Step::ToolDown { force_gf: 1 },
        line(1, 1, 0),
        line(2, 2, 1),
        Step::ToolUp,
    ]);
    assert_eq!(job.move_count(), 3);
    assert_eq!(job.drawn_count(), 2);
}

#[test]
fn tool_changes_are_loads_after_the_first() {
    let load = |slot, holder: &str| Step::LoadTool {
        slot,
        holder: HolderId::new(holder),
        pause: None,
    };
    let job = mat_job(vec![
        load(1, "A"),
        line(1, 1, 0),
        load(2, "B"),
        line(2, 2, 1),
        load(1, "A"),
    ]);
    assert_eq!(job.tool_change_count(), 2);
    assert_eq!(mat_job(vec![]).tool_change_count(), 0);
}

#[test]
fn pause_messages_expose_their_text() {
    let m = PauseMsg::new("Load pen: Fine liner, black 0.4 into clamp A");
    assert_eq!(m.text(), "Load pen: Fine liner, black 0.4 into clamp A");
}

#[test]
fn steps_serialise_with_snake_case_tags() {
    let json = serde_json::to_string(&Step::ToolDown { force_gf: 350 }).unwrap();
    assert_eq!(json, r#"{"tool_down":{"force_gf":350}}"#);
    assert_eq!(serde_json::to_string(&Step::ToolUp).unwrap(), "\"tool_up\"");
    let back: Step = serde_json::from_str(r#"{"dwell":{"ms":80}}"#).unwrap();
    assert_eq!(back, Step::Dwell { ms: 80 });
}

#[test]
fn a_job_round_trips_through_json() {
    let job = Job {
        mats: vec![mat_job(vec![
            Step::Travel {
                to: PtUm::new(12_500, 20_000),
            },
            Step::ToolDown { force_gf: 150 },
            line(62_500, 20_000, 0),
            Step::ToolUp,
        ])],
        diagnostics: Diagnostics::new(),
        input_hash: ps_core::hash_bytes(b"input"),
    };
    let json = serde_json::to_string(&job).unwrap();
    assert_eq!(serde_json::from_str::<Job>(&json).unwrap(), job);
}

#[test]
fn jobs_look_up_their_mats_and_report_blocking_errors() {
    let m = mat_job(vec![]);
    let id = m.mat;
    let mut job = Job {
        mats: vec![m],
        diagnostics: Diagnostics::new(),
        input_hash: ps_core::hash_bytes(b""),
    };
    assert!(job.mat(id).is_some());
    assert!(job.mat(MatId::at_unix_ms(99)).is_none());
    assert!(!job.is_blocked());
    job.diagnostics
        .push(ps_core::Diagnostic::error(ps_core::L10nKey::new(
            "diag-missing-tool",
        )));
    assert!(job.is_blocked());
}
