//! Ground truth for the fuzz entry points: every parser the product reads untrusted
//! files with has one entry that takes arbitrary bytes and must return, never panic or hang. The
//! libFuzzer targets in `fuzz/` call exactly these functions; here a deterministic sweep of
//! truncations and single-byte corruptions of valid seeds stands in, so a crash found by the
//! nightly fuzzer becomes a regression test by adding its bytes to `REGRESSIONS`.

use super::*;

/// Inputs that once crashed a parser, kept for good. Each is `(target, bytes)`.
const REGRESSIONS: &[(Target, &[u8])] = &[];

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><g transform="translate(5 5)"><path d="M0 0 C 10 0 20 10 30 10 A 5 5 0 0 1 40 10 Z" stroke="#f00" fill="none"/><circle cx="50" cy="50" r="10"/><text x="1" y="9">Hi</text></g></svg>"##;
const DXF: &str = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n11\n50.0\n21\n0.0\n0\nCIRCLE\n8\n0\n10\n20.0\n20\n20.0\n40\n5.0\n0\nENDSEC\n0\nEOF\n";

fn pdf_seed() -> Vec<u8> {
    use pdf_writer::{Content, Finish, Pdf, Rect, Ref};
    let (catalog, tree, page, stream) = (Ref::new(1), Ref::new(2), Ref::new(3), Ref::new(4));
    let mut pdf = Pdf::new();
    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree).kids([page]).count(1);
    let mut sheet = pdf.page(page);
    sheet.media_box(Rect::new(0.0, 0.0, 200.0, 100.0));
    sheet.parent(tree);
    sheet.contents(stream);
    sheet.finish();
    let mut c = Content::new();
    c.set_stroke_rgb(1.0, 0.0, 0.0)
        .rect(10.0, 10.0, 50.0, 50.0)
        .stroke();
    pdf.stream(stream, &Content::finish(c));
    pdf.finish()
}

fn project_seed() -> Vec<u8> {
    let meta = ps_model::ProjectMeta::new("Fuzz", "2026-11-18T00:00:00Z", "0.1.0");
    ps_model::Project::new(ps_model::Document::new(meta))
        .to_bytes()
        .unwrap()
}

fn json_seed() -> Vec<u8> {
    include_bytes!("../../ps-library/builtin/tools/generic.drag-blade.json").to_vec()
}

fn zip_seed() -> Vec<u8> {
    use std::io::Write as _;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("library.json", options).unwrap();
    zip.write_all(br#"{"schema_version":1,"id":"fuzz.seed","name":"x","version":"1.0.0","license":"CC0-1.0"}"#)
        .unwrap();
    zip.start_file("card/material.json", options).unwrap();
    zip.write_all(include_bytes!(
        "../../ps-library/builtin/materials/community.cardstock-80lb.json"
    ))
    .unwrap();
    zip.finish().unwrap().into_inner()
}

fn seeds() -> Vec<(Target, Vec<u8>)> {
    vec![
        (Target::Svg, SVG.as_bytes().to_vec()),
        (Target::Dxf, DXF.as_bytes().to_vec()),
        (Target::Pdf, pdf_seed()),
        (Target::Project, project_seed()),
        (Target::MaterialPackage, zip_seed()),
        (Target::LibraryJson, json_seed()),
    ]
}

#[test]
fn every_target_has_a_name_and_a_seed() {
    let names: Vec<&str> = Target::ALL.iter().map(|t| t.name()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        Target::ALL.len(),
        "names are unique: {names:?}"
    );
    for target in Target::ALL {
        assert!(
            seeds().iter().any(|(t, _)| *t == target),
            "{target:?} has no seed"
        );
    }
}

#[test]
fn the_valid_seeds_are_accepted_by_their_own_parser() {
    for (target, bytes) in seeds() {
        assert!(
            accepts(target, &bytes),
            "{target:?} should read its own seed"
        );
    }
}

#[test]
fn empty_and_garbage_input_is_refused_without_panicking() {
    let garbage: Vec<Vec<u8>> = vec![
        Vec::new(),
        vec![0],
        vec![0xFF; 64],
        b"PK\x03\x04".to_vec(),
        b"%PDF-1.7\n".to_vec(),
        b"<svg".to_vec(),
        b"{".to_vec(),
        (0..=255).collect(),
    ];
    for target in Target::ALL {
        for g in &garbage {
            run(target, g);
        }
    }
}

#[test]
fn truncated_seeds_never_panic() {
    for (target, bytes) in seeds() {
        let step = (bytes.len() / 200).max(1);
        for cut in (0..bytes.len()).step_by(step) {
            run(target, &bytes[..cut]);
        }
    }
}

#[test]
fn corrupted_seeds_never_panic() {
    for (target, bytes) in seeds() {
        let step = (bytes.len() / 200).max(1);
        for at in (0..bytes.len()).step_by(step) {
            for value in [0x00, 0xFF, b'<', b'{', b'0'] {
                let mut bad = bytes.clone();
                bad[at] = value;
                run(target, &bad);
            }
        }
    }
}

#[test]
fn past_crashes_stay_fixed() {
    for (target, bytes) in REGRESSIONS {
        run(*target, bytes);
    }
}

#[test]
fn a_target_name_parses_back() {
    for target in Target::ALL {
        assert_eq!(Target::from_name(target.name()), Some(target));
    }
    assert_eq!(Target::from_name("nope"), None);
}
