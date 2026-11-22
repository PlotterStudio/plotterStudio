//! Ground truth for Bézier → arc fitting. The deviation of the fitted arcs
//! from the original cubic is measured by dense sampling against closed-form distances.

use kurbo::{CubicBez, ParamCurve};

use super::*;
use crate::arc;

const TOL: f64 = 0.002;

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// Distance from `p` to a segment.
fn dist_to_line(p: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0);
    (p - a.lerp(b, t)).hypot()
}

/// Distance from `p` to an arc (closed form).
fn dist_to_arc(p: Point, from: Point, to: Point, center: Point, ccw: bool) -> f64 {
    let r = (from - center).hypot();
    let total = arc::sweep(from, to, center, ccw).abs();
    let a0 = arc::angle_of(from, center);
    let along = if ccw {
        arc::angle_of(p, center) - a0
    } else {
        a0 - arc::angle_of(p, center)
    }
    .rem_euclid(std::f64::consts::TAU);
    if along <= total {
        ((p - center).hypot() - r).abs()
    } else {
        (p - from).hypot().min((p - to).hypot())
    }
}

/// Distance from `p` to the fitted chain.
fn dist_to_chain(p: Point, start: Point, segs: &[Seg]) -> f64 {
    let mut from = start;
    let mut best = f64::INFINITY;
    for seg in segs {
        let d = match *seg {
            Seg::Line { to } => dist_to_line(p, from, to),
            Seg::Arc { to, center, ccw } => dist_to_arc(p, from, to, center, ccw),
            Seg::Cubic { .. } => panic!("fitting must not return cubics"),
        };
        best = best.min(d);
        from = seg.to();
    }
    best
}

/// Largest distance from the original cubic to the fitted chain.
fn max_deviation(p0: Point, c1: Point, c2: Point, p3: Point, segs: &[Seg]) -> f64 {
    let c = CubicBez::new(p0, c1, c2, p3);
    (0..=400)
        .map(|i| dist_to_chain(c.eval(f64::from(i) / 400.0), p0, segs))
        .fold(0.0, f64::max)
}

#[test]
fn a_straight_cubic_becomes_one_line() {
    let segs = cubic_to_arcs(pt(0.0, 0.0), pt(1.0, 0.0), pt(2.0, 0.0), pt(3.0, 0.0), TOL);
    assert_eq!(segs, vec![Seg::Line { to: pt(3.0, 0.0) }]);
}

#[test]
fn a_degenerate_cubic_becomes_one_line() {
    let p = pt(1.0, 1.0);
    let segs = cubic_to_arcs(p, p, p, pt(4.0, 5.0), TOL);
    assert_eq!(segs, vec![Seg::Line { to: pt(4.0, 5.0) }]);
}

#[test]
fn a_circular_quarter_needs_only_a_few_arcs() {
    // Unit radius: the cubic itself is within 2.7e-4 of the circle, inside the tolerance, so one
    // biarc (two arcs) must suffice. (At r = 10 the cubic is 0.0027 off a circle and a single
    // circular fit legitimately cannot stay within 0.002 of it.)
    let k = 0.552_284_749_830_793_4;
    let (p0, c1, c2, p3) = (pt(1.0, 0.0), pt(1.0, k), pt(k, 1.0), pt(0.0, 1.0));
    let segs = cubic_to_arcs(p0, c1, c2, p3, TOL);
    assert!(!segs.is_empty() && segs.len() <= 2, "got {}", segs.len());
    assert!(max_deviation(p0, c1, c2, p3, &segs) <= TOL + 1e-9);
}

#[test]
fn fitted_chains_start_and_end_exactly_on_the_curve_ends() {
    let (p0, c1, c2, p3) = (pt(0.0, 0.0), pt(0.0, 5.0), pt(10.0, -5.0), pt(10.0, 0.0));
    let segs = cubic_to_arcs(p0, c1, c2, p3, TOL);
    assert_eq!(segs.last().unwrap().to(), p3);
    let first_end = segs[0].to();
    assert!((first_end - p0).hypot() > 0.0);
}

#[test]
fn an_s_curve_is_fitted_within_tolerance() {
    let (p0, c1, c2, p3) = (pt(0.0, 0.0), pt(0.0, 5.0), pt(10.0, -5.0), pt(10.0, 0.0));
    let segs = cubic_to_arcs(p0, c1, c2, p3, TOL);
    assert!(segs.len() >= 2);
    assert!(max_deviation(p0, c1, c2, p3, &segs) <= TOL + 1e-9);
}

#[test]
fn a_tighter_tolerance_gives_more_arcs() {
    let (p0, c1, c2, p3) = (pt(0.0, 0.0), pt(0.0, 5.0), pt(10.0, -5.0), pt(10.0, 0.0));
    let loose = cubic_to_arcs(p0, c1, c2, p3, 0.05);
    let tight = cubic_to_arcs(p0, c1, c2, p3, 0.0005);
    assert!(tight.len() > loose.len());
    assert!(max_deviation(p0, c1, c2, p3, &tight) <= 0.0005 + 1e-9);
}

#[test]
fn fitted_chains_are_tangent_continuous() {
    let (p0, c1, c2, p3) = (pt(0.0, 0.0), pt(2.0, 6.0), pt(8.0, 6.0), pt(10.0, 0.0));
    let segs = cubic_to_arcs(p0, c1, c2, p3, TOL);
    let mut from = p0;
    let mut prev_tangent: Option<Vec2> = None;
    for seg in &segs {
        let (t_start, t_end) = match *seg {
            Seg::Line { to } => {
                let d = (to - from).normalize();
                (d, d)
            }
            Seg::Arc { to, center, ccw } => {
                let s = if ccw { 1.0 } else { -1.0 };
                let tan = |p: Point| {
                    let v = p - center;
                    Vec2::new(-v.y, v.x).normalize() * s
                };
                (tan(from), tan(to))
            }
            Seg::Cubic { .. } => unreachable!(),
        };
        if let Some(prev) = prev_tangent {
            assert!((prev - t_start).hypot() < 1e-6, "kink between arcs");
        }
        prev_tangent = Some(t_end);
        from = seg.to();
    }
}

#[test]
fn arcs_have_consistent_radius() {
    let (p0, c1, c2, p3) = (pt(0.0, 0.0), pt(0.0, 5.0), pt(10.0, -5.0), pt(10.0, 0.0));
    let mut from = p0;
    for seg in cubic_to_arcs(p0, c1, c2, p3, TOL) {
        if let Seg::Arc { to, center, .. } = seg {
            assert!(((from - center).hypot() - (to - center).hypot()).abs() < 1e-6);
        }
        from = seg.to();
    }
}

#[test]
fn path_fitting_replaces_only_cubics() {
    let k = 0.552_284_749_830_793_4 * 10.0;
    let p = Path::new(pt(0.0, 0.0))
        .line_to(pt(10.0, 0.0))
        .arc_to(pt(10.0, 10.0), pt(10.0, 5.0), true)
        .cubic_to(
            pt(10.0 - k * 0.0, 10.0 + k),
            pt(0.0, 10.0 + k),
            pt(0.0, 10.0),
        )
        .closed();
    let fitted = path_to_arcs(&p, TOL);
    assert_eq!(fitted.start, p.start);
    assert!(fitted.closed);
    assert_eq!(fitted.segs[0], p.segs[0]);
    assert_eq!(fitted.segs[1], p.segs[1]);
    assert!(fitted.segs.iter().all(|s| !matches!(s, Seg::Cubic { .. })));
    assert_eq!(fitted.end(), p.end());
}
