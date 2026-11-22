//! Ground truth for the nester. Every placement is verified with exact integer
//! geometry from `ps-geometry`, never with the collision engine that produced it:
//! containment in the bin, no overlap with keep-outs or other items, the requested spacing,
//! the rotation rule, and rigid motion of attached sets.

use std::sync::atomic::{AtomicBool, Ordering};

use ps_core::PtUm;
use ps_geometry::{BoolOp, FillRule, Join, boolean, normalize, offset_shapes, shapes_area2};

use super::*;
use crate::{NestBin, NestItem, NestParams, Rotations, bbox, placed_outline};

const MM: i64 = 1_000;

fn rect(x0: i64, y0: i64, x1: i64, y1: i64) -> Vec<PtUm> {
    vec![
        PtUm::new(x0, y0),
        PtUm::new(x1, y0),
        PtUm::new(x1, y1),
        PtUm::new(x0, y1),
    ]
}

fn item(id: u32, w_mm: i64, h_mm: i64, rotations: Rotations) -> NestItem {
    NestItem {
        id,
        outline: vec![rect(0, 0, w_mm * MM, h_mm * MM)],
        rotations,
    }
}

fn bin(w_mm: i64, h_mm: i64) -> NestBin {
    NestBin {
        outline: rect(0, 0, w_mm * MM, h_mm * MM),
        keep_outs: vec![],
    }
}

fn params(spacing_mm: i64) -> NestParams {
    NestParams {
        spacing_um: spacing_mm * MM,
        seed: 1,
        iterations: 0,
    }
}

fn never() -> bool {
    false
}

fn area2(rings: &[Vec<PtUm>], clip: &[Vec<PtUm>], op: BoolOp) -> i128 {
    shapes_area2(&boolean(rings, clip, op, FillRule::NonZero))
}

/// Asserts that a nesting is geometrically valid, exactly.
fn assert_valid(bins: &[NestBin], items: &[NestItem], spacing_um: i64, nesting: &Nesting) {
    let outlines: Vec<(usize, Vec<Vec<PtUm>>)> = nesting
        .placements
        .iter()
        .map(|p| {
            let it = items.iter().find(|i| i.id == p.item).expect("known item");
            (p.bin, placed_outline(it, p))
        })
        .collect();
    // Contained in the bin and clear of its keep-outs.
    for (b, rings) in &outlines {
        let bin = &bins[*b];
        assert_eq!(
            area2(
                rings,
                std::slice::from_ref(&bin.outline),
                BoolOp::Difference
            ),
            0,
            "an item sticks out of its bin"
        );
        assert_eq!(
            area2(rings, &bin.keep_outs, BoolOp::Intersect),
            0,
            "an item overlaps a keep-out"
        );
    }
    // Pairwise clear, with the requested gap: outlines grown by half the spacing (less 4 µm for
    // rotation rounding) must not overlap.
    let grow = (spacing_um / 2 - 4).max(0);
    let grown: Vec<(usize, Vec<Vec<PtUm>>)> = outlines
        .iter()
        .map(|(b, rings)| {
            let shapes = normalize(rings, FillRule::NonZero);
            // Round joins: the gap between items is a distance, so diagonal neighbours may sit
            // corner to corner at exactly the spacing.
            let out = offset_shapes(&shapes, grow, Join::Round);
            (*b, out.into_iter().flatten().collect())
        })
        .collect();
    for (i, (bi, a)) in grown.iter().enumerate() {
        for (bj, b) in &grown[i + 1..] {
            if bi == bj {
                assert_eq!(area2(a, b, BoolOp::Intersect), 0, "two items are too close");
            }
        }
    }
    // Every item is either placed once or reported unplaced.
    let mut seen: Vec<u32> = nesting.placements.iter().map(|p| p.item).collect();
    seen.extend(&nesting.unplaced);
    seen.sort_unstable();
    let mut want: Vec<u32> = items.iter().map(|i| i.id).collect();
    want.sort_unstable();
    assert_eq!(seen, want);
}

#[test]
fn two_squares_fit_in_a_larger_bin() {
    let bins = [bin(100, 100)];
    let items = [
        item(1, 40, 40, Rotations::None),
        item(2, 40, 40, Rotations::None),
    ];
    let n = nest(&bins, &items, &params(0), &never);
    assert_eq!(n.placements.len(), 2);
    assert_eq!(n.unplaced.len(), 0);
    assert_valid(&bins, &items, 0, &n);
}

#[test]
fn nine_squares_tile_a_bin_with_room_for_the_spacing() {
    let bins = [bin(110, 110)];
    let items: Vec<NestItem> = (1..=9).map(|i| item(i, 30, 30, Rotations::None)).collect();
    let n = nest(&bins, &items, &params(5), &never);
    assert_eq!(n.placements.len(), 9, "unplaced: {:?}", n.unplaced);
    assert_valid(&bins, &items, 5 * MM, &n);
}

#[test]
fn an_item_that_cannot_fit_is_reported_not_forced() {
    let bins = [bin(100, 100)];
    let items = [
        item(1, 150, 20, Rotations::Quarter),
        item(2, 30, 30, Rotations::None),
    ];
    let n = nest(&bins, &items, &params(0), &never);
    assert_eq!(n.unplaced, vec![1]);
    assert_eq!(n.placements.len(), 1);
    assert_valid(&bins, &items, 0, &n);
}

#[test]
fn more_items_than_fit_leave_the_rest_unplaced() {
    let bins = [bin(100, 100)];
    let items: Vec<NestItem> = (1..=10).map(|i| item(i, 40, 40, Rotations::None)).collect();
    let n = nest(&bins, &items, &params(0), &never);
    // Only four 40 x 40 squares fit in 100 x 100.
    assert_eq!(n.placements.len(), 4);
    assert_eq!(n.unplaced.len(), 6);
    assert_valid(&bins, &items, 0, &n);
}

#[test]
fn keep_outs_are_never_covered() {
    let mut b = bin(100, 100);
    b.keep_outs.push(rect(30 * MM, 0, 70 * MM, 100 * MM)); // leaves two 30-wide side strips
    let bins = [b];
    let items = [
        item(1, 40, 40, Rotations::None),
        item(2, 25, 25, Rotations::None),
    ];
    let n = nest(&bins, &items, &params(0), &never);
    assert_eq!(n.unplaced, vec![1]); // 40 is wider than either strip
    assert_eq!(n.placements.len(), 1);
    assert_valid(&bins, &items, 0, &n);
}

#[test]
fn rotation_lets_a_tall_item_fit_a_wide_bin() {
    let bins = [bin(100, 30)];
    let tall = |r| [item(1, 25, 90, r)];
    let n = nest(&bins, &tall(Rotations::Quarter), &params(0), &never);
    assert_eq!(n.placements.len(), 1);
    let angle = n.placements[0].rotation_deg;
    assert!(
        (angle - 90.0).abs() < 1e-9 || (angle - 270.0).abs() < 1e-9,
        "{angle}"
    );
    assert_valid(&bins, &tall(Rotations::Quarter), 0, &n);
}

#[test]
fn a_rotation_lock_is_respected() {
    let bins = [bin(100, 30)];
    for rotations in [Rotations::None, Rotations::GrainLock] {
        let items = [item(1, 25, 90, rotations)];
        let n = nest(&bins, &items, &params(0), &never);
        assert_eq!(n.unplaced, vec![1], "{rotations:?}");
    }
}

#[test]
fn grain_locked_items_only_turn_by_half_turns() {
    let bins = [bin(100, 100)];
    let items: Vec<NestItem> = (1..=4)
        .map(|i| item(i, 45, 30 + i64::from(i), Rotations::GrainLock))
        .collect();
    let n = nest(&bins, &items, &params(0), &never);
    for p in &n.placements {
        let a = p.rotation_deg;
        assert!(a.abs() < 1e-9 || (a - 180.0).abs() < 1e-9, "{a}");
    }
}

#[test]
fn free_rotation_uses_five_degree_steps() {
    // A thin diagonal bar only fits a square bin when turned 45 degrees.
    let bar = NestItem {
        id: 1,
        outline: vec![rect(0, 0, 130 * MM, 8 * MM)],
        rotations: Rotations::Free5,
    };
    let bins = [bin(100, 100)];
    let n = nest(&bins, std::slice::from_ref(&bar), &params(0), &never);
    assert_eq!(
        n.placements.len(),
        1,
        "a 130 x 8 bar fits 100 x 100 at 45 degrees"
    );
    let a = n.placements[0].rotation_deg;
    assert!(
        (a / 5.0 - (a / 5.0).round()).abs() < 1e-9,
        "{a} is not a multiple of 5"
    );
    assert_valid(&bins, std::slice::from_ref(&bar), 0, &n);
}

#[test]
fn the_requested_spacing_separates_items() {
    // Grown items are 50 x 50 each; the grown bin is 110 x 55, so both fit with room to spare.
    let bins = [bin(100, 45)];
    let items = [
        item(1, 40, 40, Rotations::None),
        item(2, 40, 40, Rotations::None),
    ];
    let n = nest(&bins, &items, &params(10), &never);
    assert_eq!(n.placements.len(), 2);
    assert_valid(&bins, &items, 10 * MM, &n);
}

#[test]
fn spacing_that_leaves_no_room_keeps_items_apart_or_unplaced() {
    // Two 45-wide items in a 102-wide bin leave a 12 mm gap, enough for 10 mm of spacing...
    let bins = [bin(102, 50)];
    let items = [
        item(1, 45, 45, Rotations::None),
        item(2, 45, 45, Rotations::None),
    ];
    let ok = nest(&bins, &items, &params(10), &never);
    assert_eq!(ok.placements.len(), 2);
    assert_valid(&bins, &items, 10 * MM, &ok);
    // ...but 13 mm cannot be honoured, so one is left out rather than placed too close.
    let tight = nest(&bins, &items, &params(13), &never);
    assert_eq!(tight.placements.len(), 1);
    assert_valid(&bins, &items, 13 * MM, &tight);
}

#[test]
fn items_overflow_into_the_next_bin() {
    let bins = [bin(50, 50), bin(50, 50)];
    let items: Vec<NestItem> = (1..=2).map(|i| item(i, 40, 40, Rotations::None)).collect();
    let n = nest(&bins, &items, &params(0), &never);
    assert_eq!(n.placements.len(), 2);
    let mut used: Vec<usize> = n.placements.iter().map(|p| p.bin).collect();
    used.sort_unstable();
    assert_eq!(used, vec![0, 1]);
    assert_valid(&bins, &items, 0, &n);
}

#[test]
fn an_attached_set_moves_as_one_rigid_item() {
    let set = NestItem {
        id: 1,
        outline: vec![
            rect(0, 0, 20 * MM, 20 * MM),
            rect(30 * MM, 0, 50 * MM, 20 * MM),
        ],
        rotations: Rotations::Quarter,
    };
    let bins = [bin(100, 100)];
    let n = nest(&bins, std::slice::from_ref(&set), &params(0), &never);
    assert_eq!(n.placements.len(), 1);
    let rings = placed_outline(&set, &n.placements[0]);
    assert_eq!(rings.len(), 2);
    let centre = |r: &Vec<PtUm>| {
        let b = bbox(r).unwrap();
        (b.min.x.midpoint(b.max.x), b.min.y.midpoint(b.max.y))
    };
    let (a, b) = (centre(&rings[0]), centre(&rings[1]));
    // The two squares' centres stay 30 mm apart, whatever the rotation.
    let d2 = (a.0 - b.0).pow(2) + (a.1 - b.1).pow(2);
    assert_eq!(d2, 30 * MM * 30 * MM);
    assert_valid(&bins, std::slice::from_ref(&set), 0, &n);
}

#[test]
fn nesting_is_deterministic() {
    let bins = [bin(120, 120)];
    let items: Vec<NestItem> = (1..=12)
        .map(|i| {
            item(
                i,
                20 + i64::from(i % 4) * 7,
                15 + i64::from(i % 3) * 9,
                Rotations::Quarter,
            )
        })
        .collect();
    let p = NestParams {
        spacing_um: 2 * MM,
        seed: 42,
        iterations: 30,
    };
    assert_eq!(
        nest(&bins, &items, &p, &never),
        nest(&bins, &items, &p, &never)
    );
}

#[test]
fn search_never_does_worse_than_the_greedy_start() {
    let bins = [bin(100, 100)];
    let items: Vec<NestItem> = (1..=14)
        .map(|i| {
            item(
                i,
                18 + i64::from(i % 5) * 6,
                22 + i64::from(i % 4) * 8,
                Rotations::Quarter,
            )
        })
        .collect();
    let greedy = nest(&bins, &items, &params(1), &never);
    let searched = nest(
        &bins,
        &items,
        &NestParams {
            spacing_um: MM,
            seed: 7,
            iterations: 40,
        },
        &never,
    );
    assert!(searched.placements.len() >= greedy.placements.len());
    assert!(searched.utilisation[0] >= greedy.utilisation[0] - 1e-12);
    assert_valid(&bins, &items, MM, &searched);
}

#[test]
fn cancelling_returns_the_best_layout_found_so_far() {
    let bins = [bin(100, 100)];
    let items: Vec<NestItem> = (1..=8)
        .map(|i| item(i, 30, 20 + i64::from(i), Rotations::Quarter))
        .collect();
    let stop = AtomicBool::new(true);
    let cancelled = nest(
        &bins,
        &items,
        &NestParams {
            spacing_um: 0,
            seed: 3,
            iterations: 1_000,
        },
        &|| stop.load(Ordering::Relaxed),
    );
    let greedy = nest(&bins, &items, &params(0), &never);
    assert_eq!(cancelled, greedy);
    assert_valid(&bins, &items, 0, &cancelled);
}

#[test]
fn utilisation_is_reported_per_bin() {
    let bins = [bin(100, 100)];
    let items = [
        item(1, 40, 40, Rotations::None),
        item(2, 40, 40, Rotations::None),
    ];
    let n = nest(&bins, &items, &params(0), &never);
    assert_eq!(n.utilisation.len(), 1);
    assert!((n.utilisation[0] - 0.32).abs() < 1e-9);
}

#[test]
fn fifty_objects_nest_without_trouble() {
    let bins = [bin(400, 300)];
    let items: Vec<NestItem> = (1..=50)
        .map(|i| {
            item(
                i,
                25 + i64::from(i % 5) * 4,
                20 + i64::from(i % 7) * 3,
                Rotations::Quarter,
            )
        })
        .collect();
    let n = nest(&bins, &items, &params(2), &never);
    assert_eq!(n.placements.len() + n.unplaced.len(), 50);
    assert!(n.placements.len() >= 45, "placed {}", n.placements.len());
    assert_valid(&bins, &items, 2 * MM, &n);
}

#[test]
fn an_empty_request_gives_an_empty_result() {
    let n = nest(&[bin(100, 100)], &[], &params(0), &never);
    assert_eq!(n.placements.len() + n.unplaced.len(), 0);
    assert_eq!(n.utilisation, vec![0.0]);
    let no_bins = nest(&[], &[item(1, 10, 10, Rotations::None)], &params(0), &never);
    assert_eq!(no_bins.unplaced, vec![1]);
}
