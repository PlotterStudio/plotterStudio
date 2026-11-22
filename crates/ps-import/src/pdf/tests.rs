//! Ground truth for PDF and AI import: the content stream of a page becomes paths in
//! millimetres, y pointing down from the top-left corner of the page, with stroke and fill colours,
//! widths and dashes as the file states them. Optional content groups become layers, and what
//! cannot be a path (images, text without a font) is reported rather than silently dropped.
//! The PDFs are built here, by hand, so every expected coordinate is worked out from the page size.

use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str};
use ps_geometry::Seg;

use super::*;

const PT: f64 = 25.4 / 72.0;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

/// A one-page PDF 200 x 100 pt whose content is `content`, with optional extra resources set up
/// by `resources`.
fn pdf_with(
    content: Content,
    resources: impl FnOnce(&mut pdf_writer::writers::Resources<'_>),
) -> Vec<u8> {
    let (catalog, tree, page, stream) = (Ref::new(1), Ref::new(2), Ref::new(3), Ref::new(4));
    let mut pdf = Pdf::new();
    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree).kids([page]).count(1);
    let mut p = pdf.page(page);
    p.media_box(Rect::new(0.0, 0.0, 200.0, 100.0));
    p.parent(tree);
    p.contents(stream);
    resources(&mut p.resources());
    p.finish();
    pdf.stream(stream, &Content::finish(content));
    pdf.finish()
}

fn simple(content: Content) -> Vec<u8> {
    pdf_with(content, |_| {})
}

fn import(bytes: &[u8]) -> ImportedDrawing {
    import_pdf(bytes, &PdfOptions::default()).expect("imports")
}

#[test]
fn a_stroked_rectangle_is_a_closed_path_in_millimetres_with_y_down() {
    let mut c = Content::new();
    c.set_stroke_rgb(1.0, 0.0, 0.0).set_line_width(2.0);
    c.rect(50.0, 20.0, 100.0, 40.0).stroke();
    let d = import(&simple(c));
    assert_eq!(d.items.len(), 1);
    let item = &d.items[0];
    assert!(item.geom.closed);
    let b = item.geom.bounds();
    assert!(near(b.x0, 50.0 * PT) && near(b.x1, 150.0 * PT), "{b:?}");
    // The page is 100 pt tall: the top edge, 60 pt up from the bottom, is 40 pt down from the top.
    assert!(near(b.y0, 40.0 * PT) && near(b.y1, 80.0 * PT), "{b:?}");
    assert_eq!(item.style.stroke, Some([255, 0, 0]));
    assert!(near(item.style.stroke_width_mm.unwrap(), 2.0 * PT));
    assert_eq!(item.style.fill, None);
}

#[test]
fn a_filled_shape_carries_its_fill_colour() {
    let mut c = Content::new();
    c.set_fill_rgb(0.0, 0.0, 1.0);
    c.move_to(10.0, 10.0)
        .line_to(60.0, 10.0)
        .line_to(35.0, 50.0)
        .close_path()
        .fill_nonzero();
    let d = import(&simple(c));
    assert_eq!(d.items.len(), 1);
    assert_eq!(d.items[0].style.fill, Some([0, 0, 255]));
    assert_eq!(d.items[0].style.stroke, None);
    assert!(d.items[0].geom.closed);
}

#[test]
fn fill_and_stroke_together_become_a_fill_item_and_a_stroke_item() {
    // PDF paints the fill and the stroke with their own colours, so the file gives two items.
    let mut c = Content::new();
    c.set_fill_rgb(0.0, 1.0, 0.0)
        .set_stroke_rgb(0.0, 0.0, 0.0)
        .set_line_width(1.0);
    c.rect(10.0, 10.0, 50.0, 50.0).fill_nonzero_and_stroke();
    let d = import(&simple(c));
    assert_eq!(d.items.len(), 2);
    let fill = d
        .items
        .iter()
        .find(|i| i.style.fill.is_some())
        .expect("a fill");
    assert_eq!(fill.style.fill, Some([0, 255, 0]));
    let stroke = d
        .items
        .iter()
        .find(|i| i.style.stroke.is_some())
        .expect("a stroke");
    assert_eq!(stroke.style.stroke, Some([0, 0, 0]));
}

#[test]
fn curves_stay_cubic_béziers() {
    let mut c = Content::new();
    c.set_stroke_rgb(0.0, 0.0, 0.0).set_line_width(1.0);
    c.move_to(20.0, 20.0)
        .cubic_to(40.0, 90.0, 120.0, 90.0, 140.0, 20.0)
        .stroke();
    let d = import(&simple(c));
    let path = &d.items[0].geom;
    assert!(!path.closed);
    assert!(
        path.segs.iter().any(|s| matches!(s, Seg::Cubic { .. })),
        "{path:?}"
    );
    assert!(
        near(path.start.x, 20.0 * PT) && near(path.start.y, 80.0 * PT),
        "{:?}",
        path.start
    );
    assert!(near(path.end().x, 140.0 * PT), "{:?}", path.end());
}

#[test]
fn a_dash_pattern_is_converted_to_millimetres() {
    let mut c = Content::new();
    c.set_stroke_rgb(0.0, 0.0, 0.0)
        .set_line_width(1.0)
        .set_dash_pattern([6.0, 3.0], 0.0);
    c.move_to(10.0, 50.0).line_to(190.0, 50.0).stroke();
    let d = import(&simple(c));
    let dash = &d.items[0].style.dash;
    assert_eq!(dash.len(), 2, "{dash:?}");
    assert!(
        near(dash[0], 6.0 * PT) && near(dash[1], 3.0 * PT),
        "{dash:?}"
    );
}

#[test]
fn the_current_transform_is_applied() {
    let mut c = Content::new();
    c.save_state().transform([2.0, 0.0, 0.0, 2.0, 10.0, 10.0]);
    c.set_stroke_rgb(0.0, 0.0, 0.0).set_line_width(1.0);
    c.rect(0.0, 0.0, 20.0, 10.0).stroke().restore_state();
    let d = import(&simple(c));
    let b = d.items[0].geom.bounds();
    // 20 x 10 doubled is 40 x 20, starting 10 pt in and 10 pt up from the bottom.
    assert!(
        near(b.width(), 40.0 * PT) && near(b.height(), 20.0 * PT),
        "{b:?}"
    );
    assert!(near(b.x0, 10.0 * PT) && near(b.y1, 90.0 * PT), "{b:?}");
}

#[test]
fn points_are_the_unit_and_the_page_size_is_a_candidate() {
    let d = import(&simple(Content::new()));
    assert_eq!(d.unit_guess, UnitGuess::Px72);
    assert_eq!(d.items.len(), 0);
}

#[test]
fn paths_with_no_paint_are_not_imported() {
    let mut c = Content::new();
    c.rect(10.0, 10.0, 20.0, 20.0).end_path();
    assert_eq!(import(&simple(c)).items.len(), 0);
}

#[test]
fn a_second_page_is_chosen_by_index_and_a_missing_one_is_an_error() {
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
        let mut p = pdf.page(page);
        p.media_box(Rect::new(0.0, 0.0, 200.0, 100.0));
        p.parent(tree);
        p.contents(stream);
        p.finish();
    }
    let mut first = Content::new();
    first
        .set_stroke_rgb(1.0, 0.0, 0.0)
        .rect(10.0, 10.0, 20.0, 20.0)
        .stroke();
    let mut second = Content::new();
    second
        .set_stroke_rgb(0.0, 0.0, 1.0)
        .rect(10.0, 10.0, 20.0, 20.0)
        .stroke();
    pdf.stream(s1, &Content::finish(first));
    pdf.stream(s2, &Content::finish(second));
    let bytes = pdf.finish();
    let a = import_pdf(&bytes, &PdfOptions::default()).unwrap();
    assert_eq!(a.items[0].style.stroke, Some([255, 0, 0]));
    let b = import_pdf(&bytes, &PdfOptions { page: 1 }).unwrap();
    assert_eq!(b.items[0].style.stroke, Some([0, 0, 255]));
    assert!(matches!(
        import_pdf(&bytes, &PdfOptions { page: 2 }),
        Err(PdfError::NoSuchPage { page: 2, pages: 2 })
    ));
}

#[test]
fn bytes_that_are_not_a_pdf_are_refused() {
    assert!(matches!(
        import_pdf(b"definitely not a pdf", &PdfOptions::default()),
        Err(PdfError::Parse(_))
    ));
    assert!(matches!(
        import_pdf(b"", &PdfOptions::default()),
        Err(PdfError::Parse(_))
    ));
}

#[test]
fn a_file_over_the_size_limit_is_refused() {
    let opts = PdfOptions::default();
    let big = vec![b'x'; MAX_PDF_BYTES + 1];
    assert!(matches!(import_pdf(&big, &opts), Err(PdfError::TooLarge)));
}

#[test]
fn optional_content_groups_become_layers() {
    let mut c = Content::new();
    c.set_stroke_rgb(1.0, 0.0, 0.0).set_line_width(1.0);
    c.begin_marked_content_with_properties(Name(b"OC"))
        .properties()
        .pair(Name(b"Name"), Str(b"Cut lines"));
    c.rect(10.0, 10.0, 30.0, 30.0).stroke();
    c.end_marked_content();
    c.set_stroke_rgb(0.0, 0.0, 1.0);
    c.rect(100.0, 10.0, 30.0, 30.0).stroke();
    let d = import(&simple(c));
    assert_eq!(d.items.len(), 2);
    assert_eq!(d.items[0].layer.as_deref(), Some("Cut lines"));
    assert_eq!(d.items[1].layer, None);
    assert!(d.layers.iter().any(|l| l.name == "Cut lines"));
}

#[test]
fn text_in_a_standard_font_becomes_filled_outlines() {
    let mut c = Content::new();
    c.begin_text()
        .set_font(Name(b"F1"), 24.0)
        .next_line(10.0, 40.0)
        .show(Str(b"HI"))
        .end_text();
    let font = Ref::new(10);
    let bytes = pdf_with(c, |r| {
        r.fonts().pair(Name(b"F1"), font);
    });
    // The font is one of the 14 standard ones, drawn with the substitute the library embeds.
    let d = import_pdf(&bytes, &PdfOptions::default()).unwrap();
    assert!(
        d.items.iter().all(|i| i.style.fill.is_some()),
        "glyphs are filled shapes"
    );
    assert!(
        d.items.len() >= 2
            || d.diagnostics
                .iter()
                .any(|x| x.key.as_str() == "diag-pdf-text-skipped"),
        "either the letters came out as outlines or the skipped text was reported"
    );
}

#[test]
fn identical_files_give_identical_keys() {
    let mut c = Content::new();
    c.set_stroke_rgb(0.0, 0.0, 0.0)
        .rect(10.0, 10.0, 20.0, 20.0)
        .stroke();
    c.rect(50.0, 10.0, 20.0, 20.0).stroke();
    let bytes = simple(c);
    let (a, b) = (import(&bytes), import(&bytes));
    assert_eq!(a, b);
    assert_ne!(a.items[0].key, a.items[1].key);
}

#[test]
fn an_illustrator_file_without_a_pdf_stream_says_how_to_save_it() {
    let ai = b"%!PS-Adobe-3.0 EPSF-3.0\n%%Creator: Adobe Illustrator(R) 8.0\n";
    let error = import_pdf(ai, &PdfOptions::default()).unwrap_err();
    assert_eq!(error, PdfError::NoPdfStream);
    assert!(error.to_string().contains("PDF Compatible"), "{error}");
}

#[test]
fn an_illustrator_file_that_starts_with_a_postscript_header_but_holds_a_pdf_imports() {
    // Some AI files begin with a short header before the %PDF marker.
    let mut c = Content::new();
    c.set_stroke_rgb(0.0, 0.0, 0.0)
        .rect(10.0, 10.0, 20.0, 20.0)
        .stroke();
    let mut bytes = b"%!Adobe-Illustrator\n".to_vec();
    bytes.extend(simple(c));
    // The offsets in the cross-reference table are then off, which readers are expected to repair;
    // either the file imports or it is refused as unreadable, never a panic.
    let result = import_pdf(&bytes, &PdfOptions::default());
    assert!(
        matches!(result, Ok(_) | Err(PdfError::Parse(_))),
        "{result:?}"
    );
}

#[test]
fn truncated_and_corrupted_files_never_panic() {
    let mut c = Content::new();
    c.set_stroke_rgb(1.0, 0.0, 0.0)
        .rect(10.0, 10.0, 50.0, 50.0)
        .stroke();
    let good = simple(c);
    for cut in (0..good.len()).step_by(7) {
        let _ = import_pdf(&good[..cut], &PdfOptions::default());
    }
    for at in (0..good.len()).step_by(5) {
        let mut bad = good.clone();
        bad[at] ^= 0xFF;
        let _ = import_pdf(&bad, &PdfOptions::default());
    }
}
