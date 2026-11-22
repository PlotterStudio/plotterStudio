//! Ground truth for the DXF importer. Entities map as follows: LINE and ARC directly;
//! CIRCLE as two arcs; LWPOLYLINE and POLYLINE with bulges as lines and arcs (bulge is the
//! tangent of a quarter of the sweep); ELLIPSE as cubic Béziers; SPLINE converted to cubic Béziers
//! by knot insertion (exact for non-rational cubics; others fitted within 0.002 mm); INSERT
//! recursively with block transforms and a depth limit of 32. Units come from `$INSUNITS`;
//! unitless files default to millimetres and ask. Layers become layer info; ACI and true colours
//! become stroke colours. DXF is y-up; the importer flips y so drawings keep their appearance.

use ps_core::PathKey;
use ps_geometry::{Path, Point, Seg, flatten};

use super::*;
use crate::units::UnitGuess;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn near_pt(p: Point, x: f64, y: f64) -> bool {
    near(p.x, x) && near(p.y, y)
}

/// Writes group code / value pairs, one per line, as DXF expects.
fn pairs(items: &[(i32, String)]) -> String {
    let mut out = String::new();
    for (code, value) in items {
        out.push_str(&code.to_string());
        out.push('\n');
        out.push_str(value);
        out.push('\n');
    }
    out
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "a test helper that takes literals of several types"
)]
fn g(code: i32, value: impl ToString) -> (i32, String) {
    (code, value.to_string())
}

fn document(units: Option<i32>, tables: &str, blocks: &str, entities: &str) -> String {
    let header = match units {
        Some(u) => pairs(&[
            g(0, "SECTION"),
            g(2, "HEADER"),
            g(9, "$INSUNITS"),
            g(70, u),
            g(0, "ENDSEC"),
        ]),
        None => String::new(),
    };
    let tables = if tables.is_empty() {
        String::new()
    } else {
        format!(
            "{}{tables}{}",
            pairs(&[g(0, "SECTION"), g(2, "TABLES")]),
            pairs(&[g(0, "ENDSEC")])
        )
    };
    let blocks = if blocks.is_empty() {
        String::new()
    } else {
        format!(
            "{}{blocks}{}",
            pairs(&[g(0, "SECTION"), g(2, "BLOCKS")]),
            pairs(&[g(0, "ENDSEC")])
        )
    };
    format!(
        "{header}{tables}{blocks}{}{entities}{}",
        pairs(&[g(0, "SECTION"), g(2, "ENTITIES")]),
        pairs(&[g(0, "ENDSEC"), g(0, "EOF")])
    )
}

fn import(units: Option<i32>, entities: &str) -> ImportedDrawing {
    import_dxf(document(units, "", "", entities).as_bytes()).unwrap()
}

fn line_entity(layer: &str, p1: (f64, f64), p2: (f64, f64)) -> String {
    pairs(&[
        g(0, "LINE"),
        g(8, layer),
        g(10, p1.0),
        g(20, p1.1),
        g(30, 0),
        g(11, p2.0),
        g(21, p2.1),
        g(31, 0),
    ])
}

fn keys(d: &ImportedDrawing) -> Vec<String> {
    d.diagnostics
        .iter()
        .map(|x| x.key.as_str().to_owned())
        .collect()
}

fn first(d: &ImportedDrawing) -> &ImportedPath {
    &d.items[0]
}

fn radius_error(path: &Path, radius: f64) -> f64 {
    flatten(path, 0.0005)
        .iter()
        .map(|p| (libm::hypot(p.x, p.y) - radius).abs())
        .fold(0.0, f64::max)
}

// ---- units and orientation ----

#[test]
fn a_line_is_imported_with_y_flipped() {
    let d = import(Some(4), &line_entity("0", (0.0, 0.0), (10.0, 5.0)));
    assert_eq!(d.items.len(), 1);
    assert!(near_pt(first(&d).geom.start, 0.0, 0.0));
    assert!(near_pt(first(&d).geom.end(), 10.0, -5.0));
    assert_eq!(d.unit_guess, UnitGuess::DxfInsUnits(4));
}

#[test]
fn inches_are_converted_to_millimetres() {
    let d = import(Some(1), &line_entity("0", (0.0, 0.0), (1.0, 0.0)));
    assert!(near_pt(first(&d).geom.end(), 25.4, 0.0));
}

#[test]
fn centimetres_and_metres_are_converted() {
    let cm = import(Some(5), &line_entity("0", (0.0, 0.0), (2.0, 0.0)));
    assert!(near(first(&cm).geom.end().x, 20.0));
    let m = import(Some(6), &line_entity("0", (0.0, 0.0), (0.5, 0.0)));
    assert!(near(first(&m).geom.end().x, 500.0));
}

#[test]
fn a_unitless_file_defaults_to_millimetres_and_asks() {
    let d = import(None, &line_entity("0", (0.0, 0.0), (10.0, 0.0)));
    assert!(near(first(&d).geom.end().x, 10.0));
    assert!(
        keys(&d).contains(&"diag-import-unitless".to_owned()),
        "{:?}",
        keys(&d)
    );
    let units: Vec<UnitGuess> = d.size_candidates.iter().map(|c| c.unit).collect();
    assert_eq!(units, vec![UnitGuess::Mm, UnitGuess::In]);
}

#[test]
fn a_file_with_units_has_a_single_size_reading_and_no_question() {
    let d = import(Some(4), &line_entity("0", (0.0, 0.0), (10.0, 5.0)));
    assert!(!keys(&d).contains(&"diag-import-unitless".to_owned()));
    assert_eq!(d.size_candidates.len(), 1);
    assert!(near(d.size_candidates[0].width_mm, 10.0) && near(d.size_candidates[0].height_mm, 5.0));
}

// ---- entities ----

#[test]
fn an_arc_runs_counter_clockwise_in_the_source_and_is_mirrored_by_the_flip() {
    let e = pairs(&[
        g(0, "ARC"),
        g(8, "0"),
        g(10, 0),
        g(20, 0),
        g(30, 0),
        g(40, 10),
        g(50, 0),
        g(51, 90),
    ]);
    let d = import(Some(4), &e);
    let g0 = &first(&d).geom;
    assert!(near_pt(g0.start, 10.0, 0.0));
    let Seg::Arc { to, center, ccw } = g0.segs[0] else {
        panic!("{:?}", g0.segs)
    };
    assert!(
        near_pt(to, 0.0, -10.0) && near_pt(center, 0.0, 0.0),
        "{to:?} {center:?}"
    );
    assert!(!ccw, "the y flip turns counter-clockwise into clockwise");
}

#[test]
fn an_arc_that_crosses_zero_degrees_sweeps_the_short_way_round() {
    // 270 to 90 counter-clockwise is a half turn through angle 0.
    let e = pairs(&[
        g(0, "ARC"),
        g(8, "0"),
        g(10, 0),
        g(20, 0),
        g(30, 0),
        g(40, 5),
        g(50, 270),
        g(51, 90),
    ]);
    let d = import(Some(4), &e);
    let g0 = &first(&d).geom;
    assert!(near_pt(g0.start, 0.0, 5.0), "{:?}", g0.start);
    assert!(near_pt(g0.end(), 0.0, -5.0));
    assert!(
        (g0.length() - std::f64::consts::PI * 5.0).abs() < 1e-3,
        "{}",
        g0.length()
    );
}

#[test]
fn a_circle_is_two_arcs_and_closed() {
    let e = pairs(&[
        g(0, "CIRCLE"),
        g(8, "0"),
        g(10, 20),
        g(20, 30),
        g(30, 0),
        g(40, 5),
    ]);
    let d = import(Some(4), &e);
    let p = &first(&d).geom;
    assert!(p.closed);
    assert_eq!(p.segs.len(), 2);
    assert!(p.segs.iter().all(|s| matches!(s, Seg::Arc { .. })));
    let b = p.bounds();
    assert!(
        near(b.x0, 15.0) && near(b.x1, 25.0) && near(b.y0, -35.0) && near(b.y1, -25.0),
        "{b:?}"
    );
}

fn lw(layer: &str, flags: i32, vertices: &[(f64, f64, f64)]) -> String {
    let mut items = vec![
        g(0, "LWPOLYLINE"),
        g(8, layer),
        g(90, vertices.len()),
        g(70, flags),
    ];
    for (x, y, bulge) in vertices {
        items.push(g(10, x));
        items.push(g(20, y));
        if *bulge != 0.0 {
            items.push(g(42, bulge));
        }
    }
    pairs(&items)
}

#[test]
fn a_closed_lwpolyline_is_a_closed_path_of_lines() {
    let e = lw(
        "0",
        1,
        &[
            (0.0, 0.0, 0.0),
            (10.0, 0.0, 0.0),
            (10.0, 10.0, 0.0),
            (0.0, 10.0, 0.0),
        ],
    );
    let d = import(Some(4), &e);
    let p = &first(&d).geom;
    assert!(p.closed);
    assert!(p.segs.iter().all(|s| matches!(s, Seg::Line { .. })));
    assert!(near(p.bounds().height(), 10.0));
}

#[test]
fn an_open_lwpolyline_stays_open() {
    let d = import(
        Some(4),
        &lw(
            "0",
            0,
            &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 5.0, 0.0)],
        ),
    );
    assert!(!first(&d).geom.closed);
    assert_eq!(first(&d).geom.segs.len(), 2);
}

#[test]
fn a_bulge_of_one_is_a_semicircle() {
    let d = import(Some(4), &lw("0", 0, &[(0.0, 0.0, 1.0), (10.0, 0.0, 0.0)]));
    let g0 = &first(&d).geom;
    let Seg::Arc { to, center, ccw } = g0.segs[0] else {
        panic!("{:?}", g0.segs)
    };
    assert!(
        near_pt(center, 5.0, 0.0) && near_pt(to, 10.0, 0.0),
        "{center:?} {to:?}"
    );
    assert!(
        !ccw,
        "counter-clockwise in the source becomes clockwise after the flip"
    );
    assert!((g0.length() - std::f64::consts::PI * 5.0).abs() < 1e-3);
}

#[test]
fn a_bulge_of_tan_eighth_pi_is_a_quarter_circle() {
    let b = libm::tan(std::f64::consts::PI / 8.0);
    let d = import(Some(4), &lw("0", 0, &[(0.0, 0.0, b), (10.0, 0.0, 0.0)]));
    let Seg::Arc { center, .. } = first(&d).geom.segs[0] else {
        panic!()
    };
    // Source centre (5, 5), flipped to (5, -5).
    assert!(near_pt(center, 5.0, -5.0), "{center:?}");
    let radius = 10.0 / (2.0 * libm::sin(std::f64::consts::PI / 4.0));
    assert!((first(&d).geom.length() - radius * std::f64::consts::FRAC_PI_2).abs() < 1e-3);
}

#[test]
fn a_negative_bulge_curves_the_other_way() {
    let b = -libm::tan(std::f64::consts::PI / 8.0);
    let d = import(Some(4), &lw("0", 0, &[(0.0, 0.0, b), (10.0, 0.0, 0.0)]));
    let Seg::Arc { center, ccw, .. } = first(&d).geom.segs[0] else {
        panic!()
    };
    assert!(
        near_pt(center, 5.0, 5.0),
        "source centre (5, -5) flips to (5, 5): {center:?}"
    );
    assert!(ccw);
}

#[test]
fn a_bulge_on_the_closing_segment_is_honoured() {
    let e = lw(
        "0",
        1,
        &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 1.0)],
    );
    let d = import(Some(4), &e);
    let arcs = first(&d)
        .geom
        .segs
        .iter()
        .filter(|s| matches!(s, Seg::Arc { .. }))
        .count();
    assert_eq!(arcs, 1);
}

#[test]
fn an_old_style_polyline_is_a_polyline() {
    let e = pairs(&[
        g(0, "POLYLINE"),
        g(8, "0"),
        g(66, 1),
        g(70, 0),
        g(0, "VERTEX"),
        g(8, "0"),
        g(10, 0),
        g(20, 0),
        g(30, 0),
        g(0, "VERTEX"),
        g(8, "0"),
        g(10, 10),
        g(20, 0),
        g(30, 0),
        g(0, "VERTEX"),
        g(8, "0"),
        g(10, 10),
        g(20, 10),
        g(30, 0),
        g(0, "SEQEND"),
        g(8, "0"),
    ]);
    let d = import(Some(4), &e);
    assert_eq!(d.items.len(), 1);
    assert_eq!(first(&d).geom.segs.len(), 2);
    assert!(near_pt(first(&d).geom.end(), 10.0, -10.0));
}

fn ellipse(ratio: f64, start: f64, end: f64) -> String {
    pairs(&[
        g(0, "ELLIPSE"),
        g(8, "0"),
        g(10, 0),
        g(20, 0),
        g(30, 0),
        g(11, 10),
        g(21, 0),
        g(31, 0),
        g(40, ratio),
        g(41, start),
        g(42, end),
    ])
}

#[test]
fn a_full_ellipse_is_closed_cubic_beziers_with_the_right_area() {
    let d = import(Some(4), &ellipse(0.5, 0.0, std::f64::consts::TAU));
    let p = &first(&d).geom;
    assert!(p.closed);
    assert!(
        p.segs.iter().all(|s| matches!(s, Seg::Cubic { .. })),
        "{:?}",
        p.segs
    );
    let b = p.bounds();
    assert!(
        (b.width() - 20.0).abs() < 1e-3 && (b.height() - 10.0).abs() < 1e-3,
        "{b:?}"
    );
    let area = p.signed_area().abs();
    assert!((area - std::f64::consts::PI * 50.0).abs() < 0.2, "{area}");
}

#[test]
fn a_partial_ellipse_is_open_and_ends_where_the_parameter_says() {
    let d = import(Some(4), &ellipse(0.5, 0.0, std::f64::consts::FRAC_PI_2));
    let p = &first(&d).geom;
    assert!(!p.closed);
    assert!(near_pt(p.start, 10.0, 0.0));
    assert!(near_pt(p.end(), 0.0, -5.0), "{:?}", p.end());
}

fn spline(degree: i32, knots: &[f64], control: &[(f64, f64)], weights: &[f64]) -> String {
    let mut items = vec![
        g(0, "SPLINE"),
        g(8, "0"),
        g(70, if weights.is_empty() { 8 } else { 12 }),
        g(71, degree),
        g(72, knots.len()),
        g(73, control.len()),
        g(74, 0),
    ];
    for k in knots {
        items.push(g(40, k));
    }
    for w in weights {
        items.push(g(41, w));
    }
    for (x, y) in control {
        items.push(g(10, x));
        items.push(g(20, y));
        items.push(g(30, 0));
    }
    pairs(&items)
}

#[test]
fn a_single_span_cubic_spline_is_exactly_its_bezier() {
    let e = spline(
        3,
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        &[(0.0, 0.0), (1.0, 2.0), (3.0, 2.0), (4.0, 0.0)],
        &[],
    );
    let d = import(Some(4), &e);
    let p = &first(&d).geom;
    assert_eq!(p.segs.len(), 1);
    let Seg::Cubic { c1, c2, to } = p.segs[0] else {
        panic!("{:?}", p.segs)
    };
    assert!(
        near_pt(p.start, 0.0, 0.0)
            && near_pt(c1, 1.0, -2.0)
            && near_pt(c2, 3.0, -2.0)
            && near_pt(to, 4.0, 0.0)
    );
}

#[test]
fn a_multi_span_cubic_spline_is_split_by_knot_insertion() {
    // Reference values from an independent de Boor evaluation: the curve passes through
    // (3.0, 2.5) at u = 0.5 and (1.5, 2.0) at u = 0.25.
    let e = spline(
        3,
        &[0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
        &[(0.0, 0.0), (1.0, 2.0), (3.0, 3.0), (5.0, 2.0), (6.0, 0.0)],
        &[],
    );
    let d = import(Some(4), &e);
    let p = &first(&d).geom;
    assert_eq!(p.segs.len(), 2);
    let Seg::Cubic { c1, c2, to } = p.segs[0] else {
        panic!("{:?}", p.segs)
    };
    assert!(near_pt(to, 3.0, -2.5), "join {to:?}");
    // The first piece's midpoint is the curve at u = 0.25.
    let mid = Point::new(
        (p.start.x + 3.0 * c1.x + 3.0 * c2.x + to.x) / 8.0,
        (p.start.y + 3.0 * c1.y + 3.0 * c2.y + to.y) / 8.0,
    );
    assert!(near_pt(mid, 1.5, -2.0), "{mid:?}");
    assert!(near_pt(p.end(), 6.0, 0.0));
}

#[test]
fn a_rational_spline_is_fitted_within_the_tolerance() {
    // A quarter of the unit circle as a rational quadratic: weights 1, sqrt(2)/2, 1.
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let e = spline(
        2,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[(1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        &[1.0, w, 1.0],
    );
    let d = import(Some(4), &e);
    let p = &first(&d).geom;
    assert!(near_pt(p.start, 1.0, 0.0) && near_pt(p.end(), 0.0, -1.0));
    let err = radius_error(p, 1.0);
    assert!(err < 0.002 + 1e-4, "{err}");
}

#[test]
fn a_quadratic_non_rational_spline_is_exact_as_a_cubic() {
    // Quadratic Bézier (0,0), (5,10), (10,0): its midpoint is (5, 5).
    let e = spline(
        2,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[(0.0, 0.0), (5.0, 10.0), (10.0, 0.0)],
        &[],
    );
    let d = import(Some(4), &e);
    let p = &first(&d).geom;
    assert_eq!(p.segs.len(), 1);
    let Seg::Cubic { c1, c2, to } = p.segs[0] else {
        panic!()
    };
    let mid = Point::new(
        (p.start.x + 3.0 * c1.x + 3.0 * c2.x + to.x) / 8.0,
        (p.start.y + 3.0 * c1.y + 3.0 * c2.y + to.y) / 8.0,
    );
    assert!(near_pt(mid, 5.0, -5.0), "{mid:?}");
}

// ---- blocks ----

fn block(name: &str, entities: &str) -> String {
    format!(
        "{}{entities}{}",
        pairs(&[
            g(0, "BLOCK"),
            g(8, "0"),
            g(2, name),
            g(70, 0),
            g(10, 0),
            g(20, 0),
            g(30, 0),
            g(3, name),
            g(1, "")
        ]),
        pairs(&[g(0, "ENDBLK"), g(8, "0")])
    )
}

fn insert(name: &str, at: (f64, f64), scale: (f64, f64), rotation: f64) -> String {
    pairs(&[
        g(0, "INSERT"),
        g(8, "0"),
        g(2, name),
        g(10, at.0),
        g(20, at.1),
        g(30, 0),
        g(41, scale.0),
        g(42, scale.1),
        g(43, 1),
        g(50, rotation),
    ])
}

#[test]
fn an_insert_applies_the_block_transform() {
    // The block's line (0,0)-(1,0) scaled by 2 and rotated 90 degrees becomes (0,0)-(0,2); placed
    // at (10, 20) it runs from (10, 20) to (10, 22); the y flip makes that (10,-20) to (10,-22).
    let blocks = block("B", &line_entity("0", (0.0, 0.0), (1.0, 0.0)));
    let doc = document(
        Some(4),
        "",
        &blocks,
        &insert("B", (10.0, 20.0), (2.0, 2.0), 90.0),
    );
    let d = import_dxf(doc.as_bytes()).unwrap();
    assert_eq!(d.items.len(), 1);
    assert!(
        near_pt(first(&d).geom.start, 10.0, -20.0),
        "{:?}",
        first(&d).geom.start
    );
    assert!(
        near_pt(first(&d).geom.end(), 10.0, -22.0),
        "{:?}",
        first(&d).geom.end()
    );
}

#[test]
fn a_block_base_point_is_subtracted() {
    let blocks = format!(
        "{}{}{}",
        pairs(&[
            g(0, "BLOCK"),
            g(8, "0"),
            g(2, "B"),
            g(70, 0),
            g(10, 1),
            g(20, 1),
            g(30, 0),
            g(3, "B"),
            g(1, "")
        ]),
        line_entity("0", (1.0, 1.0), (2.0, 1.0)),
        pairs(&[g(0, "ENDBLK"), g(8, "0")])
    );
    let doc = document(
        Some(4),
        "",
        &blocks,
        &insert("B", (10.0, 10.0), (1.0, 1.0), 0.0),
    );
    let d = import_dxf(doc.as_bytes()).unwrap();
    assert!(
        near_pt(first(&d).geom.start, 10.0, -10.0),
        "{:?}",
        first(&d).geom.start
    );
}

#[test]
fn an_arrayed_insert_repeats_the_block() {
    let blocks = block("B", &line_entity("0", (0.0, 0.0), (1.0, 0.0)));
    let mut e = insert("B", (0.0, 0.0), (1.0, 1.0), 0.0);
    e.push_str(&pairs(&[g(70, 3), g(71, 2), g(44, 5), g(45, 8)]));
    let d = import_dxf(document(Some(4), "", &blocks, &e).as_bytes()).unwrap();
    assert_eq!(d.items.len(), 6);
    let mut starts: Vec<(f64, f64)> = d
        .items
        .iter()
        .map(|i| (i.geom.start.x, i.geom.start.y))
        .collect();
    starts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let expected = [
        (0.0, -8.0),
        (0.0, 0.0),
        (5.0, -8.0),
        (5.0, 0.0),
        (10.0, -8.0),
        (10.0, 0.0),
    ];
    assert_eq!(starts.len(), expected.len());
    for (got, want) in starts.iter().zip(expected) {
        assert!(near(got.0, want.0) && near(got.1, want.1), "{starts:?}");
    }
}

#[test]
fn nested_inserts_compose() {
    let inner = block("INNER", &line_entity("0", (0.0, 0.0), (1.0, 0.0)));
    let outer = block("OUTER", &insert("INNER", (5.0, 0.0), (1.0, 1.0), 0.0));
    let doc = document(
        Some(4),
        "",
        &format!("{inner}{outer}"),
        &insert("OUTER", (0.0, 10.0), (1.0, 1.0), 0.0),
    );
    let d = import_dxf(doc.as_bytes()).unwrap();
    assert!(
        near_pt(first(&d).geom.start, 5.0, -10.0),
        "{:?}",
        first(&d).geom.start
    );
}

#[test]
fn a_block_that_inserts_itself_stops_at_the_depth_limit() {
    let looping = block(
        "LOOP",
        &format!(
            "{}{}",
            line_entity("0", (0.0, 0.0), (1.0, 0.0)),
            insert("LOOP", (1.0, 0.0), (1.0, 1.0), 0.0)
        ),
    );
    let doc = document(
        Some(4),
        "",
        &looping,
        &insert("LOOP", (0.0, 0.0), (1.0, 1.0), 0.0),
    );
    let d = import_dxf(doc.as_bytes()).unwrap();
    assert!(d.items.len() <= 33, "{}", d.items.len());
    assert!(
        keys(&d).contains(&"diag-import-insert-depth".to_owned()),
        "{:?}",
        keys(&d)
    );
}

#[test]
fn an_insert_of_an_unknown_block_is_reported_not_fatal() {
    let d = import(Some(4), &insert("MISSING", (0.0, 0.0), (1.0, 1.0), 0.0));
    assert_eq!(d.items.len(), 0);
    assert!(
        keys(&d).contains(&"diag-import-dxf-missing-block".to_owned()),
        "{:?}",
        keys(&d)
    );
}

// ---- layers and colours ----

fn layer_table(layers: &[(&str, i32)]) -> String {
    let mut items = vec![g(0, "TABLE"), g(2, "LAYER"), g(70, layers.len())];
    for (name, colour) in layers {
        items.extend([
            g(0, "LAYER"),
            g(2, name),
            g(70, 0),
            g(62, colour),
            g(6, "CONTINUOUS"),
        ]);
    }
    items.push(g(0, "ENDTAB"));
    pairs(&items)
}

#[test]
fn layers_become_layer_info_and_entities_take_the_layer_colour() {
    let tables = layer_table(&[("CUT", 1), ("DRAW", 5)]);
    let entities = format!(
        "{}{}",
        line_entity("CUT", (0.0, 0.0), (1.0, 0.0)),
        line_entity("DRAW", (0.0, 1.0), (1.0, 1.0))
    );
    let d = import_dxf(document(Some(4), &tables, "", &entities).as_bytes()).unwrap();
    let names: Vec<(&str, bool)> = d
        .layers
        .iter()
        .map(|l| (l.name.as_str(), l.visible))
        .collect();
    assert!(
        names.contains(&("CUT", true)) && names.contains(&("DRAW", true)),
        "{names:?}"
    );
    assert_eq!(d.items[0].layer.as_deref(), Some("CUT"));
    assert_eq!(d.items[0].style.stroke, Some([255, 0, 0]));
    assert_eq!(d.items[1].style.stroke, Some([0, 0, 255]));
    assert_eq!(d.items[0].style.layer.as_deref(), Some("CUT"));
}

#[test]
fn an_entity_colour_overrides_the_layer_colour() {
    let tables = layer_table(&[("CUT", 1)]);
    let mut e = line_entity("CUT", (0.0, 0.0), (1.0, 0.0));
    e.push_str(&pairs(&[g(62, 3)]));
    let d = import_dxf(document(Some(4), &tables, "", &e).as_bytes()).unwrap();
    assert_eq!(d.items[0].style.stroke, Some([0, 255, 0]));
}

#[test]
fn true_colours_win_over_indexed_ones() {
    let mut e = line_entity("0", (0.0, 0.0), (1.0, 0.0));
    e.push_str(&pairs(&[g(62, 1), g(420, 0x0011_2233)]));
    let d = import_dxf(document(Some(4), "", "", &e).as_bytes()).unwrap();
    assert_eq!(d.items[0].style.stroke, Some([0x11, 0x22, 0x33]));
}

#[test]
fn entities_on_a_layer_that_is_off_are_left_out() {
    // A negative layer colour means the layer is turned off.
    let tables = layer_table(&[("OFF", -1), ("ON", 1)]);
    let entities = format!(
        "{}{}",
        line_entity("OFF", (0.0, 0.0), (1.0, 0.0)),
        line_entity("ON", (0.0, 1.0), (1.0, 1.0))
    );
    let d = import_dxf(document(Some(4), &tables, "", &entities).as_bytes()).unwrap();
    assert_eq!(d.items.len(), 1);
    assert_eq!(d.items[0].layer.as_deref(), Some("ON"));
    assert!(d.layers.iter().any(|l| l.name == "OFF" && !l.visible));
}

#[test]
fn the_index_colour_table_matches_the_standard() {
    assert_eq!(aci_rgb(1), [255, 0, 0]);
    assert_eq!(aci_rgb(2), [255, 255, 0]);
    assert_eq!(aci_rgb(3), [0, 255, 0]);
    assert_eq!(aci_rgb(4), [0, 255, 255]);
    assert_eq!(aci_rgb(5), [0, 0, 255]);
    assert_eq!(aci_rgb(6), [255, 0, 255]);
    assert_eq!(
        aci_rgb(7),
        [0, 0, 0],
        "white-on-black reads as black ink on paper"
    );
    assert_eq!(aci_rgb(8), [128, 128, 128]);
    assert_eq!(aci_rgb(9), [192, 192, 192]);
    assert_eq!(aci_rgb(10), [255, 0, 0]);
    assert_eq!(aci_rgb(11), [255, 127, 127]);
    assert_eq!(aci_rgb(12), [204, 0, 0]);
    assert_eq!(aci_rgb(14), [153, 0, 0]);
    assert_eq!(aci_rgb(19), [76, 38, 38]);
    assert_eq!(aci_rgb(30), [255, 127, 0]);
    assert_eq!(aci_rgb(40), [255, 191, 0]);
    assert_eq!(aci_rgb(50), [255, 255, 0]);
    assert_eq!(aci_rgb(70), [127, 255, 0]);
    assert_eq!(aci_rgb(250), [51, 51, 51]);
    assert_eq!(aci_rgb(255), [255, 255, 255]);
}

// ---- keys, diagnostics and errors ----

#[test]
fn entity_handles_become_source_keys() {
    let mut e = pairs(&[g(0, "LINE"), g(5, "1A"), g(8, "0")]);
    e.push_str(&pairs(&[
        g(10, 0),
        g(20, 0),
        g(30, 0),
        g(11, 1),
        g(21, 0),
        g(31, 0),
    ]));
    let d = import(Some(4), &e);
    assert_eq!(d.items[0].key, PathKey::source("1A"));
}

#[test]
fn entities_without_handles_get_stable_fingerprints() {
    let doc = document(Some(4), "", "", &line_entity("0", (0.0, 0.0), (1.0, 0.0)));
    let a = import_dxf(doc.as_bytes()).unwrap();
    let b = import_dxf(doc.as_bytes()).unwrap();
    assert!(matches!(a.items[0].key, PathKey::Fingerprint(_)));
    assert_eq!(a.items[0].key, b.items[0].key);
}

#[test]
fn unsupported_entities_are_counted_in_a_diagnostic() {
    let e = pairs(&[
        g(0, "TEXT"),
        g(8, "0"),
        g(10, 0),
        g(20, 0),
        g(30, 0),
        g(40, 2),
        g(1, "hello"),
    ]);
    let d = import(Some(4), &e);
    assert_eq!(d.items.len(), 0);
    assert!(
        keys(&d).contains(&"diag-import-dxf-unsupported".to_owned()),
        "{:?}",
        keys(&d)
    );
}

#[test]
fn a_file_that_is_not_a_dxf_is_an_error() {
    assert!(matches!(
        import_dxf(b"this is not a dxf file at all"),
        Err(DxfError::Parse(_))
    ));
}

#[test]
fn importing_is_deterministic() {
    let tables = layer_table(&[("A", 1)]);
    let e = format!(
        "{}{}",
        lw(
            "A",
            1,
            &[(0.0, 0.0, 0.5), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0)]
        ),
        ellipse(0.4, 0.0, std::f64::consts::TAU)
    );
    let doc = document(Some(4), &tables, "", &e);
    assert_eq!(
        import_dxf(doc.as_bytes()).unwrap(),
        import_dxf(doc.as_bytes()).unwrap()
    );
}
