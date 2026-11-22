//! Ground truth for joining fill strokes into pen-down chains: nothing is lost, nothing is drawn
//! outside the region, and a letter whose rows change width still comes out in a few strokes.

use super::*;
use ps_geometry::{FillRule, FillSpec, Pattern, Region, Ring, Shape, generate, normalize};

const TIP: f64 = 0.4;

fn hatch(region: &[Shape]) -> Vec<Vec<PtUm>> {
    let spec = FillSpec {
        pattern: Pattern::Parallel,
        angle_deg: 0.0,
        overlap: 0.15,
        outline_pass: false,
    };
    generate(region, TIP, &spec)
}

/// The region the pen stays inside: the fill region as the fill sees it.
fn rings_of(region: &[Shape]) -> Region {
    let rings: Vec<Ring> = ps_geometry::inset_region(region, TIP)
        .iter()
        .flatten()
        .cloned()
        .collect();
    Region::new(&rings)
}

fn poly(points: &[(i64, i64)]) -> Ring {
    points.iter().map(|&(x, y)| PtUm::new(x, y)).collect()
}

/// A 10 x 10 mm capital E, bars and spine 2 mm thick: the rows change width at every bar.
fn letter_e() -> Vec<Shape> {
    vec![vec![poly(&[
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
    ])]]
}

/// Twice the row spacing for a 0.4 mm tip with 15 % overlap, in µm.
const REACH: i64 = 680;

#[test]
fn a_square_fill_becomes_one_stroke() {
    let region = vec![vec![poly(&[
        (0, 0),
        (10_000, 0),
        (10_000, 10_000),
        (0, 10_000),
    ])]];
    let rows = hatch(&region);
    assert!(rows.len() > 20, "the fill has many rows to join");
    let chained = connect(rows, &rings_of(&region), REACH);
    assert_eq!(chained.len(), 1);
}

#[test]
fn every_row_survives_joining() {
    let region = letter_e();
    let rows = hatch(&region);
    let chained = connect(rows.clone(), &rings_of(&region), REACH);
    for row in &rows {
        let (a, b) = (row[0], row[row.len() - 1]);
        let drawn = chained.iter().any(|stroke| {
            stroke
                .windows(2)
                .any(|w| (w[0], w[1]) == (a, b) || (w[0], w[1]) == (b, a))
        });
        assert!(drawn, "the row {a:?}..{b:?} was lost");
    }
}

#[test]
fn no_join_crosses_a_notch_of_the_letter() {
    let region = letter_e();
    let chained = connect(hatch(&region), &rings_of(&region), REACH);
    for stroke in &chained {
        for w in stroke.windows(2) {
            let mid = PtUm::new(i64::midpoint(w[0].x, w[1].x), i64::midpoint(w[0].y, w[1].y));
            let in_notch = mid.x > 2_000
                && ((mid.y > 2_000 && mid.y < 4_000) || (mid.y > 6_000 && mid.y < 8_000));
            assert!(!in_notch, "{:?} -> {:?} crosses a notch", w[0], w[1]);
        }
    }
}

#[test]
fn a_letter_with_changing_rows_needs_far_fewer_pen_lifts_than_rows() {
    let region = letter_e();
    let rows = hatch(&region);
    let before = rows.len();
    let chained = connect(rows, &rings_of(&region), REACH);
    // The least possible is one stroke, but the search is a heuristic and the rows narrow and
    // widen at every bar; a third of the rows would be a poor result.
    assert!(
        chained.len() * 3 <= before,
        "{before} rows became {} strokes",
        chained.len()
    );
}

#[test]
fn no_join_crosses_a_hole() {
    let mut hole = poly(&[
        (8_000, 8_000),
        (12_000, 8_000),
        (12_000, 12_000),
        (8_000, 12_000),
    ]);
    hole.reverse();
    let region = normalize(
        &[
            poly(&[(0, 0), (20_000, 0), (20_000, 20_000), (0, 20_000)]),
            hole,
        ],
        FillRule::NonZero,
    );
    let chained = connect(hatch(&region), &rings_of(&region), REACH);
    for stroke in &chained {
        for w in stroke.windows(2) {
            let mid = PtUm::new(i64::midpoint(w[0].x, w[1].x), i64::midpoint(w[0].y, w[1].y));
            assert!(
                !(mid.x > 8_200 && mid.x < 11_800 && mid.y > 8_200 && mid.y < 11_800),
                "{:?} -> {:?} crosses the hole",
                w[0],
                w[1]
            );
        }
    }
}

#[test]
fn joining_is_deterministic() {
    let region = letter_e();
    let rings = rings_of(&region);
    assert_eq!(
        connect(hatch(&region), &rings, REACH),
        connect(hatch(&region), &rings, REACH)
    );
}

#[test]
fn nothing_to_join_comes_back_unchanged() {
    let none = Region::new(&[]);
    assert_eq!(connect(Vec::new(), &none, 1_000), Vec::<Vec<PtUm>>::new());
    let lone = vec![vec![PtUm::new(0, 0), PtUm::new(5_000, 0)]];
    assert_eq!(connect(lone.clone(), &none, 1_000), lone);
}

// ---- rounded turns ----
//
// Measured on a hobby cutting plotter (hardware speed test, 2026-11-21, 4 in/s): joining 30 mm rows 0.8 mm
// apart with a sharp hairpin costs 0.21 s of corner time a row, a semicircular turn of 6 chords
// 0.064 s and one of 12 chords 0.029 s, so a turn is worth drawing as many gentle chords.

/// A wide open square in which a turn fits anywhere.
fn open_square() -> Region {
    Region::new(&[poly(&[
        (-5_000, -5_000),
        (20_000, -5_000),
        (20_000, 5_000),
        (-5_000, 5_000),
    ])])
}

/// Two 10 mm rows 0.8 mm apart, the second drawn back the other way: a hairpin.
fn hairpin() -> Vec<Vec<PtUm>> {
    vec![
        vec![PtUm::new(0, 0), PtUm::new(10_000, 0)],
        vec![PtUm::new(10_000, 800), PtUm::new(0, 800)],
    ]
}

fn points(chain: &[PtUm]) -> Vec<(i64, i64)> {
    chain.iter().map(|p| (p.x, p.y)).collect()
}

#[test]
fn a_hairpin_is_a_straight_join_unless_turns_are_to_be_rounded() {
    let chained = connect_rounded(hairpin(), &open_square(), 2_000, None);
    assert_eq!(chained.len(), 1);
    assert_eq!(
        points(&chained[0]),
        vec![(0, 0), (10_000, 0), (10_000, 800), (0, 800)]
    );
}

#[test]
fn a_two_chord_turn_is_a_corner_at_the_side_halfway_between_the_rows() {
    // Rows 800 um apart: radius 400 um. Both rows are cut back by it, to x = 9,600, and the turn
    // reaches out to x = 10,000 at the height between them: a 90 degree step gives two chords.
    let chained = connect_rounded(hairpin(), &open_square(), 2_000, Some(90.0));
    assert_eq!(
        points(&chained[0]),
        vec![(0, 0), (9_600, 0), (10_000, 400), (9_600, 800), (0, 800)]
    );
}

#[test]
fn a_four_chord_turn_follows_the_semicircle_at_45_degree_steps() {
    // About the centre (9,600, 400), radius 400: 45 degrees round is 400 sin 45 = 282.8 um out and
    // up, so the points are (9,882.8, 117.2), (10,000, 400) and (9,882.8, 682.8), to the micrometre.
    let chained = connect_rounded(hairpin(), &open_square(), 2_000, Some(45.0));
    assert_eq!(
        points(&chained[0]),
        vec![
            (0, 0),
            (9_600, 0),
            (9_883, 117),
            (10_000, 400),
            (9_883, 683),
            (9_600, 800),
            (0, 800)
        ]
    );
}

#[test]
fn a_fifteen_degree_step_makes_twelve_chords_and_the_ends_are_unchanged() {
    let chained = connect_rounded(hairpin(), &open_square(), 2_000, Some(15.0));
    let chain = &chained[0];
    // The first row's start and its shortened end, 11 points round the turn, the second row's
    // shortened start and its far end: 15.
    assert_eq!(chain.len(), 15, "{chain:?}");
    assert_eq!(chain.first(), Some(&PtUm::new(0, 0)));
    assert_eq!(chain.last(), Some(&PtUm::new(0, 800)));
    // Every turn point is on the circle of radius 400 about (9,600, 400), to a micrometre.
    for p in &chain[2..chain.len() - 2] {
        let (dx, dy) = (
            f64::from(i32::try_from(p.x - 9_600).unwrap()),
            f64::from(i32::try_from(p.y - 400).unwrap()),
        );
        let r = libm::hypot(dx, dy);
        assert!((r - 400.0).abs() < 1.0, "{p:?} is {r} from the centre");
    }
}

#[test]
fn a_row_shorter_than_the_turn_is_joined_sharply() {
    // 300 um rows cannot be cut back by the 400 um radius.
    let rows = vec![
        vec![PtUm::new(0, 0), PtUm::new(300, 0)],
        vec![PtUm::new(300, 800), PtUm::new(0, 800)],
    ];
    let chained = connect_rounded(rows, &open_square(), 2_000, Some(15.0));
    assert_eq!(
        points(&chained[0]),
        vec![(0, 0), (300, 0), (300, 800), (0, 800)]
    );
}

#[test]
fn a_join_between_rows_that_cross_is_not_a_hairpin() {
    // The second row leaves at a right angle, so the join is a corner and not a turn: no rounding.
    let rows = vec![
        vec![PtUm::new(0, 0), PtUm::new(10_000, 0)],
        vec![PtUm::new(10_000, 800), PtUm::new(10_000, 4_000)],
    ];
    let rounded = connect_rounded(rows.clone(), &open_square(), 2_000, Some(15.0));
    let sharp = connect_rounded(rows, &open_square(), 2_000, None);
    assert_eq!(rounded, sharp);
    assert_eq!(
        points(&rounded[0]),
        vec![(0, 0), (10_000, 0), (10_000, 800), (10_000, 4_000)]
    );
}

#[test]
fn a_whole_fill_joined_with_rounded_turns_still_covers_every_row_and_stays_inside() {
    let region = vec![vec![poly(&[
        (0, 0),
        (10_000, 0),
        (10_000, 10_000),
        (0, 10_000),
    ])]];
    let rows = hatch(&region);
    let inside = rings_of(&region);
    let chained = connect_rounded(rows.clone(), &inside, REACH, Some(15.0));
    assert_eq!(chained.len(), 1);
    for chain in &chained {
        for pair in chain.windows(2) {
            assert!(
                inside.contains_segment(pair[0], pair[1]),
                "{pair:?} leaves the region"
            );
        }
    }
    // Each row is still drawn, 5 um short at most of its ends being a turn radius at the joins.
    assert!(chained[0].len() > rows.len() * 2, "turns add points");
}

#[test]
fn rounding_is_deterministic() {
    let region = letter_e();
    let rings = rings_of(&region);
    assert_eq!(
        connect_rounded(hatch(&region), &rings, REACH, Some(15.0)),
        connect_rounded(hatch(&region), &rings, REACH, Some(15.0))
    );
}

#[test]
fn rows_shorter_than_eight_millimetres_are_joined_sharply() {
    // Measured on the machine (2026-11-21): on a real fill of 2 mm letter strokes, rounding every
    // hairpin made a continuous wiggle that jerked the carriage and gained little.
    let rows = |len: i64| {
        vec![
            vec![PtUm::new(0, 0), PtUm::new(len, 0)],
            vec![PtUm::new(len, 800), PtUm::new(0, 800)],
        ]
    };
    let short = connect_rounded(rows(5_000), &open_square(), 2_000, Some(15.0));
    assert_eq!(
        points(&short[0]),
        vec![(0, 0), (5_000, 0), (5_000, 800), (0, 800)]
    );
    // Just under the limit stays sharp, the limit itself rounds.
    let under = connect_rounded(rows(7_999), &open_square(), 2_000, Some(15.0));
    assert_eq!(under[0].len(), 4);
    let at = connect_rounded(rows(8_000), &open_square(), 2_000, Some(15.0));
    assert_eq!(at[0].len(), 15);
}
