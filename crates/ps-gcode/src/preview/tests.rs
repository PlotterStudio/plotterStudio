//! Ground truth for toolpath previews. The SVG is
//! deterministic text: the work area as a frame, then one group per tool slot with one path per
//! contiguous drawn run (tool down to tool up). Travel moves are not drawn. SVG arcs use the same
//! y-down coordinates as the machine, so `ccw` maps straight to the sweep flag.

use ps_core::{MatId, ObjectId, OpId, PathKey, PtUm};
use ps_job::{Estimate, HolderId, MatJob, SrcRef, Step, ToolEntry};

use super::*;

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
        feed_mm_min: 3_000,
        a_mdeg: None,
        src: src(),
    }
}

fn arc(to: (f64, f64), center: (f64, f64), ccw: bool) -> Step {
    Step::Arc {
        to: p(to.0, to.1),
        center: p(center.0, center.1),
        ccw,
        feed_mm_min: 3_000,
        src: src(),
    }
}

fn load(slot: u8) -> Step {
    Step::LoadTool {
        slot,
        holder: HolderId::new("h"),
        pause: None,
    }
}

fn job(steps: Vec<Step>) -> MatJob {
    MatJob {
        mat: MatId::at_unix_ms(1),
        tool_table: vec![ToolEntry {
            slot: 1,
            tool: "pen.x@1.0.0".parse().unwrap(),
            holder: HolderId::new("h"),
            label: "Pen".into(),
            pen: None,
        }],
        steps,
        estimate: Estimate::default(),
    }
}

fn svg(steps: Vec<Step>) -> String {
    svg_preview(&job(steps), 292_000, 597_000)
}

const FRAME: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 292 597\" width=\"292mm\" height=\"597mm\">\n<rect x=\"0\" y=\"0\" width=\"292\" height=\"597\" fill=\"none\" stroke=\"#999999\" stroke-width=\"0.2\"/>\n";

#[test]
fn an_empty_program_is_just_the_frame() {
    assert_eq!(svg(vec![]), format!("{FRAME}</svg>\n"));
}

#[test]
fn a_drawn_run_becomes_one_path_in_its_tool_group() {
    let s = svg(vec![
        load(1),
        Step::Travel { to: p(12.5, 20.0) },
        Step::ToolDown { force_gf: 150 },
        line(62.5, 20.0),
        line(62.5, 70.0),
        Step::ToolUp,
    ]);
    let expected = format!(
        "{FRAME}<g id=\"tool-1\" fill=\"none\" stroke=\"#231F20\" stroke-width=\"0.4\" stroke-linecap=\"round\" stroke-linejoin=\"round\">\n<path d=\"M12.5 20L62.5 20L62.5 70\"/>\n</g>\n</svg>\n"
    );
    assert_eq!(s, expected);
}

#[test]
fn travel_between_runs_is_not_drawn_and_starts_a_new_path() {
    let s = svg(vec![
        load(1),
        Step::Travel { to: p(0.0, 0.0) },
        Step::ToolDown { force_gf: 1 },
        line(10.0, 0.0),
        Step::ToolUp,
        Step::Travel { to: p(50.0, 50.0) },
        Step::ToolDown { force_gf: 1 },
        line(60.0, 50.0),
        Step::ToolUp,
    ]);
    assert_eq!(s.matches("<path").count(), 2);
    assert!(s.contains("d=\"M0 0L10 0\""));
    assert!(s.contains("d=\"M50 50L60 50\""));
}

#[test]
fn a_quarter_arc_has_a_small_arc_flag_and_follows_the_direction_flag() {
    let ccw = svg(vec![
        load(1),
        Step::Travel { to: p(10.0, 0.0) },
        Step::ToolDown { force_gf: 1 },
        arc((0.0, 10.0), (0.0, 0.0), true),
        Step::ToolUp,
    ]);
    assert!(ccw.contains("d=\"M10 0A10 10 0 0 1 0 10\""), "{ccw}");
    let cw = svg(vec![
        load(1),
        Step::Travel { to: p(10.0, 0.0) },
        Step::ToolDown { force_gf: 1 },
        arc((0.0, -10.0), (0.0, 0.0), false),
        Step::ToolUp,
    ]);
    assert!(cw.contains("d=\"M10 0A10 10 0 0 0 0 -10\""), "{cw}");
}

#[test]
fn an_arc_of_more_than_half_a_turn_has_the_large_arc_flag() {
    // Counter-clockwise from (10, 0) to (0, -10) is three quarters of a turn.
    let s = svg(vec![
        load(1),
        Step::Travel { to: p(10.0, 0.0) },
        Step::ToolDown { force_gf: 1 },
        arc((0.0, -10.0), (0.0, 0.0), true),
        Step::ToolUp,
    ]);
    assert!(s.contains("A10 10 0 1 1 0 -10"), "{s}");
}

#[test]
fn a_full_circle_is_drawn_as_two_half_arcs() {
    let s = svg(vec![
        load(1),
        Step::Travel { to: p(10.0, 0.0) },
        Step::ToolDown { force_gf: 1 },
        arc((10.0, 0.0), (0.0, 0.0), true),
        Step::ToolUp,
    ]);
    assert!(
        s.contains("d=\"M10 0A10 10 0 0 1 -10 0A10 10 0 0 1 10 0\""),
        "{s}"
    );
}

#[test]
fn each_tool_slot_gets_its_own_group_and_colour() {
    let s = svg(vec![
        load(1),
        Step::Travel { to: p(0.0, 0.0) },
        Step::ToolDown { force_gf: 1 },
        line(5.0, 0.0),
        Step::ToolUp,
        load(2),
        Step::Travel { to: p(0.0, 10.0) },
        Step::ToolDown { force_gf: 1 },
        line(5.0, 10.0),
        Step::ToolUp,
    ]);
    assert!(s.contains("<g id=\"tool-1\""));
    assert!(s.contains("<g id=\"tool-2\""));
    let colours: Vec<&str> = s.lines().filter(|l| l.starts_with("<g ")).collect();
    assert_ne!(
        colours[0].split("stroke=\"").nth(1),
        colours[1].split("stroke=\"").nth(1)
    );
}

#[test]
fn a_run_left_open_at_the_end_is_still_drawn() {
    let s = svg(vec![
        load(1),
        Step::Travel { to: p(0.0, 0.0) },
        Step::ToolDown { force_gf: 1 },
        line(5.0, 5.0),
    ]);
    assert!(s.contains("d=\"M0 0L5 5\""));
}

#[test]
fn a_tool_that_never_draws_has_no_group() {
    let s = svg(vec![load(1), Step::Travel { to: p(5.0, 5.0) }]);
    assert!(!s.contains("<g "));
}

#[test]
fn the_preview_is_deterministic_and_well_formed() {
    let steps = vec![
        load(1),
        Step::Travel { to: p(1.0, 1.0) },
        Step::ToolDown { force_gf: 1 },
        line(9.0, 9.0),
        Step::ToolUp,
    ];
    let a = svg_preview(&job(steps.clone()), 292_000, 597_000);
    assert_eq!(a, svg_preview(&job(steps), 292_000, 597_000));
    assert!(a.starts_with("<svg ") && a.ends_with("</svg>\n"));
    assert_eq!(a.matches("<g ").count(), a.matches("</g>").count());
}
