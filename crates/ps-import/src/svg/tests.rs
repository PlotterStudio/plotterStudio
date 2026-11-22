//! Ground truth for the SVG importer. Each test is a small SVG whose geometry in
//! millimetres can be worked out by hand. The wrapper makes one user unit equal one millimetre
//! (`width="100mm"` with `viewBox="0 0 100 100"`) unless a test says otherwise.

use std::io::Write;

use ps_core::PathKey;
use ps_geometry::Seg;

use super::*;
use crate::units::{Generator, UnitGuess};

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"";

fn mm_svg(body: &str) -> String {
    format!("<svg {NS} width=\"100mm\" height=\"100mm\" viewBox=\"0 0 100 100\">{body}</svg>")
}

fn import(body: &str) -> ImportedDrawing {
    import_svg(mm_svg(body).as_bytes(), &SvgOptions::default()).unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

fn bounds(item: &ImportedPath) -> (f64, f64, f64, f64) {
    let b = item.geom.bounds();
    (b.x0, b.y0, b.x1, b.y1)
}

fn diag_keys(d: &ImportedDrawing) -> Vec<String> {
    d.diagnostics
        .iter()
        .map(|x| x.key.as_str().to_owned())
        .collect()
}

// ---- shapes and style ----

#[test]
fn a_rectangle_becomes_a_closed_path_with_its_stroke_style() {
    let d = import(
        "<rect x=\"10\" y=\"20\" width=\"30\" height=\"40\" fill=\"none\" stroke=\"#ff0000\" stroke-width=\"2\"/>",
    );
    assert_eq!(d.items.len(), 1);
    let item = &d.items[0];
    let (x0, y0, x1, y1) = bounds(item);
    assert!(
        near(x0, 10.0) && near(y0, 20.0) && near(x1, 40.0) && near(y1, 60.0),
        "{:?}",
        bounds(item)
    );
    assert!(item.geom.closed);
    assert_eq!(item.style.stroke, Some([255, 0, 0]));
    assert!(near(item.style.stroke_width_mm.unwrap(), 2.0));
    assert_eq!(item.style.fill, None);
    assert_eq!(item.style.dash, Vec::<f64>::new());
}

#[test]
fn fill_colour_and_opacity_are_kept() {
    let d = import("<rect width=\"10\" height=\"10\" fill=\"#00ff00\" fill-opacity=\"0.5\"/>");
    assert_eq!(d.items[0].style.fill, Some([0, 255, 0]));
    assert!(near(d.items[0].style.opacity, 0.5));
    assert_eq!(d.items[0].style.stroke, None);
}

#[test]
fn dash_arrays_are_converted_to_millimetres() {
    let d = import(
        "<path d=\"M0 0 L50 0\" fill=\"none\" stroke=\"#0000ff\" stroke-dasharray=\"4 2\"/>",
    );
    let dash = &d.items[0].style.dash;
    assert_eq!(dash.len(), 2);
    assert!(near(dash[0], 4.0) && near(dash[1], 2.0), "{dash:?}");
}

#[test]
fn transforms_are_applied_including_nested_ones() {
    let d = import(
        "<g transform=\"translate(10,5)\"><path d=\"M0 0 L10 0\" fill=\"none\" stroke=\"#000\"/></g>",
    );
    let g = &d.items[0].geom;
    assert!(near(g.start.x, 10.0) && near(g.start.y, 5.0));
    assert!(near(g.end().x, 20.0) && near(g.end().y, 5.0));
    let rotated = import(
        "<g transform=\"rotate(90)\"><path d=\"M10 0 L20 0\" fill=\"none\" stroke=\"#000\"/></g>",
    );
    let r = &rotated.items[0].geom;
    assert!(
        near(r.start.x, 0.0) && near(r.start.y, 10.0),
        "{:?}",
        r.start
    );
    assert!(near(r.end().x, 0.0) && near(r.end().y, 20.0));
}

#[test]
fn the_stroke_width_follows_the_transform_scale() {
    let d = import(
        "<g transform=\"scale(2)\"><path d=\"M0 0 L10 0\" fill=\"none\" stroke=\"#000\" stroke-width=\"1.5\"/></g>",
    );
    assert!(near(d.items[0].style.stroke_width_mm.unwrap(), 3.0));
}

#[test]
fn bezier_curves_stay_curves() {
    let d = import("<path d=\"M0 0 C 10 0 10 10 0 10\" fill=\"none\" stroke=\"#000\"/>");
    let Seg::Cubic { c1, c2, to } = d.items[0].geom.segs[0] else {
        panic!("{:?}", d.items[0].geom.segs)
    };
    assert!(near(c1.x, 10.0) && near(c1.y, 0.0));
    assert!(near(c2.x, 10.0) && near(c2.y, 10.0));
    assert!(near(to.x, 0.0) && near(to.y, 10.0));
}

#[test]
fn quadratic_curves_are_raised_to_cubics_exactly() {
    let d = import("<path d=\"M0 0 Q 5 5 10 0\" fill=\"none\" stroke=\"#000\"/>");
    let Seg::Cubic { c1, c2, to } = d.items[0].geom.segs[0] else {
        panic!("{:?}", d.items[0].geom.segs)
    };
    assert!(near(c1.x, 10.0 / 3.0) && near(c1.y, 10.0 / 3.0), "{c1:?}");
    assert!(near(c2.x, 20.0 / 3.0) && near(c2.y, 10.0 / 3.0), "{c2:?}");
    assert!(near(to.x, 10.0) && near(to.y, 0.0));
}

#[test]
fn a_compound_path_becomes_one_item_per_subpath() {
    let d = import(
        "<path d=\"M0 0 L10 0 L10 10 L0 10 Z M3 3 L7 3 L7 7 L3 7 Z\" fill=\"#000\" fill-rule=\"evenodd\"/>",
    );
    assert_eq!(d.items.len(), 2);
    assert!(
        d.items
            .iter()
            .all(|i| i.geom.closed && i.style.fill == Some([0, 0, 0]))
    );
}

#[test]
fn use_elements_are_resolved() {
    let d = import(
        "<defs><rect id=\"r\" width=\"5\" height=\"5\" fill=\"#000\"/></defs><use href=\"#r\" x=\"10\" y=\"0\"/>",
    );
    assert_eq!(d.items.len(), 1);
    let (x0, _, x1, _) = bounds(&d.items[0]);
    assert!(near(x0, 10.0) && near(x1, 15.0));
}

#[test]
fn hidden_elements_are_left_out() {
    let d = import(
        "<rect width=\"10\" height=\"10\" fill=\"#000\" style=\"display:none\"/><rect width=\"5\" height=\"5\" fill=\"#000\"/>",
    );
    assert_eq!(d.items.len(), 1);
}

#[test]
fn a_gradient_stroke_has_no_colour_and_says_so() {
    let d = import(
        "<defs><linearGradient id=\"g\"><stop offset=\"0\" stop-color=\"#f00\"/><stop offset=\"1\" stop-color=\"#00f\"/></linearGradient></defs><path d=\"M0 0 L10 10\" fill=\"none\" stroke=\"url(#g)\"/>",
    );
    assert_eq!(d.items[0].style.stroke, None);
    assert!(
        diag_keys(&d).contains(&"diag-import-paint-server".to_owned()),
        "{:?}",
        diag_keys(&d)
    );
}

#[test]
fn an_empty_drawing_imports_to_nothing() {
    let d = import("");
    assert_eq!(d.items.len(), 0);
    assert_eq!(d.diagnostics.len(), 0);
}

// ---- units ----

fn unitless(body: &str) -> String {
    format!("<svg {NS} width=\"96\" height=\"96\">{body}</svg>")
}

#[test]
fn unitless_files_are_read_at_96_pixels_per_inch() {
    let svg = unitless("<rect width=\"48\" height=\"48\" fill=\"#000\"/>");
    let d = import_svg(svg.as_bytes(), &SvgOptions::default()).unwrap();
    assert_eq!(d.unit_guess, UnitGuess::Px96);
    let (_, _, x1, y1) = bounds(&d.items[0]);
    assert!(near(x1, 12.7) && near(y1, 12.7), "{x1} {y1}");
}

#[test]
fn the_72_per_inch_choice_rescales_unitless_files() {
    let svg = unitless("<rect width=\"48\" height=\"48\" fill=\"#000\"/>");
    let opts = SvgOptions {
        pixels_per_inch: 72.0,
        ..SvgOptions::default()
    };
    let d = import_svg(svg.as_bytes(), &opts).unwrap();
    assert_eq!(d.unit_guess, UnitGuess::Px72);
    let (_, _, x1, _) = bounds(&d.items[0]);
    assert!(near(x1, 48.0 * 25.4 / 72.0), "{x1}");
}

#[test]
fn files_with_physical_units_ignore_the_pixel_choice() {
    let opts = SvgOptions {
        pixels_per_inch: 72.0,
        ..SvgOptions::default()
    };
    let svg = mm_svg("<rect x=\"10\" y=\"10\" width=\"30\" height=\"30\" fill=\"#000\"/>");
    let d = import_svg(svg.as_bytes(), &opts).unwrap();
    let (x0, _, x1, _) = bounds(&d.items[0]);
    assert!(near(x0, 10.0) && near(x1, 40.0), "{x0} {x1}");
}

#[test]
fn a_unitless_illustrator_file_offers_both_sizes() {
    let svg = "<?xml version=\"1.0\"?>\n<!-- Generator: Adobe Illustrator 27.0.0, SVG Export Plug-In -->\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"800\" height=\"600\"><rect width=\"10\" height=\"10\" fill=\"#000\"/></svg>";
    let d = import_svg(svg.as_bytes(), &SvgOptions::default()).unwrap();
    let units: Vec<UnitGuess> = d.size_candidates.iter().map(|c| c.unit).collect();
    assert_eq!(units, vec![UnitGuess::Px96, UnitGuess::Px72]);
    assert_eq!(detect_generator(svg), Generator::Illustrator);
}

#[test]
fn a_physical_size_has_one_candidate() {
    let d = import("");
    assert_eq!(d.size_candidates.len(), 1);
    assert_eq!(d.size_candidates[0].unit, UnitGuess::Mm);
    assert!(near(d.size_candidates[0].width_mm, 100.0));
}

// ---- layers ----

#[test]
fn inkscape_layers_name_their_paths_even_without_an_id() {
    let d = import(
        "<g inkscape:groupmode=\"layer\" inkscape:label=\"Cut\"><rect width=\"10\" height=\"10\" fill=\"none\" stroke=\"#000\"/></g>\
         <g inkscape:groupmode=\"layer\" inkscape:label=\"Draw\"><path d=\"M0 0 L5 5\" stroke=\"#000\"/></g>",
    );
    assert_eq!(d.items.len(), 2);
    assert_eq!(d.items[0].layer.as_deref(), Some("Cut"));
    assert_eq!(d.items[1].layer.as_deref(), Some("Draw"));
    let layers: Vec<(&str, bool, u32)> = d
        .layers
        .iter()
        .map(|l| (l.name.as_str(), l.visible, l.order))
        .collect();
    assert_eq!(layers, vec![("Cut", true, 0), ("Draw", true, 1)]);
    assert_eq!(d.items[0].style.layer.as_deref(), Some("Cut"));
}

#[test]
fn paths_in_nested_groups_belong_to_the_enclosing_layer() {
    let d = import(
        "<g inkscape:groupmode=\"layer\" inkscape:label=\"L\"><g transform=\"translate(1,1)\"><rect width=\"10\" height=\"10\" fill=\"#000\"/></g></g>",
    );
    assert_eq!(d.items[0].layer.as_deref(), Some("L"));
}

#[test]
fn illustrator_layers_use_data_name() {
    let d =
        import("<g id=\"Layer_1\" data-name=\"Score\"><path d=\"M0 0 L5 5\" stroke=\"#000\"/></g>");
    assert_eq!(d.items[0].layer.as_deref(), Some("Score"));
    assert_eq!(d.layers[0].name, "Score");
}

#[test]
fn affinity_layers_use_serif_ids() {
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:serif=\"http://www.serif.com/\" width=\"100mm\" height=\"100mm\" viewBox=\"0 0 100 100\"><g id=\"Layer-1\" serif:id=\"Outline\"><path d=\"M0 0 L5 5\" stroke=\"#000\"/></g></svg>";
    let d = import_svg(svg.as_bytes(), &SvgOptions::default()).unwrap();
    assert_eq!(d.items[0].layer.as_deref(), Some("Outline"));
}

#[test]
fn top_level_groups_become_layers_when_nothing_else_marks_them() {
    let d = import("<g id=\"Frame 1\"><path d=\"M0 0 L5 5\" stroke=\"#000\"/></g>");
    assert_eq!(d.items[0].layer.as_deref(), Some("Frame 1"));
}

#[test]
fn a_hidden_layer_is_listed_as_hidden_and_contributes_no_paths() {
    let d = import(
        "<g inkscape:groupmode=\"layer\" inkscape:label=\"Hidden\" style=\"display:none\"><rect width=\"10\" height=\"10\" fill=\"#000\"/></g>\
         <g inkscape:groupmode=\"layer\" inkscape:label=\"Shown\"><rect width=\"5\" height=\"5\" fill=\"#000\"/></g>",
    );
    assert_eq!(d.items.len(), 1);
    assert_eq!(d.items[0].layer.as_deref(), Some("Shown"));
    assert!(d.layers.iter().any(|l| l.name == "Hidden" && !l.visible));
}

// ---- keys ----

#[test]
fn a_path_with_an_id_is_keyed_by_it() {
    let d = import("<path id=\"shape1\" d=\"M0 0 L5 5\" stroke=\"#000\"/>");
    assert_eq!(d.items[0].key, PathKey::source("shape1"));
}

#[test]
fn subpaths_of_one_id_get_distinct_keys() {
    let d = import("<path id=\"p\" d=\"M0 0 L1 0 L1 1 Z M5 5 L6 5 L6 6 Z\" fill=\"#000\"/>");
    assert_eq!(d.items.len(), 2);
    assert_ne!(d.items[0].key, d.items[1].key);
}

#[test]
fn a_path_without_an_id_gets_a_stable_fingerprint() {
    let svg = mm_svg("<path d=\"M0 0 L5 5\" stroke=\"#000\"/>");
    let a = import_svg(svg.as_bytes(), &SvgOptions::default()).unwrap();
    let b = import_svg(svg.as_bytes(), &SvgOptions::default()).unwrap();
    assert!(matches!(a.items[0].key, PathKey::Fingerprint(_)));
    assert_eq!(a.items[0].key, b.items[0].key);
    let other = import("<path d=\"M0 0 L5 6\" stroke=\"#000\"/>");
    assert_ne!(a.items[0].key, other.items[0].key);
}

#[test]
fn fingerprints_distinguish_identical_paths_in_different_places() {
    let d =
        import("<path d=\"M0 0 L5 5\" stroke=\"#000\"/><path d=\"M0 0 L5 5\" stroke=\"#000\"/>");
    assert_ne!(
        d.items[0].key, d.items[1].key,
        "the element index is part of the fingerprint"
    );
}

// ---- clip paths ----

const CLIP: &str =
    "<defs><clipPath id=\"c\"><rect x=\"0\" y=\"0\" width=\"10\" height=\"10\"/></clipPath></defs>";

#[test]
fn a_filled_shape_is_intersected_with_its_clip() {
    let d = import(&format!(
        "{CLIP}<rect x=\"5\" y=\"5\" width=\"20\" height=\"20\" fill=\"#f00\" clip-path=\"url(#c)\"/>"
    ));
    assert_eq!(d.items.len(), 1);
    let (x0, y0, x1, y1) = bounds(&d.items[0]);
    assert!(
        near(x0, 5.0) && near(y0, 5.0) && near(x1, 10.0) && near(y1, 10.0),
        "{:?}",
        bounds(&d.items[0])
    );
    assert!(near(d.items[0].geom.signed_area().abs(), 25.0));
    assert_eq!(d.items[0].style.fill, Some([255, 0, 0]));
}

#[test]
fn a_stroke_is_clipped_as_a_polyline() {
    let d = import(&format!(
        "{CLIP}<path d=\"M0 5 L20 5\" fill=\"none\" stroke=\"#000\" clip-path=\"url(#c)\"/>"
    ));
    assert_eq!(d.items.len(), 1);
    let g = &d.items[0].geom;
    assert!(
        near(g.start.x, 0.0) && near(g.end().x, 10.0) && near(g.end().y, 5.0),
        "{g:?}"
    );
}

#[test]
fn a_shape_entirely_outside_its_clip_disappears() {
    let d = import(&format!(
        "{CLIP}<rect x=\"50\" y=\"50\" width=\"5\" height=\"5\" fill=\"#000\" clip-path=\"url(#c)\"/>"
    ));
    assert_eq!(d.items.len(), 0);
}

#[test]
fn a_clip_on_a_group_applies_to_everything_inside_it() {
    let d = import(&format!(
        "{CLIP}<g clip-path=\"url(#c)\"><rect x=\"5\" y=\"5\" width=\"20\" height=\"20\" fill=\"#000\"/><path d=\"M0 2 L30 2\" fill=\"none\" stroke=\"#000\"/></g>"
    ));
    assert_eq!(d.items.len(), 2);
    for item in &d.items {
        let (_, _, x1, y1) = bounds(item);
        assert!(x1 <= 10.0 + 1e-3 && y1 <= 10.0 + 1e-3);
    }
}

// ---- things that are ignored, with a note ----

#[test]
fn masks_are_ignored_with_an_info_diagnostic() {
    let d = import(
        "<defs><mask id=\"m\"><rect width=\"10\" height=\"10\" fill=\"#fff\"/></mask></defs><rect width=\"10\" height=\"10\" fill=\"#000\" mask=\"url(#m)\"/>",
    );
    assert_eq!(d.items.len(), 1, "the shape itself is kept");
    assert!(
        diag_keys(&d).contains(&"diag-import-mask-ignored".to_owned()),
        "{:?}",
        diag_keys(&d)
    );
}

#[test]
fn filters_are_ignored_with_an_info_diagnostic() {
    let d = import(
        "<defs><filter id=\"f\"><feGaussianBlur stdDeviation=\"1\"/></filter></defs><rect width=\"10\" height=\"10\" fill=\"#000\" filter=\"url(#f)\"/>",
    );
    assert_eq!(d.items.len(), 1);
    assert!(
        diag_keys(&d).contains(&"diag-import-filter-ignored".to_owned()),
        "{:?}",
        diag_keys(&d)
    );
}

#[test]
fn bitmaps_are_skipped_with_a_diagnostic() {
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
    let d = import(&format!(
        "<image width=\"10\" height=\"10\" href=\"data:image/png;base64,{png}\"/>"
    ));
    assert_eq!(d.items.len(), 0);
    assert!(
        diag_keys(&d).contains(&"diag-import-image-skipped".to_owned()),
        "{:?}",
        diag_keys(&d)
    );
}

// ---- containers and errors ----

fn gzip(text: &str) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(text.as_bytes()).unwrap();
    enc.finish().unwrap()
}

#[test]
fn svgz_imports_like_svg() {
    let svg = mm_svg(
        "<rect x=\"10\" y=\"20\" width=\"30\" height=\"40\" fill=\"none\" stroke=\"#ff0000\"/>",
    );
    let plain = import_svg(svg.as_bytes(), &SvgOptions::default()).unwrap();
    let packed = import_svg_auto(&gzip(&svg), &SvgOptions::default()).unwrap();
    assert_eq!(plain, packed);
    // Plain text goes through the same entry point.
    assert_eq!(
        import_svg_auto(svg.as_bytes(), &SvgOptions::default()).unwrap(),
        plain
    );
}

#[test]
fn a_decompression_bomb_is_refused() {
    let big = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\">".to_owned()
        + &" ".repeat(2_000_000)
        + "</svg>";
    let opts = SvgOptions {
        max_bytes: 100_000,
        ..SvgOptions::default()
    };
    assert_eq!(
        import_svg_auto(&gzip(&big), &opts).unwrap_err(),
        ImportError::TooLarge
    );
}

#[test]
fn broken_xml_is_an_error() {
    assert!(matches!(
        import_svg(b"<svg><rect></svg>", &SvgOptions::default()),
        Err(ImportError::Svg(_))
    ));
}

#[test]
fn a_file_that_is_not_an_svg_is_an_error() {
    assert!(matches!(
        import_svg(b"<html><body/></html>", &SvgOptions::default()),
        Err(ImportError::Svg(_))
    ));
}

#[test]
fn a_file_without_a_size_takes_its_size_from_its_content() {
    // usvg sizes such a document by what it contains, so the file still imports.
    let svg = format!(
        "<svg {NS}><rect x=\"10\" y=\"10\" width=\"30\" height=\"30\" fill=\"#000\"/></svg>"
    );
    let d = import_svg(svg.as_bytes(), &SvgOptions::default()).unwrap();
    assert_eq!(d.items.len(), 1);
    let (x0, _, x1, _) = bounds(&d.items[0]);
    assert!(
        near(x0, 10.0 * 25.4 / 96.0) && near(x1, 40.0 * 25.4 / 96.0),
        "{x0} {x1}"
    );
}

#[test]
fn non_utf8_input_is_an_error() {
    assert_eq!(
        import_svg(&[0xFF, 0xFE, 0x00], &SvgOptions::default()).unwrap_err(),
        ImportError::NotUtf8
    );
}

#[test]
fn importing_is_deterministic() {
    let svg = mm_svg(
        "<g inkscape:groupmode=\"layer\" inkscape:label=\"A\"><path d=\"M0 0 C 5 0 5 5 0 5\" stroke=\"#000\" fill=\"none\"/><circle cx=\"20\" cy=\"20\" r=\"5\" fill=\"#000\"/></g>",
    );
    let a = import_svg(svg.as_bytes(), &SvgOptions::default()).unwrap();
    let b = import_svg(svg.as_bytes(), &SvgOptions::default()).unwrap();
    assert_eq!(a, b);
}
