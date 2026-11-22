//! Ground truth for flattening: polylines never deviate from the curve by more than
//! the requested tolerance. Deviation is measured against exact circles, not the implementation.

use std::f64::consts::PI;

use super::*;

const TOL: f64 = 0.002;

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn dist(a: Point, b: Point) -> f64 {
    (a - b).hypot()
}

/// Largest distance from the circle (centre `centre`, radius `radius`) over every chord of the
/// polyline, sampled densely along each chord.
fn max_chord_deviation(points: &[Point], closed: bool, centre: Point, radius: f64) -> f64 {
    let count = points.len();
    let edges = if closed { count } else { count - 1 };
    let mut worst = 0.0_f64;
    for i in 0..edges {
        let from = points[i];
        let to = points[(i + 1) % count];
        for step in 0..=20 {
            let along = from.lerp(to, f64::from(step) / 20.0);
            worst = worst.max((dist(along, centre) - radius).abs());
        }
    }
    worst
}

#[test]
fn a_polyline_flattens_to_its_own_vertices() {
    let p = Path::polyline(&[pt(0.0, 0.0), pt(3.0, 0.0), pt(3.0, 4.0)], false);
    assert_eq!(
        flatten(&p, TOL),
        vec![pt(0.0, 0.0), pt(3.0, 0.0), pt(3.0, 4.0)]
    );
}

#[test]
fn closed_paths_do_not_repeat_the_start_point() {
    let p = Path::rect(0.0, 0.0, 2.0, 1.0);
    let pts = flatten(&p, TOL);
    assert_eq!(
        pts,
        vec![pt(0.0, 0.0), pt(2.0, 0.0), pt(2.0, 1.0), pt(0.0, 1.0)]
    );
}

#[test]
fn an_empty_path_flattens_to_its_start() {
    assert_eq!(flatten(&Path::new(pt(1.0, 2.0)), TOL), vec![pt(1.0, 2.0)]);
}

#[test]
fn arc_vertices_lie_exactly_on_the_circle() {
    let c = Path::circle(pt(5.0, -3.0), 10.0);
    for p in flatten(&c, TOL) {
        assert!((dist(p, pt(5.0, -3.0)) - 10.0).abs() < 1e-9);
    }
}

#[test]
fn arc_chords_stay_within_tolerance() {
    let c = Path::circle(pt(0.0, 0.0), 10.0);
    let pts = flatten(&c, TOL);
    // Sagitta bound: chords lie inside the circle by at most TOL.
    assert!(max_chord_deviation(&pts, true, pt(0.0, 0.0), 10.0) <= TOL + 1e-9);
}

#[test]
fn arc_flattening_is_economical() {
    // Step 2*sqrt(2*tol/r) = 0.04 rad gives about 157 chords for a full circle of radius 10.
    let pts = flatten(&Path::circle(pt(0.0, 0.0), 10.0), TOL);
    assert!(pts.len() >= 150 && pts.len() <= 170, "got {}", pts.len());
}

#[test]
fn tighter_tolerance_gives_more_points() {
    let c = Path::circle(pt(0.0, 0.0), 10.0);
    assert!(flatten(&c, 0.0005).len() > flatten(&c, 0.01).len());
}

#[test]
fn open_arc_keeps_both_end_points() {
    let arc = Path::new(pt(10.0, 0.0)).arc_to(pt(0.0, 10.0), pt(0.0, 0.0), true);
    let pts = flatten(&arc, TOL);
    assert_eq!(pts.first().copied(), Some(pt(10.0, 0.0)));
    assert_eq!(pts.last().copied(), Some(pt(0.0, 10.0)));
}

#[test]
fn cubic_flattening_respects_the_tolerance() {
    // Unit-circle quarter approximated by one cubic; the cubic itself is within 3e-4 of the circle.
    let k = 0.552_284_749_830_793_4;
    let p = Path::new(pt(1.0, 0.0)).cubic_to(pt(1.0, k), pt(k, 1.0), pt(0.0, 1.0));
    let pts = flatten(&p, TOL);
    assert!(pts.len() > 2);
    let dev = max_chord_deviation(&pts, false, pt(0.0, 0.0), 1.0);
    assert!(dev <= TOL + 3.0e-4, "deviation {dev}");
}

#[test]
fn flattened_length_is_within_tolerance_of_the_curve_length() {
    // Full circle r = 10: true length 20*pi; chords are shorter by a tiny amount.
    let pts = flatten(&Path::circle(pt(0.0, 0.0), 10.0), TOL);
    let mut len = 0.0;
    for i in 0..pts.len() {
        len += dist(pts[i], pts[(i + 1) % pts.len()]);
    }
    assert!(len < 20.0 * PI);
    assert!(20.0 * PI - len < 0.01);
}

#[test]
fn flattening_is_deterministic() {
    let p = Path::new(pt(0.0, 0.0))
        .cubic_to(pt(0.0, 4.0), pt(4.0, 4.0), pt(4.0, 0.0))
        .arc_to(pt(0.0, 0.0), pt(2.0, 0.0), true);
    assert_eq!(flatten(&p, TOL), flatten(&p, TOL));
}

#[test]
fn flattened_points_snap_to_the_um_grid() {
    let p = Path::polyline(&[pt(0.0, 0.0), pt(12.345, 0.0), pt(12.345, 0.0004)], false);
    let g = flatten_to_grid(&p, TOL).unwrap();
    // Consecutive duplicates created by snapping are removed: (12.345, 0.0004) snaps to y = 0.
    assert_eq!(g, vec![PtUm::new(0, 0), PtUm::new(12_345, 0)]);
}

#[test]
fn grid_flattening_rejects_non_finite_input() {
    let p = Path::polyline(&[pt(0.0, 0.0), pt(f64::NAN, 0.0)], false);
    assert!(flatten_to_grid(&p, TOL).is_err());
}

#[test]
fn closed_grid_rings_drop_a_duplicated_closing_point() {
    let p = Path::rect(0.0, 0.0, 1.0, 1.0);
    let g = flatten_to_grid(&p, TOL).unwrap();
    assert_eq!(g.len(), 4);
    assert_ne!(g.first(), g.last());
}
