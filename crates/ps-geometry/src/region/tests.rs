//! Ground truth for the region test used to decide whether a pen-down link stays inside a fill.

use super::*;

fn ring(points: &[(i64, i64)]) -> Ring {
    points.iter().map(|&(x, y)| PtUm::new(x, y)).collect()
}

fn p(x: i64, y: i64) -> PtUm {
    PtUm::new(x, y)
}

fn square() -> Region {
    Region::new(&[ring(&[(0, 0), (10_000, 0), (10_000, 10_000), (0, 10_000)])])
}

/// A square with a 4 mm square hole in the middle.
fn holed() -> Region {
    Region::new(&[
        ring(&[(0, 0), (10_000, 0), (10_000, 10_000), (0, 10_000)]),
        ring(&[
            (3_000, 3_000),
            (3_000, 7_000),
            (7_000, 7_000),
            (7_000, 3_000),
        ]),
    ])
}

/// A U: two 2 mm legs joined by a 2 mm base, with a notch between the legs.
fn letter_u() -> Region {
    Region::new(&[ring(&[
        (0, 0),
        (10_000, 0),
        (10_000, 10_000),
        (8_000, 10_000),
        (8_000, 2_000),
        (2_000, 2_000),
        (2_000, 10_000),
        (0, 10_000),
    ])])
}

#[test]
fn a_chord_inside_is_inside() {
    assert!(square().contains_segment(p(1_000, 1_000), p(9_000, 9_000)));
}

#[test]
fn a_chord_leaving_the_region_is_not_inside() {
    assert!(!square().contains_segment(p(1_000, 1_000), p(11_000, 1_000)));
}

#[test]
fn a_chord_along_the_boundary_is_inside() {
    assert!(square().contains_segment(p(0, 1_000), p(0, 9_000)));
    assert!(square().contains_segment(p(1_000, 0), p(9_000, 0)));
}

#[test]
fn a_chord_across_a_hole_is_not_inside() {
    assert!(!holed().contains_segment(p(1_000, 5_000), p(9_000, 5_000)));
    assert!(holed().contains_segment(p(1_000, 1_000), p(9_000, 1_000)));
}

#[test]
fn a_chord_across_a_notch_is_not_inside() {
    // Between the legs of the U, above the base.
    assert!(!letter_u().contains_segment(p(1_900, 8_000), p(8_100, 8_000)));
    // Along one leg.
    assert!(letter_u().contains_segment(p(1_000, 1_000), p(1_000, 9_000)));
}

#[test]
fn a_chord_cutting_a_corner_of_the_notch_is_not_inside() {
    // From the top of the left leg to the base, passing through the notch's corner.
    assert!(!letter_u().contains_segment(p(1_000, 9_000), p(3_000, 1_000)));
}

#[test]
fn a_chord_through_the_notch_corner_is_judged_by_where_it_goes_next() {
    // Touches the corner (2000, 2000) between the leg and the base, and stays in the metal.
    assert!(letter_u().contains_segment(p(1_000, 3_000), p(3_000, 1_000)));
    // Passes through the same corner and on into the notch.
    assert!(!letter_u().contains_segment(p(1_000, 1_000), p(3_000, 3_000)));
}

#[test]
fn a_point_is_inside_a_holed_region_only_outside_the_hole() {
    let r = holed();
    assert!(r.contains(p(1_000, 1_000)));
    assert!(!r.contains(p(5_000, 5_000)));
    assert!(!r.contains(p(-500, 5_000)));
}

#[test]
fn an_empty_region_contains_nothing() {
    let r = Region::new(&[]);
    assert!(!r.contains(p(0, 0)));
    assert!(!r.contains_segment(p(0, 0), p(1, 1)));
}

#[test]
fn a_degenerate_chord_is_inside_when_its_point_is() {
    assert!(square().contains_segment(p(5_000, 5_000), p(5_000, 5_000)));
    assert!(!square().contains_segment(p(-5_000, 5_000), p(-5_000, 5_000)));
}
