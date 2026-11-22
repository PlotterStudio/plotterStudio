//! Corpus-style robustness: damaged input is refused or imported, never a panic.

use ps_import::{SvgOptions, import_dxf, import_svg_auto};

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><g transform="translate(5 5)"><path d="M0 0 C 10 0 20 10 30 10 A 5 5 0 0 1 40 10 Z" stroke="#f00" fill="none"/><circle cx="50" cy="50" r="10"/></g></svg>"##;
const DXF: &str = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n11\n50.0\n21\n0.0\n0\nCIRCLE\n8\n0\n10\n20.0\n20\n20.0\n40\n5.0\n0\nENDSEC\n0\nEOF\n";

#[test]
fn truncated_svg_never_panics() {
    let bytes = SVG.as_bytes();
    for cut in 0..bytes.len() {
        let _ = import_svg_auto(&bytes[..cut], &SvgOptions::default());
    }
}

#[test]
fn corrupted_svg_never_panics() {
    for at in 0..SVG.len() {
        let mut bad = SVG.as_bytes().to_vec();
        bad[at] = b'<';
        let _ = import_svg_auto(&bad, &SvgOptions::default());
    }
}

#[test]
fn truncated_and_corrupted_dxf_never_panic() {
    let bytes = DXF.as_bytes();
    for cut in 0..bytes.len() {
        let _ = import_dxf(&bytes[..cut]);
    }
    for at in 0..bytes.len() {
        let mut bad = bytes.to_vec();
        bad[at] = b'x';
        let _ = import_dxf(&bad);
    }
}
