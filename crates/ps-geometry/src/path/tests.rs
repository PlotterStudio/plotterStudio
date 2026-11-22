//! Ground truth for the path type: lines, true circular arcs and cubic Béziers,
//! with hand-computed lengths, areas and bounds.

use std::f64::consts::{FRAC_PI_2, PI};

use super::*;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn near_pt(a: Point, b: Point) -> bool {
    near(a.x, b.x) && near(a.y, b.y)
}

fn unit_square() -> Path {
    Path::polyline(
        &[pt(0.0, 0.0), pt(1.0, 0.0), pt(1.0, 1.0), pt(0.0, 1.0)],
        true,
    )
}

#[test]
fn builder_records_segments_in_order() {
    let p = Path::new(pt(0.0, 0.0))
        .line_to(pt(10.0, 0.0))
        .arc_to(pt(10.0, 10.0), pt(10.0, 5.0), true)
        .cubic_to(pt(8.0, 12.0), pt(2.0, 12.0), pt(0.0, 10.0))
        .closed();
    assert_eq!(p.start, pt(0.0, 0.0));
    // `closed()` appends the closing line, so a closed path always ends where it starts.
    assert_eq!(p.segs.len(), 4);
    assert!(p.closed);
    assert_eq!(p.end(), pt(0.0, 0.0));
    assert!(matches!(p.segs[0], Seg::Line { .. }));
    assert!(matches!(p.segs[1], Seg::Arc { ccw: true, .. }));
    assert!(matches!(p.segs[2], Seg::Cubic { .. }));
}

#[test]
fn empty_path_has_its_start_as_end() {
    let p = Path::new(pt(3.0, 4.0));
    assert!(p.is_empty());
    assert_eq!(p.end(), pt(3.0, 4.0));
    assert!(near(p.length(), 0.0));
}

#[test]
fn rect_is_a_closed_four_line_path() {
    let r = Path::rect(1.0, 2.0, 3.0, 4.0);
    assert_eq!(r.segs.len(), 4);
    assert!(r.closed);
    assert!(near(r.length(), 14.0));
    assert!(near(r.signed_area(), 12.0));
}

#[test]
fn circle_is_two_arcs() {
    let c = Path::circle(pt(5.0, 5.0), 2.0);
    assert_eq!(c.segs.len(), 2);
    assert!(c.closed);
    assert!(
        c.segs
            .iter()
            .all(|s| matches!(s, Seg::Arc { ccw: true, .. }))
    );
    assert!(near(c.length(), 4.0 * PI));
    assert!(near(c.signed_area(), 4.0 * PI));
}

#[test]
fn length_of_lines_arcs_and_closing_segment() {
    // Open 3-4-5: two legs.
    let open = Path::polyline(&[pt(0.0, 0.0), pt(3.0, 0.0), pt(3.0, 4.0)], false);
    assert!(near(open.length(), 7.0));
    // Closing it adds the hypotenuse.
    let closed = Path::polyline(&[pt(0.0, 0.0), pt(3.0, 0.0), pt(3.0, 4.0)], true);
    assert!(near(closed.length(), 12.0));
    // Quarter circle of radius 2: length pi.
    let arc = Path::new(pt(2.0, 0.0)).arc_to(pt(0.0, 2.0), pt(0.0, 0.0), true);
    assert!(near(arc.length(), PI));
}

#[test]
fn cubic_length_matches_a_straight_cubic() {
    // A cubic whose controls lie on the line is exactly that long.
    let p = Path::new(pt(0.0, 0.0)).cubic_to(pt(1.0, 0.0), pt(2.0, 0.0), pt(3.0, 0.0));
    assert!((p.length() - 3.0).abs() < 1e-6);
}

#[test]
fn signed_area_is_positive_counter_clockwise() {
    assert!(near(unit_square().signed_area(), 1.0));
    assert!(near(unit_square().reversed().signed_area(), -1.0));
}

#[test]
fn signed_area_includes_arc_segments() {
    // Half disc: diameter along x from (-1,0) to (1,0) closed through the top arc.
    // Going (1,0) -> top -> (-1,0) is counter-clockwise; the closing chord returns along the axis.
    let half = Path::new(pt(1.0, 0.0))
        .arc_to(pt(-1.0, 0.0), pt(0.0, 0.0), true)
        .closed();
    assert!(near(half.signed_area(), PI / 2.0));
}

#[test]
fn signed_area_includes_cubic_segments() {
    // The standard 4-cubic circle approximation has area within 0.03% of pi.
    let k = 0.552_284_749_830_793_4;
    let p = Path::new(pt(1.0, 0.0))
        .cubic_to(pt(1.0, k), pt(k, 1.0), pt(0.0, 1.0))
        .cubic_to(pt(-k, 1.0), pt(-1.0, k), pt(-1.0, 0.0))
        .cubic_to(pt(-1.0, -k), pt(-k, -1.0), pt(0.0, -1.0))
        .cubic_to(pt(k, -1.0), pt(1.0, -k), pt(1.0, 0.0))
        .closed();
    assert!((p.signed_area() - PI).abs() < 1.0e-3);
}

#[test]
fn bounds_of_lines() {
    let b = unit_square().bounds();
    assert!(near(b.x0, 0.0) && near(b.y0, 0.0) && near(b.x1, 1.0) && near(b.y1, 1.0));
}

#[test]
fn bounds_of_an_arc_include_the_axis_extremes() {
    // The arc from (1,0) over the top to (-1,0) bulges to y = 1, beyond its endpoints.
    let arc = Path::new(pt(1.0, 0.0)).arc_to(pt(-1.0, 0.0), pt(0.0, 0.0), true);
    let b = arc.bounds();
    assert!(near(b.y1, 1.0));
    assert!(near(b.y0, 0.0));
    assert!(near(b.x0, -1.0) && near(b.x1, 1.0));
    // Full circle.
    let b = Path::circle(pt(5.0, 5.0), 2.0).bounds();
    assert!(near(b.x0, 3.0) && near(b.x1, 7.0) && near(b.y0, 3.0) && near(b.y1, 7.0));
}

#[test]
fn bounds_of_a_cubic_are_tight() {
    // Symmetric hump reaching y = 0.75 * 4 = 3 at t = 0.5.
    let p = Path::new(pt(0.0, 0.0)).cubic_to(pt(0.0, 4.0), pt(4.0, 4.0), pt(4.0, 0.0));
    let b = p.bounds();
    assert!(near(b.y1, 3.0));
    assert!(near(b.y0, 0.0));
}

#[test]
fn reversing_twice_gives_the_original() {
    let p = Path::new(pt(0.0, 0.0))
        .line_to(pt(10.0, 0.0))
        .arc_to(pt(10.0, 10.0), pt(10.0, 5.0), true)
        .cubic_to(pt(8.0, 12.0), pt(2.0, 12.0), pt(0.0, 10.0));
    assert_eq!(p.reversed().reversed(), p);
}

#[test]
fn reversing_swaps_ends_and_flips_arc_direction() {
    let p =
        Path::new(pt(0.0, 0.0))
            .line_to(pt(10.0, 0.0))
            .arc_to(pt(10.0, 10.0), pt(10.0, 5.0), true);
    let r = p.reversed();
    assert_eq!(r.start, pt(10.0, 10.0));
    assert_eq!(r.end(), pt(0.0, 0.0));
    assert!(matches!(r.segs[0], Seg::Arc { ccw: false, .. }));
    assert!(near(r.length(), p.length()));
}

#[test]
fn reversing_a_cubic_swaps_its_controls() {
    let p = Path::new(pt(0.0, 0.0)).cubic_to(pt(1.0, 2.0), pt(3.0, 4.0), pt(5.0, 0.0));
    let r = p.reversed();
    assert_eq!(r.start, pt(5.0, 0.0));
    match r.segs[0] {
        Seg::Cubic { c1, c2, to } => {
            assert_eq!(c1, pt(3.0, 4.0));
            assert_eq!(c2, pt(1.0, 2.0));
            assert_eq!(to, pt(0.0, 0.0));
        }
        _ => panic!("expected a cubic"),
    }
}

#[test]
fn translation_keeps_arcs() {
    let c = Path::circle(pt(0.0, 0.0), 1.0).transformed(Affine::translate((5.0, -2.0)));
    assert!(c.segs.iter().all(|s| matches!(s, Seg::Arc { .. })));
    assert!(near(c.bounds().x0, 4.0) && near(c.bounds().y1, -1.0));
}

#[test]
fn uniform_scale_and_rotation_keep_arcs_and_scale_the_radius() {
    let t = Affine::rotate(FRAC_PI_2) * Affine::scale(3.0);
    let c = Path::circle(pt(0.0, 0.0), 1.0).transformed(t);
    assert!(c.segs.iter().all(|s| matches!(s, Seg::Arc { .. })));
    assert!(near(c.length(), 6.0 * PI));
}

#[test]
fn mirroring_flips_arc_direction() {
    let c = Path::circle(pt(0.0, 0.0), 1.0).transformed(Affine::scale_non_uniform(-1.0, 1.0));
    assert!(
        c.segs
            .iter()
            .all(|s| matches!(s, Seg::Arc { ccw: false, .. }))
    );
    // The area is now negative (the path winds the other way).
    assert!(near(c.signed_area(), -PI));
}

#[test]
fn non_uniform_scale_turns_arcs_into_cubics() {
    let c = Path::circle(pt(0.0, 0.0), 1.0).transformed(Affine::scale_non_uniform(2.0, 1.0));
    assert!(c.segs.iter().all(|s| matches!(s, Seg::Cubic { .. })));
    // An ellipse with semi-axes 2 and 1 has area 2*pi (cubic approximation within 0.1%).
    assert!((c.signed_area() - 2.0 * PI).abs() < 2.0e-3);
}

#[test]
fn transform_moves_the_start_point() {
    let p = unit_square().transformed(Affine::translate((10.0, 20.0)));
    assert_eq!(p.start, pt(10.0, 20.0));
    assert!(p.closed);
}

#[test]
fn split_a_line_at_distances() {
    let p = Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0)], false);
    let parts = p.split_at_lengths(&[3.0, 7.0]);
    assert_eq!(parts.len(), 3);
    assert!(near_pt(parts[0].start, pt(0.0, 0.0)) && near_pt(parts[0].end(), pt(3.0, 0.0)));
    assert!(near_pt(parts[1].start, pt(3.0, 0.0)) && near_pt(parts[1].end(), pt(7.0, 0.0)));
    assert!(near_pt(parts[2].start, pt(7.0, 0.0)) && near_pt(parts[2].end(), pt(10.0, 0.0)));
    let lens: Vec<f64> = parts.iter().map(Path::length).collect();
    assert!(near(lens[0], 3.0) && near(lens[1], 4.0) && near(lens[2], 3.0));
    assert!(parts.iter().all(|p| !p.closed));
}

#[test]
fn split_crosses_segment_boundaries() {
    // L-shape of two legs of 10; split at 15 lands 5 along the second leg.
    let p = Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)], false);
    let parts = p.split_at_lengths(&[15.0]);
    assert_eq!(parts.len(), 2);
    assert!(near_pt(parts[0].end(), pt(10.0, 5.0)));
    assert_eq!(parts[0].segs.len(), 2);
    assert!(near(parts[1].length(), 5.0));
}

#[test]
fn splitting_an_arc_keeps_arcs_with_the_same_radius() {
    // Quarter circle r = 2 (length pi), split in the middle.
    let arc = Path::new(pt(2.0, 0.0)).arc_to(pt(0.0, 2.0), pt(0.0, 0.0), true);
    let parts = arc.split_at_lengths(&[PI / 2.0]);
    assert_eq!(parts.len(), 2);
    let mid = pt(2.0 * (PI / 4.0).cos(), 2.0 * (PI / 4.0).sin());
    assert!(near_pt(parts[0].end(), mid));
    assert!(matches!(parts[0].segs[0], Seg::Arc { ccw: true, .. }));
    assert!(near(parts[0].length(), PI / 2.0));
    assert!(near(parts[1].length(), PI / 2.0));
}

#[test]
fn splitting_a_cubic_preserves_total_length() {
    let p = Path::new(pt(0.0, 0.0)).cubic_to(pt(0.0, 4.0), pt(4.0, 4.0), pt(4.0, 0.0));
    let total = p.length();
    let parts = p.split_at_lengths(&[total / 3.0, 2.0 * total / 3.0]);
    assert_eq!(parts.len(), 3);
    for part in &parts {
        assert!((part.length() - total / 3.0).abs() < 1e-6);
    }
}

#[test]
fn split_distances_are_sorted_deduplicated_and_clamped() {
    let p = Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0)], false);
    // Unsorted, duplicated, at the ends and beyond: only 4 and 6 are real cuts.
    let parts = p.split_at_lengths(&[6.0, 4.0, 4.0, 0.0, 10.0, 99.0, -1.0]);
    assert_eq!(parts.len(), 3);
    assert!(near(parts[1].length(), 2.0));
    // No cuts: the path comes back whole.
    assert_eq!(p.split_at_lengths(&[]), vec![p.clone()]);
}

#[test]
fn closed_paths_split_into_open_pieces_that_cover_the_ring() {
    let sq = unit_square(); // perimeter 4
    let parts = sq.split_at_lengths(&[1.5]);
    // The ring is cut once: the piece before and after the cut join into the seam, so two pieces.
    assert_eq!(parts.len(), 2);
    assert!(parts.iter().all(|p| !p.closed));
    let total: f64 = parts.iter().map(Path::length).sum();
    assert!(near(total, 4.0));
    assert!(near(parts[0].length(), 1.5));
}

#[test]
fn point_and_tangent_at_a_distance() {
    let p = Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)], false);
    let (q, tan) = p.sample_at(15.0).unwrap();
    assert!(near_pt(q, pt(10.0, 5.0)));
    assert!(near(tan.x, 0.0) && near(tan.y, 1.0));
    assert!(p.sample_at(25.0).is_none());
    assert!(p.sample_at(-1.0).is_none());
    // On an arc the tangent is perpendicular to the radius: at (0,2) of a ccw circle it points -x.
    let arc = Path::new(pt(2.0, 0.0)).arc_to(pt(0.0, 2.0), pt(0.0, 0.0), true);
    let (q, tan) = arc.sample_at(PI).unwrap_or((arc.end(), Vec2::ZERO));
    assert!(near_pt(q, pt(0.0, 2.0)));
    assert!(near(tan.x, -1.0) && near(tan.y, 0.0));
}

#[test]
fn serialises_to_json_and_back() {
    let p = Path::circle(pt(1.0, 2.0), 3.0);
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(serde_json::from_str::<Path>(&json).unwrap(), p);
}
