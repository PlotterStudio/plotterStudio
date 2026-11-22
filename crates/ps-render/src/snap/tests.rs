//! Ground truth for snapping: candidates within 8 screen px (grid, mat edges, usable-area
//! edges, guides, bounding-box anchors, nodes, segment midpoints, intersections). The nearest wins,
//! with the priority node > intersection > anchor > guide > grid to break ties.

use ps_geometry::{Point, Rect};

use super::*;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn cand(x: f64, y: f64, kind: SnapKind) -> Candidate {
    Candidate {
        point: Point::new(x, y),
        kind,
    }
}

#[test]
fn the_snap_radius_is_eight_pixels() {
    assert!(close(SNAP_RADIUS_PX, 8.0));
    assert!(close(snap_radius_mm(4.0), 2.0));
}

#[test]
fn the_nearest_candidate_inside_the_radius_wins() {
    let cs = [
        cand(10.0, 10.0, SnapKind::Grid),
        cand(11.0, 10.0, SnapKind::Grid),
    ];
    let s = snap(Point::new(10.8, 10.0), &cs, 4.0).unwrap();
    assert!(close(s.point.x, 11.0));
}

#[test]
fn nothing_beyond_the_radius_snaps() {
    let cs = [cand(10.0, 10.0, SnapKind::Node)];
    // 4 px/mm: radius 2 mm.
    assert!(snap(Point::new(12.5, 10.0), &cs, 4.0).is_none());
    assert!(snap(Point::new(11.9, 10.0), &cs, 4.0).is_some());
}

#[test]
fn a_tie_goes_to_the_higher_priority_kind() {
    let here = |k| cand(10.0, 10.0, k);
    for (winner, loser) in [
        (SnapKind::Node, SnapKind::Intersection),
        (SnapKind::Intersection, SnapKind::Anchor),
        (SnapKind::Anchor, SnapKind::Guide),
        (SnapKind::Guide, SnapKind::Grid),
        (SnapKind::Node, SnapKind::Grid),
    ] {
        let s = snap(Point::new(10.1, 10.0), &[here(loser), here(winner)], 4.0).unwrap();
        assert_eq!(s.kind, winner, "{winner:?} should beat {loser:?}");
    }
}

#[test]
fn a_clearly_nearer_low_priority_candidate_still_wins_over_a_far_high_priority_one() {
    let cs = [
        cand(10.0, 10.0, SnapKind::Grid),
        cand(11.5, 10.0, SnapKind::Node),
    ];
    // The cursor is on the grid point; the node is 1.5 mm = 6 px away: nearest wins.
    let s = snap(Point::new(10.0, 10.0), &cs, 4.0).unwrap();
    assert_eq!(s.kind, SnapKind::Grid);
}

#[test]
fn the_snap_explains_what_it_hit() {
    assert_eq!(SnapKind::Node.label(), "node");
    assert_eq!(SnapKind::Intersection.label(), "intersection");
    assert_eq!(SnapKind::Anchor.label(), "bounding-box anchor");
    assert_eq!(SnapKind::Guide.label(), "guide");
    assert_eq!(SnapKind::Grid.label(), "grid");
    assert_eq!(SnapKind::Midpoint.label(), "midpoint");
    assert_eq!(SnapKind::Edge.label(), "edge");
}

#[test]
fn a_midpoint_ranks_with_the_anchors_and_an_edge_with_the_guides() {
    assert!(SnapKind::Midpoint.priority() > SnapKind::Guide.priority());
    assert!(SnapKind::Edge.priority() <= SnapKind::Guide.priority());
    assert!(SnapKind::Node.priority() > SnapKind::Intersection.priority());
    assert!(SnapKind::Intersection.priority() > SnapKind::Anchor.priority());
    assert!(SnapKind::Anchor.priority() > SnapKind::Guide.priority());
    assert!(SnapKind::Guide.priority() > SnapKind::Grid.priority());
}

#[test]
fn grid_snapping_rounds_to_the_nearest_line() {
    let p = grid_point(Point::new(12.4, 7.6), 5.0);
    assert!(close(p.x, 10.0) && close(p.y, 10.0));
    let q = grid_point(Point::new(-2.6, 2.4), 5.0);
    assert!(close(q.x, -5.0) && close(q.y, 0.0));
}

#[test]
fn a_zero_grid_does_not_snap() {
    let p = grid_point(Point::new(1.3, 2.7), 0.0);
    assert!(close(p.x, 1.3) && close(p.y, 2.7));
}

#[test]
fn bounding_box_anchors_are_corners_edge_midpoints_and_centre() {
    let pts = bbox_anchors(Rect::new(0.0, 0.0, 10.0, 20.0));
    assert_eq!(pts.len(), 9);
    for expected in [
        (0.0, 0.0),
        (10.0, 0.0),
        (0.0, 20.0),
        (10.0, 20.0),
        (5.0, 0.0),
        (5.0, 20.0),
        (0.0, 10.0),
        (10.0, 10.0),
        (5.0, 10.0),
    ] {
        assert!(
            pts.iter()
                .any(|p| close(p.x, expected.0) && close(p.y, expected.1)),
            "{expected:?}"
        );
    }
}

#[test]
fn segment_midpoints_are_halfway_along_each_segment() {
    let poly = [
        Point::new(0.0, 0.0),
        Point::new(10.0, 0.0),
        Point::new(10.0, 6.0),
    ];
    let mids = segment_midpoints(&poly, false);
    assert_eq!(mids.len(), 2);
    assert!(close(mids[0].x, 5.0) && close(mids[0].y, 0.0));
    assert!(close(mids[1].x, 10.0) && close(mids[1].y, 3.0));
    // Closed adds the closing segment.
    assert_eq!(segment_midpoints(&poly, true).len(), 3);
}

#[test]
fn two_crossing_segments_intersect_at_the_crossing() {
    let p = segment_intersection(
        Point::new(0.0, 0.0),
        Point::new(10.0, 10.0),
        Point::new(0.0, 10.0),
        Point::new(10.0, 0.0),
    )
    .unwrap();
    assert!(close(p.x, 5.0) && close(p.y, 5.0));
}

#[test]
fn segments_that_do_not_reach_or_are_parallel_do_not_intersect() {
    assert!(
        segment_intersection(
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(5.0, 0.0),
            Point::new(5.0, 10.0)
        )
        .is_none()
    );
    assert!(
        segment_intersection(
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(0.0, 1.0),
            Point::new(10.0, 1.0)
        )
        .is_none()
    );
}

#[test]
fn touching_end_points_count_as_an_intersection() {
    let p = segment_intersection(
        Point::new(0.0, 0.0),
        Point::new(5.0, 5.0),
        Point::new(5.0, 5.0),
        Point::new(9.0, 0.0),
    )
    .unwrap();
    assert!(close(p.x, 5.0) && close(p.y, 5.0));
}

#[test]
fn polyline_intersections_find_every_crossing() {
    let a = [Point::new(0.0, 5.0), Point::new(10.0, 5.0)];
    let b = [
        Point::new(2.0, 0.0),
        Point::new(2.0, 10.0),
        Point::new(8.0, 10.0),
        Point::new(8.0, 0.0),
    ];
    let mut pts = polyline_intersections(&a, false, &b, false);
    pts.sort_by(|p, q| p.x.total_cmp(&q.x));
    assert_eq!(pts.len(), 2);
    assert!(close(pts[0].x, 2.0) && close(pts[1].x, 8.0));
}

#[test]
fn rectangle_edge_candidates_snap_to_the_nearest_point_on_the_edge() {
    let e = edge_point(Point::new(4.0, -1.0), Rect::new(0.0, 0.0, 10.0, 10.0));
    assert!(close(e.x, 4.0) && close(e.y, 0.0));
    let f = edge_point(Point::new(13.0, 5.0), Rect::new(0.0, 0.0, 10.0, 10.0));
    assert!(close(f.x, 10.0) && close(f.y, 5.0));
}
