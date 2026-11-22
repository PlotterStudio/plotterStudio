//! Ground truth for geometry preparation: per path and operation,
//! passes, perforation, fills, and drive-type processing (drag compensation, tangential splitting).

use std::collections::BTreeMap;

use ps_core::{PathKey, PtUm};
use ps_geometry::{Affine, FillRule, FillSpec, Path, Pattern, Point, Seg, flatten};
use ps_library::DriveType;
use ps_model::{OpKind, ShapePath, SourceStyle};

use super::*;

fn shape(geom: Path) -> ShapePath {
    ShapePath {
        key: PathKey::source("p"),
        geom,
        fill_rule: FillRule::NonZero,
        style: SourceStyle::default(),
        ops: Vec::new(),
        start_locked: false,
    }
}

fn input<'a>(
    path: &'a ShapePath,
    kind: OpKind,
    drive: DriveType,
    params: &'a BTreeMap<String, f64>,
) -> PrepareInput<'a> {
    PrepareInput {
        path,
        to_machine: Affine::IDENTITY,
        kind,
        drive,
        tip_width_mm: Some(0.4),
        fill: None,
        blade_offset_mm: 0.25,
        wheel_radius_mm: None,
        params,
        turn_step_deg: None,
    }
}

fn no_params() -> BTreeMap<String, f64> {
    BTreeMap::new()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn near_pt(a: Point, x: f64, y: f64) -> bool {
    near(a.x, x) && near(a.y, y)
}

#[test]
fn a_drag_cut_is_compensated_and_starts_on_the_longest_straight_edge() {
    let s = shape(Path::rect(0.0, 0.0, 50.0, 30.0));
    let p = no_params();
    let out = prepare(&input(&s, OpKind::Cut, DriveType::Drag, &p));
    assert_eq!(out.strokes.len(), 1);
    // The tip starts mid-edge at (25, 0); the holder is displaced forward by 0.25 mm.
    assert!(
        near_pt(out.strokes[0].path.start, 25.25, 0.0),
        "{:?}",
        out.strokes[0].path.start
    );
    assert!(
        !out.strokes[0].reversible,
        "a drag blade keeps its direction"
    );
}

#[test]
fn a_closed_cut_keeps_its_artwork_ring_for_containment() {
    let s = shape(Path::rect(0.0, 0.0, 50.0, 30.0));
    let p = no_params();
    let out = prepare(&input(&s, OpKind::Cut, DriveType::Drag, &p));
    let ring = out.strokes[0].tip_ring.as_ref().unwrap();
    // The same rectangle, listed from the default drag start (the middle of the top edge).
    assert_eq!(
        ring,
        &vec![
            PtUm::new(25_000, 0),
            PtUm::new(50_000, 0),
            PtUm::new(50_000, 30_000),
            PtUm::new(0, 30_000),
            PtUm::new(0, 0)
        ]
    );
}

#[test]
fn the_overcut_parameter_extends_the_closed_cut() {
    let s = shape(Path::rect(0.0, 0.0, 50.0, 30.0));
    let mut p = no_params();
    p.insert("overcut".into(), 1.0);
    let out = prepare(&input(&s, OpKind::Cut, DriveType::Drag, &p));
    let end = out.strokes[0].path.end();
    assert!(near_pt(end, 26.25, 0.0), "{end:?}");
}

#[test]
fn an_open_cut_is_displaced_but_not_overcut() {
    let s = shape(Path::polyline(
        &[Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
        false,
    ));
    let mut p = no_params();
    p.insert("overcut".into(), 1.0);
    let out = prepare(&input(&s, OpKind::Cut, DriveType::Drag, &p));
    assert!(near_pt(out.strokes[0].path.start, 0.25, 0.0));
    assert!(near_pt(out.strokes[0].path.end(), 10.25, 0.0));
    assert!(out.strokes[0].tip_ring.is_none());
    assert!(!out.strokes[0].reversible);
}

#[test]
fn a_pen_follows_the_path_exactly_and_may_run_either_way() {
    let s = shape(Path::polyline(
        &[Point::new(1.0, 2.0), Point::new(11.0, 2.0)],
        false,
    ));
    let p = no_params();
    let out = prepare(&input(&s, OpKind::Draw, DriveType::FixedPoint, &p));
    assert_eq!(out.strokes.len(), 1);
    assert!(near_pt(out.strokes[0].path.start, 1.0, 2.0));
    assert!(near_pt(out.strokes[0].path.end(), 11.0, 2.0));
    assert!(out.strokes[0].reversible);
}

#[test]
fn a_closed_pen_outline_stays_closed_and_reversible_in_direction_only_when_open() {
    let s = shape(Path::rect(0.0, 0.0, 10.0, 10.0));
    let p = no_params();
    let out = prepare(&input(&s, OpKind::Draw, DriveType::FixedPoint, &p));
    assert!(out.strokes[0].path.closed);
    assert!(out.strokes[0].tip_ring.is_some());
}

#[test]
fn the_transform_is_applied_first() {
    let s = shape(Path::polyline(
        &[Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
        false,
    ));
    let p = no_params();
    let mut i = input(&s, OpKind::Draw, DriveType::FixedPoint, &p);
    i.to_machine = Affine::translate((5.0, 7.0));
    let out = prepare(&i);
    assert!(near_pt(out.strokes[0].path.start, 5.0, 7.0));
    assert!(near_pt(out.strokes[0].path.end(), 15.0, 7.0));
}

#[test]
fn a_mirror_reverses_the_direction_of_travel() {
    let s = shape(Path::polyline(
        &[Point::new(10.0, 0.0), Point::new(30.0, 0.0)],
        false,
    ));
    let p = no_params();
    let mut i = input(&s, OpKind::Draw, DriveType::FixedPoint, &p);
    i.to_machine = Affine::new([-1.0, 0.0, 0.0, 1.0, 100.0, 0.0]);
    let out = prepare(&i);
    assert!(near_pt(out.strokes[0].path.start, 90.0, 0.0));
    assert!(near_pt(out.strokes[0].path.end(), 70.0, 0.0));
}

#[test]
fn passes_repeat_the_stroke() {
    let s = shape(Path::polyline(
        &[Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
        false,
    ));
    let mut p = no_params();
    p.insert("passes".into(), 3.0);
    let out = prepare(&input(&s, OpKind::Draw, DriveType::FixedPoint, &p));
    assert_eq!(out.strokes.len(), 3);
    assert_eq!(out.strokes[0], out.strokes[2]);
}

#[test]
fn a_fill_hatches_the_region_inside_the_tip_inset() {
    let s = shape(Path::rect(0.0, 0.0, 20.0, 20.0));
    let p = no_params();
    let mut i = input(&s, OpKind::Fill, DriveType::FixedPoint, &p);
    i.fill = Some(FillSpec {
        pattern: Pattern::Parallel,
        angle_deg: 0.0,
        overlap: 0.15,
        outline_pass: false,
    });
    let out = prepare(&i);
    // Inset by half the tip: 0.2 .. 19.8, so 19.6 mm of height at 0.34 mm spacing. The rows are
    // joined into strokes where the pen can stay down, so count the rows, not the strokes.
    let (mut rows, mut links) = (0, 0);
    for st in &out.strokes {
        for (from, seg) in st.path.segments() {
            let Seg::Line { to } = seg else {
                panic!("fill strokes are lines")
            };
            assert!(
                to.x >= 0.2 - 1e-6
                    && to.x <= 19.8 + 1e-6
                    && to.y >= 0.2 - 1e-6
                    && to.y <= 19.8 + 1e-6
            );
            if near(from.y, to.y) {
                rows += 1;
            } else {
                links += 1;
                assert!(
                    libm::hypot(to.x - from.x, to.y - from.y) < 1.0,
                    "a join between rows is short, not {from:?} -> {to:?}"
                );
            }
        }
    }
    assert!((55..=60).contains(&rows), "{rows} rows");
    assert_eq!(
        links,
        rows - 1,
        "each row but the first is joined to the one before"
    );
}

#[test]
fn an_unfilled_fill_operation_defaults_to_a_parallel_hatch() {
    let s = shape(Path::rect(0.0, 0.0, 10.0, 10.0));
    let p = no_params();
    let out = prepare(&input(&s, OpKind::Fill, DriveType::FixedPoint, &p));
    assert_ne!(out.strokes.len(), 0);
}

#[test]
fn perforation_alternates_cut_and_gap_lengths() {
    let s = shape(Path::polyline(
        &[Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
        false,
    ));
    let mut p = no_params();
    p.insert("cut_mm".into(), 2.0);
    p.insert("gap_mm".into(), 1.0);
    let out = prepare(&input(&s, OpKind::Perforate, DriveType::FixedPoint, &p));
    let spans: Vec<(f64, f64)> = out
        .strokes
        .iter()
        .map(|s| (s.path.start.x, s.path.end().x))
        .collect();
    assert_eq!(spans.len(), 4, "{spans:?}");
    for (got, want) in spans
        .iter()
        .zip([(0.0, 2.0), (3.0, 5.0), (6.0, 8.0), (9.0, 10.0)])
    {
        assert!(near(got.0, want.0) && near(got.1, want.1), "{spans:?}");
    }
}

#[test]
fn a_wavy_cut_is_noted_as_approximated() {
    let s = shape(Path::polyline(
        &[Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
        false,
    ));
    let p = no_params();
    let out = prepare(&input(&s, OpKind::WavyCut, DriveType::FixedPoint, &p));
    assert_eq!(out.notes, vec![Note::WavyCutAsStroke]);
    assert_eq!(out.strokes.len(), 1);
}

#[test]
fn a_tangential_tool_lifts_at_sharp_corners() {
    let s = shape(Path::polyline(
        &[
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ],
        false,
    ));
    let p = no_params();
    let out = prepare(&input(&s, OpKind::Cut, DriveType::Tangential, &p));
    assert_eq!(out.strokes.len(), 2);
    assert!(out.strokes.iter().all(|s| s.tangential));
}

#[test]
fn a_tangential_tool_keeps_gentle_curves_in_one_stroke() {
    let s = shape(Path::circle(Point::new(0.0, 0.0), 10.0));
    let p = no_params();
    let out = prepare(&input(&s, OpKind::Cut, DriveType::Tangential, &p));
    assert_eq!(out.strokes.len(), 1);
    assert!(out.strokes[0].tangential);
}

#[test]
fn a_small_drag_arc_is_reported() {
    let tight =
        Path::new(Point::new(0.2, 0.0)).arc_to(Point::new(0.0, 0.2), Point::new(0.0, 0.0), true);
    let s = shape(tight);
    let p = no_params();
    let out = prepare(&input(&s, OpKind::Cut, DriveType::Drag, &p));
    assert_eq!(out.notes.len(), 1);
    assert!(matches!(out.notes[0], Note::MinRadius { .. }));
}

#[test]
fn degenerate_paths_prepare_to_nothing() {
    let s = shape(Path::new(Point::new(1.0, 1.0)));
    let p = no_params();
    assert_eq!(
        prepare(&input(&s, OpKind::Cut, DriveType::Drag, &p))
            .strokes
            .len(),
        0
    );
    assert_eq!(
        prepare(&input(&s, OpKind::Draw, DriveType::FixedPoint, &p))
            .strokes
            .len(),
        0
    );
}

#[test]
fn preparation_is_deterministic() {
    let s = shape(Path::circle(Point::new(20.0, 20.0), 8.0));
    let mut p = no_params();
    p.insert("overcut".into(), 1.0);
    let i = input(&s, OpKind::Cut, DriveType::Drag, &p);
    assert_eq!(prepare(&i), prepare(&i));
}

// ---- bridges, lead-in and lead-out, force ramp ----

fn params(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
}

fn drawn_length(out: &Prepared) -> f64 {
    out.strokes.iter().map(|s| s.path.length()).sum()
}

#[test]
fn tabs_leave_uncut_bridges_spread_evenly_round_a_closed_cut() {
    let s = shape(Path::rect(0.0, 0.0, 100.0, 50.0));
    let p = params(&[("tab_count", 4.0), ("tab_width_mm", 2.0)]);
    let out = prepare(&input(&s, OpKind::Cut, DriveType::FixedPoint, &p));
    assert_eq!(
        out.strokes.len(),
        4,
        "four tabs split a closed path into four pieces"
    );
    assert!(
        (drawn_length(&out) - (300.0 - 4.0 * 2.0)).abs() < 1e-6,
        "{}",
        drawn_length(&out)
    );
}

#[test]
fn tabs_are_centred_at_equal_spacing() {
    // A 100 mm line with 1 tab of 4 mm: the gap is centred at 50 mm, so pieces end at 48 and
    // start at 52.
    let s = shape(Path::polyline(
        &[Point::new(0.0, 0.0), Point::new(100.0, 0.0)],
        false,
    ));
    let p = params(&[("tab_count", 1.0), ("tab_width_mm", 4.0)]);
    let out = prepare(&input(&s, OpKind::Cut, DriveType::FixedPoint, &p));
    assert_eq!(out.strokes.len(), 2);
    assert!(
        near_pt(out.strokes[0].path.end(), 48.0, 0.0),
        "{:?}",
        out.strokes[0].path.end()
    );
    assert!(
        near_pt(out.strokes[1].path.start, 52.0, 0.0),
        "{:?}",
        out.strokes[1].path.start
    );
}

#[test]
fn no_tabs_leaves_the_cut_whole() {
    let s = shape(Path::rect(0.0, 0.0, 100.0, 50.0));
    for p in [
        no_params(),
        params(&[("tab_count", 0.0), ("tab_width_mm", 2.0)]),
        params(&[("tab_count", 3.0), ("tab_width_mm", 0.0)]),
    ] {
        let out = prepare(&input(&s, OpKind::Cut, DriveType::FixedPoint, &p));
        assert_eq!(out.strokes.len(), 1);
        assert!((drawn_length(&out) - 300.0).abs() < 1e-6);
    }
}

#[test]
fn tabs_apply_to_cuts_and_kiss_cuts_only() {
    let s = shape(Path::rect(0.0, 0.0, 100.0, 50.0));
    let p = params(&[("tab_count", 4.0), ("tab_width_mm", 2.0)]);
    assert_eq!(
        prepare(&input(&s, OpKind::KissCut, DriveType::FixedPoint, &p))
            .strokes
            .len(),
        4
    );
    assert_eq!(
        prepare(&input(&s, OpKind::Draw, DriveType::FixedPoint, &p))
            .strokes
            .len(),
        1
    );
    assert_eq!(
        prepare(&input(&s, OpKind::Score, DriveType::FixedPoint, &p))
            .strokes
            .len(),
        1
    );
}

#[test]
fn tabs_too_wide_for_the_path_are_limited_so_something_is_still_cut() {
    let s = shape(Path::polyline(
        &[Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
        false,
    ));
    let p = params(&[("tab_count", 5.0), ("tab_width_mm", 50.0)]);
    let out = prepare(&input(&s, OpKind::Cut, DriveType::FixedPoint, &p));
    assert!(
        drawn_length(&out) >= 5.0,
        "at least half the path stays cut: {}",
        drawn_length(&out)
    );
}

#[test]
fn an_open_cut_is_extended_by_its_lead_in_and_lead_out() {
    let s = shape(Path::polyline(
        &[Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
        false,
    ));
    let p = params(&[("lead_in_mm", 2.0), ("lead_out_mm", 3.0)]);
    let out = prepare(&input(&s, OpKind::Cut, DriveType::FixedPoint, &p));
    assert_eq!(out.strokes.len(), 1);
    let path = &out.strokes[0].path;
    assert!(near_pt(path.start, -2.0, 0.0), "{:?}", path.start);
    assert!(near_pt(path.end(), 13.0, 0.0), "{:?}", path.end());
}

#[test]
fn lead_ins_follow_the_direction_of_the_ends() {
    let s = shape(Path::polyline(
        &[
            Point::new(0.0, 0.0),
            Point::new(0.0, 10.0),
            Point::new(10.0, 10.0),
        ],
        false,
    ));
    let p = params(&[("lead_in_mm", 2.0), ("lead_out_mm", 2.0)]);
    let out = prepare(&input(&s, OpKind::Cut, DriveType::FixedPoint, &p));
    let path = &out.strokes[0].path;
    assert!(near_pt(path.start, 0.0, -2.0), "{:?}", path.start);
    assert!(near_pt(path.end(), 12.0, 10.0), "{:?}", path.end());
}

#[test]
fn closed_cuts_have_no_lead_in_because_they_overcut_instead() {
    let s = shape(Path::rect(0.0, 0.0, 100.0, 50.0));
    let p = params(&[("lead_in_mm", 2.0), ("lead_out_mm", 2.0)]);
    let out = prepare(&input(&s, OpKind::Cut, DriveType::FixedPoint, &p));
    assert!((drawn_length(&out) - 300.0).abs() < 1e-6);
}

#[test]
fn leads_apply_only_to_cut_operations() {
    let s = shape(Path::polyline(
        &[Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
        false,
    ));
    let p = params(&[("lead_in_mm", 2.0)]);
    let out = prepare(&input(&s, OpKind::Draw, DriveType::FixedPoint, &p));
    assert!(near_pt(out.strokes[0].path.start, 0.0, 0.0));
}

#[test]
fn passes_start_light_and_build_up_to_full_force_with_a_ramp() {
    let s = shape(Path::polyline(
        &[Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
        false,
    ));
    let p = params(&[("passes", 3.0), ("force_ramp", 0.5)]);
    let out = prepare(&input(&s, OpKind::Cut, DriveType::FixedPoint, &p));
    let scales: Vec<f64> = out.strokes.iter().map(|s| s.force_scale).collect();
    assert_eq!(scales.len(), 3);
    for (got, want) in scales.iter().zip([0.5 + 0.5 / 3.0, 0.5 + 1.0 / 3.0, 1.0]) {
        assert!((got - want).abs() < 1e-9, "{scales:?}");
    }
}

#[test]
fn without_a_ramp_or_with_one_pass_force_is_full() {
    let s = shape(Path::polyline(
        &[Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
        false,
    ));
    for p in [
        params(&[("passes", 3.0)]),
        params(&[("passes", 1.0), ("force_ramp", 0.8)]),
        no_params(),
    ] {
        let out = prepare(&input(&s, OpKind::Cut, DriveType::FixedPoint, &p));
        assert!(
            out.strokes
                .iter()
                .all(|s| (s.force_scale - 1.0).abs() < 1e-12),
            "{p:?}"
        );
    }
}

#[test]
fn a_ramp_is_limited_to_the_range_zero_to_one() {
    let s = shape(Path::polyline(
        &[Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
        false,
    ));
    let p = params(&[("passes", 2.0), ("force_ramp", 5.0)]);
    let out = prepare(&input(&s, OpKind::Cut, DriveType::FixedPoint, &p));
    assert!(
        out.strokes
            .iter()
            .all(|s| s.force_scale > 0.0 && s.force_scale <= 1.0)
    );
    assert!((out.strokes[1].force_scale - 1.0).abs() < 1e-12);
}

// ---- compound fills ----

fn fill_input<'a>(path: &'a ShapePath, params: &'a BTreeMap<String, f64>) -> PrepareInput<'a> {
    PrepareInput {
        fill: Some(FillSpec {
            pattern: Pattern::Parallel,
            angle_deg: 0.0,
            overlap: 0.15,
            outline_pass: false,
        }),
        ..input(path, OpKind::Fill, DriveType::FixedPoint, params)
    }
}

fn even_odd(geom: Path) -> ShapePath {
    ShapePath {
        fill_rule: FillRule::EvenOdd,
        ..shape(geom)
    }
}

/// True when some stroke point lies strictly inside the box.
fn any_point_inside(out: &Prepared, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
    out.strokes.iter().any(|s| {
        flatten(&s.path, 0.01)
            .iter()
            .any(|p| p.x > x0 && p.x < x1 && p.y > y0 && p.y < y1)
    })
}

#[test]
fn a_compound_fill_leaves_a_hole_where_an_inner_ring_is() {
    let outer = even_odd(Path::rect(0.0, 0.0, 40.0, 40.0));
    let inner = even_odd(Path::rect(10.0, 10.0, 20.0, 20.0));
    let p = no_params();
    let out = prepare_compound(&[fill_input(&outer, &p), fill_input(&inner, &p)]);
    assert_ne!(out.strokes.len(), 0);
    assert!(
        !any_point_inside(&out, 11.0, 11.0, 29.0, 29.0),
        "the hole stays empty"
    );
    assert!(
        any_point_inside(&out, 0.0, 0.0, 10.0, 40.0),
        "the ring around it is filled"
    );
}

#[test]
fn rings_wound_oppositely_make_a_hole_under_the_nonzero_rule() {
    let outer = shape(Path::rect(0.0, 0.0, 40.0, 40.0));
    let inner = shape(Path::rect(10.0, 10.0, 20.0, 20.0).reversed());
    let p = no_params();
    let out = prepare_compound(&[fill_input(&outer, &p), fill_input(&inner, &p)]);
    assert!(!any_point_inside(&out, 11.0, 11.0, 29.0, 29.0));
}

#[test]
fn separate_shapes_are_each_filled() {
    let a = even_odd(Path::rect(0.0, 0.0, 20.0, 20.0));
    let b = even_odd(Path::rect(50.0, 0.0, 20.0, 20.0));
    let p = no_params();
    let out = prepare_compound(&[fill_input(&a, &p), fill_input(&b, &p)]);
    assert!(any_point_inside(&out, 0.0, 0.0, 20.0, 20.0));
    assert!(any_point_inside(&out, 50.0, 0.0, 70.0, 20.0));
}

#[test]
fn a_single_path_compound_is_the_same_as_preparing_it_alone() {
    let a = even_odd(Path::rect(0.0, 0.0, 30.0, 20.0));
    let p = no_params();
    let alone = prepare(&fill_input(&a, &p));
    let compound = prepare_compound(&[fill_input(&a, &p)]);
    assert_eq!(alone.strokes, compound.strokes);
}

#[test]
fn an_empty_group_prepares_to_nothing() {
    assert_eq!(prepare_compound(&[]).strokes.len(), 0);
}

// ---- pen-down links between hatch rows ----

/// A 20 mm capital E (spine and bars 4 mm thick), filled with the default parallel hatch.
fn letter_e_fill() -> Prepared {
    let pts = [
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 4.0),
        (4.0, 4.0),
        (4.0, 8.0),
        (20.0, 8.0),
        (20.0, 12.0),
        (4.0, 12.0),
        (4.0, 16.0),
        (20.0, 16.0),
        (20.0, 20.0),
        (0.0, 20.0),
    ]
    .map(|(x, y)| Point::new(x, y));
    let s = shape(Path::polyline(&pts, true));
    let p = no_params();
    prepare(&fill_input(&s, &p))
}

#[test]
fn a_filled_square_is_drawn_without_lifting_the_pen() {
    let s = shape(Path::rect(0.0, 0.0, 20.0, 20.0));
    let p = no_params();
    let out = prepare(&fill_input(&s, &p));
    assert_eq!(out.strokes.len(), 1, "one stroke, not one per row");
}

#[test]
fn the_rows_of_a_letter_are_joined_into_a_few_strokes() {
    let out = letter_e_fill();
    // About 57 rows of hatch; a few strokes is the most the pen should need.
    assert!(
        out.strokes.len() <= 12,
        "{} strokes for one letter",
        out.strokes.len()
    );
}

#[test]
fn no_link_between_rows_crosses_a_notch_of_the_letter() {
    let out = letter_e_fill();
    for st in &out.strokes {
        let pts = flatten(&st.path, 0.01);
        for w in pts.windows(2) {
            let (mx, my) = (f64::midpoint(w[0].x, w[1].x), f64::midpoint(w[0].y, w[1].y));
            let in_notch = mx > 4.0 + 1e-6 && ((my > 4.0 && my < 8.0) || (my > 12.0 && my < 16.0));
            assert!(!in_notch, "{:?} -> {:?} crosses a notch", w[0], w[1]);
        }
    }
}

#[test]
fn joined_strokes_stay_inside_the_tip_inset() {
    let out = letter_e_fill();
    for st in &out.strokes {
        for p in flatten(&st.path, 0.01) {
            assert!(
                p.x >= 0.2 - 0.03 && p.x <= 19.8 + 0.03 && p.y >= 0.2 - 0.03 && p.y <= 19.8 + 0.03,
                "{p:?} is outside the inset"
            );
        }
    }
}

// ---- rolling wheels ----

fn with_wheel(mut i: PrepareInput<'_>, radius: f64) -> PrepareInput<'_> {
    i.wheel_radius_mm = Some(radius);
    i
}

#[test]
fn a_rolling_wheel_overshoots_each_corner_by_its_radius_so_the_contact_reaches_it() {
    // An L: right 10 mm, then down 10 mm. The wheel is 2 mm in radius.
    let s = shape(Path::polyline(
        &[
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ],
        false,
    ));
    let p = params(&[("corner_lift_deg", 30.0)]);
    let out = prepare(&with_wheel(
        input(&s, OpKind::Cut, DriveType::Rolling, &p),
        2.0,
    ));
    assert_eq!(out.strokes.len(), 2, "lifted at the corner");
    assert!(
        near_pt(out.strokes[0].path.end(), 12.0, 0.0),
        "{:?}",
        out.strokes[0].path.end()
    );
    assert!(
        near_pt(out.strokes[1].path.start, 10.0, -2.0),
        "{:?}",
        out.strokes[1].path.start
    );
    assert!(near_pt(out.strokes[1].path.end(), 10.0, 10.0));
}

#[test]
fn without_a_wheel_radius_a_rolling_tool_is_plain_tangential() {
    let s = shape(Path::polyline(
        &[
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ],
        false,
    ));
    let p = no_params();
    let out = prepare(&input(&s, OpKind::Cut, DriveType::Rolling, &p));
    assert!(near_pt(out.strokes[0].path.end(), 10.0, 0.0));
    assert!(near_pt(out.strokes[1].path.start, 10.0, 0.0));
}

#[test]
fn a_wheel_does_not_change_paths_with_no_corners() {
    let s = shape(Path::polyline(
        &[Point::new(0.0, 0.0), Point::new(20.0, 0.0)],
        false,
    ));
    let p = no_params();
    let out = prepare(&with_wheel(
        input(&s, OpKind::Cut, DriveType::Rolling, &p),
        2.0,
    ));
    assert_eq!(out.strokes.len(), 1);
    assert!(
        near_pt(out.strokes[0].path.start, 0.0, 0.0)
            && near_pt(out.strokes[0].path.end(), 20.0, 0.0)
    );
}

#[test]
fn only_rolling_tools_use_the_wheel_radius() {
    let s = shape(Path::polyline(
        &[
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ],
        false,
    ));
    let p = no_params();
    let out = prepare(&with_wheel(
        input(&s, OpKind::Cut, DriveType::Tangential, &p),
        2.0,
    ));
    assert!(near_pt(out.strokes[0].path.end(), 10.0, 0.0));
}

// ---- user start points ----

fn locked(mut s: ShapePath) -> ShapePath {
    s.start_locked = true;
    s
}

#[test]
fn a_locked_start_keeps_a_closed_pen_stroke_where_it_starts() {
    let s = locked(shape(Path::rect(0.0, 0.0, 20.0, 10.0)));
    let p = no_params();
    let out = prepare(&input(&s, OpKind::Draw, DriveType::FixedPoint, &p));
    assert!(out.strokes[0].fixed_start);
    assert!(near_pt(out.strokes[0].path.start, 0.0, 0.0));
}

#[test]
fn an_unlocked_start_is_decided_before_ordering_so_nothing_downstream_moves_it() {
    let s = shape(Path::rect(0.0, 0.0, 20.0, 10.0));
    let p = no_params();
    let out = prepare(&input(&s, OpKind::Draw, DriveType::FixedPoint, &p));
    assert!(out.strokes[0].fixed_start);
}

#[test]
fn a_drag_blade_starts_at_the_locked_vertex_instead_of_the_longest_edge() {
    let s = locked(shape(Path::rect(0.0, 0.0, 50.0, 30.0)));
    let p = no_params();
    let out = prepare(&input(&s, OpKind::Cut, DriveType::Drag, &p));
    let start = out.strokes[0].path.start;
    // The corner (0, 0), the holder displaced a little ahead of it, not mid-edge (25, 0).
    assert!(start.x < 2.0 && start.y.abs() < 2.0, "{start:?}");
    assert!(out.strokes[0].fixed_start);
}

#[test]
fn a_locked_start_is_ignored_on_open_paths() {
    let s = locked(shape(Path::polyline(
        &[Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
        false,
    )));
    let p = no_params();
    let out = prepare(&input(&s, OpKind::Draw, DriveType::FixedPoint, &p));
    assert!(!out.strokes[0].fixed_start);
}

#[test]
fn the_default_start_is_the_longest_edge_for_a_drag_blade_and_the_path_start_otherwise() {
    let s = shape(Path::rect(0.0, 0.0, 50.0, 30.0));
    let drag = effective_start_geom(&s, DriveType::Drag);
    assert!(near_pt(drag.start, 25.0, 0.0), "{:?}", drag.start);
    let pen = effective_start_geom(&s, DriveType::FixedPoint);
    assert!(near_pt(pen.start, 0.0, 0.0));
    let locked = effective_start_geom(&locked(s), DriveType::Drag);
    assert!(near_pt(locked.start, 0.0, 0.0), "a locked start is kept");
}

/// A regular five-pointed star: all ten edges are the same length, up to float noise.
fn regular_star() -> Path {
    let vertices: Vec<Point> = (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { 20.0 } else { 8.0 };
            let a = f64::from(i) * std::f64::consts::PI / 5.0;
            Point::new(50.0 + r * libm::cos(a), 50.0 + r * libm::sin(a))
        })
        .collect();
    Path::polyline(&vertices, true)
}

#[test]
fn the_cut_starts_exactly_where_the_start_marker_is_drawn_whatever_the_placement() {
    // All ten edges of a regular star tie, so any second calculation of the start would be at the
    // mercy of float noise from the placement.
    let s = shape(regular_star());
    let p = no_params();
    let mut inp = input(&s, OpKind::Cut, DriveType::Drag, &p);
    inp.blade_offset_mm = 0.0;
    let to_machine = ps_geometry::Affine::translate((13.7, 4.1))
        * ps_geometry::Affine::new([
            libm::cos(0.7),
            libm::sin(0.7),
            -libm::sin(0.7),
            libm::cos(0.7),
            0.0,
            0.0,
        ])
        * ps_geometry::Affine::scale_non_uniform(-1.0, 1.0);
    inp.to_machine = to_machine;
    let marker = to_machine * effective_start_geom(&s, DriveType::Drag).start;
    let start = prepare(&inp).strokes[0].path.start;
    assert!(
        near_pt(start, marker.x, marker.y),
        "{start:?} vs {marker:?}"
    );
}
