//! Ground truth for placement geometry and the fast grid layout ("Fill mat with
//! copies" uses a grid first). Every check is exact, using integer µm geometry.

use ps_core::PtUm;
use ps_geometry::{BoolOp, FillRule, boolean, shapes_area2};

use super::*;
use crate::{NestBin, NestItem, Rotations};

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

fn overlap_area2(a: &[Vec<PtUm>], b: &[Vec<PtUm>]) -> i128 {
    shapes_area2(&boolean(a, b, BoolOp::Intersect, FillRule::NonZero))
}

#[test]
fn a_placed_outline_is_rotated_about_the_origin_then_moved() {
    let it = item(1, 2, 1, Rotations::Quarter); // 2000 x 1000
    let p = Placement {
        item: 1,
        bin: 0,
        rotation_deg: 90.0,
        offset: PtUm::new(5 * MM, 5 * MM),
    };
    let rings = placed_outline(&it, &p);
    let b = bbox(&rings[0]).unwrap();
    // Rotating by 90 turns (x, y) into (-y, x): x in [-1000, 0], y in [0, 2000]; then +5000.
    assert_eq!(
        (b.min, b.max),
        (PtUm::new(4_000, 5_000), PtUm::new(5_000, 7_000))
    );
}

#[test]
fn an_attached_set_keeps_its_rings_together() {
    let set = NestItem {
        id: 7,
        outline: vec![rect(0, 0, 1_000, 1_000), rect(3_000, 0, 4_000, 1_000)],
        rotations: Rotations::Quarter,
    };
    let p = Placement {
        item: 7,
        bin: 0,
        rotation_deg: 180.0,
        offset: PtUm::new(10_000, 0),
    };
    let rings = placed_outline(&set, &p);
    assert_eq!(rings.len(), 2);
    let (a, b) = (bbox(&rings[0]).unwrap(), bbox(&rings[1]).unwrap());
    // 180 degrees: the second ring ends up 3000 to the left of the first.
    assert_eq!(a.min.x - b.min.x, 3_000);
}

#[test]
fn utilisation_is_item_area_over_bin_area() {
    let b = bin(100, 100);
    let items = [item(1, 50, 50, Rotations::None)];
    assert!((utilisation(&b, &items) - 0.25).abs() < 1e-12);
    let two = [
        item(1, 40, 40, Rotations::None),
        item(2, 40, 40, Rotations::None),
    ];
    assert!((utilisation(&b, &two) - 0.32).abs() < 1e-12);
}

#[test]
fn the_bin_area_excludes_keep_outs() {
    let mut b = bin(100, 100);
    b.keep_outs.push(rect(0, 0, 50 * MM, 100 * MM)); // half the mat
    let items = [item(1, 50, 50, Rotations::None)];
    assert!((utilisation(&b, &items) - 0.5).abs() < 1e-12);
}

// ---- grid layout ----

#[test]
fn grid_layout_fills_rows_and_columns_with_the_spacing() {
    let places = grid_layout(&bin(100, 100), &item(1, 20, 10, Rotations::None), 2 * MM);
    // Columns: 4 x 20 + 3 x 2 = 86 <= 100 (5 would need 108). Rows: 8 x 10 + 7 x 2 = 94 (9: 106).
    assert_eq!(places.len(), 32);
    let it = item(1, 20, 10, Rotations::None);
    let mut xs: Vec<i64> = places.iter().map(|p| p.offset.x).collect();
    xs.sort_unstable();
    xs.dedup();
    assert_eq!(xs, vec![0, 22_000, 44_000, 66_000]);
    let mut ys: Vec<i64> = places.iter().map(|p| p.offset.y).collect();
    ys.sort_unstable();
    ys.dedup();
    assert_eq!(
        ys,
        vec![0, 12_000, 24_000, 36_000, 48_000, 60_000, 72_000, 84_000]
    );
    assert!(places.iter().all(|p| p.item == 1 && p.bin == 0));
    // Exact: no two copies overlap and all lie inside the mat.
    let outlines: Vec<_> = places.iter().map(|p| placed_outline(&it, p)).collect();
    for (i, a) in outlines.iter().enumerate() {
        for b in &outlines[i + 1..] {
            assert_eq!(overlap_area2(a, b), 0);
        }
        let outside = boolean(
            a,
            &bin(100, 100).outline_rings(),
            BoolOp::Difference,
            FillRule::NonZero,
        );
        assert_eq!(shapes_area2(&outside), 0);
    }
}

#[test]
fn grid_layout_with_no_spacing_packs_edge_to_edge() {
    let places = grid_layout(&bin(100, 100), &item(1, 25, 25, Rotations::None), 0);
    assert_eq!(places.len(), 16);
}

#[test]
fn grid_layout_picks_the_rotation_with_more_copies() {
    // 60 x 20 in 100 x 70: upright gives 1 column x 3 rows = 3; turned (20 x 60) gives 5 x 1 = 5.
    let places = grid_layout(&bin(100, 70), &item(1, 60, 20, Rotations::Quarter), 0);
    assert_eq!(places.len(), 5);
    assert!(places.iter().all(|p| (p.rotation_deg - 90.0).abs() < 1e-9));
    let it = item(1, 60, 20, Rotations::Quarter);
    for p in &places {
        let b = bbox(&placed_outline(&it, p)[0]).unwrap();
        assert!(b.min.x >= 0 && b.min.y >= 0 && b.max.x <= 100 * MM && b.max.y <= 70 * MM);
    }
}

#[test]
fn grid_layout_prefers_upright_on_a_tie() {
    let places = grid_layout(&bin(100, 100), &item(1, 60, 20, Rotations::Quarter), 0);
    // Upright: 1 x 5; turned: 5 x 1. Equal, so no rotation.
    assert_eq!(places.len(), 5);
    assert!(places.iter().all(|p| p.rotation_deg == 0.0));
}

#[test]
fn grid_layout_respects_a_rotation_lock() {
    let places = grid_layout(&bin(100, 70), &item(1, 60, 20, Rotations::None), 0);
    assert_eq!(places.len(), 3);
}

#[test]
fn grid_layout_skips_cells_that_touch_a_keep_out() {
    let mut b = bin(100, 100);
    b.keep_outs.push(rect(45 * MM, 0, 55 * MM, 100 * MM)); // a vertical strip down the middle
    let it = item(1, 20, 20, Rotations::None);
    let places = grid_layout(&b, &it, 0);
    // Columns at 0, 20, 40 (overlaps the strip), 60, 80: the strip removes 40 and 60 is clear.
    assert!(places.len() < 25 && !places.is_empty());
    for p in &places {
        assert_eq!(
            overlap_area2(&placed_outline(&it, p), &b.keep_out_rings()),
            0
        );
    }
}

#[test]
fn grid_layout_of_an_oversized_item_is_empty() {
    let result = grid_layout(&bin(100, 100), &item(1, 150, 20, Rotations::None), 0);
    assert_eq!(result.len(), 0);
}

#[test]
fn grid_layout_respects_a_non_origin_bin() {
    let b = NestBin {
        outline: rect(10 * MM, 20 * MM, 110 * MM, 120 * MM),
        keep_outs: vec![],
    };
    let places = grid_layout(&b, &item(1, 50, 50, Rotations::None), 0);
    assert_eq!(places.len(), 4);
    let mut origin: Vec<(i64, i64)> = places.iter().map(|p| (p.offset.x, p.offset.y)).collect();
    origin.sort_unstable();
    assert_eq!(
        origin,
        vec![
            (10_000, 20_000),
            (10_000, 70_000),
            (60_000, 20_000),
            (60_000, 70_000)
        ]
    );
}
