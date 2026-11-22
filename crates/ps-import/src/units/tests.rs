//! Ground truth for unit handling. The import dialog shows the physical size
//! under each plausible interpretation when they disagree. For SVG that is 96 px/in against
//! 72 px/in: both are offered when the file is unitless and came from an Illustrator-style
//! generator, or when the 72 px/in reading is exactly a common paper size (equivalently, the
//! 96 px/in size is 3/4 of one). DXF takes its units from `$INSUNITS`; unitless asks, defaulting
//! to mm.

use super::*;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn pixels_convert_at_96_and_72_per_inch() {
    assert!(near(mm_per_unit(UnitGuess::Px96), 25.4 / 96.0));
    assert!(near(mm_per_unit(UnitGuess::Px72), 25.4 / 72.0));
    assert!(near(mm_per_unit(UnitGuess::Mm), 1.0));
    assert!(near(mm_per_unit(UnitGuess::In), 25.4));
}

#[test]
fn dxf_units_follow_the_insunits_table() {
    let mm = |code| mm_per_unit(UnitGuess::DxfInsUnits(code));
    assert!(near(mm(1), 25.4), "inches");
    assert!(near(mm(2), 304.8), "feet");
    assert!(near(mm(4), 1.0), "millimetres");
    assert!(near(mm(5), 10.0), "centimetres");
    assert!(near(mm(6), 1_000.0), "metres");
    assert!(near(mm(9), 0.0254), "mils");
    assert!(near(mm(10), 914.4), "yards");
    assert!(near(mm(13), 0.001), "microns");
    assert!(near(mm(14), 100.0), "decimetres");
}

#[test]
fn unitless_dxf_defaults_to_millimetres() {
    assert!(near(mm_per_unit(UnitGuess::DxfInsUnits(0)), 1.0));
    assert!(
        near(mm_per_unit(UnitGuess::DxfInsUnits(99)), 1.0),
        "unknown codes are unitless too"
    );
}

#[test]
fn the_guess_for_a_dxf_header_code() {
    assert_eq!(dxf_unit_guess(4), UnitGuess::DxfInsUnits(4));
    assert!(dxf_is_unitless(0));
    assert!(dxf_is_unitless(99));
    assert!(!dxf_is_unitless(1));
}

#[test]
fn a_size_in_pixels_converts_under_each_reading() {
    let c = SizeCandidate::of(UnitGuess::Px96, 960.0, 480.0);
    assert!(near(c.width_mm, 254.0) && near(c.height_mm, 127.0));
    let c = SizeCandidate::of(UnitGuess::Px72, 720.0, 360.0);
    assert!(near(c.width_mm, 254.0) && near(c.height_mm, 127.0));
}

#[test]
fn a_unitless_illustrator_file_offers_both_readings() {
    let found = svg_size_candidates(800.0, 600.0, true, Generator::Illustrator);
    let units: Vec<UnitGuess> = found.iter().map(|c| c.unit).collect();
    assert_eq!(units, vec![UnitGuess::Px96, UnitGuess::Px72]);
    assert!(near(found[0].width_mm, 800.0 * 25.4 / 96.0));
    assert!(near(found[1].width_mm, 800.0 * 25.4 / 72.0));
}

#[test]
fn a_unitless_inkscape_file_is_taken_at_96() {
    let found = svg_size_candidates(800.0, 600.0, true, Generator::Inkscape);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].unit, UnitGuess::Px96);
}

#[test]
fn a_file_with_real_units_is_not_ambiguous() {
    // width="210mm" and the like are already physical: one reading, in millimetres.
    let found = svg_size_candidates(210.0, 297.0, false, Generator::Illustrator);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].unit, UnitGuess::Mm);
    assert!(near(found[0].width_mm, 210.0));
}

#[test]
fn a_72_dpi_paper_size_offers_both_whatever_the_generator() {
    // 612 x 792 is US Letter at 72 px/in.
    let found = svg_size_candidates(612.0, 792.0, true, Generator::Unknown);
    assert_eq!(found.len(), 2);
    // 595.276 x 841.89 is A4 at 72 px/in (a PDF point rounding).
    let a4 = svg_size_candidates(595.276, 841.89, true, Generator::Unknown);
    assert_eq!(a4.len(), 2);
}

#[test]
fn an_ordinary_size_from_an_unknown_generator_is_taken_at_96() {
    let found = svg_size_candidates(500.0, 500.0, true, Generator::Unknown);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].unit, UnitGuess::Px96);
}

#[test]
fn common_paper_sizes_are_recognised_in_either_orientation() {
    assert_eq!(paper_name(210.0, 297.0), Some("A4"));
    assert_eq!(paper_name(297.0, 210.0), Some("A4"));
    assert_eq!(paper_name(215.9, 279.4), Some("Letter"));
    assert_eq!(
        paper_name(216.0, 279.0),
        Some("Letter"),
        "within half a millimetre"
    );
    assert_eq!(paper_name(100.0, 100.0), None);
}

#[test]
fn generators_are_detected_from_the_root_markup() {
    let ink = "<svg xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" inkscape:version=\"1.3\"/>";
    assert_eq!(detect_generator(ink), Generator::Inkscape);
    let ai = "<?xml version=\"1.0\"?>\n<!-- Generator: Adobe Illustrator 27.0.0, SVG Export Plug-In . SVG Version: 6.00 Build 0)  -->\n<svg/>";
    assert_eq!(detect_generator(ai), Generator::Illustrator);
    let fig = "<svg><g id=\"Frame 1\"/><!-- figma --></svg>";
    assert_eq!(detect_generator(fig), Generator::Figma);
    let aff = "<svg xmlns:serif=\"http://www.serif.com/\" serif:id=\"Layer 1\"/>";
    assert_eq!(detect_generator(aff), Generator::Affinity);
    assert_eq!(detect_generator("<svg/>"), Generator::Unknown);
}

#[test]
fn generator_names_are_stable_keys_for_remembering_a_choice() {
    assert_eq!(Generator::Illustrator.key(), "illustrator");
    assert_eq!(Generator::Unknown.key(), "unknown");
}
