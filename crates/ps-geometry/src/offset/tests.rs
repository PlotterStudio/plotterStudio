//! Ground truth for offsets and outlines. Expected areas are closed-form: a square
//! grown by d with round corners gains 4·s·d + π·d², with mitred corners 4·s·d + 4·d², and with
//! bevelled corners 4·s·d + 2·d².

use std::f64::consts::PI;

use super::*;
use crate::boolean::{Shape, shapes_area2};

const MM2: i128 = 1_000_000;

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn sq(x0: i64, y0: i64, x1: i64, y1: i64) -> Ring {
    vec![
        PtUm::new(x0, y0),
        PtUm::new(x1, y0),
        PtUm::new(x1, y1),
        PtUm::new(x0, y1),
    ]
}

fn square10() -> Vec<Shape> {
    vec![vec![sq(0, 0, 10_000, 10_000)]]
}

/// Area in mm² of a set of shapes.
fn mm2(shapes: &[Shape]) -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "test areas are far below 2^53")]
    let a = shapes_area2(shapes) as f64;
    a / 2.0 / 1.0e6
}

fn arc_area(paths: &[Path]) -> f64 {
    paths.iter().map(Path::signed_area).sum()
}

// ---- grid offsets (i_overlay) ----

#[test]
fn mitre_outset_gives_a_larger_square() {
    let out = offset_shapes(&square10(), 1_000, Join::Miter { limit: 4.0 });
    assert_eq!(out.len(), 1);
    assert_eq!(shapes_area2(&out), 2 * 144 * MM2); // 12 x 12 mm
}

#[test]
fn bevel_outset_cuts_the_corners() {
    let out = offset_shapes(&square10(), 1_000, Join::Bevel);
    // 144 mm² minus four corner triangles of 0.5 mm² each.
    assert_eq!(shapes_area2(&out), 2 * 142 * MM2);
}

#[test]
fn round_outset_adds_a_quarter_disc_per_corner() {
    let out = offset_shapes(&square10(), 1_000, Join::Round);
    let expected = 140.0 + 100.0 - 100.0 + PI; // 100 + 4*10*1 + pi
    assert!((mm2(&out) - (expected)).abs() < 0.05, "area {}", mm2(&out));
    // A polygonal approximation of the disc lies inside it.
    assert!(mm2(&out) <= expected + 1e-9);
}

#[test]
fn inset_shrinks_the_square_exactly() {
    for join in [Join::Round, Join::Bevel, Join::Miter { limit: 4.0 }] {
        let out = offset_shapes(&square10(), -1_000, join);
        assert_eq!(shapes_area2(&out), 2 * 64 * MM2, "join {join:?}");
    }
}

#[test]
fn insetting_past_the_middle_leaves_nothing() {
    let result = offset_shapes(&square10(), -6_000, Join::Round);
    assert_eq!(result.len(), 0);
}

#[test]
fn outsetting_a_hole_shrinks_it() {
    let mut hole = sq(4_000, 4_000, 6_000, 6_000);
    hole.reverse();
    let shape = vec![vec![sq(0, 0, 10_000, 10_000), hole]];
    let out = offset_shapes(&shape, 500, Join::Miter { limit: 4.0 });
    // Outer grows to 11x11 = 121; the hole shrinks to 1x1 = 1.
    assert_eq!(shapes_area2(&out), 2 * 120 * MM2);
    assert_eq!(out[0].len(), 2);
}

#[test]
fn zero_offset_leaves_the_area_alone() {
    let out = offset_shapes(&square10(), 0, Join::Round);
    assert_eq!(shapes_area2(&out), 2 * 100 * MM2);
}

#[test]
fn miter_limit_falls_back_to_a_bevel_at_sharp_corners() {
    // A thin triangle has a very sharp tip; a low limit must not spike far out.
    let tri: Vec<Shape> = vec![vec![vec![
        PtUm::new(0, 0),
        PtUm::new(20_000, 0),
        PtUm::new(0, 1_000),
    ]]];
    let spiked = offset_shapes(&tri, 500, Join::Miter { limit: 100.0 });
    let limited = offset_shapes(&tri, 500, Join::Miter { limit: 2.0 });
    assert!(shapes_area2(&limited) < shapes_area2(&spiked));
}

// ---- strokes ----

#[test]
fn a_butt_capped_stroke_is_a_rectangle() {
    let line = [PtUm::new(0, 0), PtUm::new(10_000, 0)];
    let out = stroke_outline(&line, 1_000, false, Cap::Butt, Join::Bevel);
    assert_eq!(shapes_area2(&out), 2 * 10 * MM2);
}

#[test]
fn a_square_capped_stroke_extends_by_half_the_width() {
    let line = [PtUm::new(0, 0), PtUm::new(10_000, 0)];
    let out = stroke_outline(&line, 1_000, false, Cap::Square, Join::Bevel);
    assert_eq!(shapes_area2(&out), 2 * 11 * MM2);
}

#[test]
fn a_round_capped_stroke_adds_a_disc() {
    let line = [PtUm::new(0, 0), PtUm::new(10_000, 0)];
    let out = stroke_outline(&line, 1_000, false, Cap::Round, Join::Round);
    let expected = 10.0 + PI * 0.25; // r = 0.5
    assert!((mm2(&out) - expected).abs() < 0.02, "area {}", mm2(&out));
}

#[test]
fn a_closed_stroke_is_a_ring() {
    let ring = [
        PtUm::new(0, 0),
        PtUm::new(10_000, 0),
        PtUm::new(10_000, 10_000),
        PtUm::new(0, 10_000),
    ];
    let out = stroke_outline(&ring, 1_000, true, Cap::Butt, Join::Miter { limit: 4.0 });
    // Outer 11x11 minus inner 9x9.
    assert_eq!(shapes_area2(&out), 2 * (121 - 81) * MM2);
    assert_eq!(out[0].len(), 2);
}

// ---- arc-preserving offsets (cavalier_contours) ----

#[test]
fn circle_outset_is_a_bigger_circle_of_arcs() {
    let out = offset_arcs(&Path::circle(pt(0.0, 0.0), 10.0), 1.0, 0.002);
    assert_eq!(out.len(), 1);
    assert!(out[0].closed);
    assert!(out[0].segs.iter().all(|s| matches!(s, Seg::Arc { .. })));
    assert!((out[0].length() - 2.0 * PI * 11.0).abs() < 1e-6);
    assert!((arc_area(&out) - PI * 121.0).abs() < 1e-6);
}

#[test]
fn circle_inset_is_a_smaller_circle() {
    let out = offset_arcs(&Path::circle(pt(0.0, 0.0), 10.0), -2.0, 0.002);
    assert_eq!(out.len(), 1);
    assert!((arc_area(&out) - PI * 64.0).abs() < 1e-6);
}

#[test]
fn clockwise_circles_also_grow_with_a_positive_offset() {
    let cw = Path::circle(pt(0.0, 0.0), 10.0).reversed();
    let out = offset_arcs(&cw, 1.0, 0.002);
    assert_eq!(out.len(), 1);
    assert!((arc_area(&out).abs() - PI * 121.0).abs() < 1e-6);
}

#[test]
fn square_outset_gets_round_corners() {
    let out = offset_arcs(&Path::rect(0.0, 0.0, 10.0, 10.0), 1.0, 0.002);
    assert_eq!(out.len(), 1);
    assert!((arc_area(&out) - (100.0 + 40.0 + PI)).abs() < 1e-6);
    assert_eq!(
        out[0]
            .segs
            .iter()
            .filter(|s| matches!(s, Seg::Arc { .. }))
            .count(),
        4
    );
}

#[test]
fn square_inset_keeps_sharp_corners() {
    let out = offset_arcs(&Path::rect(0.0, 0.0, 10.0, 10.0), -1.0, 0.002);
    assert_eq!(out.len(), 1);
    assert!((arc_area(&out) - 64.0).abs() < 1e-6);
    assert!(out[0].segs.iter().all(|s| matches!(s, Seg::Line { .. })));
}

#[test]
fn insetting_past_the_middle_leaves_nothing_in_arc_space() {
    let result = offset_arcs(&Path::rect(0.0, 0.0, 10.0, 10.0), -6.0, 0.002);
    assert_eq!(result.len(), 0);
}

#[test]
fn an_open_path_offsets_to_its_left_for_positive_distances() {
    let line = Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0)], false);
    let out = offset_arcs(&line, 2.0, 0.002);
    assert_eq!(out.len(), 1);
    assert!(!out[0].closed);
    assert!((out[0].start.y - 2.0).abs() < 1e-9);
    assert!((out[0].length() - 10.0).abs() < 1e-9);
    let right = offset_arcs(&line, -2.0, 0.002);
    assert!((right[0].start.y + 2.0).abs() < 1e-9);
}

#[test]
fn cubic_edges_are_fitted_to_arcs_first() {
    // The standard four-cubic circle of radius 10.
    let k = 0.552_284_749_830_793_4 * 10.0;
    let c = Path::new(pt(10.0, 0.0))
        .cubic_to(pt(10.0, k), pt(k, 10.0), pt(0.0, 10.0))
        .cubic_to(pt(-k, 10.0), pt(-10.0, k), pt(-10.0, 0.0))
        .cubic_to(pt(-10.0, -k), pt(-k, -10.0), pt(0.0, -10.0))
        .cubic_to(pt(k, -10.0), pt(10.0, -k), pt(10.0, 0.0))
        .closed();
    let out = offset_arcs(&c, 1.0, 0.002);
    assert_eq!(out.len(), 1);
    assert!(out[0].segs.iter().all(|s| !matches!(s, Seg::Cubic { .. })));
    // Fit error (<= 0.002 + the cubic's own 0.003) keeps the area within a fraction of a mm².
    assert!((arc_area(&out) - PI * 121.0).abs() < 0.5);
}
