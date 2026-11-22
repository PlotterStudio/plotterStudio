//! Ground truth for pen/foil fill generation. Numbers follow the design
//! document: spacing s = t(1 − o), passes n = ceil((w − t)/s) + 1, ink edge on the boundary.

use super::*;
use crate::boolean::{FillRule, Ring, Shape, normalize, ring_area2};

fn sq(x0: i64, y0: i64, x1: i64, y1: i64) -> Ring {
    vec![
        PtUm::new(x0, y0),
        PtUm::new(x1, y0),
        PtUm::new(x1, y1),
        PtUm::new(x0, y1),
    ]
}

/// A 10 x 10 mm square.
fn square10() -> Vec<Shape> {
    vec![vec![sq(0, 0, 10_000, 10_000)]]
}

fn spec(pattern: Pattern, angle_deg: f64) -> FillSpec {
    FillSpec {
        pattern,
        angle_deg,
        overlap: 0.15,
        outline_pass: false,
    }
}

const TIP: f64 = 0.4;

/// True when `p` is closer than half a tip (200 µm, less 10 µm for the sag of the polygon that
/// approximates the grown hole's round corners) to the hole
/// `[8000, 12000]²`. The grown hole has round corners, so this is a distance test, not a box test.
fn too_close_to_hole(p: PtUm) -> bool {
    let dx = (8_000 - p.x).max(0).max(p.x - 12_000);
    let dy = (8_000 - p.y).max(0).max(p.y - 12_000);
    #[expect(clippy::cast_precision_loss, reason = "test coordinates are tiny")]
    let d = (dx as f64).hypot(dy as f64);
    d < 190.0
}

fn bbox(lines: &[Vec<PtUm>]) -> (i64, i64, i64, i64) {
    let pts = lines.iter().flatten();
    let x0 = pts.clone().map(|p| p.x).min().unwrap();
    let x1 = pts.clone().map(|p| p.x).max().unwrap();
    let y0 = pts.clone().map(|p| p.y).min().unwrap();
    let y1 = pts.map(|p| p.y).max().unwrap();
    (x0, y0, x1, y1)
}

// ---- pure helpers ----

#[test]
fn spacing_is_tip_times_one_minus_overlap() {
    assert!((spacing(0.4, 0.15) - 0.34).abs() < 1e-12); // a worked example
    assert!((spacing(1.0, 0.0) - 1.0).abs() < 1e-12);
    assert!((spacing(0.5, 0.5) - 0.25).abs() < 1e-12);
}

#[test]
fn spacing_clamps_absurd_overlaps() {
    // Overlap is clamped so spacing stays positive and never exceeds the tip.
    assert!(spacing(0.4, 1.0) > 0.0);
    assert!(spacing(0.4, 2.0) > 0.0);
    assert!((spacing(0.4, -1.0) - 0.4).abs() < 1e-12);
}

#[test]
fn stroke_passes_follow_the_design_formula() {
    let s = spacing(0.4, 0.15);
    assert_eq!(stroke_passes(0.4, 0.4, s), 1); // a stroke as wide as the tip
    assert_eq!(stroke_passes(0.3, 0.4, s), 1); // narrower than the tip
    assert_eq!(stroke_passes(1.0, 0.4, s), 3); // ceil(0.6 / 0.34) + 1
    assert_eq!(stroke_passes(2.0, 0.4, s), 6); // ceil(1.6 / 0.34) + 1
}

#[test]
fn inset_moves_the_boundary_in_by_half_the_tip() {
    let r = inset_region(&square10(), TIP);
    assert_eq!(r.len(), 1);
    let (x0, y0, x1, y1) = bbox(&[r[0][0].clone()]);
    assert_eq!((x0, y0, x1, y1), (200, 200, 9_800, 9_800));
}

#[test]
fn a_region_smaller_than_the_tip_has_no_inset() {
    let tiny = vec![vec![sq(0, 0, 300, 300)]];
    let result = inset_region(&tiny, TIP);
    assert_eq!(result.len(), 0);
    let result = generate(&tiny, TIP, &spec(Pattern::Parallel, 0.0));
    assert_eq!(result.len(), 0);
}

// ---- hatch ----

#[test]
fn parallel_hatch_runs_edge_to_edge_with_no_gap_wider_than_the_spacing() {
    let lines = generate(&square10(), TIP, &spec(Pattern::Parallel, 0.0));
    let mut ys: Vec<i64> = lines.iter().map(|l| l[0].y).collect();
    ys.sort_unstable();
    // The inset region is 200..=9800 high; 9600 / 340 needs 29 steps, so 30 rows, evenly spaced.
    assert_eq!(ys.len(), 30);
    assert_eq!(ys[0], 200);
    assert_eq!(
        *ys.last().unwrap(),
        9_800,
        "the last row is on the far edge"
    );
    for pair in ys.windows(2) {
        let gap = pair[1] - pair[0];
        assert!((330..=340).contains(&gap), "gap {gap}");
    }
    // Each line spans the full inset width, horizontally.
    for l in &lines {
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].y, l[1].y);
        let mut xs = [l[0].x, l[1].x];
        xs.sort_unstable();
        assert_eq!(xs, [200, 9_800]);
    }
}

#[test]
fn hatch_at_ninety_degrees_is_vertical() {
    let lines = generate(&square10(), TIP, &spec(Pattern::Parallel, 90.0));
    assert_eq!(lines.len(), 30);
    for l in &lines {
        assert!((l[0].x - l[1].x).abs() <= 1);
        let mut ys = [l[0].y, l[1].y];
        ys.sort_unstable();
        assert!((ys[0] - 200).abs() <= 1 && (ys[1] - 9_800).abs() <= 1);
    }
    let mut xs: Vec<i64> = lines.iter().map(|l| l[0].x).collect();
    xs.sort_unstable();
    for pair in xs.windows(2) {
        let gap = pair[1] - pair[0];
        assert!((330..=341).contains(&gap), "gap {gap}");
    }
}

#[test]
fn crosshatch_adds_the_perpendicular_set() {
    let lines = generate(&square10(), TIP, &spec(Pattern::Cross, 0.0));
    assert_eq!(lines.len(), 60);
    let horizontal = lines.iter().filter(|l| l[0].y == l[1].y).count();
    let vertical = lines.iter().filter(|l| l[0].x == l[1].x).count();
    assert_eq!((horizontal, vertical), (30, 30));
}

#[test]
fn rotated_hatch_stays_inside_the_inset_region() {
    for angle in [15.0, 30.0, 45.0, 120.0, -60.0] {
        let lines = generate(&square10(), TIP, &spec(Pattern::Parallel, angle));
        assert!(!lines.is_empty(), "angle {angle}");
        // Every vertex is within the inset square (2 µm allows for the rotate-back rounding).
        let (x0, y0, x1, y1) = bbox(&lines);
        assert!(
            x0 >= 198 && y0 >= 198 && x1 <= 9_802 && y1 <= 9_802,
            "angle {angle}"
        );
    }
}

#[test]
fn adjacent_rotated_lines_are_one_spacing_apart() {
    let angle = 30.0_f64;
    let lines = generate(&square10(), TIP, &spec(Pattern::Parallel, angle));
    // Distance of each line's first point along the normal of the hatch direction.
    let (s, c) = (angle.to_radians().sin(), angle.to_radians().cos());
    let mut offsets: Vec<f64> = lines
        .iter()
        .map(|l| {
            #[expect(clippy::cast_precision_loss, reason = "test coordinates are tiny")]
            let (x, y) = (l[0].x as f64, l[0].y as f64);
            -s * x + c * y
        })
        .collect();
    offsets.sort_by(f64::total_cmp);
    offsets.dedup_by(|a, b| (*a - *b).abs() < 2.0);
    for pair in offsets.windows(2) {
        assert!(
            pair[1] - pair[0] <= 340.0 + 2.0 && pair[1] - pair[0] >= 325.0,
            "gap {}",
            pair[1] - pair[0]
        );
    }
}

#[test]
fn hatch_never_enters_a_hole() {
    let mut hole = sq(8_000, 8_000, 12_000, 12_000);
    hole.reverse();
    let region = normalize(&[sq(0, 0, 20_000, 20_000), hole], FillRule::NonZero);
    let lines = generate(&region, TIP, &spec(Pattern::Parallel, 0.0));
    // Rows crossing the hole are split in two; no vertex lies strictly inside the grown hole.
    let split_rows = lines
        .iter()
        .filter(|l| l[0].y > 7_800 && l[0].y < 12_200)
        .count();
    assert!(split_rows >= 2 * 10, "rows {split_rows}");
    for p in lines.iter().flatten() {
        assert!(
            !too_close_to_hole(*p),
            "{p:?} is closer than half a tip to the hole"
        );
    }
}

#[test]
fn separate_regions_are_hatched_separately() {
    let two = normalize(
        &[sq(0, 0, 3_000, 3_000), sq(10_000, 0, 13_000, 3_000)],
        FillRule::NonZero,
    );
    let lines = generate(&two, TIP, &spec(Pattern::Parallel, 0.0));
    // 2600 µm tall inset regions: ceil(2600/340) = 8 steps, so 9 rows each, edge to edge.
    assert_eq!(lines.len(), 18);
}

// ---- zig-zag ----

#[test]
fn zigzag_links_adjacent_rows_into_one_stroke() {
    let lines = generate(&square10(), TIP, &spec(Pattern::ZigZag, 0.0));
    assert_eq!(lines.len(), 1); // fewest possible pen lifts
    assert_eq!(lines[0].len(), 60); // two vertices per row
    // Rows alternate direction: x goes 200 -> 9800 -> 9800 -> 200 ...
    assert_eq!((lines[0][0].x, lines[0][1].x), (200, 9_800));
    assert_eq!((lines[0][2].x, lines[0][3].x), (9_800, 200));
}

#[test]
fn zigzag_does_not_link_across_a_hole() {
    let mut hole = sq(8_000, 8_000, 12_000, 12_000);
    hole.reverse();
    let region = normalize(&[sq(0, 0, 20_000, 20_000), hole], FillRule::NonZero);
    let lines = generate(&region, TIP, &spec(Pattern::ZigZag, 0.0));
    assert!(lines.len() > 1);
    for p in lines.iter().flatten() {
        assert!(
            !too_close_to_hole(*p),
            "{p:?} is closer than half a tip to the hole"
        );
    }
}

/// A capital E made of one polygon: a spine on the left and three bars to the right, 10 mm wide
/// and 10 mm tall, every bar and the spine 2 mm thick. Rows change width at every bar, which is
/// what defeats linking row k to row k + 1 by position.
fn letter_e() -> Vec<Shape> {
    let outline: Ring = [
        (0, 0),
        (10_000, 0),
        (10_000, 2_000),
        (2_000, 2_000),
        (2_000, 4_000),
        (10_000, 4_000),
        (10_000, 6_000),
        (2_000, 6_000),
        (2_000, 8_000),
        (10_000, 8_000),
        (10_000, 10_000),
        (0, 10_000),
    ]
    .iter()
    .map(|&(x, y)| PtUm::new(x, y))
    .collect();
    vec![vec![outline]]
}

#[test]
fn zigzag_draws_every_piece_the_parallel_fill_draws() {
    let hatch = generate(&letter_e(), TIP, &spec(Pattern::Parallel, 0.0));
    let chained = generate(&letter_e(), TIP, &spec(Pattern::ZigZag, 0.0));
    for piece in &hatch {
        let (a, b) = (piece[0], piece[piece.len() - 1]);
        let drawn = chained.iter().any(|stroke| {
            stroke
                .windows(2)
                .any(|w| (w[0], w[1]) == (a, b) || (w[0], w[1]) == (b, a))
        });
        assert!(drawn, "the hatch row {a:?}..{b:?} is not drawn");
    }
}

#[test]
fn zigzag_never_links_across_a_notch() {
    // The notches are x > 2000 for 2000 < y < 4000 and for 6000 < y < 8000.
    let chained = generate(&letter_e(), TIP, &spec(Pattern::ZigZag, 0.0));
    for stroke in &chained {
        for w in stroke.windows(2) {
            let mid = PtUm::new(i64::midpoint(w[0].x, w[1].x), i64::midpoint(w[0].y, w[1].y));
            let in_notch = mid.x > 2_000
                && ((mid.y > 2_000 && mid.y < 4_000) || (mid.y > 6_000 && mid.y < 8_000));
            assert!(!in_notch, "{:?} -> {:?} crosses a notch", w[0], w[1]);
        }
    }
}

// ---- concentric and outline ----

#[test]
fn concentric_fill_makes_nested_closed_rings() {
    let rings = generate(&square10(), TIP, &spec(Pattern::Concentric, 0.0));
    // Inset side 9.6 mm, rings every 0.34 mm: sides 9.6 - 0.68 k > 0 for k = 0..=14.
    assert_eq!(rings.len(), 15);
    for r in &rings {
        assert_eq!(r.first(), r.last(), "rings are closed polylines");
    }
    let (x0, _, x1, _) = bbox(&[rings[0].clone()]);
    assert_eq!((x0, x1), (200, 9_800));
    let (x0, _, x1, _) = bbox(&[rings[1].clone()]);
    assert!((x0 - 540).abs() <= 2 && (x1 - 9_460).abs() <= 2);
}

#[test]
fn concentric_rings_shrink_monotonically() {
    let rings = generate(&square10(), TIP, &spec(Pattern::Concentric, 0.0));
    let areas: Vec<i128> = rings
        .iter()
        .map(|r| ring_area2(&r[..r.len() - 1]).abs())
        .collect();
    assert!(areas.windows(2).all(|w| w[0] > w[1]));
}

#[test]
fn outline_pass_adds_the_boundary_ring() {
    let mut s = spec(Pattern::Parallel, 0.0);
    s.outline_pass = true;
    let lines = generate(&square10(), TIP, &s);
    assert_eq!(lines.len(), 31);
    let ring = lines.iter().find(|l| l.len() > 2).unwrap();
    assert_eq!(ring.first(), ring.last());
    assert_eq!(bbox(std::slice::from_ref(ring)), (200, 200, 9_800, 9_800));
}

#[test]
fn generation_is_deterministic() {
    let a = generate(&square10(), TIP, &spec(Pattern::Cross, 37.0));
    let b = generate(&square10(), TIP, &spec(Pattern::Cross, 37.0));
    assert_eq!(a, b);
}
