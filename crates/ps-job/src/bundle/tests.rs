//! Ground truth for the `.psjob` reader: a zip with a fixed layout, a manifest
//! listing the sha256 of every other file, read through the safe zip reader.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

use super::*;
use crate::ir::{Estimate, HolderId, ToolEntry};
use crate::manifest::{MatEntry, SCHEMA_VERSION, sha256_hex};

const GCODE: &[u8] = b"; plotter-studio 0.1.0\nG21\nG90\nM2\n";
const PROFILE: &[u8] = br#"{"id":"generic.cutter-medium"}"#;
const PREVIEW: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>";

fn manifest_for(files: &[(&str, &[u8])]) -> Manifest {
    let hashes: BTreeMap<String, String> = files
        .iter()
        .map(|(n, d)| ((*n).to_owned(), sha256_hex(d)))
        .collect();
    Manifest {
        schema_version: SCHEMA_VERSION,
        job_id: "0192f3a4-7c1e-7b2a-9f00-3d5c1a2b4e6f".to_owned(),
        app_version: "0.1.0".to_owned(),
        export_time: "2026-11-17T12:00:00Z".to_owned(),
        dialect: "ps-gcode/1".to_owned(),
        profile: "generic.cutter-medium@1.0.0".parse().unwrap(),
        mats: vec![MatEntry {
            index: 1,
            name: "Mat 1".to_owned(),
            gcode: "mats/01.gcode".to_owned(),
            preview: Some("previews/01.svg".to_owned()),
            material: None,
            tools: vec![ToolEntry {
                slot: 1,
                tool: "pen.fineliner-black-0.4@1.0.0".parse().unwrap(),
                holder: HolderId::new("A"),
                label: "Fine liner".to_owned(),
                pen: None,
            }],
            pauses: vec![],
            estimate: Estimate::default(),
        }],
        files: hashes,
    }
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        w.start_file(*name, SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn standard_files() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("profile.json", PROFILE),
        ("mats/01.gcode", GCODE),
        ("previews/01.svg", PREVIEW),
    ]
}

fn bundle_with(manifest: &Manifest, files: &[(&str, &[u8])]) -> Vec<u8> {
    let json = serde_json::to_vec_pretty(manifest).unwrap();
    let mut all: Vec<(&str, &[u8])> = vec![("manifest.json", json.as_slice())];
    all.extend_from_slice(files);
    zip_of(&all)
}

fn good_bundle() -> Vec<u8> {
    bundle_with(&manifest_for(&standard_files()), &standard_files())
}

#[test]
fn a_valid_bundle_opens() {
    let b = Bundle::from_bytes(&good_bundle()).unwrap();
    assert_eq!(b.manifest.mats.len(), 1);
    assert_eq!(
        b.manifest.profile.to_string(),
        "generic.cutter-medium@1.0.0"
    );
    assert_eq!(b.gcode(1), Some("; plotter-studio 0.1.0\nG21\nG90\nM2\n"));
    assert_eq!(b.profile_json(), Some(PROFILE));
    assert_eq!(b.file("previews/01.svg"), Some(PREVIEW));
}

#[test]
fn unknown_mats_have_no_gcode() {
    let b = Bundle::from_bytes(&good_bundle()).unwrap();
    assert_eq!(b.gcode(2), None);
    assert_eq!(b.gcode(0), None);
}

#[test]
fn a_changed_file_is_a_hash_mismatch() {
    let manifest = manifest_for(&standard_files());
    let tampered: Vec<(&str, &[u8])> = vec![
        ("profile.json", PROFILE),
        ("mats/01.gcode", b"; plotter-studio 0.1.0\nG1 X999 Y999\n"),
        ("previews/01.svg", PREVIEW),
    ];
    let err = Bundle::from_bytes(&bundle_with(&manifest, &tampered)).unwrap_err();
    assert_eq!(err, BundleError::HashMismatch("mats/01.gcode".to_owned()));
}

#[test]
fn a_listed_but_missing_file_is_reported() {
    let manifest = manifest_for(&standard_files());
    let without_preview: Vec<(&str, &[u8])> =
        vec![("profile.json", PROFILE), ("mats/01.gcode", GCODE)];
    let err = Bundle::from_bytes(&bundle_with(&manifest, &without_preview)).unwrap_err();
    assert_eq!(err, BundleError::MissingFile("previews/01.svg".to_owned()));
}

#[test]
fn an_unlisted_extra_file_is_refused() {
    let manifest = manifest_for(&standard_files());
    let mut files = standard_files();
    files.push(("mats/02.gcode", b"G1 X1\n"));
    let err = Bundle::from_bytes(&bundle_with(&manifest, &files)).unwrap_err();
    assert_eq!(err, BundleError::UnlistedFile("mats/02.gcode".to_owned()));
}

#[test]
fn files_outside_the_layout_never_get_read() {
    let manifest = manifest_for(&standard_files());
    let mut files = standard_files();
    files.push(("payload.exe", b"MZ"));
    let err = Bundle::from_bytes(&bundle_with(&manifest, &files)).unwrap_err();
    assert!(
        matches!(err, BundleError::Zip(ZipError::NotAllowed(_))),
        "{err:?}"
    );
}

#[test]
fn path_traversal_is_refused() {
    let err = Bundle::from_bytes(&zip_of(&[("../manifest.json", b"{}")])).unwrap_err();
    assert!(
        matches!(err, BundleError::Zip(ZipError::UnsafePath(_))),
        "{err:?}"
    );
}

#[test]
fn a_bundle_without_a_manifest_is_refused() {
    let err = Bundle::from_bytes(&zip_of(&standard_files())).unwrap_err();
    assert_eq!(err, BundleError::MissingManifest);
}

#[test]
fn a_malformed_manifest_is_refused() {
    let err = Bundle::from_bytes(&zip_of(&[("manifest.json", b"{ not json")])).unwrap_err();
    assert!(matches!(err, BundleError::Manifest(_)), "{err:?}");
}

#[test]
fn a_newer_schema_asks_the_user_to_update() {
    let mut manifest = manifest_for(&standard_files());
    manifest.schema_version = SCHEMA_VERSION + 1;
    let err = Bundle::from_bytes(&bundle_with(&manifest, &standard_files())).unwrap_err();
    assert!(
        matches!(err, BundleError::Invalid(ManifestError::TooNew { .. })),
        "{err:?}"
    );
}

#[test]
fn a_manifest_that_breaks_its_own_rules_is_refused() {
    let mut manifest = manifest_for(&standard_files());
    manifest.mats[0].index = 5;
    let err = Bundle::from_bytes(&bundle_with(&manifest, &standard_files())).unwrap_err();
    assert!(
        matches!(err, BundleError::Invalid(ManifestError::MatOrder { .. })),
        "{err:?}"
    );
}

#[test]
fn gcode_must_be_utf8() {
    let bad: &[u8] = &[0xff, 0xfe, b'G', b'2', b'1'];
    let files: Vec<(&str, &[u8])> = vec![
        ("profile.json", PROFILE),
        ("mats/01.gcode", bad),
        ("previews/01.svg", PREVIEW),
    ];
    let err = Bundle::from_bytes(&bundle_with(&manifest_for(&files), &files)).unwrap_err();
    assert_eq!(err, BundleError::NotUtf8("mats/01.gcode".to_owned()));
}

#[test]
fn garbage_is_not_a_bundle() {
    let err = Bundle::from_bytes(b"definitely not a zip").unwrap_err();
    assert!(
        matches!(err, BundleError::Zip(ZipError::Invalid(_))),
        "{err:?}"
    );
}

#[test]
fn two_mats_open_in_run_order() {
    let files: Vec<(&str, &[u8])> = vec![
        ("profile.json", PROFILE),
        ("mats/01.gcode", b"M2\n"),
        ("mats/02.gcode", b"G21\nM2\n"),
    ];
    let mut manifest = manifest_for(&files);
    let mut second = manifest.mats[0].clone();
    second.index = 2;
    second.name = "Mat 2".to_owned();
    second.gcode = "mats/02.gcode".to_owned();
    manifest.mats[0].preview = None;
    second.preview = None;
    manifest.mats.push(second);
    let b = Bundle::from_bytes(&bundle_with(&manifest, &files)).unwrap();
    assert_eq!(b.gcode(1), Some("M2\n"));
    assert_eq!(b.gcode(2), Some("G21\nM2\n"));
}
