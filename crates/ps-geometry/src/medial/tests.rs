//! Ground truth for the medial axis of thin shapes. Shapes are polygons in µm whose
//! centre lines are known by inspection: a strip of width w has its axis at mid-height from
//! w/2 in from each end, an L of equal arms meets at (w/2, w/2), and a step in a strip joins its
//! two axes along an exact parabola (points equidistant from the reflex corner and the far wall).

use super::*;
use crate::boolean::Shape;

const TOL: f64 = 2.0; // discretisation tolerance, µm

fn ring(points: &[(i64, i64)]) -> Ring {
    points.iter().map(|&(x, y)| PtUm::new(x, y)).collect()
}

fn strip(length: i64, width: i64) -> Shape {
    vec![ring(&[(0, 0), (length, 0), (length, width), (0, width)])]
}

#[expect(clippy::cast_precision_loss, reason = "test coordinates are tiny")]
fn f(v: i64) -> f64 {
    v as f64
}

fn near(a: i64, b: f64, eps: f64) -> bool {
    (f(a) - b).abs() <= eps
}

fn chain_length(chain: &[PtUm]) -> f64 {
    chain
        .windows(2)
        .map(|w| libm::hypot(f(w[1].x - w[0].x), f(w[1].y - w[0].y)))
        .sum()
}

#[test]
fn a_strip_has_one_centre_line_at_mid_height() {
    let m = medial_axis(&strip(10_000, 300), TOL).unwrap();
    assert_eq!(m.chains.len(), 1, "{:?}", m.chains);
    let chain = &m.chains[0];
    assert!(chain.iter().all(|p| near(p.y, 150.0, 2.0)), "{chain:?}");
    let xs: Vec<i64> = chain.iter().map(|p| p.x).collect();
    let (lo, hi) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
    assert!(near(lo, 150.0, 3.0) && near(hi, 9_850.0, 3.0), "{lo}..{hi}");
    assert!((chain_length(chain) - 9_700.0).abs() < 6.0);
}

#[test]
fn the_inscribed_radius_is_half_the_width() {
    let m = medial_axis(&strip(10_000, 300), TOL).unwrap();
    assert!((m.max_radius_um - 150.0).abs() < 2.0, "{}", m.max_radius_um);
}

#[test]
fn a_square_has_no_axis_but_reports_its_radius() {
    // The X-shaped bisectors of a square all end on its corners, so no centre line is kept.
    let m = medial_axis(&strip(300, 300), TOL).unwrap();
    assert!(m.chains.is_empty(), "{:?}", m.chains);
    assert!((m.max_radius_um - 150.0).abs() < 2.0);
}

#[test]
fn an_l_shaped_strip_has_one_bent_centre_line() {
    let shape = vec![ring(&[
        (0, 0),
        (3_000, 0),
        (3_000, 300),
        (300, 300),
        (300, 3_000),
        (0, 3_000),
    ])];
    let m = medial_axis(&shape, TOL).unwrap();
    assert_eq!(m.chains.len(), 1, "{:?}", m.chains);
    let chain = &m.chains[0];
    let ends = [chain[0], chain[chain.len() - 1]];
    let near_point = |p: PtUm, x: f64, y: f64| near(p.x, x, 10.0) && near(p.y, y, 10.0);
    let ok = (near_point(ends[0], 2_850.0, 150.0) && near_point(ends[1], 150.0, 2_850.0))
        || (near_point(ends[1], 2_850.0, 150.0) && near_point(ends[0], 150.0, 2_850.0));
    assert!(ok, "ends {ends:?}");
    // The arms meet on the diagonal where the distance to the two outer walls (p) equals the
    // distance to the reflex corner (300, 300): p = sqrt(2) (300 - p), so p = 175.7. A circle
    // there touches both outer walls and the reflex vertex.
    let p = 300.0 * std::f64::consts::SQRT_2 / (1.0 + std::f64::consts::SQRT_2);
    assert!(
        chain.iter().any(|q| near_point(*q, p, p)),
        "junction near {p}"
    );
}

#[test]
fn the_step_between_two_widths_follows_a_parabola() {
    // Strip of width 300 for x < 6000, width 500 beyond; the reflex corner is (6000, 300).
    let shape = vec![ring(&[
        (0, 0),
        (12_000, 0),
        (12_000, 500),
        (6_000, 500),
        (6_000, 300),
        (0, 300),
    ])];
    let m = medial_axis(&shape, TOL).unwrap();
    // The axis has a T-junction where the parabola meets the wide strip's own axis, with a short
    // branch toward the step wall: three chains in all.
    assert_eq!(m.chains.len(), 3, "{:?}", m.chains);
    let chain: Vec<PtUm> = m.chains.iter().flatten().copied().collect();
    let xs: Vec<i64> = chain.iter().map(|p| p.x).collect();
    assert!(near(*xs.iter().min().unwrap(), 150.0, 5.0));
    assert!(near(*xs.iter().max().unwrap(), 11_750.0, 5.0));

    // Narrow part at y = 150, wide part at y = 250.
    for p in chain.iter().filter(|p| p.x < 5_990) {
        assert!(near(p.y, 150.0, 2.0), "{p:?}");
    }
    for p in chain.iter().filter(|p| p.x > 6_260 && p.x < 11_700) {
        assert!(near(p.y, 250.0, 2.0), "{p:?}");
    }
    // In between, every vertex lies on h(x) = ((x - 6000)^2 + 300^2) / (2 * 300).
    let transition: Vec<&PtUm> = chain
        .iter()
        .filter(|p| p.x > 6_000 && p.x < 6_245 && p.y <= 250)
        .collect();
    assert!(transition.len() >= 2, "the parabola must be discretised");
    for p in transition {
        let dx = f(p.x - 6_000);
        let expected = (dx * dx + 90_000.0) / 600.0;
        assert!(near(p.y, expected, 3.0), "{p:?}: expected y = {expected}");
    }
}

#[test]
fn every_axis_point_is_inside_and_clear_of_the_boundary() {
    let shape = vec![ring(&[
        (0, 0),
        (12_000, 0),
        (12_000, 500),
        (6_000, 500),
        (6_000, 300),
        (0, 300),
    ])];
    let m = medial_axis(&shape, TOL).unwrap();
    for p in &m.chains[0] {
        // Distance to the polygon boundary is at least the local half-width less rounding.
        assert!(p.y >= 145 && p.y <= 355, "{p:?}");
    }
}

#[test]
fn shapes_with_holes_are_supported() {
    // A frame: outer 3000 x 3000, hole 2400 x 2400, so a 300-wide ring. Its axis is a closed loop.
    let outer = ring(&[(0, 0), (3_000, 0), (3_000, 3_000), (0, 3_000)]);
    let mut hole = ring(&[(300, 300), (2_700, 300), (2_700, 2_700), (300, 2_700)]);
    hole.reverse();
    let m = medial_axis(&[outer, hole], TOL).unwrap();
    assert_ne!(m.chains.len(), 0);
    let total: f64 = m.chains.iter().map(|c| chain_length(c)).sum();
    // The loop runs at 150 µm in: a 2700 x 2700 square of perimeter 10800, less the four
    // corner bisectors that end on input vertices.
    assert!(total > 9_000.0 && total < 11_000.0, "{total}");
    // At each corner the largest circle also touches the hole's corner: radius 175.7, not 150.
    let corner = 300.0 * std::f64::consts::SQRT_2 / (1.0 + std::f64::consts::SQRT_2);
    assert!(
        (m.max_radius_um - corner).abs() < 3.0,
        "{}",
        m.max_radius_um
    );
}

#[test]
fn degenerate_input_is_an_error_not_a_panic() {
    assert!(medial_axis(&[], TOL).is_err());
    let line = [ring(&[(0, 0), (10, 0)])];
    assert!(medial_axis(&line, TOL).is_err());
}

#[test]
fn coordinates_beyond_the_engine_range_are_rejected() {
    let huge = vec![ring(&[
        (0, 0),
        (5_000_000_000, 0),
        (5_000_000_000, 300),
        (0, 300),
    ])];
    assert_eq!(
        medial_axis(&huge, TOL).unwrap_err(),
        MedialError::OutOfRange
    );
}

#[test]
fn the_axis_is_deterministic() {
    let a = medial_axis(&strip(10_000, 300), TOL).unwrap();
    let b = medial_axis(&strip(10_000, 300), TOL).unwrap();
    assert_eq!(a.chains, b.chains);
}
