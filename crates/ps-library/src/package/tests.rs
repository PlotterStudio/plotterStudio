//! Ground truth for material packages: a zip with `library.json` and
//! `<folder>/material.json` plus textures; safe-zip rules, every material validated, all errors
//! shown at once grouped by file, import nothing if any is invalid unless asked, packages that
//! declare no licence refused.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use image::{ImageFormat, Rgba, RgbaImage};
use serde_json::json;
use zip::write::SimpleFileOptions;

use super::*;
use crate::testdata::MATERIAL;

fn zip_of(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        w.start_file(*name, SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    RgbaImage::from_pixel(w, h, Rgba([200, 150, 90, 255]))
        .write_to(&mut out, ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

fn library_json(license: Option<&str>) -> Vec<u8> {
    let mut v = json!({"schema_version": 1, "id": "community.test-pack", "name": "Test pack", "version": "1.0.0"});
    if let Some(l) = license {
        v["license"] = json!(l);
    }
    serde_json::to_vec(&v).unwrap()
}

fn material(id: &str, textures: bool) -> Vec<u8> {
    let mut v: serde_json::Value = serde_json::from_str(MATERIAL).unwrap();
    v["id"] = json!(id);
    if textures {
        v["textures"] = json!({"base_color": "base.png", "scale_mm": 50.0});
    }
    serde_json::to_vec(&v).unwrap()
}

fn good_package() -> Vec<u8> {
    zip_of(&[
        ("library.json", library_json(Some("CC0-1.0"))),
        ("kraft/material.json", material("community.kraft", true)),
        ("kraft/base.png", png(16, 16)),
        (
            "glitter/material.json",
            material("community.glitter", false),
        ),
    ])
}

#[test]
fn a_valid_package_imports_every_material() {
    let report = read_package(&good_package()).unwrap();
    assert_eq!(report.library.id, "community.test-pack");
    assert_eq!(report.library.license, "CC0-1.0");
    let ids: Vec<&str> = report
        .materials
        .iter()
        .map(|m| m.material.id.as_str())
        .collect();
    assert_eq!(ids, ["community.glitter", "community.kraft"]); // folder order
    assert_eq!(report.issues.len(), 0);
    assert!(report.all_valid());
    let kraft = report
        .materials
        .iter()
        .find(|m| m.folder == "kraft")
        .unwrap();
    assert!(kraft.textures.contains_key("base.png"));
}

#[test]
fn an_invalid_material_is_reported_with_its_file_and_pointer() {
    let mut bad: serde_json::Value =
        serde_json::from_slice(&material("community.bad", false)).unwrap();
    bad.as_object_mut().unwrap().remove("name");
    let pkg = zip_of(&[
        ("library.json", library_json(Some("CC0-1.0"))),
        ("bad/material.json", serde_json::to_vec(&bad).unwrap()),
        ("ok/material.json", material("community.ok", false)),
    ]);
    let report = read_package(&pkg).unwrap();
    assert!(!report.all_valid());
    assert_eq!(report.issues.len(), 1, "{:?}", report.issues);
    assert_eq!(report.issues[0].0, "bad/material.json");
    assert!(report.issues[0].1.message.contains("name"));
}

#[test]
fn nothing_imports_by_default_when_any_material_is_invalid() {
    let mut bad: serde_json::Value =
        serde_json::from_slice(&material("community.bad", false)).unwrap();
    bad["category"] = json!(7);
    let pkg = zip_of(&[
        ("library.json", library_json(Some("CC0-1.0"))),
        ("bad/material.json", serde_json::to_vec(&bad).unwrap()),
        ("ok/material.json", material("community.ok", false)),
    ]);
    let report = read_package(&pkg).unwrap();
    assert_eq!(report.importable(false).len(), 0);
    let valid: Vec<&str> = report
        .importable(true)
        .iter()
        .map(|m| m.material.id.as_str())
        .collect();
    assert_eq!(valid, ["community.ok"]);
}

#[test]
fn every_error_in_every_file_is_listed_at_once() {
    let mut a: serde_json::Value = serde_json::from_slice(&material("a", false)).unwrap();
    a["category"] = json!(1);
    a["license"] = json!(2);
    let mut b: serde_json::Value = serde_json::from_slice(&material("b", false)).unwrap();
    b.as_object_mut().unwrap().remove("id");
    let pkg = zip_of(&[
        ("library.json", library_json(Some("CC0-1.0"))),
        ("a/material.json", serde_json::to_vec(&a).unwrap()),
        ("b/material.json", serde_json::to_vec(&b).unwrap()),
    ]);
    let report = read_package(&pkg).unwrap();
    let grouped = report.issues_by_file();
    assert_eq!(grouped["a/material.json"].len(), 2);
    assert_eq!(grouped["b/material.json"].len(), 1);
}

#[test]
fn a_package_must_declare_a_licence() {
    let pkg = zip_of(&[
        ("library.json", library_json(None)),
        ("ok/material.json", material("community.ok", false)),
    ]);
    let report = read_package(&pkg).unwrap();
    assert!(!report.all_valid());
    assert!(
        report
            .issues
            .iter()
            .any(|(f, i)| f == "library.json" && i.pointer.contains("license"))
    );
}

#[test]
fn an_empty_licence_counts_as_none() {
    let pkg = zip_of(&[
        ("library.json", library_json(Some(""))),
        ("ok/material.json", material("community.ok", false)),
    ]);
    assert!(!read_package(&pkg).unwrap().all_valid());
}

#[test]
fn a_package_without_library_json_is_refused() {
    let pkg = zip_of(&[("ok/material.json", material("community.ok", false))]);
    assert_eq!(
        read_package(&pkg).unwrap_err(),
        PackageError::MissingLibrary
    );
}

#[test]
fn a_package_with_no_materials_is_refused() {
    let pkg = zip_of(&[("library.json", library_json(Some("CC0-1.0")))]);
    assert_eq!(read_package(&pkg).unwrap_err(), PackageError::NoMaterials);
}

#[test]
fn unsafe_and_unlisted_names_are_refused_before_anything_is_read() {
    for name in [
        "../escape/material.json",
        "/abs/material.json",
        "ok/run.exe",
        "ok/sub/deep/material.json",
        "notes.txt",
    ] {
        let pkg = zip_of(&[
            ("library.json", library_json(Some("CC0-1.0"))),
            (name, b"x".to_vec()),
        ]);
        let err = read_package(&pkg).unwrap_err();
        assert!(matches!(err, PackageError::Zip(_)), "{name}: {err:?}");
    }
}

#[test]
fn a_missing_texture_file_is_reported() {
    let pkg = zip_of(&[
        ("library.json", library_json(Some("CC0-1.0"))),
        ("kraft/material.json", material("community.kraft", true)),
    ]);
    let report = read_package(&pkg).unwrap();
    assert!(
        report
            .issues
            .iter()
            .any(|(f, i)| f == "kraft/material.json" && i.pointer == "/textures/base_color")
    );
}

#[test]
fn an_oversized_texture_is_reported() {
    let pkg = zip_of(&[
        ("library.json", library_json(Some("CC0-1.0"))),
        ("kraft/material.json", material("community.kraft", true)),
        ("kraft/base.png", png(4097, 2)),
    ]);
    let report = read_package(&pkg).unwrap();
    assert!(
        report
            .issues
            .iter()
            .any(|(f, i)| f == "kraft/base.png" && i.message.contains("4096")),
        "{:?}",
        report.issues
    );
}

#[test]
fn a_corrupt_texture_is_reported() {
    let pkg = zip_of(&[
        ("library.json", library_json(Some("CC0-1.0"))),
        ("kraft/material.json", material("community.kraft", true)),
        ("kraft/base.png", b"not a png".to_vec()),
    ]);
    let report = read_package(&pkg).unwrap();
    assert!(report.issues.iter().any(|(f, _)| f == "kraft/base.png"));
}

#[test]
fn export_then_import_round_trips() {
    let report = read_package(&good_package()).unwrap();
    let items: Vec<(Material, BTreeMap<String, Vec<u8>>)> = report
        .materials
        .iter()
        .map(|m| (m.material.clone(), m.textures.clone()))
        .collect();
    let bytes = export_package(&report.library, &items).unwrap();
    let again = read_package(&bytes).unwrap();
    assert!(again.all_valid());
    assert_eq!(again.library, report.library);
    let before: Vec<&Material> = report.materials.iter().map(|m| &m.material).collect();
    let after: Vec<&Material> = again.materials.iter().map(|m| &m.material).collect();
    assert_eq!(before, after);
    let tex = |r: &ImportReport| {
        r.materials
            .iter()
            .flat_map(|m| m.textures.values().cloned())
            .collect::<Vec<_>>()
    };
    assert_eq!(tex(&again), tex(&report));
}

#[test]
fn export_is_deterministic() {
    let report = read_package(&good_package()).unwrap();
    let items: Vec<(Material, BTreeMap<String, Vec<u8>>)> = report
        .materials
        .iter()
        .map(|m| (m.material.clone(), m.textures.clone()))
        .collect();
    assert_eq!(
        export_package(&report.library, &items).unwrap(),
        export_package(&report.library, &items).unwrap()
    );
}

#[test]
fn export_refuses_a_package_without_a_licence() {
    let mut info = read_package(&good_package()).unwrap().library;
    info.license.clear();
    let err = export_package(&info, &[]).unwrap_err();
    assert_eq!(err, PackageError::NoLicense);
}
