//! Ground truth for node editing: works on a path's own geometry; converting line and
//! curve and smooth and corner follow the usual vector-editor rules. Coordinates are hand
//! computed; a node insert on a cubic must leave the curve itself unchanged (de Casteljau).

use kurbo::{CubicBez, ParamCurve, Point};

use super::*;
use crate::path::{Path, Seg};

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn close(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
}

fn square() -> Path {
    Path::polyline(
        &[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)],
        true,
    )
}

fn open_line() -> Path {
    Path::polyline(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)], false)
}

fn curve() -> Path {
    Path::new(p(0.0, 0.0)).cubic_to(p(0.0, 10.0), p(10.0, 10.0), p(10.0, 0.0))
}

// ---- counting and reading ----

#[test]
fn an_open_path_has_one_more_node_than_segments_and_a_closed_one_as_many() {
    assert_eq!(node_count(&open_line()), 3);
    assert_eq!(node_count(&square()), 4);
    assert_eq!(node_count(&Path::new(p(1.0, 1.0))), 1);
}

#[test]
fn node_positions_are_the_segment_end_points_in_order() {
    let path = open_line();
    assert!(close(node_position(&path, 0).unwrap(), p(0.0, 0.0)));
    assert!(close(node_position(&path, 1).unwrap(), p(10.0, 0.0)));
    assert!(close(node_position(&path, 2).unwrap(), p(10.0, 10.0)));
    assert!(node_position(&path, 3).is_none());
}

#[test]
fn the_closing_point_of_a_closed_path_is_its_first_node_not_another() {
    let path = square();
    let positions: Vec<Point> = (0..node_count(&path))
        .filter_map(|i| node_position(&path, i))
        .collect();
    assert_eq!(positions.len(), 4);
    assert!(close(positions[0], p(0.0, 0.0)) && close(positions[3], p(0.0, 10.0)));
}

// ---- moving ----

#[test]
fn moving_a_middle_node_moves_both_its_segments_ends() {
    let moved = move_node(&open_line(), 1, p(12.0, 3.0)).unwrap();
    assert!(close(node_position(&moved, 1).unwrap(), p(12.0, 3.0)));
    assert!(close(node_position(&moved, 0).unwrap(), p(0.0, 0.0)));
    assert!(close(node_position(&moved, 2).unwrap(), p(10.0, 10.0)));
}

#[test]
fn moving_the_first_node_moves_the_start() {
    let moved = move_node(&open_line(), 0, p(-5.0, 2.0)).unwrap();
    assert!(close(moved.start, p(-5.0, 2.0)));
}

#[test]
fn moving_the_first_node_of_a_closed_path_keeps_it_closed() {
    let moved = move_node(&square(), 0, p(-2.0, -2.0)).unwrap();
    assert!(moved.closed);
    assert!(close(moved.start, p(-2.0, -2.0)));
    assert!(
        close(moved.end(), p(-2.0, -2.0)),
        "the closing segment follows"
    );
}

#[test]
fn moving_a_node_takes_its_curve_handles_with_it() {
    let path = Path::new(p(0.0, 0.0))
        .cubic_to(p(0.0, 10.0), p(10.0, 10.0), p(10.0, 0.0))
        .cubic_to(p(10.0, -10.0), p(20.0, -10.0), p(20.0, 0.0));
    let moved = move_node(&path, 1, p(12.0, 2.0)).unwrap(); // by (+2, +2)
    let segs: Vec<Seg> = moved.segs.clone();
    let Seg::Cubic { c2, to, .. } = segs[0] else {
        panic!()
    };
    assert!(close(to, p(12.0, 2.0)));
    assert!(
        close(c2, p(12.0, 12.0)),
        "the incoming handle moved by the same offset"
    );
    let Seg::Cubic { c1, .. } = segs[1] else {
        panic!()
    };
    assert!(close(c1, p(12.0, -8.0)), "and so did the outgoing handle");
}

#[test]
fn a_node_that_does_not_exist_cannot_be_moved() {
    assert!(move_node(&open_line(), 9, p(0.0, 0.0)).is_none());
}

#[test]
fn moving_a_node_next_to_an_arc_turns_the_arc_into_a_line() {
    let path = Path::new(p(0.0, 0.0)).arc_to(p(10.0, 10.0), p(0.0, 10.0), false);
    let moved = move_node(&path, 1, p(12.0, 12.0)).unwrap();
    assert!(
        matches!(moved.segs[0], Seg::Line { .. }),
        "an arc cannot follow a moved end"
    );
}

// ---- inserting ----

#[test]
fn inserting_on_a_line_splits_it_at_the_parameter() {
    let split = insert_node(&open_line(), 0, 0.25).unwrap();
    assert_eq!(node_count(&split), 4);
    assert!(close(node_position(&split, 1).unwrap(), p(2.5, 0.0)));
    assert!(close(node_position(&split, 2).unwrap(), p(10.0, 0.0)));
}

#[test]
fn inserting_on_a_curve_keeps_the_curve_unchanged() {
    let original = CubicBez::new(p(0.0, 0.0), p(0.0, 10.0), p(10.0, 10.0), p(10.0, 0.0));
    let split = insert_node(&curve(), 0, 0.3).unwrap();
    assert_eq!(node_count(&split), 3);
    // The new node lies on the original curve at t = 0.3.
    assert!(close(node_position(&split, 1).unwrap(), original.eval(0.3)));
    // Every point of the first half is on the original curve at a scaled parameter.
    let Seg::Cubic { c1, c2, to } = split.segs[0] else {
        panic!()
    };
    let first = CubicBez::new(p(0.0, 0.0), c1, c2, to);
    for k in 0..=10 {
        let u = f64::from(k) / 10.0;
        assert!(close(first.eval(u), original.eval(0.3 * u)), "u = {u}");
    }
    let Seg::Cubic { c1, c2, to } = split.segs[1] else {
        panic!()
    };
    let second = CubicBez::new(node_position(&split, 1).unwrap(), c1, c2, to);
    for k in 0..=10 {
        let u = f64::from(k) / 10.0;
        assert!(
            close(second.eval(u), original.eval(0.3 + 0.7 * u)),
            "u = {u}"
        );
    }
}

#[test]
fn inserting_on_an_arc_splits_the_angle_about_the_same_centre() {
    // A quarter circle of radius 10 about the origin from (10, 0) to (0, 10), counter-clockwise.
    let path = Path::new(p(10.0, 0.0)).arc_to(p(0.0, 10.0), p(0.0, 0.0), true);
    let split = insert_node(&path, 0, 0.5).unwrap();
    let mid = node_position(&split, 1).unwrap();
    let r = 10.0 / 2.0_f64.sqrt() * 1.0;
    assert!(
        (mid.x - r).abs() < 1e-9 && (mid.y - r).abs() < 1e-9,
        "{mid:?}"
    );
    assert!(matches!(split.segs[0], Seg::Arc { .. }) && matches!(split.segs[1], Seg::Arc { .. }));
}

#[test]
fn inserting_into_the_closing_segment_of_a_closed_path_works() {
    let split = insert_node(&square(), 3, 0.5).unwrap();
    assert_eq!(node_count(&split), 5);
    assert!(close(node_position(&split, 4).unwrap(), p(0.0, 5.0)));
    assert!(split.closed);
}

#[test]
fn inserting_at_an_end_or_into_a_missing_segment_is_refused() {
    assert!(insert_node(&open_line(), 0, 0.0).is_none());
    assert!(insert_node(&open_line(), 0, 1.0).is_none());
    assert!(insert_node(&open_line(), 7, 0.5).is_none());
    assert!(insert_node(&open_line(), 0, f64::NAN).is_none());
}

// ---- deleting ----

#[test]
fn deleting_a_middle_node_joins_its_neighbours_with_a_line() {
    let shorter = delete_node(&open_line(), 1).unwrap();
    assert_eq!(node_count(&shorter), 2);
    assert!(close(node_position(&shorter, 1).unwrap(), p(10.0, 10.0)));
    assert!(matches!(shorter.segs[0], Seg::Line { .. }));
}

#[test]
fn deleting_an_end_node_shortens_the_path() {
    let first = delete_node(&open_line(), 0).unwrap();
    assert!(close(first.start, p(10.0, 0.0)));
    let last = delete_node(&open_line(), 2).unwrap();
    assert!(close(last.end(), p(10.0, 0.0)));
}

#[test]
fn deleting_the_first_node_of_a_closed_path_moves_the_start() {
    let triangle = delete_node(&square(), 0).unwrap();
    assert_eq!(node_count(&triangle), 3);
    assert!(triangle.closed);
    assert!(close(triangle.start, p(10.0, 0.0)));
}

#[test]
fn a_path_cannot_lose_its_last_nodes() {
    let line = Path::polyline(&[p(0.0, 0.0), p(1.0, 0.0)], false);
    assert!(
        delete_node(&line, 0).is_none(),
        "an open path keeps two nodes"
    );
    let triangle = Path::polyline(&[p(0.0, 0.0), p(1.0, 0.0), p(0.0, 1.0)], true);
    assert!(
        delete_node(&triangle, 1).is_none(),
        "a closed path keeps three"
    );
    assert!(delete_node(&open_line(), 9).is_none());
}

// ---- converting segments ----

#[test]
fn a_line_turned_into_a_curve_is_still_straight() {
    let c = segment_to_curve(&open_line(), 0).unwrap();
    let Seg::Cubic { c1, c2, to } = c.segs[0] else {
        panic!()
    };
    assert!(
        close(c1, p(10.0 / 3.0, 0.0)) && close(c2, p(20.0 / 3.0, 0.0)) && close(to, p(10.0, 0.0))
    );
}

#[test]
fn converting_a_curve_to_a_curve_changes_nothing() {
    assert_eq!(segment_to_curve(&curve(), 0).unwrap(), curve());
}

#[test]
fn a_curve_turned_into_a_line_keeps_its_end_points() {
    let l = segment_to_line(&curve(), 0).unwrap();
    assert!(matches!(l.segs[0], Seg::Line { to } if close(to, p(10.0, 0.0))));
}

#[test]
fn an_arc_can_become_a_line_or_a_curve_through_the_same_end_points() {
    let arc = Path::new(p(10.0, 0.0)).arc_to(p(0.0, 10.0), p(0.0, 0.0), true);
    assert!(matches!(
        segment_to_line(&arc, 0).unwrap().segs[0],
        Seg::Line { .. }
    ));
    let c = segment_to_curve(&arc, 0).unwrap();
    let Seg::Cubic { to, .. } = c.segs[0] else {
        panic!()
    };
    assert!(close(to, p(0.0, 10.0)));
}

#[test]
fn converting_a_segment_that_does_not_exist_is_refused() {
    assert!(segment_to_line(&open_line(), 5).is_none());
    assert!(segment_to_curve(&open_line(), 5).is_none());
}

// ---- smooth and corner nodes ----

#[test]
fn a_smooth_node_has_collinear_handles_along_the_neighbours_direction() {
    let path = Path::polyline(&[p(0.0, 0.0), p(10.0, 10.0), p(20.0, 0.0)], false);
    let smooth = smooth_node(&path, 1).unwrap();
    let Seg::Cubic { c2, to, .. } = smooth.segs[0] else {
        panic!()
    };
    let Seg::Cubic { c1, .. } = smooth.segs[1] else {
        panic!()
    };
    assert!(close(to, p(10.0, 10.0)));
    // The tangent at the apex is horizontal (from (0,0) to (20,0)); the handles lie on y = 10,
    // one on each side and an equal distance from the node.
    assert!((c2.y - 10.0).abs() < 1e-9 && (c1.y - 10.0).abs() < 1e-9);
    assert!(c2.x < 10.0 && c1.x > 10.0);
    let third = (200.0_f64).sqrt() / 3.0; // each neighbour is 10*sqrt(2) away
    assert!((c2.x - (10.0 - third)).abs() < 1e-9 && (c1.x - (10.0 + third)).abs() < 1e-9);
}

#[test]
fn smoothing_an_end_node_is_refused() {
    assert!(smooth_node(&open_line(), 0).is_none());
    assert!(smooth_node(&open_line(), 2).is_none());
}

#[test]
fn a_corner_node_has_its_handles_pulled_onto_it() {
    let path = Path::new(p(0.0, 0.0))
        .cubic_to(p(0.0, 10.0), p(10.0, 10.0), p(10.0, 0.0))
        .cubic_to(p(10.0, -10.0), p(20.0, -10.0), p(20.0, 0.0));
    let corner = corner_node(&path, 1).unwrap();
    let Seg::Cubic { c2, to, .. } = corner.segs[0] else {
        panic!()
    };
    let Seg::Cubic { c1, .. } = corner.segs[1] else {
        panic!()
    };
    assert!(close(c2, to) && close(c1, to));
}

// ---- finding where to insert ----

#[test]
fn the_nearest_point_on_a_path_names_the_segment_and_parameter() {
    let hit = nearest_on_path(&open_line(), p(5.0, 3.0), 0.01).unwrap();
    assert_eq!(hit.segment, 0);
    assert!((hit.t - 0.5).abs() < 1e-9);
    assert!((hit.distance - 3.0).abs() < 1e-9);
    let hit = nearest_on_path(&open_line(), p(12.0, 4.0), 0.01).unwrap();
    assert_eq!(hit.segment, 1);
    assert!((hit.t - 0.4).abs() < 1e-9);
    assert!(nearest_on_path(&Path::new(p(0.0, 0.0)), p(1.0, 1.0), 0.01).is_none());
}

#[test]
fn the_nearest_node_to_a_point_is_found_within_a_radius() {
    let path = open_line();
    assert_eq!(nearest_node(&path, p(10.3, 0.2), 1.0), Some(1));
    assert_eq!(nearest_node(&path, p(50.0, 50.0), 1.0), None);
    assert_eq!(nearest_node(&path, p(0.0, 0.0), 1.0), Some(0));
}

/// Two curves meeting at (10, 0): handles (7, 5) and (13, -5) are in line through the node.
fn two_curves() -> Path {
    let mut path = Path::new(p(0.0, 0.0));
    path.segs.push(Seg::Cubic {
        c1: p(2.0, 5.0),
        c2: p(7.0, 5.0),
        to: p(10.0, 0.0),
    });
    path.segs.push(Seg::Cubic {
        c1: p(13.0, -5.0),
        c2: p(18.0, -5.0),
        to: p(20.0, 0.0),
    });
    path
}

#[test]
fn a_node_between_two_curves_has_both_handles() {
    let path = two_curves();
    assert_eq!(
        node_handles(&path, 1),
        (Some(p(7.0, 5.0)), Some(p(13.0, -5.0)))
    );
    assert_eq!(node_handles(&path, 0), (None, Some(p(2.0, 5.0))));
    assert_eq!(node_handles(&path, 2), (Some(p(18.0, -5.0)), None));
}

#[test]
fn a_line_has_no_handles() {
    assert_eq!(node_handles(&open_line(), 0), (None, None));
}

#[test]
fn dragging_a_handle_moves_only_that_handle_when_not_smooth() {
    let moved = move_handle(&two_curves(), 1, true, p(14.0, -8.0), false).unwrap();
    assert_eq!(
        node_handles(&moved, 1),
        (Some(p(7.0, 5.0)), Some(p(14.0, -8.0)))
    );
}

#[test]
fn dragging_a_smooth_handle_swings_the_other_side_in_line_keeping_its_length() {
    // Drag the leaving handle straight up from the node: the arriving one points straight down,
    // and stays 5 long as it was (the distance from (10, 0) to (7, 5) is sqrt(34)).
    let moved = move_handle(&two_curves(), 1, true, p(10.0, 6.0), true).unwrap();
    let (incoming, outgoing) = node_handles(&moved, 1);
    assert_eq!(outgoing, Some(p(10.0, 6.0)));
    let incoming = incoming.unwrap();
    assert!((incoming.x - 10.0).abs() < 1e-9, "{incoming:?}");
    assert!(
        (incoming.y + 34.0_f64.sqrt()).abs() < 1e-9,
        "below the node, as long as before: {incoming:?}"
    );
}

#[test]
fn a_handle_that_does_not_exist_cannot_be_dragged() {
    assert!(move_handle(&open_line(), 0, true, p(1.0, 1.0), false).is_none());
    assert!(move_handle(&two_curves(), 0, false, p(1.0, 1.0), false).is_none());
}
