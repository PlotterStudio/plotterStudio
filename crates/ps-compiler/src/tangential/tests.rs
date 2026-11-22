//! Ground truth for tangential tools: the tool angle A follows the path heading,
//! unwrapped to a continuous value so the tool never spins 360 degrees to cross +-180; at corners
//! above the corner-lift angle the tool lifts, rotates and goes down again, so the stroke is split.

use ps_core::PtUm;

use super::*;

fn p(x: i64, y: i64) -> PtUm {
    PtUm::new(x * 1_000, y * 1_000)
}

#[test]
fn headings_are_in_milli_degrees_counter_clockwise_from_x() {
    assert_eq!(heading_mdeg(p(0, 0), p(10, 0)), 0);
    assert_eq!(heading_mdeg(p(0, 0), p(0, 10)), 90_000);
    assert_eq!(heading_mdeg(p(0, 0), p(-10, 0)), 180_000);
    assert_eq!(heading_mdeg(p(0, 0), p(0, -10)), -90_000);
    assert_eq!(heading_mdeg(p(0, 0), p(10, 10)), 45_000);
}

#[test]
fn unwrapping_picks_the_representation_nearest_the_previous_angle() {
    assert_eq!(unwrap_near_mdeg(-179_000, 179_000), 181_000);
    assert_eq!(unwrap_near_mdeg(179_000, -179_000), -181_000);
    assert_eq!(unwrap_near_mdeg(10_000, 0), 10_000);
    assert_eq!(unwrap_near_mdeg(0, 720_000), 720_000);
    assert_eq!(unwrap_near_mdeg(170_000, 540_000 + 5_000), 530_000);
}

#[test]
fn a_full_turn_never_spins_back() {
    // Four quarter turns anticlockwise: 0, 90, 180, 270 (not -90), 360.
    let pts = [p(0, 0), p(10, 0), p(10, 10), p(0, 10), p(0, 0), p(10, 0)];
    let a = unwrapped_angles(&pts, 0);
    assert_eq!(a, vec![0, 90_000, 180_000, 270_000, 360_000]);
}

#[test]
fn the_first_angle_continues_from_the_previous_stroke() {
    // Heading 180 deg from a previous 360 deg: 180 and 540 are equally near, the higher one wins.
    assert_eq!(
        unwrapped_angles(&[p(0, 0), p(-10, 0)], 360_000),
        vec![540_000]
    );
    // Heading -90 deg from a previous -360 deg: -450 is the nearest representation.
    assert_eq!(
        unwrapped_angles(&[p(0, 0), p(0, -10)], -360_000),
        vec![-450_000]
    );
}

#[test]
fn zero_length_edges_keep_the_previous_heading() {
    let pts = [p(0, 0), p(10, 0), p(10, 0), p(10, 10)];
    assert_eq!(unwrapped_angles(&pts, 0), vec![0, 0, 90_000]);
}

#[test]
fn gentle_polylines_stay_in_one_stroke() {
    let pts = [p(0, 0), p(10, 0), p(20, 1), p(30, 3)];
    let strokes = split_at_corners(&pts, 30.0);
    assert_eq!(strokes, vec![pts.to_vec()]);
}

#[test]
fn a_right_angle_splits_at_the_corner_when_the_threshold_is_lower() {
    let pts = [p(0, 0), p(10, 0), p(10, 10)];
    let strokes = split_at_corners(&pts, 30.0);
    assert_eq!(
        strokes,
        vec![vec![p(0, 0), p(10, 0)], vec![p(10, 0), p(10, 10)]]
    );
}

#[test]
fn a_corner_exactly_at_the_threshold_is_lifted() {
    let pts = [p(0, 0), p(10, 0), p(10, 10)];
    assert_eq!(split_at_corners(&pts, 90.0).len(), 2);
    assert_eq!(split_at_corners(&pts, 90.1).len(), 1);
}

#[test]
fn several_corners_make_several_strokes() {
    let pts = [p(0, 0), p(10, 0), p(10, 10), p(0, 10), p(0, 0)];
    let strokes = split_at_corners(&pts, 45.0);
    assert_eq!(strokes.len(), 4);
    assert!(strokes.iter().all(|s| s.len() == 2));
}

#[test]
fn degenerate_inputs_are_handled() {
    assert_eq!(split_at_corners(&[], 30.0).len(), 0);
    assert_eq!(split_at_corners(&[p(1, 1)], 30.0), vec![vec![p(1, 1)]]);
    assert_eq!(
        split_at_corners(&[p(0, 0), p(5, 5)], 30.0),
        vec![vec![p(0, 0), p(5, 5)]]
    );
}

#[test]
fn the_turn_between_headings_is_the_smaller_angle() {
    assert_eq!(turn_mdeg(0, 90_000), 90_000);
    assert_eq!(turn_mdeg(170_000, -170_000), 20_000);
    assert_eq!(turn_mdeg(-170_000, 170_000), 20_000);
    assert_eq!(turn_mdeg(0, 180_000), 180_000);
}
