//! Ground truth for geometry repair. Defaults: close nearly closed paths within
//! 0.05 mm when the path is filled; drop segments shorter than 1 µm; merge duplicates (same
//! geometry within 1 µm after quantisation, keeping the topmost); join touching open paths only
//! when asked (0.05 mm, same style); flag self-intersections without changing them; normalise
//! winding (outer contours counter-clockwise, holes clockwise). Every repair is reported.

use ps_core::PathKey;
use ps_geometry::{Path, Point, Seg};
use ps_model::SourceStyle;

use super::*;
use crate::drawing::ImportedPath;

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn item(key: &str, geom: Path) -> ImportedPath {
    ImportedPath {
        key: PathKey::source(key),
        layer: None,
        geom,
        style: SourceStyle::default(),
    }
}

fn filled(key: &str, geom: Path) -> ImportedPath {
    let mut i = item(key, geom);
    i.style.fill = Some([0, 0, 0]);
    i
}

fn keys(items: &[ImportedPath]) -> Vec<String> {
    items.iter().map(|i| i.key.to_string()).collect()
}

fn opts() -> RepairOptions {
    RepairOptions::default()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// ---- close nearly closed paths ----

#[test]
fn a_filled_path_with_a_tiny_gap_is_closed() {
    let open = Path::polyline(
        &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 0.02)],
        false,
    );
    let r = repair(vec![filled("a", open)], &opts());
    assert!(r.items[0].geom.closed);
    assert!(
        r.report
            .iter()
            .any(|a| matches!(a, RepairAction::Closed { gap_mm, .. } if near(*gap_mm, 0.02)))
    );
}

#[test]
fn a_gap_larger_than_the_tolerance_stays_open() {
    let open = Path::polyline(
        &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 0.2)],
        false,
    );
    let r = repair(vec![filled("a", open)], &opts());
    assert!(!r.items[0].geom.closed);
}

#[test]
fn an_unfilled_open_path_is_left_alone() {
    let open = Path::polyline(
        &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 0.02)],
        false,
    );
    let r = repair(vec![item("a", open)], &opts());
    assert!(
        !r.items[0].geom.closed,
        "only filled paths are closed by default"
    );
}

#[test]
fn closing_a_gap_snaps_the_end_onto_the_start() {
    let open = Path::polyline(
        &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 0.03)],
        false,
    );
    let r = repair(vec![filled("a", open)], &opts());
    let g = &r.items[0].geom;
    assert_eq!(g.end(), g.start, "no sliver is left between the ends");
}

#[test]
fn an_already_closed_path_is_untouched_and_unreported() {
    let r = repair(vec![filled("a", Path::rect(0.0, 0.0, 10.0, 10.0))], &opts());
    assert_eq!(r.report.len(), 0);
}

// ---- zero-length segments ----

#[test]
fn segments_shorter_than_a_micrometre_are_removed() {
    let g = Path::polyline(
        &[
            pt(0.0, 0.0),
            pt(5.0, 0.0),
            pt(5.0, 0.0),
            pt(5.0004, 0.0),
            pt(10.0, 0.0),
        ],
        false,
    );
    let r = repair(vec![item("a", g)], &opts());
    assert_eq!(r.items[0].geom.segs.len(), 2, "{:?}", r.items[0].geom.segs);
    assert!(
        r.report
            .iter()
            .any(|a| matches!(a, RepairAction::ZeroLengthRemoved { count: 2, .. }))
    );
}

#[test]
fn a_one_micrometre_segment_is_kept() {
    let g = Path::polyline(&[pt(0.0, 0.0), pt(0.001, 0.0), pt(10.0, 0.0)], false);
    let r = repair(vec![item("a", g)], &opts());
    assert_eq!(r.items[0].geom.segs.len(), 2);
}

#[test]
fn removing_a_segment_keeps_the_path_continuous() {
    let g = Path::polyline(
        &[pt(0.0, 0.0), pt(5.0, 0.0), pt(5.0, 0.0), pt(5.0, 5.0)],
        false,
    );
    let r = repair(vec![item("a", g)], &opts());
    assert_eq!(r.items[0].geom.end(), pt(5.0, 5.0));
    assert!(matches!(r.items[0].geom.segs[0], Seg::Line { to } if to == pt(5.0, 0.0)));
}

// ---- duplicates ----

#[test]
fn duplicate_geometry_keeps_the_topmost() {
    let a = item("bottom", Path::rect(0.0, 0.0, 10.0, 10.0));
    let b = item("middle", Path::rect(20.0, 0.0, 5.0, 5.0));
    let c = item("top", Path::rect(0.0, 0.0, 10.0, 10.0));
    let r = repair(vec![a, b, c], &opts());
    assert_eq!(keys(&r.items), vec!["src:middle", "src:top"]);
    assert!(
        r.report
            .iter()
            .any(|x| matches!(x, RepairAction::DuplicateMerged { .. }))
    );
}

#[test]
fn geometry_within_a_micrometre_counts_as_the_same() {
    let a = item("a", Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0)], false));
    let b = item(
        "b",
        Path::polyline(&[pt(0.0004, 0.0), pt(10.0004, 0.0)], false),
    );
    assert_eq!(repair(vec![a, b], &opts()).items.len(), 1);
}

#[test]
fn geometry_more_than_a_micrometre_apart_is_distinct() {
    let a = item("a", Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0)], false));
    let b = item(
        "b",
        Path::polyline(&[pt(0.0, 0.002), pt(10.0, 0.002)], false),
    );
    assert_eq!(repair(vec![a, b], &opts()).items.len(), 2);
}

#[test]
fn a_reversed_open_path_is_a_duplicate() {
    let a = item(
        "a",
        Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 5.0)], false),
    );
    let b = item(
        "b",
        Path::polyline(&[pt(10.0, 5.0), pt(10.0, 0.0), pt(0.0, 0.0)], false),
    );
    assert_eq!(repair(vec![a, b], &opts()).items.len(), 1);
}

#[test]
fn a_closed_path_traced_from_another_corner_is_a_duplicate() {
    let a = item(
        "a",
        Path::polyline(
            &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)],
            true,
        ),
    );
    let b = item(
        "b",
        Path::polyline(
            &[pt(10.0, 10.0), pt(0.0, 10.0), pt(0.0, 0.0), pt(10.0, 0.0)],
            true,
        ),
    );
    assert_eq!(repair(vec![a, b], &opts()).items.len(), 1);
}

#[test]
fn an_open_and_a_closed_path_are_not_duplicates() {
    let a = item(
        "a",
        Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)], false),
    );
    let b = item(
        "b",
        Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)], true),
    );
    assert_eq!(repair(vec![a, b], &opts()).items.len(), 2);
}

// ---- joining ----

fn join_opts() -> RepairOptions {
    RepairOptions {
        join_open: true,
        ..RepairOptions::default()
    }
}

#[test]
fn touching_open_paths_of_one_style_are_joined_when_asked() {
    let a = item("a", Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0)], false));
    let b = item(
        "b",
        Path::polyline(&[pt(10.02, 0.0), pt(10.0, 10.0)], false),
    );
    let r = repair(vec![a, b], &join_opts());
    assert_eq!(r.items.len(), 1);
    assert_eq!(r.items[0].geom.start, pt(0.0, 0.0));
    assert_eq!(r.items[0].geom.end(), pt(10.0, 10.0));
    assert!(
        r.report
            .iter()
            .any(|x| matches!(x, RepairAction::Joined { .. }))
    );
}

#[test]
fn joining_is_off_by_default() {
    let a = item("a", Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0)], false));
    let b = item("b", Path::polyline(&[pt(10.0, 0.0), pt(10.0, 10.0)], false));
    assert_eq!(repair(vec![a, b], &opts()).items.len(), 2);
}

#[test]
fn paths_of_different_styles_are_not_joined() {
    let a = item("a", Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0)], false));
    let mut b = item("b", Path::polyline(&[pt(10.0, 0.0), pt(10.0, 10.0)], false));
    b.style.stroke = Some([255, 0, 0]);
    assert_eq!(repair(vec![a, b], &join_opts()).items.len(), 2);
}

#[test]
fn a_path_may_be_joined_end_to_end_with_a_reversed_neighbour() {
    let a = item("a", Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0)], false));
    let b = item("b", Path::polyline(&[pt(10.0, 10.0), pt(10.0, 0.0)], false));
    let r = repair(vec![a, b], &join_opts());
    assert_eq!(r.items.len(), 1);
    assert_eq!(r.items[0].geom.end(), pt(10.0, 10.0));
}

#[test]
fn joining_a_chain_that_closes_up_makes_a_closed_path() {
    let a = item("a", Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0)], false));
    let b = item("b", Path::polyline(&[pt(10.0, 0.0), pt(10.0, 10.0)], false));
    let c = item("c", Path::polyline(&[pt(10.0, 10.0), pt(0.0, 0.0)], false));
    let r = repair(vec![a, b, c], &join_opts());
    assert_eq!(r.items.len(), 1);
    assert!(r.items[0].geom.closed);
}

// ---- self-intersections ----

#[test]
fn a_bow_tie_is_flagged_but_not_changed() {
    let bow = Path::polyline(
        &[pt(0.0, 0.0), pt(10.0, 10.0), pt(10.0, 0.0), pt(0.0, 10.0)],
        true,
    );
    let before = bow.clone();
    let r = repair(vec![item("x", bow)], &opts());
    assert!(
        r.report
            .iter()
            .any(|a| matches!(a, RepairAction::SelfIntersection { .. }))
    );
    assert_eq!(r.items[0].geom.segs, before.segs, "reported, not changed");
}

#[test]
fn a_simple_polygon_is_not_flagged() {
    let r = repair(vec![item("x", Path::rect(0.0, 0.0, 10.0, 10.0))], &opts());
    assert!(
        !r.report
            .iter()
            .any(|a| matches!(a, RepairAction::SelfIntersection { .. }))
    );
}

#[test]
fn a_path_that_only_touches_itself_at_a_vertex_is_not_flagged() {
    // A figure of eight pinched at a shared vertex is still reported by the segment test only
    // when segments cross; adjacent segments sharing an end never count.
    let r = repair(
        vec![item(
            "x",
            Path::polyline(&[pt(0.0, 0.0), pt(5.0, 0.0), pt(5.0, 5.0)], false),
        )],
        &opts(),
    );
    assert!(
        !r.report
            .iter()
            .any(|a| matches!(a, RepairAction::SelfIntersection { .. }))
    );
}

// ---- winding ----

#[test]
fn outer_contours_wind_counter_clockwise_and_holes_clockwise() {
    // The outer square is clockwise and its hole counter-clockwise: both are wrong.
    let outer = filled(
        "outer",
        Path::polyline(
            &[pt(0.0, 0.0), pt(0.0, 20.0), pt(20.0, 20.0), pt(20.0, 0.0)],
            true,
        ),
    );
    let hole = filled(
        "hole",
        Path::polyline(
            &[pt(5.0, 5.0), pt(15.0, 5.0), pt(15.0, 15.0), pt(5.0, 15.0)],
            true,
        ),
    );
    assert!(outer.geom.signed_area() < 0.0 && hole.geom.signed_area() > 0.0);
    let r = repair(vec![outer, hole], &opts());
    assert!(
        r.items[0].geom.signed_area() > 0.0,
        "outer is counter-clockwise"
    );
    assert!(r.items[1].geom.signed_area() < 0.0, "hole is clockwise");
    assert_eq!(
        r.report
            .iter()
            .filter(|a| matches!(a, RepairAction::Rewound { .. }))
            .count(),
        2
    );
}

#[test]
fn contours_that_already_wind_correctly_are_not_touched() {
    let outer = filled(
        "outer",
        Path::polyline(
            &[pt(0.0, 0.0), pt(20.0, 0.0), pt(20.0, 20.0), pt(0.0, 20.0)],
            true,
        ),
    );
    let hole = filled(
        "hole",
        Path::polyline(
            &[pt(5.0, 5.0), pt(5.0, 15.0), pt(15.0, 15.0), pt(15.0, 5.0)],
            true,
        ),
    );
    let r = repair(vec![outer, hole], &opts());
    assert_eq!(
        r.report
            .iter()
            .filter(|a| matches!(a, RepairAction::Rewound { .. }))
            .count(),
        0
    );
}

#[test]
fn unfilled_closed_paths_keep_their_direction() {
    // The direction of a drawn outline is the artist's choice; only filled contours are rewound.
    let cw = item(
        "cw",
        Path::polyline(
            &[pt(0.0, 0.0), pt(0.0, 20.0), pt(20.0, 20.0), pt(20.0, 0.0)],
            true,
        ),
    );
    let r = repair(vec![cw], &opts());
    assert!(r.items[0].geom.signed_area() < 0.0);
}

#[test]
fn an_island_inside_a_hole_is_an_outer_contour_again() {
    let big = filled(
        "big",
        Path::polyline(
            &[pt(0.0, 0.0), pt(40.0, 0.0), pt(40.0, 40.0), pt(0.0, 40.0)],
            true,
        ),
    );
    let hole = filled(
        "hole",
        Path::polyline(
            &[pt(5.0, 5.0), pt(5.0, 35.0), pt(35.0, 35.0), pt(35.0, 5.0)],
            true,
        ),
    );
    let island = filled(
        "island",
        Path::polyline(
            &[
                pt(15.0, 15.0),
                pt(25.0, 15.0),
                pt(25.0, 25.0),
                pt(15.0, 25.0),
            ],
            true,
        ),
    );
    let r = repair(vec![big, hole, island], &opts());
    let areas: Vec<f64> = r.items.iter().map(|i| i.geom.signed_area()).collect();
    assert!(
        areas[0] > 0.0 && areas[1] < 0.0 && areas[2] > 0.0,
        "{areas:?}"
    );
}

// ---- general ----

#[test]
fn repair_keeps_the_order_of_the_remaining_items() {
    let items = vec![
        item("a", Path::rect(0.0, 0.0, 1.0, 1.0)),
        item("b", Path::rect(10.0, 0.0, 1.0, 1.0)),
        item("c", Path::rect(20.0, 0.0, 1.0, 1.0)),
    ];
    assert_eq!(
        keys(&repair(items, &opts()).items),
        vec!["src:a", "src:b", "src:c"]
    );
}

#[test]
fn repairing_twice_changes_nothing_more() {
    let open = Path::polyline(
        &[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 0.02)],
        false,
    );
    let first = repair(
        vec![
            filled("a", open),
            item("dup", Path::rect(30.0, 0.0, 2.0, 2.0)),
            item("dup2", Path::rect(30.0, 0.0, 2.0, 2.0)),
        ],
        &opts(),
    );
    let second = repair(first.items.clone(), &opts());
    assert_eq!(second.items, first.items);
    assert_eq!(second.report.len(), 0);
}

#[test]
fn an_empty_drawing_repairs_to_nothing() {
    let r = repair(Vec::new(), &opts());
    assert_eq!(r.items.len(), 0);
    assert_eq!(r.report.len(), 0);
}
