//! Ground truth for re-import matching. When a source file changes, paths are matched
//! old to new in three passes: (1) same source id, (2) same structural fingerprint, (3) same layer
//! and style with a Hausdorff distance under 0.5 mm. Matched paths keep their operations and
//! overrides. The user sees a summary (matched, changed geometry, added, removed) before anything
//! is applied.

use ps_core::{ContentHash, PathKey};
use ps_geometry::{Path, Point};
use ps_model::SourceStyle;

use super::*;
use crate::drawing::ImportedPath;

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Path {
    Path::polyline(&[pt(x0, y0), pt(x1, y1)], false)
}

fn path(key: PathKey, layer: Option<&str>, geom: Path) -> ImportedPath {
    ImportedPath {
        key,
        layer: layer.map(str::to_owned),
        geom,
        style: SourceStyle {
            stroke: Some([0, 0, 0]),
            layer: layer.map(str::to_owned),
            ..SourceStyle::default()
        },
    }
}

fn id(name: &str) -> PathKey {
    PathKey::source(name)
}

fn fp(n: u8) -> PathKey {
    PathKey::fingerprint(ContentHash::from_bytes([n; 32]))
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

// ---- Hausdorff distance ----

#[test]
fn identical_paths_are_zero_apart() {
    let a = line(0.0, 0.0, 10.0, 0.0);
    assert!(near(hausdorff(&a, &a), 0.0, 1e-9));
}

#[test]
fn a_translation_is_the_hausdorff_distance() {
    let a = line(0.0, 0.0, 10.0, 0.0);
    let b = line(0.0, 0.3, 10.0, 0.3);
    assert!(near(hausdorff(&a, &b), 0.3, 1e-6), "{}", hausdorff(&a, &b));
}

#[test]
fn a_shorter_path_is_as_far_as_its_missing_part() {
    // (10, 0) is 5 away from the nearest point of the shorter path.
    let long = line(0.0, 0.0, 10.0, 0.0);
    let short = line(0.0, 0.0, 5.0, 0.0);
    assert!(
        near(hausdorff(&long, &short), 5.0, 0.06),
        "{}",
        hausdorff(&long, &short)
    );
}

#[test]
fn the_distance_is_symmetric() {
    let a = Path::polyline(&[pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0)], false);
    let b = line(0.0, 1.0, 12.0, 1.0);
    assert!(near(hausdorff(&a, &b), hausdorff(&b, &a), 1e-9));
}

#[test]
fn a_curve_is_measured_along_its_length_not_just_at_its_vertices() {
    // A semicircle of radius 10 against its chord: the arc bulges 10 mm away from the chord.
    let arc = Path::new(pt(10.0, 0.0)).arc_to(pt(-10.0, 0.0), pt(0.0, 0.0), true);
    let chord = line(10.0, 0.0, -10.0, 0.0);
    assert!(
        near(hausdorff(&arc, &chord), 10.0, 0.06),
        "{}",
        hausdorff(&arc, &chord)
    );
}

#[test]
fn an_empty_path_is_infinitely_far_from_anything() {
    assert!(hausdorff(&Path::new(pt(0.0, 0.0)), &line(0.0, 0.0, 1.0, 0.0)).is_infinite());
}

// ---- the three passes ----

fn summary(old: &[ImportedPath], new: &[ImportedPath]) -> Reimport {
    match_paths(old, new)
}

#[test]
fn pass_one_matches_by_source_id_even_when_the_geometry_changed() {
    let old = [path(id("a"), None, line(0.0, 0.0, 10.0, 0.0))];
    let new = [path(id("a"), None, line(50.0, 50.0, 60.0, 50.0))];
    let r = summary(&old, &new);
    assert_eq!(r.matched.len(), 1);
    assert_eq!(r.matched[0].pass, MatchPass::SourceId);
    assert!(r.matched[0].geometry_changed);
    assert_eq!((r.added.len(), r.removed.len()), (0, 0));
}

#[test]
fn pass_two_matches_by_fingerprint_and_reports_no_change() {
    let g = line(0.0, 0.0, 10.0, 0.0);
    let old = [path(fp(1), None, g.clone())];
    let new = [path(fp(1), None, g)];
    let r = summary(&old, &new);
    assert_eq!(r.matched[0].pass, MatchPass::Fingerprint);
    assert!(!r.matched[0].geometry_changed);
}

#[test]
fn pass_three_matches_a_nearby_path_in_the_same_layer_and_style() {
    let old = [path(fp(1), Some("Cut"), line(0.0, 0.0, 10.0, 0.0))];
    let new = [path(fp(2), Some("Cut"), line(0.0, 0.2, 10.0, 0.2))];
    let r = summary(&old, &new);
    assert_eq!(r.matched.len(), 1);
    assert_eq!(r.matched[0].pass, MatchPass::Proximity);
    assert!(r.matched[0].geometry_changed);
}

#[test]
fn a_path_moved_by_more_than_half_a_millimetre_is_removed_and_added() {
    let old = [path(fp(1), Some("Cut"), line(0.0, 0.0, 10.0, 0.0))];
    let new = [path(fp(2), Some("Cut"), line(0.0, 0.6, 10.0, 0.6))];
    let r = summary(&old, &new);
    assert_eq!(r.matched.len(), 0);
    assert_eq!(r.removed, vec![0]);
    assert_eq!(r.added, vec![0]);
}

#[test]
fn proximity_needs_the_same_layer() {
    let old = [path(fp(1), Some("Cut"), line(0.0, 0.0, 10.0, 0.0))];
    let new = [path(fp(2), Some("Draw"), line(0.0, 0.0, 10.0, 0.0))];
    assert_eq!(summary(&old, &new).matched.len(), 0);
}

#[test]
fn proximity_needs_the_same_style() {
    let old = [path(fp(1), None, line(0.0, 0.0, 10.0, 0.0))];
    let mut other = path(fp(2), None, line(0.0, 0.0, 10.0, 0.0));
    other.style.stroke = Some([255, 0, 0]);
    assert_eq!(summary(&old, &[other]).matched.len(), 0);
}

#[test]
fn each_path_is_matched_at_most_once_and_the_nearest_wins() {
    let old = [
        path(fp(1), None, line(0.0, 0.0, 10.0, 0.0)),
        path(fp(2), None, line(0.0, 0.3, 10.0, 0.3)),
    ];
    let new = [path(fp(9), None, line(0.0, 0.25, 10.0, 0.25))];
    let r = summary(&old, &new);
    assert_eq!(r.matched.len(), 1);
    assert_eq!(r.matched[0].old, 1, "0.05 away beats 0.25 away");
    assert_eq!(r.removed, vec![0]);
}

#[test]
fn earlier_passes_take_their_paths_before_later_ones() {
    // Old 0 would be nearest to new 0 by position, but new 0 carries old 1's source id.
    let old = [
        path(id("first"), None, line(0.0, 0.0, 10.0, 0.0)),
        path(id("second"), None, line(30.0, 0.0, 40.0, 0.0)),
    ];
    let new = [
        path(id("second"), None, line(0.0, 0.0, 10.0, 0.0)),
        path(id("brand-new"), None, line(80.0, 0.0, 90.0, 0.0)),
    ];
    let r = summary(&old, &new);
    let pairs: Vec<(usize, usize)> = r.matched.iter().map(|m| (m.old, m.new)).collect();
    assert_eq!(pairs, vec![(1, 0)]);
    assert_eq!(r.removed, vec![0]);
    assert_eq!(r.added, vec![1]);
}

#[test]
fn the_counts_always_add_up() {
    let old: Vec<ImportedPath> = (0..6)
        .map(|i| {
            path(
                fp(i),
                Some("L"),
                line(f64::from(i) * 20.0, 0.0, f64::from(i) * 20.0 + 10.0, 0.0),
            )
        })
        .collect();
    let new: Vec<ImportedPath> = (3..9)
        .map(|i| {
            path(
                fp(i + 20),
                Some("L"),
                line(f64::from(i) * 20.0, 0.1, f64::from(i) * 20.0 + 10.0, 0.1),
            )
        })
        .collect();
    let r = summary(&old, &new);
    assert_eq!(r.matched.len() + r.removed.len(), old.len());
    assert_eq!(r.matched.len() + r.added.len(), new.len());
    assert_eq!(r.matched.len(), 3);
}

#[test]
fn a_summary_counts_each_kind_of_change() {
    let old = [
        path(id("same"), None, line(0.0, 0.0, 10.0, 0.0)),
        path(id("edited"), None, line(0.0, 5.0, 10.0, 5.0)),
        path(id("gone"), None, line(0.0, 90.0, 10.0, 90.0)),
    ];
    let new = [
        path(id("same"), None, line(0.0, 0.0, 10.0, 0.0)),
        path(id("edited"), None, line(0.0, 5.0, 12.0, 5.0)),
        path(id("fresh"), None, line(50.0, 50.0, 60.0, 50.0)),
    ];
    let s = summary(&old, &new).counts();
    assert_eq!(
        (s.matched, s.changed_geometry, s.added, s.removed),
        (2, 1, 1, 1)
    );
}

#[test]
fn matching_empty_inputs_is_fine() {
    let r = summary(&[], &[]);
    assert_eq!((r.matched.len(), r.added.len(), r.removed.len()), (0, 0, 0));
    let only_new = summary(&[], &[path(id("a"), None, line(0.0, 0.0, 1.0, 0.0))]);
    assert_eq!(only_new.added, vec![0]);
}

#[test]
fn matching_is_deterministic() {
    let old: Vec<ImportedPath> = (0..10)
        .map(|i| {
            path(
                fp(i),
                None,
                line(0.0, f64::from(i) * 0.3, 10.0, f64::from(i) * 0.3),
            )
        })
        .collect();
    let new: Vec<ImportedPath> = (0..10)
        .map(|i| {
            path(
                fp(i + 50),
                None,
                line(
                    0.0,
                    f64::from(i) * 0.3 + 0.1,
                    10.0,
                    f64::from(i) * 0.3 + 0.1,
                ),
            )
        })
        .collect();
    assert_eq!(summary(&old, &new), summary(&old, &new));
}
