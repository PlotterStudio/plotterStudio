//! Ground truth for SVG single-stroke fonts: the EMS and Hershey sets used by the
//! Inkscape Hershey Text extension are SVG fonts whose glyphs are open stroke paths. Coordinates
//! are font units with y up; the output is millimetres with y down and the baseline at y = 0.

use ps_geometry::{Path, Point};

use super::*;

/// A tiny font, 1000 units per em, capitals 700 tall: `H` is two stems and a bar, `I` a stem,
/// `-` a bar, and `O` a closed box.
const FONT: &str = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs>
<font id="t" horiz-adv-x="600">
  <font-face font-family="Test Single" units-per-em="1000" cap-height="700"/>
  <missing-glyph horiz-adv-x="500" d=""/>
  <glyph unicode="H" glyph-name="H" horiz-adv-x="800" d="M 100 0 L 100 700 M 700 0 L 700 700 M 100 350 L 700 350"/>
  <glyph unicode="I" glyph-name="I" horiz-adv-x="300" d="M 150 0 L 150 700"/>
  <glyph unicode="-" glyph-name="hyphen" horiz-adv-x="500" d="M 50 350 L 450 350"/>
  <glyph unicode="O" glyph-name="O" horiz-adv-x="800" d="M 100 0 L 700 0 L 700 700 L 100 700 Z"/>
  <glyph unicode=" " glyph-name="space" horiz-adv-x="400"/>
  <glyph unicode="C" glyph-name="C" horiz-adv-x="700" d="M 100 0 C 100 300 300 700 600 700"/>
</font></defs></svg>"#;

fn font() -> SvgStrokeFont {
    SvgStrokeFont::parse(FONT).expect("a valid font")
}

fn bounds(paths: &[Path]) -> (f64, f64, f64, f64) {
    let mut b = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for p in paths {
        for q in ps_geometry::flatten(p, 0.01) {
            b = (b.0.min(q.x), b.1.min(q.y), b.2.max(q.x), b.3.max(q.y));
        }
    }
    b
}

#[test]
fn the_font_reports_its_name_and_glyphs() {
    let f = font();
    assert_eq!(f.name(), "Test Single");
    assert!(f.has_glyph('H') && f.has_glyph('-') && f.has_glyph(' '));
    assert!(!f.has_glyph('Z'));
}

#[test]
fn a_letter_is_scaled_so_capitals_have_the_requested_height() {
    let paths = font().text("H", 10.0).unwrap();
    // H = 3 strokes, all open.
    assert_eq!(paths.len(), 3);
    assert!(paths.iter().all(|p| !p.closed));
    let (x0, y0, x1, y1) = bounds(&paths);
    // Scale 10 mm / 700 units; y is down, the baseline at 0, so capitals reach -10.
    assert!(
        (y1 - 0.0).abs() < 1e-9 && (y0 + 10.0).abs() < 1e-9,
        "{y0} {y1}"
    );
    assert!((x0 - 100.0 * 10.0 / 700.0).abs() < 1e-9);
    assert!((x1 - 700.0 * 10.0 / 700.0).abs() < 1e-9);
}

#[test]
fn letters_advance_by_their_own_width() {
    let paths = font().text("II", 7.0).unwrap();
    // 7 mm cap = 0.01 mm per unit. The second stem is one advance (300 units) to the right.
    let xs: Vec<f64> = paths.iter().map(|p| p.start.x).collect();
    assert_eq!(xs.len(), 2);
    assert!((xs[1] - xs[0] - 3.0).abs() < 1e-9, "{xs:?}");
}

#[test]
fn a_space_advances_without_drawing_and_an_unknown_letter_uses_the_missing_glyph_width() {
    let spaced = font().text("I I", 7.0).unwrap();
    assert!((spaced[1].start.x - spaced[0].start.x - (300.0 + 400.0) * 0.01).abs() < 1e-9);
    let unknown = font().text("IZI", 7.0).unwrap();
    assert!((unknown[1].start.x - unknown[0].start.x - (300.0 + 500.0) * 0.01).abs() < 1e-9);
}

#[test]
fn a_closed_glyph_path_stays_closed() {
    let paths = font().text("O", 7.0).unwrap();
    assert_eq!(paths.len(), 1);
    assert!(paths[0].closed);
}

#[test]
fn curves_are_kept_as_cubics() {
    let paths = font().text("C", 7.0).unwrap();
    assert!(
        paths[0]
            .segs
            .iter()
            .any(|s| matches!(s, ps_geometry::Seg::Cubic { .. }))
    );
}

#[test]
fn new_lines_drop_by_one_and_a_half_cap_heights() {
    let paths = font().text("I\nI", 7.0).unwrap();
    assert_eq!(paths.len(), 2);
    let dy = paths[1].start.y - paths[0].start.y;
    assert!((dy - 10.5).abs() < 1e-9, "{dy}");
}

#[test]
fn the_cap_height_falls_back_to_the_letter_h_when_the_font_does_not_say() {
    let src = FONT.replace(" cap-height=\"700\"", "");
    let f = SvgStrokeFont::parse(&src).unwrap();
    let (_, y0, _, _) = bounds(&f.text("H", 10.0).unwrap());
    assert!((y0 + 10.0).abs() < 1e-9);
}

#[test]
fn a_file_without_a_font_or_with_bad_xml_is_refused() {
    assert_eq!(
        SvgStrokeFont::parse("<svg xmlns='http://www.w3.org/2000/svg'/>").unwrap_err(),
        SvgFontError::NoFont
    );
    assert!(matches!(
        SvgStrokeFont::parse("<svg"),
        Err(SvgFontError::Xml(_))
    ));
}

#[test]
fn a_bad_size_is_refused() {
    assert_eq!(font().text("H", 0.0).unwrap_err(), SvgFontError::BadSize);
    assert_eq!(
        font().text("H", f64::NAN).unwrap_err(),
        SvgFontError::BadSize
    );
}

#[test]
fn a_glyph_with_broken_path_data_is_skipped_not_fatal() {
    let src = FONT.replace("M 150 0 L 150 700", "M 150 0 L nonsense");
    let f = SvgStrokeFont::parse(&src).unwrap();
    assert_eq!(f.text("I", 7.0).unwrap(), [] as [ps_geometry::Path; 0]);
    assert_ne!(f.text("H", 7.0).unwrap(), [] as [ps_geometry::Path; 0]);
}

#[test]
fn entities_and_external_references_are_not_followed() {
    let evil = "<?xml version='1.0'?><!DOCTYPE svg [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><svg xmlns='http://www.w3.org/2000/svg'><font><glyph unicode='&x;'/></font></svg>";
    assert!(SvgStrokeFont::parse(evil).is_err());
}

#[test]
fn text_is_placed_from_the_origin_with_the_first_stroke_on_the_baseline() {
    let paths = font().text("-", 7.0).unwrap();
    assert_eq!(paths[0].start, Point::new(0.5, -3.5));
}
