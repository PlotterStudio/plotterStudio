//! Ground truth for refitting polylines with curves.
//!
//! Budget: the polyline itself is within 0.002 mm of the true shape, the µm snap adds
//! 0.0005 mm, and the refit may add 0.002 mm, so fitted curves must lie within 0.0045 mm of the
//! ideal shape. Distances to ideal shapes are closed-form, not taken from the implementation.

use std::f64::consts::PI;

use kurbo::CubicBez;

use super::*;
use crate::boolean::{BoolOp, FillRule};
use crate::cubic;
use crate::path_boolean::{BooleanError, Fallback, Grid, PathBoolean, RefitGrid};

const TOL: f64 = 0.002;
const IDEAL_BUDGET: f64 = 0.0045;

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn dist(a: Point, b: Point) -> f64 {
    libm::hypot(a.x - b.x, a.y - b.y)
}

fn circle_ring(r: f64, tol: f64) -> Vec<Point> {
    crate::flatten::flatten(&Path::circle(pt(0.0, 0.0), r), tol)
}

/// Dense samples along a path (lines, arcs not expected here, cubics sampled by parameter).
fn sample(path: &Path, per_segment: u32) -> Vec<Point> {
    let mut out = Vec::new();
    for (from, seg) in path.segments() {
        match *seg {
            Seg::Line { to } => {
                for i in 0..per_segment {
                    out.push(from.lerp(to, f64::from(i) / f64::from(per_segment)));
                }
            }
            Seg::Cubic { c1, c2, to } => {
                let c = CubicBez::new(from, c1, c2, to);
                for i in 0..per_segment {
                    out.push(cubic::eval(&c, f64::from(i) / f64::from(per_segment)));
                }
            }
            Seg::Arc { .. } => panic!("refit must not produce arcs"),
        }
    }
    out.push(path.end());
    out
}

fn dist_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let len2 = ab.x * ab.x + ab.y * ab.y;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / len2).clamp(0.0, 1.0)
    };
    dist(p, pt(a.x + ab.x * t, a.y + ab.y * t))
}

/// Distance from `p` to the fitted path, sampled densely.
fn dist_to_path(p: Point, path: &Path) -> f64 {
    let pts = sample(path, 400);
    pts.windows(2)
        .map(|w| dist_to_segment(p, w[0], w[1]))
        .fold(f64::INFINITY, f64::min)
}

fn count(path: &Path) -> (usize, usize) {
    let lines = path
        .segs
        .iter()
        .filter(|s| matches!(s, Seg::Line { .. }))
        .count();
    let cubics = path
        .segs
        .iter()
        .filter(|s| matches!(s, Seg::Cubic { .. }))
        .count();
    (lines, cubics)
}

// ---- refit_ring ----

#[test]
fn a_flattened_circle_becomes_a_few_cubics() {
    let ring = circle_ring(10.0, TOL);
    let fitted = refit_ring(&ring, TOL);
    assert!(fitted.closed);
    let (lines, cubics) = count(&fitted);
    assert_eq!(lines, 0, "a smooth ring needs no lines");
    assert!(
        (4..=16).contains(&cubics),
        "{cubics} cubics for {} vertices",
        ring.len()
    );
    assert!(fitted.segs.len() * 10 < ring.len(), "must be far smaller");
}

#[test]
fn a_fitted_circle_stays_within_the_error_budget() {
    let fitted = refit_ring(&circle_ring(10.0, TOL), TOL);
    let worst = sample(&fitted, 200)
        .iter()
        .map(|p| (dist(*p, pt(0.0, 0.0)) - 10.0).abs())
        .fold(0.0, f64::max);
    assert!(worst <= IDEAL_BUDGET, "deviation {worst}");
}

#[test]
fn every_input_vertex_is_within_tolerance_of_the_fit() {
    let ring = circle_ring(10.0, TOL);
    let fitted = refit_ring(&ring, TOL);
    for p in &ring {
        let d = dist_to_path(*p, &fitted);
        assert!(d <= TOL + 1e-4, "vertex {p:?} is {d} from the fit");
    }
}

#[test]
fn a_fitted_circle_keeps_its_area() {
    let fitted = refit_ring(&circle_ring(10.0, TOL), TOL);
    // Perimeter x tolerance bounds the area error.
    assert!((fitted.signed_area() - 100.0 * PI).abs() < 2.0 * PI * 10.0 * IDEAL_BUDGET);
}

#[test]
fn a_square_stays_four_lines() {
    let ring = [pt(0.0, 0.0), pt(4.0, 0.0), pt(4.0, 4.0), pt(0.0, 4.0)];
    let fitted = refit_ring(&ring, TOL);
    assert_eq!(count(&fitted), (4, 0));
    assert_eq!(fitted.start, pt(0.0, 0.0));
    assert!(fitted.closed);
    assert!((fitted.signed_area() - 16.0).abs() < 1e-12);
}

#[test]
fn collinear_vertices_merge_into_one_line() {
    let ring = [
        pt(0.0, 0.0),
        pt(2.0, 0.0),
        pt(4.0, 0.0),
        pt(4.0, 2.0),
        pt(4.0, 4.0),
        pt(0.0, 4.0),
    ];
    let fitted = refit_ring(&ring, TOL);
    assert_eq!(count(&fitted), (4, 0));
}

#[test]
fn repeated_vertices_are_ignored() {
    let ring = [
        pt(0.0, 0.0),
        pt(0.0, 0.0),
        pt(4.0, 0.0),
        pt(4.0, 4.0),
        pt(4.0, 4.0),
        pt(0.0, 4.0),
    ];
    assert_eq!(count(&refit_ring(&ring, TOL)), (4, 0));
}

#[test]
fn a_half_disc_keeps_both_corners() {
    // Upper half of a circle of radius 10 plus the diameter: two sharp 90° corners.
    let mut ring: Vec<Point> = circle_ring(10.0, TOL)
        .into_iter()
        .filter(|p| p.y > 1e-9)
        .collect();
    ring.insert(0, pt(10.0, 0.0));
    ring.push(pt(-10.0, 0.0));
    let fitted = refit_ring(&ring, TOL);
    let (lines, cubics) = count(&fitted);
    assert_eq!(lines, 1, "only the diameter is straight");
    assert!(cubics >= 2);
    for corner in [pt(10.0, 0.0), pt(-10.0, 0.0)] {
        let hit = sample(&fitted, 4).iter().any(|p| dist(*p, corner) < 1e-9);
        assert!(hit, "corner {corner:?} lost");
    }
}

#[test]
fn tiny_inputs_do_not_panic() {
    assert!(refit_ring(&[], TOL).is_empty());
    let one = refit_ring(&[pt(1.0, 1.0)], TOL);
    assert!(one.is_empty());
    let two = refit_ring(&[pt(0.0, 0.0), pt(1.0, 0.0)], TOL);
    assert_eq!(two.start, pt(0.0, 0.0));
}

#[test]
fn fitting_is_deterministic() {
    let ring = circle_ring(7.5, TOL);
    assert_eq!(refit_ring(&ring, TOL), refit_ring(&ring, TOL));
}

// ---- refit_polyline (open) ----

#[test]
fn a_smooth_open_polyline_fits_with_exact_ends() {
    // A sine arch sampled finely: smooth, so one or a few cubics.
    let pts: Vec<Point> = (0..=200)
        .map(|i| {
            let x = f64::from(i) * 0.05;
            pt(x, 3.0 * libm::sin(x * PI / 10.0))
        })
        .collect();
    let fitted = refit_polyline(&pts, false, TOL);
    assert!(!fitted.closed);
    assert_eq!(fitted.start, pts[0]);
    assert_eq!(fitted.end(), pts[200]);
    assert!(fitted.segs.len() <= 6);
    for p in &pts {
        assert!(dist_to_path(*p, &fitted) <= TOL + 1e-4);
    }
}

#[test]
fn a_straight_open_polyline_is_one_line() {
    let pts: Vec<Point> = (0..=10)
        .map(|i| pt(f64::from(i), f64::from(i) * 0.5))
        .collect();
    let fitted = refit_polyline(&pts, false, TOL);
    assert_eq!(count(&fitted), (1, 0));
    assert_eq!(fitted.end(), pt(10.0, 5.0));
}

// ---- the refitting boolean engine ----

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Path {
    Path::rect(x0, y0, x1 - x0, y1 - y0)
}

#[test]
fn the_refit_engine_returns_curves_for_a_circle() {
    let big = rect(-50.0, -50.0, 50.0, 50.0);
    let out = RefitGrid
        .apply(
            &[Path::circle(pt(0.0, 0.0), 10.0)],
            &[big],
            BoolOp::Intersect,
            FillRule::NonZero,
        )
        .unwrap();
    assert_eq!(out.len(), 1);
    let (lines, cubics) = count(&out[0]);
    assert_eq!(lines, 0);
    assert!((4..=16).contains(&cubics), "{cubics}");
    assert!((out[0].signed_area().abs() - 100.0 * PI).abs() < 2.0 * PI * 10.0 * IDEAL_BUDGET);
}

#[test]
fn a_quarter_disc_keeps_its_sharp_corners() {
    // Unit circle at the origin clipped to the first quadrant: corners at (0,0), (1,0), (0,1).
    let quadrant = rect(0.0, 0.0, 5.0, 5.0);
    let out = RefitGrid
        .apply(
            &[Path::circle(pt(0.0, 0.0), 1.0)],
            &[quadrant],
            BoolOp::Intersect,
            FillRule::NonZero,
        )
        .unwrap();
    assert_eq!(out.len(), 1);
    let (lines, cubics) = count(&out[0]);
    assert_eq!(lines, 2);
    assert!(cubics >= 1);
    for corner in [pt(0.0, 0.0), pt(1.0, 0.0), pt(0.0, 1.0)] {
        let hit = sample(&out[0], 4)
            .iter()
            .any(|p| dist(*p, corner) < IDEAL_BUDGET);
        assert!(hit, "corner {corner:?} lost");
    }
    assert!((out[0].signed_area().abs() - PI / 4.0).abs() < 2.0 * PI * 0.25 * IDEAL_BUDGET + 0.002);
}

#[test]
fn the_refit_engine_leaves_rectangles_as_lines() {
    let out = RefitGrid
        .apply(
            &[rect(0.0, 0.0, 4.0, 4.0)],
            &[rect(2.0, 2.0, 6.0, 6.0)],
            BoolOp::Union,
            FillRule::NonZero,
        )
        .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(count(&out[0]).1, 0);
    assert!((out[0].signed_area().abs() - 28.0).abs() < 1e-3);
}

#[test]
fn the_refit_engine_reports_bad_input() {
    let bad = Path::polyline(&[pt(0.0, 0.0), pt(f64::NAN, 0.0), pt(1.0, 1.0)], true);
    let r = RefitGrid.apply(
        std::slice::from_ref(&bad),
        &[rect(0.0, 0.0, 1.0, 1.0)],
        BoolOp::Union,
        FillRule::NonZero,
    );
    assert!(r.is_err());
}

#[test]
fn a_fallback_chain_ends_in_curves_not_polylines() {
    struct Failing;
    impl PathBoolean for Failing {
        fn apply(
            &self,
            _: &[Path],
            _: &[Path],
            _: BoolOp,
            _: FillRule,
        ) -> Result<Vec<Path>, BooleanError> {
            Err(BooleanError::Engine("sweep failed".into()))
        }
    }
    let big = rect(-50.0, -50.0, 50.0, 50.0);
    let circle = [Path::circle(pt(0.0, 0.0), 10.0)];
    let curved = Fallback::new(Failing, RefitGrid)
        .apply(
            &circle,
            std::slice::from_ref(&big),
            BoolOp::Intersect,
            FillRule::NonZero,
        )
        .unwrap();
    let flat = Fallback::new(Failing, Grid)
        .apply(
            &circle,
            std::slice::from_ref(&big),
            BoolOp::Intersect,
            FillRule::NonZero,
        )
        .unwrap();
    assert!(curved[0].segs.len() * 5 < flat[0].segs.len());
    assert!(
        curved[0]
            .segs
            .iter()
            .any(|s| matches!(s, Seg::Cubic { .. }))
    );
}
