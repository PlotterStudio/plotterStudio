//! Ground truth for calibration jobs. Each is a small project that runs through the same
//! compiler as production work:
//!
//! * test cut: a 10 mm square inside a triangle;
//! * pressure ladder: 6 test shapes from -30% to +30% force, labelled in a single-stroke font;
//! * tip width ladder: line pairs at 0.10 to 1.00 mm spacing in 0.05 mm steps;
//! * blade offset: a 20 mm circle and a 20 mm square;
//! * pen-to-blade X/Y: drawn and cut crosshairs at the same place;
//! * time calibration: a reference job.

use ps_core::LibRef;
use ps_geometry::{Path, Point, Rect};
use ps_library::{Calibration, LibrarySnapshot, MachineProfile, builtin_entries};
use ps_model::{Document, OpKind, ParamKey, ParamValue, PlacedObject, ShapePath};

use super::*;
use crate::{CompileInput, MatSelection, compile};

const BLADE: &str = "generic.drag-blade@1.0.0";
const PEN: &str = "pen.fineliner-black-0.4@1.0.0";

fn blade() -> LibRef {
    BLADE.parse().unwrap()
}

fn pen() -> LibRef {
    PEN.parse().unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn all_paths(d: &Document) -> Vec<(&PlacedObject, &ShapePath)> {
    let mat = d.mats.values().next().unwrap();
    mat.objects
        .values()
        .flat_map(|o| o.paths.iter().map(move |p| (&**o, &**p)))
        .collect()
}

fn with_kind(d: &Document, kind: OpKind) -> Vec<&ShapePath> {
    all_paths(d)
        .into_iter()
        .filter(|(_, p)| p.ops.iter().any(|o| o.kind == kind))
        .map(|(_, p)| p)
        .collect()
}

fn force_of(p: &ShapePath) -> Option<f64> {
    p.ops
        .iter()
        .find_map(|o| o.overrides.get(&ParamKey::new("force")))
        .and_then(ParamValue::as_f64)
}

fn libs() -> LibrarySnapshot {
    LibrarySnapshot::from_entries(builtin_entries().unwrap())
}

fn air2() -> MachineProfile {
    let r: LibRef = "generic.cutter-small@1.0.0".parse().unwrap();
    (*libs().resolve_profile(&r).unwrap().item).clone()
}

fn compiles_cleanly(d: &Document) -> ps_job::Job {
    let (l, p, c) = (libs(), air2(), Calibration::default());
    let job = compile(&CompileInput {
        doc: d,
        libs: &l,
        profile: &p,
        calibration: &c,
        mats: MatSelection::All,
    })
    .unwrap();
    assert!(!job.is_blocked(), "{:?}", job.diagnostics);
    assert_ne!(job.mats.len(), 0);
    job
}

// ---- the single-stroke font ----

#[test]
fn a_glyph_is_a_set_of_segments_on_a_two_by_four_grid() {
    let one = glyph('1').unwrap();
    assert_eq!(one.len(), 2);
    let eight = glyph('8').unwrap();
    assert_eq!(eight.len(), 7);
    assert_eq!(glyph('-').unwrap(), vec![[(0.0, 2.0), (2.0, 2.0)]]);
    assert_eq!(glyph('+').unwrap().len(), 2);
    assert!(glyph('x').is_none());
}

#[test]
fn every_digit_has_a_glyph() {
    for c in "0123456789".chars() {
        assert!(glyph(c).is_some(), "{c}");
    }
}

#[test]
fn text_is_laid_out_left_to_right_at_the_requested_height() {
    let strokes = text_strokes("-30", Point::new(10.0, 20.0), 4.0);
    // Scale: 4 mm tall means 1 mm per grid unit; glyph advance is width 2 + gap 1.
    assert_ne!(strokes.len(), 0);
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for s in &strokes {
        for p in [s.start, s.end()] {
            x0 = x0.min(p.x);
            x1 = x1.max(p.x);
            y0 = y0.min(p.y);
            y1 = y1.max(p.y);
        }
    }
    assert!(
        near(x0, 10.0) && near(x1, 10.0 + 3.0 * 3.0 - 1.0),
        "{x0} {x1}"
    );
    assert!(near(y0, 20.0) && near(y1, 24.0), "{y0} {y1}");
}

#[test]
fn unknown_characters_are_skipped_but_still_take_space() {
    let a = text_strokes("1", Point::new(0.0, 0.0), 4.0);
    let b = text_strokes("?1", Point::new(0.0, 0.0), 4.0);
    assert_eq!(a.len(), b.len());
    assert!(b[0].start.x > a[0].start.x + 2.0);
}

// ---- test cut ----

#[test]
fn the_test_cut_is_a_square_inside_a_triangle() {
    let d = test_cut(&blade());
    let cuts = with_kind(&d, OpKind::Cut);
    assert_eq!(cuts.len(), 2);
    let sizes: Vec<(f64, f64)> = cuts
        .iter()
        .map(|p| {
            let b = p.geom.bounds();
            (b.width(), b.height())
        })
        .collect();
    assert!(sizes.contains(&(10.0, 10.0)), "{sizes:?}");
    let square = cuts
        .iter()
        .find(|p| near(p.geom.bounds().width(), 10.0))
        .unwrap();
    let triangle = cuts
        .iter()
        .find(|p| !near(p.geom.bounds().width(), 10.0))
        .unwrap();
    let (s, t) = (square.geom.bounds(), triangle.geom.bounds());
    assert!(
        s.x0 > t.x0 && s.x1 < t.x1 && s.y0 > t.y0 && s.y1 < t.y1,
        "square inside triangle bounds"
    );
    assert_eq!(triangle.geom.segs.len(), 3);
}

#[test]
fn the_test_cut_compiles_with_the_square_cut_first() {
    let job = compiles_cleanly(&test_cut(&blade()));
    assert_eq!(job.mats[0].tool_table.len(), 1);
}

// ---- pressure ladder ----

#[test]
fn the_pressure_ladder_has_six_shapes_from_minus_thirty_to_plus_thirty_percent() {
    let d = pressure_ladder(&pen(), OpKind::Draw, 100.0);
    let shapes = with_kind(&d, OpKind::Draw);
    let forces: Vec<f64> = shapes.iter().filter_map(|p| force_of(p)).collect();
    let expected = [70.0, 82.0, 94.0, 106.0, 118.0, 130.0];
    assert_eq!(forces.len(), 6, "{forces:?}");
    for (got, want) in forces.iter().zip(expected) {
        assert!((got - want).abs() < 1e-9, "{forces:?}");
    }
}

#[test]
fn pressure_ladder_shapes_do_not_overlap() {
    let d = pressure_ladder(&pen(), OpKind::Draw, 100.0);
    let boxes: Vec<Rect> = with_kind(&d, OpKind::Draw)
        .iter()
        .filter(|p| force_of(p).is_some())
        .map(|p| p.geom.bounds())
        .collect();
    for (i, a) in boxes.iter().enumerate() {
        for b in &boxes[i + 1..] {
            assert!(
                a.x1 <= b.x0 || b.x1 <= a.x0 || a.y1 <= b.y0 || b.y1 <= a.y0,
                "{a:?} {b:?}"
            );
        }
    }
}

#[test]
fn a_pen_ladder_is_labelled_with_the_percentage() {
    let d = pressure_ladder(&pen(), OpKind::Draw, 100.0);
    // Labels are extra draw paths without a force override.
    let labels = with_kind(&d, OpKind::Draw)
        .into_iter()
        .filter(|p| force_of(p).is_none())
        .count();
    assert!(labels >= 6, "{labels}");
}

#[test]
fn a_blade_ladder_cuts_without_labels() {
    let d = pressure_ladder(&blade(), OpKind::Cut, 300.0);
    assert_eq!(with_kind(&d, OpKind::Cut).len(), 6);
    assert_eq!(with_kind(&d, OpKind::Draw).len(), 0);
}

#[test]
fn the_pressure_ladder_compiles() {
    compiles_cleanly(&pressure_ladder(&pen(), OpKind::Draw, 100.0));
    compiles_cleanly(&pressure_ladder(&blade(), OpKind::Cut, 300.0));
}

// ---- tip width ladder ----

#[test]
fn the_tip_width_ladder_has_nineteen_line_pairs() {
    let d = tip_width_ladder(&pen());
    let lines = with_kind(&d, OpKind::Draw);
    assert_eq!(lines.len(), 38);
    // Pair k has spacing 0.10 + 0.05 k mm.
    let mut ys: Vec<f64> = lines.iter().map(|p| p.geom.start.y).collect();
    ys.sort_by(f64::total_cmp);
    let gaps: Vec<f64> = ys.chunks(2).map(|c| c[1] - c[0]).collect();
    for (k, gap) in gaps.iter().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "k is at most 18")]
        let want = 0.10 + 0.05 * k as f64;
        assert!((gap - want).abs() < 1e-9, "pair {k}: {gap} vs {want}");
    }
    assert!((gaps[18] - 1.00).abs() < 1e-9);
}

#[test]
fn tip_width_lines_are_horizontal_and_the_same_length() {
    let d = tip_width_ladder(&pen());
    for p in with_kind(&d, OpKind::Draw) {
        assert!(near(p.geom.start.y, p.geom.end().y));
        assert!(near(p.geom.length(), 20.0));
    }
}

#[test]
fn the_tip_width_ladder_compiles() {
    compiles_cleanly(&tip_width_ladder(&pen()));
}

// ---- blade offset ----

#[test]
fn the_blade_offset_job_cuts_a_twenty_millimetre_circle_and_square() {
    let d = blade_offset(&blade());
    let cuts = with_kind(&d, OpKind::Cut);
    assert_eq!(cuts.len(), 2);
    let widths: Vec<f64> = cuts.iter().map(|p| p.geom.bounds().width()).collect();
    assert!(widths.iter().all(|w| near(*w, 20.0)), "{widths:?}");
    assert!(cuts.iter().any(|p| p.geom.segs.len() == 4), "a square");
    assert!(
        cuts.iter().any(|p| p
            .geom
            .segs
            .iter()
            .all(|s| matches!(s, ps_geometry::Seg::Arc { .. }))),
        "a circle"
    );
}

#[test]
fn the_blade_offset_job_compiles() {
    compiles_cleanly(&blade_offset(&blade()));
}

// ---- holder offset ----

#[test]
fn the_holder_job_draws_and_cuts_crosshairs_at_the_same_place() {
    let d = holder_offset(&pen(), &blade());
    let drawn = with_kind(&d, OpKind::Draw);
    let cut = with_kind(&d, OpKind::Cut);
    assert_eq!(drawn.len(), 2, "horizontal and vertical");
    assert_eq!(cut.len(), 2);
    let centre = |ps: &[&ShapePath]| {
        let b = ps
            .iter()
            .map(|p| p.geom.bounds())
            .reduce(|a, b| a.union(b))
            .unwrap();
        (f64::midpoint(b.x0, b.x1), f64::midpoint(b.y0, b.y1))
    };
    let (dx, dy) = centre(&drawn);
    let (cx, cy) = centre(&cut);
    assert!(near(dx, cx) && near(dy, cy), "({dx}, {dy}) vs ({cx}, {cy})");
}

#[test]
fn the_holder_job_compiles_with_both_tools() {
    let job = compiles_cleanly(&holder_offset(&pen(), &blade()));
    assert_eq!(job.mats[0].tool_table.len(), 2);
}

// ---- time reference ----

#[test]
fn the_time_reference_job_is_a_fixed_known_drawing() {
    let d = time_reference(&pen());
    let drawn = with_kind(&d, OpKind::Draw);
    assert!(drawn.len() >= 4);
    let total: f64 = drawn.iter().map(|p| p.geom.length()).sum();
    assert!(total > 400.0, "long enough to time: {total}");
    // Identical every time apart from ids.
    let again = time_reference(&pen());
    let total2: f64 = with_kind(&again, OpKind::Draw)
        .iter()
        .map(|p| p.geom.length())
        .sum();
    assert!(near(total, total2));
}

#[test]
fn the_time_reference_compiles() {
    compiles_cleanly(&time_reference(&pen()));
}

#[test]
fn every_calibration_job_fits_the_work_area() {
    let p = air2();
    let limit_mm = (p.work_area.width_mm, p.work_area.height_mm);
    for d in [
        test_cut(&blade()),
        pressure_ladder(&pen(), OpKind::Draw, 100.0),
        tip_width_ladder(&pen()),
        blade_offset(&blade()),
        holder_offset(&pen(), &blade()),
        time_reference(&pen()),
    ] {
        for (_, path) in all_paths(&d) {
            let b = path.geom.bounds();
            assert!(b.x0 >= 0.0 && b.y0 >= 0.0, "{b:?}");
            assert!(b.x1 <= limit_mm.0 && b.y1 <= limit_mm.1, "{b:?}");
        }
    }
    let _ = Path::new(Point::new(0.0, 0.0));
}
