//! Ground truth for the project file: a zip with `project.json`
//! (`schema_version`, `min_reader_version`, the document), original imports stored once by BLAKE3
//! under `assets/`, thumbnails, and optionally embedded libraries. Already-compressed assets are
//! stored uncompressed and everything else is deflated. Output is deterministic. Newer files open
//! read-only with a warning; unknown fields survive a round trip; migrations are pure functions
//! on `serde_json::Value` chained from the file's version to the current one.

use std::io::{Cursor, Read};

use ps_core::{AssetId, ContentHash, MatId, ObjectId, PathKey};
use ps_geometry::{Affine, FillRule, Path};
use semver::Version;
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::*;
use crate::{Asset, Mat, MatSize, PlacedObject, ProjectMeta, ShapePath, SourceStyle};

fn app() -> Version {
    Version::new(0, 1, 0)
}

fn sample() -> Document {
    let mut doc = Document::new(ProjectMeta::new(
        "Birthday card",
        "2026-11-18T00:00:00Z",
        "0.1.0",
    ));
    let svg = Asset::new(AssetId::new(), "art.svg", b"<svg/>".to_vec());
    let pdf = Asset::new(AssetId::new(), "art.pdf", b"%PDF-1.7".to_vec());
    Arc::make_mut(&mut doc.assets).insert(svg.id, Arc::new(svg));
    Arc::make_mut(&mut doc.assets).insert(pdf.id, Arc::new(pdf));

    let mut mat = Mat::new(MatId::new(), "Mat 1", MatSize::inches(12.0, 12.0));
    let object = PlacedObject {
        id: ObjectId::new(),
        source: None,
        transform: Affine::translate((5.0, 6.0)),
        group: None,
        attach: None,
        paths: Arc::new(vec![Arc::new(ShapePath {
            key: PathKey::source("p1"),
            geom: Path::rect(0.0, 0.0, 10.0, 10.0),
            fill_rule: FillRule::NonZero,
            style: SourceStyle::default(),
            ops: Vec::new(),
            start_locked: false,
        })]),
        locked: false,
        hidden: false,
    };
    Arc::make_mut(&mut mat.objects).insert(object.id, Arc::new(object));
    Arc::make_mut(&mut doc.mats).insert(mat.id, Arc::new(mat));
    doc
}

fn entries(bytes: &[u8]) -> Vec<(String, CompressionMethod)> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let f = zip.by_index(i).unwrap();
            (f.name().unwrap().into_owned(), f.compression())
        })
        .collect()
}

fn file(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}

fn project_json(bytes: &[u8]) -> Value {
    serde_json::from_slice(&file(bytes, "project.json")).unwrap()
}

/// Rebuilds an archive with one file replaced (or added), to make tampered inputs.
fn with_file(bytes: &[u8], name: &str, content: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default();
    let mut seen = false;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        out.start_file(f.name().unwrap(), opts).unwrap();
        if f.name().unwrap() == name {
            seen = true;
            out.write_all(content).unwrap();
        } else {
            out.write_all(&data).unwrap();
        }
    }
    if !seen {
        out.start_file(name, opts).unwrap();
        out.write_all(content).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn without_file(bytes: &[u8], name: &str) -> Vec<u8> {
    use std::io::Write;
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        if f.name().unwrap() == name {
            continue;
        }
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        out.start_file(f.name().unwrap(), opts).unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn hex(h: &ContentHash) -> String {
    h.to_string()
}

// ---- layout ----

#[test]
fn a_saved_project_has_the_documented_layout() {
    let doc = sample();
    let bytes = Project::new(doc.clone()).to_bytes().unwrap();
    let names: Vec<String> = entries(&bytes).into_iter().map(|(n, _)| n).collect();
    assert!(names.contains(&"project.json".to_owned()));
    for asset in doc.assets.values() {
        let ext = asset.name.rsplit('.').next().unwrap();
        assert!(
            names.contains(&format!("assets/{}.{ext}", hex(&asset.hash))),
            "{names:?}"
        );
    }
    assert_eq!(names.len(), 3, "{names:?}");
}

#[test]
fn project_json_records_versions_and_leaves_asset_bytes_out() {
    let bytes = Project::new(sample()).to_bytes().unwrap();
    let v = project_json(&bytes);
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["min_reader_version"], "0.1.0");
    assert_eq!(v["document"]["meta"]["name"], "Birthday card");
    let first_asset = v["document"]["assets"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert!(first_asset.get("bytes").is_none());
    assert!(first_asset.get("hash").is_some());
    assert_eq!(v["template"], false);
}

#[test]
fn compressed_assets_are_stored_and_the_rest_deflated() {
    let bytes = Project::new(sample()).to_bytes().unwrap();
    let methods: Vec<(String, CompressionMethod)> = entries(&bytes);
    let method = |suffix: &str| methods.iter().find(|(n, _)| n.ends_with(suffix)).unwrap().1;
    assert_eq!(method(".pdf"), CompressionMethod::Stored);
    assert_eq!(method(".svg"), CompressionMethod::Deflated);
    assert_eq!(method("project.json"), CompressionMethod::Deflated);
}

#[test]
fn thumbnails_are_stored_under_their_mat_number() {
    let mut p = Project::new(sample());
    p.thumbnails.push(Thumbnail {
        mat_index: 1,
        png: vec![0x89, b'P', b'N', b'G'],
    });
    let bytes = p.to_bytes().unwrap();
    assert_eq!(
        file(&bytes, "thumbnails/mat-01.png"),
        vec![0x89, b'P', b'N', b'G']
    );
    let loaded = Project::from_bytes(&bytes, &app()).unwrap().project;
    assert_eq!(loaded.thumbnails, p.thumbnails);
}

#[test]
fn embedded_libraries_travel_with_the_project() {
    let mut p = Project::new(sample());
    p.embedded.push(EmbeddedFile {
        path: "libraries/tools/pen.json".into(),
        bytes: b"{\"id\":\"pen\"}".to_vec(),
    });
    let bytes = p.to_bytes().unwrap();
    let loaded = Project::from_bytes(&bytes, &app()).unwrap().project;
    assert_eq!(loaded.embedded, p.embedded);
}

// ---- round trip and integrity ----

#[test]
fn a_project_round_trips_exactly() {
    let doc = sample();
    let bytes = Project::new(doc.clone()).to_bytes().unwrap();
    let loaded = Project::from_bytes(&bytes, &app()).unwrap();
    assert_eq!(loaded.project.document, doc);
    assert_eq!(loaded.read_only, None);
    assert!(!loaded.project.template);
    // Asset content came back from the assets folder.
    let asset = loaded.project.document.assets.values().next().unwrap();
    assert_eq!(asset.bytes.as_slice(), b"<svg/>");
}

#[test]
fn saving_is_deterministic() {
    let p = Project::new(sample());
    assert_eq!(p.to_bytes().unwrap(), p.to_bytes().unwrap());
}

#[test]
fn identical_imports_are_stored_once() {
    let mut doc = sample();
    let twin = Asset::new(AssetId::new(), "art.svg", b"<svg/>".to_vec());
    Arc::make_mut(&mut doc.assets).insert(twin.id, Arc::new(twin));
    let bytes = Project::new(doc).to_bytes().unwrap();
    let svgs = entries(&bytes)
        .into_iter()
        .filter(|(n, _)| {
            std::path::Path::new(n)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
        })
        .count();
    assert_eq!(svgs, 1);
}

#[test]
fn an_asset_whose_bytes_were_changed_is_refused() {
    let doc = sample();
    let bytes = Project::new(doc.clone()).to_bytes().unwrap();
    let svg = doc
        .assets
        .values()
        .find(|a| {
            std::path::Path::new(&a.name)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
        })
        .unwrap();
    let tampered = with_file(
        &bytes,
        &format!("assets/{}.svg", hex(&svg.hash)),
        b"<svg onload='x'/>",
    );
    assert!(matches!(
        Project::from_bytes(&tampered, &app()),
        Err(ProjectError::AssetMismatch(_))
    ));
}

#[test]
fn a_missing_asset_file_is_refused() {
    let doc = sample();
    let bytes = Project::new(doc.clone()).to_bytes().unwrap();
    let svg = doc
        .assets
        .values()
        .find(|a| {
            std::path::Path::new(&a.name)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
        })
        .unwrap();
    let cut = without_file(&bytes, &format!("assets/{}.svg", hex(&svg.hash)));
    assert!(matches!(
        Project::from_bytes(&cut, &app()),
        Err(ProjectError::MissingAsset(_))
    ));
}

#[test]
fn a_file_outside_the_layout_is_refused() {
    let bytes = Project::new(sample()).to_bytes().unwrap();
    let extra = with_file(&bytes, "notes.txt", b"hi");
    assert!(matches!(
        Project::from_bytes(&extra, &app()),
        Err(ProjectError::Zip(_))
    ));
    let traversal = with_file(&bytes, "../evil.json", b"{}");
    assert!(matches!(
        Project::from_bytes(&traversal, &app()),
        Err(ProjectError::Zip(_))
    ));
}

#[test]
fn damaged_inputs_are_errors_not_panics() {
    assert!(matches!(
        Project::from_bytes(b"not a zip", &app()),
        Err(ProjectError::Zip(_))
    ));
    let bytes = Project::new(sample()).to_bytes().unwrap();
    assert!(matches!(
        Project::from_bytes(&without_file(&bytes, "project.json"), &app()),
        Err(ProjectError::MissingProject)
    ));
    assert!(matches!(
        Project::from_bytes(&with_file(&bytes, "project.json", b"{not json"), &app()),
        Err(ProjectError::Json(_))
    ));
    assert!(matches!(
        Project::from_bytes(&with_file(&bytes, "project.json", b"[1,2]"), &app()),
        Err(ProjectError::Json(_))
    ));
}

// ---- forward compatibility ----

fn edited(bytes: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut v = project_json(bytes);
    edit(&mut v);
    with_file(bytes, "project.json", &serde_json::to_vec(&v).unwrap())
}

#[test]
fn a_newer_schema_opens_read_only_with_its_version() {
    let bytes = Project::new(sample()).to_bytes().unwrap();
    let newer = edited(&bytes, |v| v["schema_version"] = json!(7));
    let loaded = Project::from_bytes(&newer, &app()).unwrap();
    assert_eq!(
        loaded.read_only,
        Some(ReadOnly::NewerSchema {
            found: 7,
            supported: SCHEMA_VERSION
        })
    );
    assert_eq!(
        loaded.project.document,
        sample_equal(&bytes),
        "its content still loads"
    );
}

fn sample_equal(bytes: &[u8]) -> Document {
    Project::from_bytes(bytes, &app()).unwrap().project.document
}

#[test]
fn a_file_that_needs_a_newer_app_opens_read_only() {
    let bytes = Project::new(sample()).to_bytes().unwrap();
    let newer = edited(&bytes, |v| v["min_reader_version"] = json!("2.0.0"));
    let loaded = Project::from_bytes(&newer, &app()).unwrap();
    assert_eq!(
        loaded.read_only,
        Some(ReadOnly::NewerApp {
            needed: Version::new(2, 0, 0),
            running: app()
        })
    );
}

#[test]
fn a_file_the_app_is_new_enough_for_opens_normally() {
    let bytes = Project::new(sample()).to_bytes().unwrap();
    let ok = Project::from_bytes(&bytes, &Version::new(3, 0, 0)).unwrap();
    assert_eq!(ok.read_only, None);
}

#[test]
fn a_malformed_min_reader_version_is_an_error() {
    let bytes = Project::new(sample()).to_bytes().unwrap();
    let bad = edited(&bytes, |v| v["min_reader_version"] = json!("soon"));
    assert!(matches!(
        Project::from_bytes(&bad, &app()),
        Err(ProjectError::Json(_))
    ));
}

#[test]
fn unknown_fields_survive_a_round_trip_at_every_depth() {
    let bytes = Project::new(sample()).to_bytes().unwrap();
    let future = edited(&bytes, |v| {
        v["future_top"] = json!({"a": 1});
        v["document"]["meta"]["future_meta"] = json!("kept");
        let mat = v["document"]["mats"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap();
        mat["future_mat"] = json!([1, 2, 3]);
        let object = mat["objects"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap();
        object["paths"][0]["future_path"] = json!(true);
    });
    let loaded = Project::from_bytes(&future, &app()).unwrap().project;
    let resaved = project_json(&loaded.to_bytes().unwrap());
    assert_eq!(resaved["future_top"], json!({"a": 1}));
    assert_eq!(resaved["document"]["meta"]["future_meta"], "kept");
    let mat = resaved["document"]["mats"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(mat["future_mat"], json!([1, 2, 3]));
    let object = mat["objects"].as_object().unwrap().values().next().unwrap();
    assert_eq!(object["paths"][0]["future_path"], true);
}

#[test]
fn edits_still_apply_and_unknown_fields_of_removed_items_go_with_them() {
    let bytes = Project::new(sample()).to_bytes().unwrap();
    let future = edited(&bytes, |v| {
        let mat = v["document"]["mats"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap();
        mat["future_mat"] = json!(1);
    });
    let mut loaded = Project::from_bytes(&future, &app()).unwrap().project;
    Arc::make_mut(&mut loaded.document.meta).name = "Renamed".into();
    Arc::make_mut(&mut loaded.document.mats).clear();
    let resaved = project_json(&loaded.to_bytes().unwrap());
    assert_eq!(resaved["document"]["meta"]["name"], "Renamed");
    assert_eq!(resaved["document"]["mats"], json!({}));
    assert!(!resaved.to_string().contains("future_mat"));
}

// ---- templates ----

#[test]
fn a_template_opens_as_an_untitled_copy() {
    let mut p = Project::new(sample());
    p.template = true;
    let loaded = Project::from_bytes(&p.to_bytes().unwrap(), &app())
        .unwrap()
        .project;
    assert!(loaded.template);
    let copy = loaded.open_template();
    assert_eq!(copy.meta.name, "Untitled");
    assert_eq!(
        copy.mats.len(),
        loaded.document.mats.len(),
        "the content is copied"
    );
    assert_ne!(copy.meta.name, loaded.document.meta.name);
}

// ---- migrations ----

fn rename_name_to_title(v: &mut Value) -> Result<(), String> {
    let meta = v["document"]["meta"].as_object_mut().ok_or("no meta")?;
    let name = meta.remove("name").ok_or("no name")?;
    meta.insert("title".into(), name);
    Ok(())
}

fn title_back_to_name(v: &mut Value) -> Result<(), String> {
    let meta = v["document"]["meta"].as_object_mut().ok_or("no meta")?;
    let title = meta.remove("title").ok_or("no title")?;
    meta.insert("name".into(), title);
    Ok(())
}

fn fail(_: &mut Value) -> Result<(), String> {
    Err("boom".into())
}

#[test]
fn migrations_chain_from_the_files_version_to_the_current_one() {
    let steps: [Migration; 2] = [rename_name_to_title, title_back_to_name];
    let old = json!({"schema_version": 0, "document": {"meta": {"name": "x"}}});
    let (migrated, from) = migrate_with(old, &steps, 2).unwrap();
    assert_eq!(from, 0);
    assert_eq!(migrated["schema_version"], 2);
    assert_eq!(migrated["document"]["meta"]["name"], "x");
}

#[test]
fn a_file_already_current_is_not_touched() {
    let v = json!({"schema_version": 2, "document": {"meta": {"name": "x"}}});
    let (same, from) =
        migrate_with(v.clone(), &[rename_name_to_title, title_back_to_name], 2).unwrap();
    assert_eq!(same, v);
    assert_eq!(from, 2);
}

#[test]
fn a_file_part_way_only_runs_the_remaining_steps() {
    let v = json!({"schema_version": 1, "document": {"meta": {"title": "x"}}});
    let (m, from) = migrate_with(v, &[rename_name_to_title, title_back_to_name], 2).unwrap();
    assert_eq!(from, 1);
    assert_eq!(m["document"]["meta"]["name"], "x");
}

#[test]
fn a_failing_step_names_the_version_it_failed_from() {
    let v = json!({"schema_version": 0, "document": {}});
    let err = migrate_with(v, &[fail], 1).unwrap_err();
    assert!(
        matches!(err, ProjectError::Migration { from: 0, .. }),
        "{err:?}"
    );
}

#[test]
fn a_gap_in_the_chain_is_an_error() {
    let v = json!({"schema_version": 0, "document": {}});
    assert!(matches!(
        migrate_with(v, &[], 3),
        Err(ProjectError::Migration { from: 0, .. })
    ));
}

#[test]
fn a_missing_schema_version_is_an_error() {
    assert!(matches!(
        migrate_with(json!({"document": {}}), &[], 1),
        Err(ProjectError::Json(_))
    ));
}

#[test]
fn the_current_schema_needs_no_migration_today() {
    assert_eq!(SCHEMA_VERSION, 1);
    assert_eq!(
        MIGRATIONS.len(),
        0,
        "add a golden test with the old file when this changes"
    );
}
