//! Ground truth for the deterministic cubic helpers: arc length, inverse arc length,
//! sub-segments and flattening, built from +, ×, ÷, √ and `libm` only.
//!
//! Expected values are geometric facts (a straight cubic is as long as its chord, lengths add)
//! or brute-force estimates computed independently in the test.

use std::f64::consts::FRAC_PI_2;

use super::*;

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn dist(a: Point, b: Point) -> f64 {
    libm::hypot(a.x - b.x, a.y - b.y)
}

/// A straight cubic with uneven control spacing: the parameter is not proportional to length.
fn straight() -> CubicBez {
    CubicBez::new(pt(0.0, 0.0), pt(1.0, 0.0), pt(2.5, 0.0), pt(3.0, 0.0))
}

fn s_curve() -> CubicBez {
    CubicBez::new(pt(0.0, 0.0), pt(0.0, 5.0), pt(10.0, -5.0), pt(10.0, 0.0))
}

fn quarter_circle() -> CubicBez {
    let k = 0.552_284_749_830_793_4;
    CubicBez::new(pt(1.0, 0.0), pt(1.0, k), pt(k, 1.0), pt(0.0, 1.0))
}

/// Length by summing a million tiny chords, independent of the implementation under test.
fn brute_force_length(c: &CubicBez) -> f64 {
    let n = 1_000_000;
    let mut prev = c.p0;
    let mut total = 0.0;
    for i in 1..=n {
        let p = eval(c, f64::from(i) / f64::from(n));
        total += dist(prev, p);
        prev = p;
    }
    total
}

fn dist_to_polyline(p: Point, poly: &[Point]) -> f64 {
    poly.windows(2)
        .map(|w| {
            let ab = w[1] - w[0];
            let len2 = ab.x * ab.x + ab.y * ab.y;
            let t = if len2 == 0.0 {
                0.0
            } else {
                (((p.x - w[0].x) * ab.x + (p.y - w[0].y) * ab.y) / len2).clamp(0.0, 1.0)
            };
            dist(p, pt(w[0].x + ab.x * t, w[0].y + ab.y * t))
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn eval_hits_the_end_points_and_a_known_midpoint() {
    let c = s_curve();
    assert_eq!(eval(&c, 0.0), c.p0);
    assert_eq!(eval(&c, 1.0), c.p3);
    // B(1/2) = (p0 + 3 p1 + 3 p2 + p3) / 8 = (30 / 8, 0) for this curve.
    let mid = eval(&c, 0.5);
    assert!((mid.x - 5.0).abs() < 1e-12 && mid.y.abs() < 1e-12);
}

#[test]
fn a_straight_cubic_is_as_long_as_its_chord() {
    assert!((arclen(&straight()) - 3.0).abs() < 1e-12);
}

#[test]
fn inverse_arclength_of_a_straight_cubic_lands_on_the_chord() {
    let c = straight();
    for fraction in [0.1, 0.25, 0.5, 0.9] {
        let t = inv_arclen(&c, 3.0 * fraction);
        let p = eval(&c, t);
        assert!(
            (p.x - 3.0 * fraction).abs() < 1e-9,
            "fraction {fraction}: x = {}",
            p.x
        );
    }
}

#[test]
fn inverse_arclength_clamps_to_the_curve() {
    let c = s_curve();
    assert!(inv_arclen(&c, 0.0).abs() < 1e-12);
    assert!(inv_arclen(&c, -5.0).abs() < 1e-12);
    assert!((inv_arclen(&c, arclen(&c)) - 1.0).abs() < 1e-9);
    assert!((inv_arclen(&c, 1.0e9) - 1.0).abs() < 1e-12);
}

#[test]
fn lengths_add_across_a_split() {
    let c = s_curve();
    let full = arclen(&c);
    for t in [0.1, 0.37, 0.5, 0.8] {
        let sum = arclen_range(&c, 0.0, t) + arclen_range(&c, t, 1.0);
        assert!((sum - full).abs() < 1e-10 * full, "t = {t}");
    }
}

#[test]
fn length_matches_a_brute_force_estimate() {
    for c in [s_curve(), quarter_circle()] {
        let expected = brute_force_length(&c);
        assert!(
            (arclen(&c) - expected).abs() < 1e-6,
            "{} vs {expected}",
            arclen(&c)
        );
    }
}

#[test]
fn the_circular_quarter_cubic_is_close_to_a_quarter_circle() {
    // The standard approximation is within about 2.7e-4 of the true arc.
    assert!((arclen(&quarter_circle()) - FRAC_PI_2).abs() < 5e-4);
}

#[test]
fn a_degenerate_cubic_has_no_length() {
    let p = pt(2.0, 3.0);
    let c = CubicBez::new(p, p, p, p);
    assert!(arclen(&c).abs() < 1e-15);
    assert!(inv_arclen(&c, 1.0).is_finite());
}

#[test]
fn subsegments_share_the_original_curve() {
    let c = s_curve();
    let s = subsegment(&c, 0.25, 0.75);
    assert!(dist(s.p0, eval(&c, 0.25)) < 1e-12);
    assert!(dist(s.p3, eval(&c, 0.75)) < 1e-12);
    // Interior points agree: s(u) = c(0.25 + 0.5 u).
    for u in [0.2, 0.5, 0.9] {
        assert!(
            dist(eval(&s, u), eval(&c, 0.25 + 0.5 * u)) < 1e-12,
            "u = {u}"
        );
    }
    assert!((arclen(&s) - arclen_range(&c, 0.25, 0.75)).abs() < 1e-9);
}

#[test]
fn halves_cover_the_whole_curve() {
    let c = s_curve();
    let (a, b) = split_half(&c);
    assert_eq!(a.p0, c.p0);
    assert_eq!(b.p3, c.p3);
    assert!(dist(a.p3, b.p0) < 1e-12);
    assert!((arclen(&a) + arclen(&b) - arclen(&c)).abs() < 1e-9);
}

#[test]
fn flattening_a_straight_cubic_needs_one_chord() {
    assert_eq!(flatten(&straight(), 0.002), vec![pt(3.0, 0.0)]);
}

#[test]
fn flattening_ends_exactly_on_the_curve_end() {
    for c in [s_curve(), quarter_circle()] {
        assert_eq!(flatten(&c, 0.002).last().copied(), Some(c.p3));
    }
}

#[test]
fn flattening_stays_within_the_tolerance() {
    for tol in [0.05, 0.002, 0.0005] {
        for c in [s_curve(), quarter_circle()] {
            let mut poly = vec![c.p0];
            poly.extend(flatten(&c, tol));
            let worst = (0..=2_000)
                .map(|i| dist_to_polyline(eval(&c, f64::from(i) / 2_000.0), &poly))
                .fold(0.0, f64::max);
            assert!(worst <= tol, "tol {tol}: deviation {worst}");
        }
    }
}

#[test]
fn tighter_tolerances_give_more_chords() {
    let c = s_curve();
    assert!(flatten(&c, 0.0005).len() > flatten(&c, 0.05).len());
}

#[test]
fn flattening_is_repeatable() {
    let c = s_curve();
    assert_eq!(flatten(&c, 0.002), flatten(&c, 0.002));
}

#[test]
fn flattening_a_degenerate_cubic_returns_its_end() {
    let p = pt(1.0, 1.0);
    assert_eq!(flatten(&CubicBez::new(p, p, p, p), 0.002), vec![p]);
}
