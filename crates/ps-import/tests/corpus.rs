//! The import corpus: files laid out the way the real tools write them, each with
//! expectations worked out from the drawing itself (a 50 x 30 mm rectangle is 50 x 30 mm, however
//! the file spells it). The SVG files copy the structure Inkscape, Illustrator, Figma and Affinity
//! produce (layer groups, `data-name`, unitless 72-dpi viewBoxes, clip paths, symbols and nested
//! `use`); the DXF files cover R12 lines, LWPOLYLINE bulges, nested blocks and a unitless header.
//! They are small hand-made stand-ins, not exports from those programs: real exports are added
//! here when contributors provide them under CC0.

use std::path::{Path, PathBuf};

use ps_geometry::{Rect, Seg};
use ps_import::{
    ImportedDrawing, ImportedPath, PdfOptions, SvgOptions, UnitGuess, import_dxf, import_pdf,
    import_svg, import_svg_auto,
};

fn corpus(kind: &str, name: &str) -> Vec<u8> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(kind)
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn svg(name: &str) -> ImportedDrawing {
    import_svg(&corpus("svg", name), &SvgOptions::default()).unwrap()
}

fn dxf(name: &str) -> ImportedDrawing {
    import_dxf(&corpus("dxf", name)).unwrap()
}

fn bounds(items: &[ImportedPath]) -> Rect {
    items
        .iter()
        .map(|i| i.geom.bounds())
        .reduce(|a, b| a.union(b))
        .expect("at least one path")
}

fn near(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

fn colours(d: &ImportedDrawing) -> Vec<[u8; 3]> {
    d.items.iter().filter_map(|i| i.style.stroke).collect()
}

// ---- SVG ----

#[test]
fn inkscape_layers_become_layers_with_their_labels_and_shapes_stay_in_millimetres() {
    let d = svg("inkscape-layers.svg");
    let names: Vec<&str> = d.layers.iter().map(|l| l.name.as_str()).collect();
    assert!(
        names.contains(&"Cut lines") && names.contains(&"Draw"),
        "{names:?}"
    );
    assert_eq!(d.items.len(), 3, "a rectangle, a circle and a curve");
    let cut: Vec<_> = d
        .items
        .iter()
        .filter(|i| i.layer.as_deref() == Some("Cut lines"))
        .collect();
    assert_eq!(cut.len(), 2);
    // The rectangle is 50 x 30 mm at (10, 10).
    let rect = bounds(&[cut[0].clone()]);
    assert!(
        near(rect.x0, 10.0, 0.01) && near(rect.width(), 50.0, 0.01),
        "{rect:?}"
    );
    assert!(
        near(rect.y0, 10.0, 0.01) && near(rect.height(), 30.0, 0.01),
        "{rect:?}"
    );
    assert_eq!(d.unit_guess, UnitGuess::Mm);
    assert!(colours(&d).contains(&[255, 0, 0]) && colours(&d).contains(&[0, 0, 255]));
}

#[test]
fn an_illustrator_file_with_a_unitless_view_box_offers_both_72_and_96_dpi_sizes() {
    // The importer that reads the generator comment, as the app and the CLI do.
    let d = import_svg_auto(
        &corpus("svg", "illustrator-72dpi.svg"),
        &SvgOptions::default(),
    )
    .unwrap();
    assert_eq!(d.items.len(), 2, "a rectangle and a line");
    let sizes: Vec<(f64, f64)> = d
        .size_candidates
        .iter()
        .map(|c| (c.width_mm, c.height_mm))
        .collect();
    // 288 x 216 units: 101.6 x 76.2 mm at 72 per inch, 76.2 x 57.15 mm at 96.
    assert!(
        sizes
            .iter()
            .any(|(w, h)| near(*w, 101.6, 0.01) && near(*h, 76.2, 0.01)),
        "{sizes:?}"
    );
    assert!(
        sizes
            .iter()
            .any(|(w, h)| near(*w, 76.2, 0.01) && near(*h, 57.15, 0.01)),
        "{sizes:?}"
    );
    // The file is read at 96 per inch until the dialog's choice says otherwise.
    assert_eq!(d.unit_guess, UnitGuess::Px96);
    let at_72 = import_svg_auto(
        &corpus("svg", "illustrator-72dpi.svg"),
        &SvgOptions {
            pixels_per_inch: 72.0,
            ..SvgOptions::default()
        },
    )
    .unwrap();
    assert_eq!(at_72.unit_guess, UnitGuess::Px72);
    let layer_names: Vec<&str> = d.layers.iter().map(|l| l.name.as_str()).collect();
    assert!(
        layer_names.contains(&"Cut") && layer_names.contains(&"Draw"),
        "{layer_names:?}"
    );
    // The rectangle is 144 x 72 units = 50.8 x 25.4 mm at 72 per inch.
    let rect = at_72
        .items
        .iter()
        .find(|i| i.style.stroke == Some([255, 0, 0]))
        .unwrap()
        .geom
        .bounds();
    assert!(
        near(rect.width(), 50.8, 0.05) && near(rect.height(), 25.4, 0.05),
        "{rect:?}"
    );
}

#[test]
fn figma_groups_import_every_shape_with_its_stroke() {
    let d = svg("figma-groups.svg");
    assert_eq!(d.items.len(), 3);
    let red = d
        .items
        .iter()
        .filter(|i| i.style.stroke == Some([255, 0, 0]))
        .count();
    let blue = d
        .items
        .iter()
        .filter(|i| i.style.stroke == Some([0, 0, 255]))
        .count();
    assert_eq!((red, blue), (2, 1));
    let triangle = d
        .items
        .iter()
        .find(|i| i.style.stroke == Some([0, 0, 255]))
        .unwrap();
    assert!(triangle.geom.closed, "the Z closes it");
    assert!(near(
        triangle.style.stroke_width_mm.unwrap(),
        2.0 * 25.4 / 96.0,
        0.01
    ));
}

#[test]
fn a_clip_path_trims_the_artwork_to_its_region() {
    let d = svg("affinity-clip.svg");
    let clipped = d
        .items
        .iter()
        .filter(|i| i.style.stroke == Some([255, 0, 0]))
        .map(|i| i.geom.bounds())
        .reduce(|a, b| a.union(b))
        .expect("the clipped circle survives in part");
    // The circle (centre 60, 60, r 50 unitless pixels at 96 per inch) is cut to the square
    // 20 .. 100 px, which is 5.29 .. 26.46 mm.
    let mm = 25.4 / 96.0;
    assert!(
        clipped.x0 >= 20.0 * mm - 0.01 && clipped.y0 >= 20.0 * mm - 0.01,
        "{clipped:?}"
    );
    assert!(
        clipped.x1 <= 100.0 * mm + 0.01 && clipped.y1 <= 100.0 * mm + 0.01,
        "{clipped:?}"
    );
    assert!(
        clipped.width() > 30.0 * mm,
        "most of the arc inside is kept: {clipped:?}"
    );
    // The unclipped blue square is whole.
    let square = d
        .items
        .iter()
        .find(|i| i.style.stroke == Some([0, 0, 255]))
        .unwrap()
        .geom
        .bounds();
    assert!(near(square.width(), 60.0 * 25.4 / 96.0, 0.01), "{square:?}");
}

#[test]
fn nested_use_elements_and_symbols_are_placed_with_every_transform() {
    let d = svg("nested-use.svg");
    assert_eq!(d.items.len(), 4, "two tiles in each of two uses");
    // Right-hand pair: translate(100, 10) scale(2): tiles are 20 mm wide (8 units scaled by
    // 20/10 symbol scale, then 2), the second at x = 30 * 2 further.
    let right: Vec<Rect> = d
        .items
        .iter()
        .map(|i| i.geom.bounds())
        .filter(|b| b.x0 > 90.0)
        .collect();
    assert_eq!(right.len(), 2);
    let widths: Vec<f64> = right.iter().map(Rect::width).collect();
    assert!(
        widths.iter().all(|w| near(*w, 8.0 * 2.0 * 2.0, 0.05)),
        "{widths:?}"
    );
    let gap = (right[1].x0 - right[0].x0).abs();
    assert!(near(gap, 60.0, 0.05), "{gap}");
}

#[test]
fn a_compressed_svg_imports_like_the_plain_one() {
    use std::io::Write as _;
    let plain = corpus("svg", "figma-groups.svg");
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&plain).unwrap();
    let packed = gz.finish().unwrap();
    let a = import_svg_auto(&packed, &SvgOptions::default()).unwrap();
    let b = svg("figma-groups.svg");
    assert_eq!(a.items.len(), b.items.len());
    assert_eq!(bounds(&a.items), bounds(&b.items));
}

// ---- DXF ----

#[test]
fn r12_lines_in_millimetres_make_a_100_by_50_rectangle_on_their_layer() {
    let d = dxf("r12-lines.dxf");
    assert_eq!(d.items.len(), 4);
    assert!(d.items.iter().all(|i| i.layer.as_deref() == Some("CUT")));
    let b = bounds(&d.items);
    assert!(
        near(b.width(), 100.0, 1e-6) && near(b.height(), 50.0, 1e-6),
        "{b:?}"
    );
    assert_eq!(d.unit_guess, UnitGuess::DxfInsUnits(4));
}

#[test]
fn polyline_bulges_become_true_arcs() {
    let d = dxf("lwpolyline-bulge.dxf");
    assert_eq!(d.items.len(), 1);
    let path = &d.items[0].geom;
    assert!(path.closed, "flag 1 closes the polyline");
    let arcs = path
        .segs
        .iter()
        .filter(|s| matches!(s, Seg::Arc { .. }))
        .count();
    assert_eq!(arcs, 2, "two bulged edges");
    // A bulge of 1 is a half circle on a 20 mm chord: radius 10, so the shape reaches 10 mm
    // beyond the chord on one side and the other.
    let b = path.bounds();
    assert!(near(b.height(), 10.0 + 10.0 + 10.0, 0.05), "{b:?}");
}

#[test]
fn nested_blocks_are_expanded_with_their_insert_offsets() {
    let d = dxf("nested-blocks.dxf");
    assert_eq!(d.items.len(), 4, "two outer inserts of two inner lines");
    let b = bounds(&d.items);
    assert!(near(b.x0, 20.0, 1e-6) && near(b.x1, 60.0, 1e-6), "{b:?}");
    // DXF's y axis points up and the document's down, so the lines at y = 20 and 25 are at -25 and -20.
    assert!(near(b.y0, -25.0, 1e-6) && near(b.y1, -20.0, 1e-6), "{b:?}");
}

#[test]
fn a_unitless_header_is_assumed_to_be_millimetres_and_says_so() {
    let d = dxf("unitless.dxf");
    assert_eq!(d.items.len(), 1);
    let b = d.items[0].geom.bounds();
    assert!(near(b.width(), 50.0, 0.01), "radius 25 is 50 wide: {b:?}");
    assert!(
        d.diagnostics
            .iter()
            .any(|x| x.key.as_str() == "diag-import-unitless"),
        "the dialog is told to ask"
    );
}

// ---- PDF ----

#[test]
fn a_two_page_pdf_imports_the_page_asked_for() {
    use pdf_writer::{Content, Finish, Pdf, Rect as PdfRect, Ref};
    let (catalog, tree, p1, p2, s1, s2) = (
        Ref::new(1),
        Ref::new(2),
        Ref::new(3),
        Ref::new(4),
        Ref::new(5),
        Ref::new(6),
    );
    let mut pdf = Pdf::new();
    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree).kids([p1, p2]).count(2);
    for (page, stream) in [(p1, s1), (p2, s2)] {
        let mut sheet = pdf.page(page);
        sheet.media_box(PdfRect::new(0.0, 0.0, 200.0, 100.0));
        sheet.parent(tree);
        sheet.contents(stream);
        sheet.finish();
    }
    let mut first = Content::new();
    first
        .set_stroke_rgb(1.0, 0.0, 0.0)
        .rect(10.0, 10.0, 72.0, 72.0)
        .stroke();
    let mut second = Content::new();
    second
        .set_stroke_rgb(0.0, 0.0, 1.0)
        .rect(10.0, 10.0, 36.0, 36.0)
        .stroke();
    pdf.stream(s1, &Content::finish(first));
    pdf.stream(s2, &Content::finish(second));
    let bytes = pdf.finish();
    let one = import_pdf(&bytes, &PdfOptions { page: 0 }).unwrap();
    let two = import_pdf(&bytes, &PdfOptions { page: 1 }).unwrap();
    // 72 points is one inch.
    assert!(near(bounds(&one.items).width(), 25.4, 0.01));
    assert!(near(bounds(&two.items).width(), 12.7, 0.01));
    assert_eq!(one.items[0].style.stroke, Some([255, 0, 0]));
    assert_eq!(two.items[0].style.stroke, Some([0, 0, 255]));
}
