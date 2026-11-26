//! Ground truth for hit testing: candidates come from the R*-tree with a box of 4
//! screen pixels around the cursor, then are tested exactly: distance to the curve for strokes
//! and open paths, winding number for filled shapes. The topmost (last drawn) hit wins.

use ps_geometry::{Path, Point, Rect};

use super::*;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Path {
    Path::polyline(&[Point::new(x0, y0), Point::new(x1, y1)], false)
}

#[test]
fn the_pick_radius_is_four_pixels() {
    assert!(close(PICK_RADIUS_PX, 4.0));
    assert!(close(pick_radius_mm(2.0), 2.0));
    assert!(close(pick_radius_mm(8.0), 0.5));
}

#[test]
fn distance_to_a_segment_is_perpendicular_inside_and_to_the_end_outside() {
    let a = Point::new(0.0, 0.0);
    let b = Point::new(10.0, 0.0);
    assert!(close(distance_to_segment(Point::new(5.0, 3.0), a, b), 3.0));
    assert!(close(distance_to_segment(Point::new(-4.0, 3.0), a, b), 5.0));
    assert!(close(
        distance_to_segment(Point::new(13.0, -4.0), a, b),
        5.0
    ));
    assert!(close(distance_to_segment(Point::new(5.0, 0.0), a, b), 0.0));
}

#[test]
fn a_degenerate_segment_is_a_point() {
    let a = Point::new(2.0, 2.0);
    assert!(close(distance_to_segment(Point::new(5.0, 6.0), a, a), 5.0));
}

#[test]
fn distance_to_a_path_is_the_distance_to_its_nearest_part() {
    let l = Path::polyline(
        &[
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ],
        false,
    );
    assert!(close(distance_to_path(Point::new(5.0, 2.0), &l, 0.01), 2.0));
    assert!(close(
        distance_to_path(Point::new(12.0, 5.0), &l, 0.01),
        2.0
    ));
}

#[test]
fn distance_to_a_circle_outline_is_the_gap_to_the_circumference() {
    let c = Path::circle(Point::new(0.0, 0.0), 10.0);
    let d = distance_to_path(Point::new(13.0, 0.0), &c, 0.001);
    assert!((d - 3.0).abs() < 0.001, "{d}");
    let inside = distance_to_path(Point::new(0.0, 0.0), &c, 0.001);
    assert!((inside - 10.0).abs() < 0.001, "{inside}");
}

#[test]
fn the_winding_number_tells_inside_from_outside() {
    let sq = Path::rect(0.0, 0.0, 10.0, 10.0);
    assert_ne!(winding_number(Point::new(5.0, 5.0), &sq, 0.01), 0);
    assert_eq!(winding_number(Point::new(15.0, 5.0), &sq, 0.01), 0);
    assert_eq!(winding_number(Point::new(5.0, -1.0), &sq, 0.01), 0);
}

#[test]
fn the_winding_number_sign_follows_the_direction() {
    let sq = Path::rect(0.0, 0.0, 10.0, 10.0);
    let a = winding_number(Point::new(5.0, 5.0), &sq, 0.01);
    let b = winding_number(Point::new(5.0, 5.0), &sq.reversed(), 0.01);
    assert_eq!(a, -b);
    assert_eq!(a.abs(), 1);
}

#[test]
fn a_stroke_is_hit_within_the_pick_radius_only() {
    let items = [Pickable {
        id: 1_u32,
        path: line(0.0, 0.0, 10.0, 0.0),
        filled: false,
    }];
    // 2 px/mm: radius 2 mm.
    assert_eq!(pick(&items, Point::new(5.0, 1.5), 2.0), Some(1));
    assert_eq!(pick(&items, Point::new(5.0, 2.5), 2.0), None);
}

#[test]
fn zooming_in_makes_the_pick_area_smaller_in_the_world() {
    let items = [Pickable {
        id: 1_u32,
        path: line(0.0, 0.0, 10.0, 0.0),
        filled: false,
    }];
    assert_eq!(pick(&items, Point::new(5.0, 1.0), 2.0), Some(1));
    assert_eq!(pick(&items, Point::new(5.0, 1.0), 20.0), None);
}

#[test]
fn the_inside_of_a_filled_shape_is_a_hit_but_not_the_inside_of_an_outline() {
    let filled = [Pickable {
        id: 1_u32,
        path: Path::rect(0.0, 0.0, 20.0, 20.0),
        filled: true,
    }];
    let outline = [Pickable {
        id: 2_u32,
        path: Path::rect(0.0, 0.0, 20.0, 20.0),
        filled: false,
    }];
    assert_eq!(pick(&filled, Point::new(10.0, 10.0), 2.0), Some(1));
    assert_eq!(pick(&outline, Point::new(10.0, 10.0), 2.0), None);
    assert_eq!(
        pick(&outline, Point::new(0.5, 10.0), 2.0),
        Some(2),
        "its edge still hits"
    );
}

#[test]
fn the_topmost_hit_wins() {
    let items = [
        Pickable {
            id: 1_u32,
            path: Path::rect(0.0, 0.0, 20.0, 20.0),
            filled: true,
        },
        Pickable {
            id: 2_u32,
            path: Path::rect(5.0, 5.0, 10.0, 10.0),
            filled: true,
        },
    ];
    assert_eq!(pick(&items, Point::new(10.0, 10.0), 2.0), Some(2));
    assert_eq!(pick(&items, Point::new(2.0, 2.0), 2.0), Some(1));
}

#[test]
fn a_hit_on_a_line_beats_an_overlapping_fill_below_it_only_by_order() {
    let items = [
        Pickable {
            id: 1_u32,
            path: Path::rect(0.0, 0.0, 20.0, 20.0),
            filled: true,
        },
        Pickable {
            id: 2_u32,
            path: line(0.0, 10.0, 20.0, 10.0),
            filled: false,
        },
    ];
    assert_eq!(pick(&items, Point::new(10.0, 10.0), 2.0), Some(2));
}

#[test]
fn nothing_under_the_cursor_is_no_hit() {
    let items = [Pickable {
        id: 1_u32,
        path: Path::rect(0.0, 0.0, 5.0, 5.0),
        filled: true,
    }];
    assert_eq!(pick(&items, Point::new(100.0, 100.0), 2.0), None);
    let none: [Pickable<u32>; 0] = [];
    assert_eq!(pick(&none, Point::new(0.0, 0.0), 2.0), None);
}

#[test]
fn a_box_selection_takes_everything_inside_or_crossing() {
    let items = [
        Pickable {
            id: 1_u32,
            path: Path::rect(0.0, 0.0, 5.0, 5.0),
            filled: true,
        },
        Pickable {
            id: 2_u32,
            path: Path::rect(20.0, 20.0, 5.0, 5.0),
            filled: true,
        },
        Pickable {
            id: 3_u32,
            path: line(8.0, 2.0, 30.0, 2.0),
            filled: false,
        },
    ];
    let mut crossing = pick_in_box(&items, Rect::new(-1.0, -1.0, 10.0, 10.0), BoxMode::Crossing);
    crossing.sort_unstable();
    assert_eq!(crossing, vec![1, 3]);
    let mut enclosed = pick_in_box(&items, Rect::new(-1.0, -1.0, 10.0, 10.0), BoxMode::Enclosed);
    enclosed.sort_unstable();
    assert_eq!(enclosed, vec![1], "the long line is not fully inside");
}

#[test]
fn the_innermost_pick_reaches_a_shape_inside_a_larger_one_drawn_over_it() {
    let square = |id: u32, at: f64, size: f64| Pickable {
        id,
        path: Path::rect(at, at, size, size),
        filled: true,
    };
    // The small square is drawn first, so the large one lies on top of it.
    let items = [square(1, 5.0, 4.0), square(2, 0.0, 20.0)];
    assert_eq!(pick(&items, Point::new(7.0, 7.0), 4.0), Some(2));
    assert_eq!(pick_innermost(&items, Point::new(7.0, 7.0), 4.0), Some(1));
    assert_eq!(
        pick_innermost(&items, Point::new(15.0, 15.0), 4.0),
        Some(2),
        "outside the small one"
    );
    assert_eq!(
        pick_innermost(&items, Point::new(0.5, 10.0), 4.0),
        Some(2),
        "on the large outline"
    );
    assert_eq!(pick_innermost(&items, Point::new(40.0, 40.0), 4.0), None);
}
