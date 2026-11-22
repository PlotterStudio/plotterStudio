//! Ground truth for drag-blade compensation. The holder follows the tip path displaced
//! forward along the tangent by the blade offset d. At corners sharper than the threshold it
//! swivels on an arc of radius d about the corner point. Closed paths start on their longest
//! straight segment and overcut past the start; an alignment stroke orients the blade first; arcs
//! tighter than d raise a minimum-radius warning. Expected values are closed-form.

use std::f64::consts::{FRAC_PI_2, TAU};

use ps_geometry::{Path, Point, Seg};

use super::*;

const D: f64 = 0.25;

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn near_pt(a: Point, b: Point) -> bool {
    near(a.x, b.x) && near(a.y, b.y)
}

fn params() -> DragParams {
    DragParams {
        offset_mm: D,
        corner_threshold_deg: 10.0,
        overcut_mm: 0.0,
        align_mm: 0.0,
        keep_start: false,
    }
}

fn line_to(s: &Seg) -> Point {
    match s {
        Seg::Line { to } => *to,
        other => panic!("expected a line, got {other:?}"),
    }
}

#[test]
fn a_straight_line_is_displaced_forward_by_the_offset() {
    let path = Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0)], false);
    let c = compensate(&path, &params());
    assert!(near_pt(c.path.start, pt(D, 0.0)));
    assert_eq!(c.path.segs.len(), 1);
    assert!(near_pt(line_to(&c.path.segs[0]), pt(10.0 + D, 0.0)));
    assert_eq!(c.warnings.len(), 0);
}

#[test]
fn the_displacement_follows_the_direction_of_travel() {
    let path = Path::polyline(&[pt(0.0, 0.0), pt(0.0, 10.0)], false);
    let c = compensate(&path, &params());
    assert!(near_pt(c.path.start, pt(0.0, D)));
    assert!(near_pt(line_to(&c.path.segs[0]), pt(0.0, 10.0 + D)));
}

#[test]
fn an_alignment_stroke_precedes_the_first_contact() {
    let path = Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0)], false);
    let c = compensate(
        &path,
        &DragParams {
            align_mm: 0.5,
            keep_start: false,
            ..params()
        },
    );
    // The holder goes down 0.5 mm behind its first position and drags forward, so the blade
    // swings into line before the tip reaches the start of the path.
    assert!(near_pt(c.path.start, pt(D - 0.5, 0.0)));
    assert!(near_pt(line_to(&c.path.segs[0]), pt(D, 0.0)));
    assert!(near_pt(line_to(&c.path.segs[1]), pt(10.0 + D, 0.0)));
}

#[test]
fn a_left_corner_swivels_counter_clockwise_about_the_corner_point() {
    let path = Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)], false);
    let c = compensate(&path, &params());
    assert_eq!(c.path.segs.len(), 3);
    assert!(near_pt(line_to(&c.path.segs[0]), pt(10.0 + D, 0.0)));
    match c.path.segs[1] {
        Seg::Arc { to, center, ccw } => {
            assert!(near_pt(center, pt(10.0, 0.0)), "pivots on the corner");
            assert!(near_pt(to, pt(10.0, D)));
            assert!(ccw);
        }
        other => panic!("{other:?}"),
    }
    assert!(near_pt(line_to(&c.path.segs[2]), pt(10.0, 10.0 + D)));
}

#[test]
fn a_right_corner_swivels_clockwise() {
    let path = Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, -10.0)], false);
    let c = compensate(&path, &params());
    match c.path.segs[1] {
        Seg::Arc { to, center, ccw } => {
            assert!(near_pt(center, pt(10.0, 0.0)));
            assert!(near_pt(to, pt(10.0, -D)));
            assert!(!ccw);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_swivel_arc_has_the_blade_offset_as_its_radius() {
    let path = Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)], false);
    let c = compensate(&path, &params());
    let Seg::Arc { to, center, .. } = c.path.segs[1] else {
        panic!()
    };
    let start = line_to(&c.path.segs[0]);
    assert!(near((start - center).hypot(), D));
    assert!(near((to - center).hypot(), D));
}

#[test]
fn gentle_turns_below_the_threshold_do_not_swivel() {
    // A 3 degree turn: joined by a short straight line, no arc.
    let bend = 3.0_f64.to_radians();
    let path = Path::polyline(
        &[
            pt(0.0, 0.0),
            pt(10.0, 0.0),
            pt(10.0 + 10.0 * bend.cos(), 10.0 * bend.sin()),
        ],
        false,
    );
    let c = compensate(&path, &params());
    assert!(
        c.path.segs.iter().all(|s| matches!(s, Seg::Line { .. })),
        "{:?}",
        c.path.segs
    );
    assert_eq!(c.path.segs.len(), 3);
}

#[test]
fn an_arc_segment_becomes_a_larger_arc_about_the_same_centre() {
    // Counter-clockwise quarter circle of radius 10: the holder rides on a circle of radius
    // sqrt(10^2 + d^2), rotated forward by atan(d / 10).
    let path = Path::new(pt(10.0, 0.0)).arc_to(pt(0.0, 10.0), pt(0.0, 0.0), true);
    let c = compensate(&path, &params());
    let big = (100.0_f64 + D * D).sqrt();
    let shift = (D / 10.0).atan();
    assert!(near_pt(c.path.start, pt(10.0, D)), "{:?}", c.path.start);
    let Seg::Arc { to, center, ccw } = c.path.segs[0] else {
        panic!()
    };
    assert!(near_pt(center, pt(0.0, 0.0)));
    assert!(ccw);
    assert!(near((to - center).hypot(), big));
    assert!(near_pt(
        to,
        pt(
            big * (FRAC_PI_2 + shift).cos(),
            big * (FRAC_PI_2 + shift).sin()
        )
    ));
    // The holder moves ahead of the tip: its start is rotated forward from (10, 0).
    assert!(near((c.path.start - center).hypot(), big));
}

#[test]
fn a_clockwise_arc_is_displaced_the_other_way() {
    let path = Path::new(pt(10.0, 0.0)).arc_to(pt(0.0, -10.0), pt(0.0, 0.0), false);
    let c = compensate(&path, &params());
    // Moving clockwise at (10, 0) means heading -y, so the holder is ahead at (10, -d).
    assert!(near_pt(c.path.start, pt(10.0, -D)));
}

#[test]
fn closed_paths_overcut_past_their_start() {
    let circle = Path::circle(pt(0.0, 0.0), 10.0);
    let c = compensate(
        &circle,
        &DragParams {
            overcut_mm: 1.0,
            ..params()
        },
    );
    let big = (100.0_f64 + D * D).sqrt();
    // All arcs: full turn plus 1 mm of overcut (1/10 rad) at the larger radius.
    let total: f64 = c
        .path
        .segments()
        .map(|(from, seg)| match seg {
            Seg::Arc { to, center, ccw } => {
                ps_geometry::arc::sweep(from, *to, *center, *ccw).abs() * big
            }
            _ => 0.0,
        })
        .sum();
    assert!((total - big * (TAU + 0.1)).abs() < 1e-6, "{total}");
    assert!(
        !c.path.closed,
        "the overcut ends past the start, so the path is open"
    );
}

#[test]
fn without_an_overcut_a_closed_path_is_just_compensated() {
    let circle = Path::circle(pt(0.0, 0.0), 10.0);
    let c = compensate(&circle, &params());
    let big = (100.0_f64 + D * D).sqrt();
    let total: f64 = c
        .path
        .segments()
        .map(|(from, seg)| match seg {
            Seg::Arc { to, center, ccw } => {
                ps_geometry::arc::sweep(from, *to, *center, *ccw).abs() * big
            }
            _ => 0.0,
        })
        .sum();
    assert!((total - big * TAU).abs() < 1e-6);
}

#[test]
fn closed_paths_start_in_the_middle_of_their_longest_straight_segment() {
    // 20 x 10 rectangle: the longest sides are the 20 mm ones; the first (y = 0) wins the tie.
    let rect = Path::rect(0.0, 0.0, 20.0, 10.0);
    let c = compensate(&rect, &params());
    assert!(
        near_pt(c.path.start, pt(10.0 + D, 0.0)),
        "{:?}",
        c.path.start
    );
}

#[test]
fn the_overcut_continues_along_the_start_segment() {
    let rect = Path::rect(0.0, 0.0, 20.0, 10.0);
    let c = compensate(
        &rect,
        &DragParams {
            overcut_mm: 1.0,
            ..params()
        },
    );
    // The tip starts at (10, 0), goes round, and ends 1 mm past the start at (11, 0).
    let end = c.path.end();
    assert!(near_pt(end, pt(11.0 + D, 0.0)), "{end:?}");
}

#[test]
fn a_closed_path_with_no_straight_segment_keeps_its_start() {
    let circle = Path::circle(pt(5.0, 5.0), 3.0);
    let c = compensate(&circle, &params());
    // The circle starts at (8, 5) heading counter-clockwise (up): the holder is ahead at (8, 5 + d).
    assert!(near_pt(c.path.start, pt(8.0, 5.0 + D)));
}

#[test]
fn arcs_tighter_than_the_offset_are_reported() {
    let tight = Path::new(pt(0.2, 0.0)).arc_to(pt(0.0, 0.2), pt(0.0, 0.0), true);
    let c = compensate(&tight, &params());
    assert_eq!(c.warnings.len(), 1);
    match c.warnings[0] {
        DragWarning::MinRadius { segment, radius_mm } => {
            assert_eq!(segment, 0);
            assert!(near(radius_mm, 0.2));
        }
    }
    let wide = Path::new(pt(1.0, 0.0)).arc_to(pt(0.0, 1.0), pt(0.0, 0.0), true);
    assert_eq!(compensate(&wide, &params()).warnings.len(), 0);
}

#[test]
fn degenerate_input_gives_an_empty_path() {
    let c = compensate(&Path::new(pt(1.0, 1.0)), &params());
    assert_eq!(c.path.segs.len(), 0);
    let zero = Path::polyline(&[pt(1.0, 1.0), pt(1.0, 1.0)], false);
    assert_eq!(compensate(&zero, &params()).path.segs.len(), 0);
}

#[test]
fn a_zero_offset_leaves_the_path_alone() {
    let path = Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)], false);
    let c = compensate(
        &path,
        &DragParams {
            offset_mm: 0.0,
            ..params()
        },
    );
    assert_eq!(c.path.start, path.start);
    assert_eq!(c.path.segs, path.segs);
}

#[test]
fn compensation_is_deterministic() {
    let path = Path::rect(0.0, 0.0, 20.0, 10.0);
    let p = DragParams {
        overcut_mm: 1.0,
        align_mm: 0.5,
        keep_start: false,
        ..params()
    };
    assert_eq!(compensate(&path, &p), compensate(&path, &p));
}

#[test]
fn cubic_edges_are_fitted_with_arcs_first() {
    let k = 0.552_284_749_830_793_4 * 10.0;
    let path = Path::new(pt(10.0, 0.0)).cubic_to(pt(10.0, k), pt(k, 10.0), pt(0.0, 10.0));
    let c = compensate(&path, &params());
    assert!(c.path.segs.iter().all(|s| !matches!(s, Seg::Cubic { .. })));
    assert_ne!(c.path.segs.len(), 0);
}
