//! Ground truth for the font list: the text tool offers the system's fonts and always
//! has a bundled face to fall back on, so text works on a machine with no usable fonts.

use super::*;

#[test]
fn the_bundled_face_is_always_there() {
    let fonts = FontLibrary::bundled_only();
    assert_eq!(fonts.families(), vec![BUNDLED_FAMILY.to_owned()]);
    let data = fonts.data_for(BUNDLED_FAMILY).expect("the bundled face");
    assert!(ps_import::outline_text(&data, "A", &ps_import::TextStyle::default()).is_ok());
}

#[test]
fn an_unknown_family_falls_back_to_the_bundled_face() {
    let fonts = FontLibrary::bundled_only();
    let (family, data) = fonts.resolve("No Such Family");
    assert_eq!(family, BUNDLED_FAMILY);
    assert_eq!(data, fonts.data_for(BUNDLED_FAMILY).unwrap());
}

#[test]
fn a_known_family_resolves_to_itself() {
    let fonts = FontLibrary::bundled_only();
    assert_eq!(fonts.resolve(BUNDLED_FAMILY).0, BUNDLED_FAMILY);
}

#[test]
fn families_are_sorted_and_have_no_duplicates() {
    let fonts = FontLibrary::system();
    let families = fonts.families();
    let mut sorted = families.clone();
    sorted.sort_by_key(|f| f.to_lowercase());
    sorted.dedup();
    assert_eq!(families, sorted);
    assert!(families.contains(&BUNDLED_FAMILY.to_owned()));
}

#[test]
fn every_listed_family_can_be_loaded() {
    let fonts = FontLibrary::system();
    for family in fonts.families().iter().take(5) {
        assert!(fonts.data_for(family).is_some(), "{family}");
    }
}

// ---- single-stroke SVG fonts ----

const STROKE_FONT: &str = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><font horiz-adv-x="600">
<font-face font-family="Mini Single" units-per-em="1000" cap-height="700"/>
<glyph unicode="H" horiz-adv-x="800" d="M 100 0 L 100 700 M 700 0 L 700 700 M 100 350 L 700 350"/>
</font></defs></svg>"#;

#[test]
fn an_svg_stroke_font_can_be_added_and_is_listed_by_its_family_name() {
    let mut fonts = FontLibrary::bundled_only();
    assert_eq!(fonts.stroke_font_names(), [] as [String; 0]);
    let name = fonts.add_stroke_font(STROKE_FONT).unwrap();
    assert_eq!(name, "Mini Single");
    assert_eq!(fonts.stroke_font_names(), vec!["Mini Single".to_owned()]);
    assert!(fonts.stroke_font("Mini Single").is_some());
    assert!(fonts.stroke_font("Other").is_none());
}

#[test]
fn a_file_that_is_not_a_font_is_refused() {
    let mut fonts = FontLibrary::bundled_only();
    assert!(
        fonts
            .add_stroke_font("<svg xmlns='http://www.w3.org/2000/svg'/>")
            .is_err()
    );
}

#[test]
fn stroke_fonts_are_loaded_from_a_folder_and_bad_files_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.svg"), STROKE_FONT).unwrap();
    std::fs::write(dir.path().join("bad.svg"), "not xml").unwrap();
    std::fs::write(dir.path().join("note.txt"), STROKE_FONT).unwrap();
    let mut fonts = FontLibrary::bundled_only();
    let loaded = fonts.load_stroke_fonts_from(dir.path());
    assert_eq!(loaded, 1);
    assert_eq!(fonts.stroke_font_names(), vec!["Mini Single".to_owned()]);
}

#[test]
fn a_missing_folder_loads_nothing() {
    let mut fonts = FontLibrary::bundled_only();
    assert_eq!(
        fonts.load_stroke_fonts_from(std::path::Path::new("no/such/folder")),
        0
    );
}
