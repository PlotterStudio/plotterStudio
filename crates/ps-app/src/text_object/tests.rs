//! Ground truth for turning the text dialog's settings into paths: outline text gives
//! closed contours, single-stroke text gives open ones, both normalised so the top-left corner of
//! the text is the origin and ready to be placed like any other object.

use ps_geometry::Path;

use super::*;
use crate::fonts::FontLibrary;

fn params(text: &str) -> TextParams {
    TextParams {
        text: text.to_owned(),
        ..TextParams::default()
    }
}

fn bounds(paths: &[Path]) -> ps_geometry::Rect {
    let mut it = paths.iter().map(Path::bounds);
    let first = it.next().unwrap();
    it.fold(first, |a, b| a.union(b))
}

#[test]
fn empty_or_blank_text_is_refused() {
    let fonts = FontLibrary::bundled_only();
    assert!(text_paths(&params(""), &fonts).is_err());
    assert!(text_paths(&params("  \n "), &fonts).is_err());
}

#[test]
fn outline_text_is_closed_contours_with_the_top_left_at_the_origin() {
    let fonts = FontLibrary::bundled_only();
    let paths = text_paths(&params("o"), &fonts).unwrap();
    assert_eq!(paths.len(), 2);
    assert!(paths.iter().all(|p| p.closed));
    let b = bounds(&paths);
    assert!(b.x0.abs() < 1e-9 && b.y0.abs() < 1e-9, "{b:?}");
}

#[test]
fn single_stroke_text_is_open_paths_with_the_top_left_at_the_origin() {
    let fonts = FontLibrary::bundled_only();
    let p = TextParams {
        stroke: Some(StrokeFont::RomanSimplex),
        ..params("H")
    };
    let paths = text_paths(&p, &fonts).unwrap();
    assert_eq!(paths.len(), 3);
    assert!(paths.iter().all(|p| !p.closed));
    let b = bounds(&paths);
    assert!(b.x0.abs() < 1e-9 && b.y0.abs() < 1e-9, "{b:?}");
    assert!(
        (b.height() - p.size_mm).abs() < 1e-6,
        "the cap height is the size"
    );
}

#[test]
fn an_unknown_family_still_makes_text() {
    let fonts = FontLibrary::bundled_only();
    let p = TextParams {
        family: "No Such Family".into(),
        ..params("Hi")
    };
    assert_ne!(text_paths(&p, &fonts).unwrap().len(), 0);
}

#[test]
fn a_bad_size_is_reported() {
    let fonts = FontLibrary::bundled_only();
    let p = TextParams {
        size_mm: 0.0,
        ..params("Hi")
    };
    assert!(text_paths(&p, &fonts).is_err());
}

#[test]
fn the_size_sets_the_scale() {
    let fonts = FontLibrary::bundled_only();
    let small = bounds(
        &text_paths(
            &TextParams {
                size_mm: 10.0,
                ..params("Hi")
            },
            &fonts,
        )
        .unwrap(),
    );
    let big = bounds(
        &text_paths(
            &TextParams {
                size_mm: 30.0,
                ..params("Hi")
            },
            &fonts,
        )
        .unwrap(),
    );
    assert!((big.width() - 3.0 * small.width()).abs() < 1e-6);
}

#[test]
fn the_defaults_are_a_sensible_starting_point() {
    let d = TextParams::default();
    assert_eq!(d.text, "Text");
    assert_eq!(d.family, BUNDLED_FAMILY);
    assert!((d.size_mm - 20.0).abs() < 1e-9);
    assert!(d.stroke.is_none());
}

#[test]
fn text_in_a_loaded_svg_stroke_font_is_open_paths_at_the_origin() {
    let mut fonts = FontLibrary::bundled_only();
    let name = fonts
        .add_stroke_font(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><font horiz-adv-x="600"><font-face font-family="Mini" cap-height="700"/><glyph unicode="H" horiz-adv-x="800" d="M 100 0 L 100 700 M 700 0 L 700 700 M 100 350 L 700 350"/></font></svg>"#,
        )
        .unwrap();
    let p = TextParams {
        svg_font: Some(name),
        ..params("H")
    };
    let paths = text_paths(&p, &fonts).unwrap();
    assert_eq!(paths.len(), 3);
    assert!(paths.iter().all(|p| !p.closed));
    let b = bounds(&paths);
    assert!(b.x0.abs() < 1e-9 && b.y0.abs() < 1e-9, "{b:?}");
    assert!((b.height() - p.size_mm).abs() < 1e-6);
}

#[test]
fn an_svg_font_that_is_not_loaded_is_an_error_not_a_silent_fallback() {
    let fonts = FontLibrary::bundled_only();
    let p = TextParams {
        svg_font: Some("Gone".to_owned()),
        ..params("H")
    };
    assert!(text_paths(&p, &fonts).unwrap_err().contains("Gone"));
}
