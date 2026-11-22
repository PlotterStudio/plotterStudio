//! Ground truth for the toolpath view: the compiled steps become line
//! segments in millimetres, travel moves apart from drawn ones, up to a step the timeline has
//! reached; tool changes and pauses become markers on the timeline.

use ps_core::{MatId, ObjectId, OpId, PathKey, PtUm};
use ps_job::{HolderId, PauseMsg, SrcRef, Step};

use super::*;

fn um(x: i64, y: i64) -> PtUm {
    PtUm::new(x * 1000, y * 1000)
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

fn line(x: i64, y: i64) -> Step {
    Step::Line {
        to: um(x, y),
        feed_mm_min: 6000,
        a_mdeg: None,
        src: src(),
    }
}

fn load(slot: u8, pause: Option<&str>) -> Step {
    Step::LoadTool {
        slot,
        holder: HolderId::new("clamp-b"),
        pause: pause.map(PauseMsg::new),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn program() -> Vec<Step> {
    vec![
        Step::Travel { to: um(10, 10) },
        Step::ToolDown { force_gf: 200 },
        line(20, 10),
        line(20, 20),
        Step::ToolUp,
        Step::Travel { to: um(0, 0) },
    ]
}

#[test]
fn a_program_becomes_travel_and_drawn_segments() {
    let segs = segments(&program(), usize::MAX);
    assert_eq!(segs.len(), 3, "{segs:?}");
    assert_eq!(segs[0].kind, SegmentKind::Draw { slot: None });
    assert!(close(segs[0].from.x, 10.0) && close(segs[0].to.x, 20.0) && close(segs[0].to.y, 10.0));
    assert_eq!(segs[1].kind, SegmentKind::Draw { slot: None });
    assert!(close(segs[1].to.y, 20.0));
    assert_eq!(segs[2].kind, SegmentKind::Travel);
    assert!(close(segs[2].from.x, 20.0) && close(segs[2].to.x, 0.0));
}

#[test]
fn the_first_move_has_no_segment_because_the_start_is_unknown() {
    let segs = segments(&[Step::Travel { to: um(10, 10) }], usize::MAX);
    assert_eq!(segs.len(), 0);
}

#[test]
fn nothing_is_shown_before_the_first_step() {
    assert_eq!(segments(&program(), 0).len(), 0);
}

#[test]
fn up_to_counts_executed_steps() {
    // Steps 0..4 are Travel, ToolDown, Line, Line: two drawn segments.
    assert_eq!(segments(&program(), 4).len(), 2);
    // Steps 0..3 are Travel, ToolDown, Line: one.
    assert_eq!(segments(&program(), 3).len(), 1);
}

#[test]
fn drawn_segments_carry_the_tool_in_the_holder() {
    let steps = vec![
        Step::Travel { to: um(0, 0) },
        load(2, Some("Load pen")),
        Step::ToolDown { force_gf: 10 },
        line(5, 0),
    ];
    let segs = segments(&steps, usize::MAX);
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].kind, SegmentKind::Draw { slot: Some(2) });
}

#[test]
fn an_arc_is_flattened_into_chords_that_stay_on_the_circle() {
    // A quarter circle of radius 10 about (0, 0), from (10, 0) to (0, 10) counter-clockwise.
    let steps = vec![
        Step::Travel { to: um(10, 0) },
        Step::ToolDown { force_gf: 10 },
        Step::Arc {
            to: um(0, 10),
            center: um(0, 0),
            ccw: true,
            feed_mm_min: 3000,
            src: src(),
        },
    ];
    let segs = segments(&steps, usize::MAX);
    assert!(segs.len() > 4, "{}", segs.len());
    for s in &segs {
        for p in [s.from, s.to] {
            assert!((p.x.hypot(p.y) - 10.0).abs() < 1e-6, "{p:?}");
        }
    }
    // The chords join up.
    for pair in segs.windows(2) {
        assert!(close(pair[0].to.x, pair[1].from.x) && close(pair[0].to.y, pair[1].from.y));
    }
    let last = segs.last().unwrap();
    assert!(close(last.to.x, 0.0) && close(last.to.y, 10.0));
}

#[test]
fn the_tool_position_is_where_the_last_move_ended() {
    let p = tool_position(&program(), 4).unwrap();
    assert!(close(p.x, 20.0) && close(p.y, 20.0));
    assert!(tool_position(&program(), 0).is_none());
    let end = tool_position(&program(), usize::MAX).unwrap();
    assert!(close(end.x, 0.0) && close(end.y, 0.0));
}

#[test]
fn tool_changes_and_pauses_are_marked_on_the_timeline() {
    let steps = vec![
        Step::Travel { to: um(0, 0) },
        load(1, None),
        line(1, 0),
        load(2, Some("Load pen")),
        Step::Pause(PauseMsg::new("Unload mat")),
    ];
    let m = markers(&steps);
    assert_eq!(
        m,
        vec![
            Marker {
                step: 1,
                kind: MarkerKind::ToolChange { slot: 1 }
            },
            Marker {
                step: 3,
                kind: MarkerKind::ToolChange { slot: 2 }
            },
            Marker {
                step: 3,
                kind: MarkerKind::Pause {
                    message: "Load pen".to_owned()
                }
            },
            Marker {
                step: 4,
                kind: MarkerKind::Pause {
                    message: "Unload mat".to_owned()
                }
            },
        ]
    );
}

#[test]
fn playback_advances_with_time_and_speed() {
    // 20 steps per second at 1x.
    assert_eq!(playback_step(1000, 1.0, 1.0), 20);
    assert_eq!(playback_step(1000, 1.0, 10.0), 200);
    assert_eq!(playback_step(1000, 0.5, 100.0), 1000, "capped at the end");
    assert_eq!(playback_step(1000, 0.0, 5.0), 0);
    assert_eq!(playback_step(0, 5.0, 5.0), 0);
}

#[test]
fn the_speeds_are_one_to_one_hundred() {
    assert_eq!(PLAYBACK_SPEEDS, [1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0]);
}

#[test]
fn a_timeline_fraction_maps_to_a_step() {
    assert_eq!(step_at_fraction(100, 0.0), 0);
    assert_eq!(step_at_fraction(100, 0.5), 50);
    assert_eq!(step_at_fraction(100, 1.0), 100);
    assert_eq!(step_at_fraction(100, 2.0), 100);
    assert_eq!(step_at_fraction(100, -1.0), 0);
    assert_eq!(step_at_fraction(0, 0.5), 0);
}
