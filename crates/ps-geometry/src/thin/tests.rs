//! Ground truth for thin-part fills: "parts wider than a dot are drawn as single
//! centre-line strokes … parts smaller than the tip are flagged, not drawn".
//!
//! The specification gives no thresholds, so the rule under test is the one documented in `thin.rs`:
//! * a leftover part is a *feature* when its inscribed diameter is at least half the tip;
//!   smaller leftovers (the slivers every convex corner leaves because a round tip cannot reach
//!   a sharp corner) are ignored silently;
//! * a feature whose centre line is at least one tip long is drawn along that centre line;
//! * any other feature is flagged.

use super::*;
use crate::boolean::{FillRule, Ring, Shape, normalize};
use crate::fill::{FillSpec, Pattern, generate};

const TIP: f64 = 0.4; // mm

fn rect(x0: i64, y0: i64, x1: i64, y1: i64) -> Ring {
    vec![
        PtUm::new(x0, y0),
        PtUm::new(x1, y0),
        PtUm::new(x1, y1),
        PtUm::new(x0, y1),
    ]
}

fn region(rings: &[Ring]) -> Vec<Shape> {
    normalize(rings, FillRule::NonZero)
}

fn spec() -> FillSpec {
    FillSpec {
        pattern: Pattern::Parallel,
        angle_deg: 0.0,
        overlap: 0.15,
        outline_pass: false,
    }
}

#[expect(clippy::cast_precision_loss, reason = "test coordinates are tiny")]
fn near(a: i64, b: f64, eps: f64) -> bool {
    (a as f64 - b).abs() <= eps
}

#[test]
fn a_plain_square_produces_no_thin_strokes_and_no_flags() {
    // Every convex corner leaves a sliver the round tip cannot reach; none may be reported.
    let t = thin_parts(&region(&[rect(0, 0, 10_000, 10_000)]), TIP);
    assert!(t.strokes.is_empty(), "{:?}", t.strokes);
    assert!(t.flagged.is_empty(), "{:?}", t.flagged);
}

#[test]
fn a_circle_like_region_produces_nothing_either() {
    let octagon: Ring = [
        (3_000, 0),
        (7_000, 0),
        (10_000, 3_000),
        (10_000, 7_000),
        (7_000, 10_000),
        (3_000, 10_000),
        (0, 7_000),
        (0, 3_000),
    ]
    .iter()
    .map(|&(x, y)| PtUm::new(x, y))
    .collect();
    let t = thin_parts(&region(&[octagon]), TIP);
    assert_eq!(t.strokes.len() + t.flagged.len(), 0);
}

#[test]
fn a_strip_narrower_than_the_tip_gets_one_centre_line() {
    let t = thin_parts(&region(&[rect(0, 0, 10_000, 300)]), TIP);
    assert_eq!(t.strokes.len(), 1, "{:?}", t.strokes);
    assert_eq!(t.flagged.len(), 0);
    let line = &t.strokes[0];
    assert!(line.iter().all(|p| near(p.y, 150.0, 3.0)));
    let xs: Vec<i64> = line.iter().map(|p| p.x).collect();
    assert!(near(*xs.iter().min().unwrap(), 150.0, 5.0));
    assert!(near(*xs.iter().max().unwrap(), 9_850.0, 5.0));
}

#[test]
fn a_dot_smaller_than_the_tip_is_flagged_not_drawn() {
    // 0.3 mm square: a real feature (inscribed diameter 0.3 >= 0.2) but shorter than the tip.
    let t = thin_parts(&region(&[rect(0, 0, 300, 300)]), TIP);
    assert_eq!(t.strokes.len(), 0);
    assert_eq!(t.flagged.len(), 1);
}

#[test]
fn specks_far_below_the_tip_are_ignored() {
    // 0.05 mm: inscribed diameter 0.05 < 0.2, treated like a corner sliver.
    let t = thin_parts(&region(&[rect(0, 0, 50, 50)]), TIP);
    assert_eq!(t.strokes.len() + t.flagged.len(), 0);
}

#[test]
fn a_strip_wide_enough_for_the_tip_needs_no_thin_stroke() {
    // 0.5 mm wide with a 0.4 mm tip: the inset leaves a 0.1 mm band and the hatch covers it.
    let t = thin_parts(&region(&[rect(0, 0, 10_000, 500)]), TIP);
    assert_eq!(t.strokes.len() + t.flagged.len(), 0);
}

#[test]
fn a_square_with_a_narrow_tail_gets_its_tail_drawn() {
    let body = rect(0, 0, 10_000, 10_000);
    let tail = rect(10_000, 4_850, 13_000, 5_150); // 3 mm long, 0.3 mm wide
    let t = thin_parts(&region(&[body, tail]), TIP);
    assert_eq!(t.strokes.len(), 1, "{:?}", t.strokes);
    assert_eq!(t.flagged.len(), 0);
    let line = &t.strokes[0];
    assert!(line.iter().all(|p| near(p.y, 5_000.0, 4.0)), "{line:?}");
    let xs: Vec<i64> = line.iter().map(|p| p.x).collect();
    // The centre line starts where the inscribed circle first fits in the tail, a little past the
    // jagged mouth (about 10 220), and ends one radius short of the tail's end. The pen's ink
    // (radius 200) then reaches back to about x = 10 020, within 20 µm of the hatch's ink.
    assert!(*xs.iter().min().unwrap() <= 10_250, "{xs:?}");
    assert!(near(*xs.iter().max().unwrap(), 12_850.0, 10.0));
}

#[test]
fn fill_combines_the_hatch_and_the_tail() {
    let body = rect(0, 0, 10_000, 10_000);
    let tail = rect(10_000, 4_850, 13_000, 5_150);
    let shapes = region(&[body, tail]);
    let hatch = generate(&shapes, TIP, &spec());
    let full = fill(&shapes, TIP, &spec());
    assert_eq!(full.strokes.len(), hatch.len() + 1);
    assert_eq!(full.flagged.len(), 0);
    // Every hatch stroke is still present.
    for line in &hatch {
        assert!(full.strokes.contains(line));
    }
}

#[test]
fn fill_of_a_plain_square_equals_the_hatch_alone() {
    let shapes = region(&[rect(0, 0, 10_000, 10_000)]);
    let full = fill(&shapes, TIP, &spec());
    assert_eq!(full.strokes, generate(&shapes, TIP, &spec()));
    assert_eq!(full.flagged.len(), 0);
}

#[test]
fn fill_reports_a_dot_that_the_hatch_cannot_draw() {
    let shapes = region(&[rect(0, 0, 300, 300)]);
    let full = fill(&shapes, TIP, &spec());
    assert_eq!(full.strokes.len(), 0);
    assert_eq!(full.flagged.len(), 1);
}

#[test]
fn thin_parts_are_deterministic() {
    let shapes = region(&[rect(0, 0, 10_000, 300), rect(0, 2_000, 300, 2_300)]);
    assert_eq!(thin_parts(&shapes, TIP), thin_parts(&shapes, TIP));
}

#[test]
fn an_empty_region_has_no_thin_parts() {
    let t = thin_parts(&[], TIP);
    assert_eq!(t.strokes.len() + t.flagged.len(), 0);
}

#[test]
fn a_pen_with_no_width_cannot_leave_anything_to_draw_or_flag() {
    let strip = region(&[rect(0, 0, 10_000, 300)]);
    for tip in [0.0, -0.4, f64::NAN, 0.000_000_1] {
        let t = thin_parts(&strip, tip);
        assert!(t.strokes.is_empty(), "tip {tip}: {:?}", t.strokes);
        assert!(t.flagged.is_empty(), "tip {tip}: {:?}", t.flagged);
    }
}
