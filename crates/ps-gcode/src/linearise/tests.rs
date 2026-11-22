//! Ground truth for arc linearisation: when a machine cannot do arcs they are
//! emitted as chords within the profile's chord tolerance. The chord's sagitta (distance from the
//! chord midpoint to the arc) bounds the error, so every chord must satisfy
//! `r - |midpoint - centre| <= tolerance`; end points are exact.

use ps_core::PtUm;

use super::*;

const MM: i64 = 1_000;

fn p(x: i64, y: i64) -> PtUm {
    PtUm::new(x * MM, y * MM)
}

fn dist(a: PtUm, b: PtUm) -> f64 {
    // Coordinates in these tests are well inside i32.
    let dx = f64::from(i32::try_from(a.x - b.x).unwrap());
    let dy = f64::from(i32::try_from(a.y - b.y).unwrap());
    libm::hypot(dx, dy)
}

fn max_sagitta(from: PtUm, chords: &[PtUm], centre: PtUm) -> f64 {
    let radius = dist(from, centre);
    let mut prev = from;
    let mut worst = 0.0_f64;
    for &next in chords {
        let mid = PtUm::new(i64::midpoint(prev.x, next.x), i64::midpoint(prev.y, next.y));
        worst = worst.max(radius - dist(mid, centre));
        prev = next;
    }
    worst
}

#[test]
fn a_quarter_circle_ends_exactly_on_the_target() {
    let chords = linearise(p(10, 0), p(0, 10), p(0, 0), true, 0.01);
    assert_eq!(chords.last().copied(), Some(p(0, 10)));
    assert!(chords.len() >= 2);
}

#[test]
fn chords_stay_within_the_tolerance() {
    for tol in [0.05, 0.01, 0.002] {
        let chords = linearise(p(10, 0), p(-10, 0), p(0, 0), true, tol);
        let worst = max_sagitta(p(10, 0), &chords, p(0, 0)) / 1_000.0;
        // One micrometre of slack for rounding the vertices to the grid.
        assert!(worst <= tol + 0.001, "tol {tol}: sagitta {worst}");
    }
}

#[test]
fn a_tighter_tolerance_needs_more_chords() {
    let coarse = linearise(p(10, 0), p(-10, 0), p(0, 0), true, 0.05).len();
    let fine = linearise(p(10, 0), p(-10, 0), p(0, 0), true, 0.005).len();
    assert!(fine > coarse, "{fine} vs {coarse}");
}

#[test]
fn every_vertex_lies_on_the_circle() {
    let chords = linearise(p(10, 0), p(0, -10), p(0, 0), false, 0.01);
    for v in &chords {
        assert!((dist(*v, p(0, 0)) - 10_000.0).abs() <= 1.0, "{v:?}");
    }
}

#[test]
fn counter_clockwise_and_clockwise_go_opposite_ways() {
    let ccw = linearise(p(10, 0), p(0, 10), p(0, 0), true, 0.5);
    let cw = linearise(p(10, 0), p(0, 10), p(0, 0), false, 0.5);
    // Counter-clockwise a quarter turn stays in the first quadrant...
    assert!(ccw.iter().all(|v| v.x >= 0 && v.y >= 0));
    // ...clockwise it takes the long way round through the other three.
    assert!(cw.iter().any(|v| v.y < 0));
    assert!(cw.len() > ccw.len());
}

#[test]
fn a_full_circle_is_a_whole_turn() {
    let chords = linearise(p(10, 0), p(10, 0), p(0, 0), true, 0.01);
    assert_eq!(chords.last().copied(), Some(p(10, 0)));
    assert!(chords.iter().any(|v| v.x < 0), "it goes all the way round");
    assert!(chords.iter().any(|v| v.y < 0));
}

#[test]
fn a_huge_tolerance_still_gives_a_chord() {
    let chords = linearise(p(10, 0), p(0, 10), p(0, 0), true, 100.0);
    assert_eq!(chords, vec![p(0, 10)]);
}

#[test]
fn a_degenerate_arc_is_one_line() {
    assert_eq!(
        linearise(p(5, 5), p(9, 9), p(5, 5), true, 0.01),
        vec![p(9, 9)]
    );
}

#[test]
fn linearising_is_deterministic() {
    let a = linearise(p(30, 0), p(0, 30), p(0, 0), true, 0.01);
    assert_eq!(a, linearise(p(30, 0), p(0, 30), p(0, 0), true, 0.01));
}
