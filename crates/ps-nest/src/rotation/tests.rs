//! Ground truth for rotation sets and grid rotation: quarter turns or free 5° steps,
//! and a grain lock that restricts rotation to 0° and 180°.

use ps_core::{PtUm, hash_bytes};

use super::*;

fn ring(points: &[(i64, i64)]) -> Vec<PtUm> {
    points.iter().map(|&(x, y)| PtUm::new(x, y)).collect()
}

#[test]
fn quarter_turns_are_four_right_angles() {
    assert_eq!(angles(Rotations::Quarter), vec![0.0, 90.0, 180.0, 270.0]);
}

#[test]
fn the_grain_lock_allows_only_half_turns() {
    assert_eq!(angles(Rotations::GrainLock), vec![0.0, 180.0]);
}

#[test]
fn no_rotation_keeps_the_item_upright() {
    assert_eq!(angles(Rotations::None), vec![0.0]);
}

#[test]
fn free_rotation_steps_by_five_degrees() {
    let a = angles(Rotations::Free5);
    assert_eq!(a.len(), 72);
    assert_eq!(a[0], 0.0);
    assert_eq!(a[1], 5.0);
    assert_eq!(a[71], 355.0);
}

#[test]
fn quarter_rotations_are_exact() {
    let r = ring(&[(1_000, 0), (0, 2_000), (-3_000, 500)]);
    assert_eq!(
        rotate_ring(&r, 90.0),
        ring(&[(0, 1_000), (-2_000, 0), (-500, -3_000)])
    );
    assert_eq!(
        rotate_ring(&r, 180.0),
        ring(&[(-1_000, 0), (0, -2_000), (3_000, -500)])
    );
    assert_eq!(
        rotate_ring(&r, 270.0),
        ring(&[(0, -1_000), (2_000, 0), (500, 3_000)])
    );
    assert_eq!(rotate_ring(&r, 0.0), r);
    assert_eq!(rotate_ring(&r, 360.0), r);
}

#[test]
fn negative_angles_wrap() {
    let r = ring(&[(1_000, 0)]);
    assert_eq!(rotate_ring(&r, -90.0), rotate_ring(&r, 270.0));
}

#[test]
fn an_oblique_rotation_rounds_once_to_the_grid() {
    // 45 degrees: (1000, 0) -> (707.1, 707.1) -> (707, 707).
    assert_eq!(rotate_ring(&ring(&[(1_000, 0)]), 45.0), ring(&[(707, 707)]));
    // 30 degrees: (2000, 0) -> (1732.05, 1000) -> (1732, 1000).
    assert_eq!(
        rotate_ring(&ring(&[(2_000, 0)]), 30.0),
        ring(&[(1_732, 1_000)])
    );
}

#[test]
fn rotation_keeps_distances_from_the_origin() {
    let r = ring(&[(12_345, -6_789)]);
    for deg in [7.0, 33.0, 95.0, 211.0] {
        let p = rotate_ring(&r, deg)[0];
        #[expect(clippy::cast_precision_loss, reason = "test coordinates are tiny")]
        let (x, y) = (p.x as f64, p.y as f64);
        let want = libm::hypot(12_345.0, -6_789.0);
        assert!((libm::hypot(x, y) - want).abs() < 1.0, "{deg}");
    }
}

#[test]
fn the_bounding_box_of_a_ring() {
    let b = bbox(&ring(&[(5, -3), (10, 7), (-2, 4)])).unwrap();
    assert_eq!((b.min, b.max), (PtUm::new(-2, -3), PtUm::new(10, 7)));
    assert_eq!((b.width(), b.height()), (12, 10));
    assert!(bbox(&[]).is_none());
}

#[test]
fn the_search_seed_is_the_first_eight_bytes_of_the_hash() {
    // BLAKE3 of the empty input starts af 13 49 b9 f5 f9 a1 a6.
    let seed = seed_from_hash(&hash_bytes(b""));
    assert_eq!(
        seed,
        u64::from_le_bytes([0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6])
    );
    assert_eq!(seed, 0xa6a1_f9f5_b949_13af);
}

// ---- convex hull (pure helper for attached sets that are not connected) ----

#[test]
fn the_hull_of_a_square_with_an_interior_point_is_the_square() {
    let pts = ring(&[(0, 0), (10, 0), (10, 10), (0, 10), (5, 5), (5, 0)]);
    let hull = convex_hull(&pts);
    // Counter-clockwise from the lowest-leftmost point; the interior and edge points are gone.
    assert_eq!(hull, ring(&[(0, 0), (10, 0), (10, 10), (0, 10)]));
}

#[test]
fn the_hull_is_independent_of_input_order() {
    let a = ring(&[(0, 0), (4, 0), (4, 3), (0, 3), (2, 1)]);
    let mut b = a.clone();
    b.reverse();
    assert_eq!(convex_hull(&a), convex_hull(&b));
}

#[test]
fn degenerate_hulls_are_returned_as_they_are() {
    assert_eq!(convex_hull(&[]), ring(&[]));
    assert_eq!(convex_hull(&ring(&[(1, 1)])), ring(&[(1, 1)]));
    // Collinear points collapse to their two extremes.
    assert_eq!(
        convex_hull(&ring(&[(0, 0), (1, 1), (2, 2), (3, 3)])),
        ring(&[(0, 0), (3, 3)])
    );
}

#[test]
fn two_separate_squares_have_a_hull_covering_the_gap() {
    let pts = ring(&[
        (0, 0),
        (10, 0),
        (10, 10),
        (0, 10),
        (30, 0),
        (40, 0),
        (40, 10),
        (30, 10),
    ]);
    assert_eq!(
        convex_hull(&pts),
        ring(&[(0, 0), (40, 0), (40, 10), (0, 10)])
    );
}
