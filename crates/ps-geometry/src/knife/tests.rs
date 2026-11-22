//! Ground truth for the knife: a straight cut through a path splits it where it
//! crosses. An open path becomes open pieces; a closed shape cut at two places becomes two closed
//! halves joined along the cut. A cut that misses leaves the path alone. Expected lengths and
//! areas are worked out by hand.

use super::*;
use crate::Point;

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn square() -> Path {
    Path::rect(0.0, 0.0, 10.0, 10.0)
}

#[test]
fn a_cut_that_misses_changes_nothing() {
    assert!(knife(&square(), p(20.0, -5.0), p(20.0, 15.0)).is_none());
    // Along the way but short of the shape.
    assert!(knife(&square(), p(5.0, 20.0), p(5.0, 12.0)).is_none());
}

#[test]
fn a_cut_across_a_line_splits_it_in_two() {
    let line = Path::polyline(&[p(0.0, 0.0), p(10.0, 0.0)], false);
    let pieces = knife(&line, p(4.0, -1.0), p(4.0, 1.0)).expect("cut");
    assert_eq!(pieces.len(), 2);
    assert!((pieces[0].length() - 4.0).abs() < 1e-9);
    assert!((pieces[1].length() - 6.0).abs() < 1e-9);
    assert!((pieces[1].start.x - 4.0).abs() < 1e-9);
}

#[test]
fn two_cuts_make_three_pieces() {
    // Two straight cuts, one after the other, at x = 3 and x = 7.
    let line = Path::polyline(&[p(0.0, 0.0), p(10.0, 0.0)], false);
    let first = knife(&line, p(3.0, -1.0), p(3.0, 1.0)).unwrap();
    assert_eq!(first.len(), 2);
    let second = knife(&first[1], p(7.0, -1.0), p(7.0, 1.0)).unwrap();
    assert_eq!(second.len(), 2);
    assert!((second[0].length() - 4.0).abs() < 1e-9);
    assert!((second[1].length() - 3.0).abs() < 1e-9);
}

#[test]
fn a_cut_through_a_square_makes_two_closed_halves() {
    // A vertical cut at x = 4 splits the 10 x 10 square into 4 x 10 and 6 x 10 rectangles.
    let halves = knife(&square(), p(4.0, -2.0), p(4.0, 12.0)).expect("cut");
    assert_eq!(halves.len(), 2);
    assert!(halves.iter().all(|h| h.closed));
    let mut areas: Vec<f64> = halves.iter().map(|h| h.signed_area().abs()).collect();
    areas.sort_by(f64::total_cmp);
    assert!((areas[0] - 40.0).abs() < 1e-6, "{areas:?}");
    assert!((areas[1] - 60.0).abs() < 1e-6, "{areas:?}");
    // Each half's outline is its own rectangle: perimeters 28 and 32.
    let mut lengths: Vec<f64> = halves.iter().map(Path::length).collect();
    lengths.sort_by(f64::total_cmp);
    assert!(
        (lengths[0] - 28.0).abs() < 1e-6 && (lengths[1] - 32.0).abs() < 1e-6,
        "{lengths:?}"
    );
}

#[test]
fn the_halves_keep_the_squares_winding() {
    let original = square().signed_area();
    for half in knife(&square(), p(4.0, -2.0), p(4.0, 12.0)).unwrap() {
        assert!(
            half.signed_area() * original > 0.0,
            "same direction as the original"
        );
    }
}

#[test]
fn a_diagonal_cut_through_two_corners_makes_two_triangles() {
    let halves = knife(&square(), p(-1.0, -1.0), p(11.0, 11.0)).expect("cut");
    assert_eq!(halves.len(), 2);
    for h in &halves {
        assert!(
            (h.signed_area().abs() - 50.0).abs() < 1e-6,
            "{}",
            h.signed_area()
        );
    }
}

#[test]
fn a_cut_that_only_touches_a_corner_does_not_split() {
    assert!(knife(&square(), p(-5.0, 5.0), p(5.0, -5.0)).is_none());
}

#[test]
fn a_circle_cut_through_the_middle_makes_two_half_discs() {
    let circle = Path::circle(p(0.0, 0.0), 10.0);
    let halves = knife(&circle, p(0.0, -20.0), p(0.0, 20.0)).expect("cut");
    assert_eq!(halves.len(), 2);
    for h in &halves {
        let area = h.signed_area().abs();
        assert!(
            (area - std::f64::consts::PI * 100.0 / 2.0).abs() < 0.2,
            "{area}"
        );
    }
}

#[test]
fn a_closed_shape_cut_at_more_than_two_places_becomes_open_pieces() {
    // A U-shape polygon cut by a line across both arms crosses four times.
    let u = Path::polyline(
        &[
            p(0.0, 0.0),
            p(10.0, 0.0),
            p(10.0, 10.0),
            p(7.0, 10.0),
            p(7.0, 3.0),
            p(3.0, 3.0),
            p(3.0, 10.0),
            p(0.0, 10.0),
        ],
        true,
    );
    let pieces = knife(&u, p(-1.0, 6.0), p(11.0, 6.0)).expect("cut");
    assert_eq!(pieces.len(), 4, "four crossings give four open pieces");
    assert!(pieces.iter().all(|piece| !piece.closed));
}

#[test]
fn a_zero_length_cut_does_nothing() {
    assert!(knife(&square(), p(4.0, 4.0), p(4.0, 4.0)).is_none());
}

#[test]
fn crossings_are_reported_as_distances_along_the_path() {
    let c = crossings(&square(), p(4.0, -2.0), p(4.0, 12.0));
    assert_eq!(c.len(), 2);
    // From the top-left corner going along the top edge: 4 mm; the bottom edge crossing comes after
    // the right edge (10) and the bottom edge's 6 mm: 10 + 10 + 6 = 26.
    assert!((c[0] - 4.0).abs() < 1e-9, "{c:?}");
    assert!((c[1] - 26.0).abs() < 1e-9, "{c:?}");
}
