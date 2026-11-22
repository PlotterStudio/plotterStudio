//! Ground truth for the `.psjob` manifest: fixed layout, sha256 of every file,
//! validated by a published JSON Schema.

use ps_core::LibRef;

use super::*;
use crate::ir::{Estimate, HolderId, ToolEntry};

fn lib(text: &str) -> LibRef {
    text.parse().unwrap()
}

fn sample() -> Manifest {
    let mut files = BTreeMap::new();
    files.insert("profile.json".to_owned(), sha256_hex(b"{}"));
    files.insert("mats/01.gcode".to_owned(), sha256_hex(b"G21\n"));
    files.insert("previews/01.svg".to_owned(), sha256_hex(b"<svg/>"));
    Manifest {
        schema_version: SCHEMA_VERSION,
        job_id: "0192f3a4-7c1e-7b2a-9f00-3d5c1a2b4e6f".to_owned(),
        app_version: "0.1.0".to_owned(),
        export_time: "2026-11-17T12:00:00Z".to_owned(),
        dialect: "ps-gcode/1".to_owned(),
        profile: lib("generic.cutter-medium@1.0.0"),
        mats: vec![MatEntry {
            index: 1,
            name: "Mat 1".to_owned(),
            gcode: "mats/01.gcode".to_owned(),
            preview: Some("previews/01.svg".to_owned()),
            material: Some(lib("community.cardstock-80lb@2.1.0")),
            tools: vec![ToolEntry {
                slot: 1,
                tool: lib("pen.fineliner-black-0.4@1.0.0"),
                holder: HolderId::new("A"),
                label: "Fine liner, black 0.4".to_owned(),
                pen: None,
            }],
            pauses: vec![PauseRow {
                line: 6,
                text: "Load: T1 Fine liner black 0.4 into clamp A".to_owned(),
            }],
            estimate: Estimate {
                machine_time_ms: 252_000,
                with_changes_ms: 252_000,
                cut_length_um: [(1, 100_000)].into_iter().collect(),
                tool_changes: 0,
                utilisation_ppm: 250_000,
            },
        }],
        files,
    }
}

// ---- pure helpers ----

#[test]
fn sha256_matches_the_published_test_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn file_names_follow_the_fixed_layout() {
    assert_eq!(gcode_path(1), "mats/01.gcode");
    assert_eq!(gcode_path(12), "mats/12.gcode");
    assert_eq!(preview_path(1), "previews/01.svg");
    assert_eq!(gcode_path(123), "mats/123.gcode");
}

#[test]
fn only_layout_files_are_allowed_in_a_bundle() {
    for ok in [
        "manifest.json",
        "profile.json",
        "mats/01.gcode",
        "mats/10.gcode",
        "previews/01.svg",
    ] {
        assert!(is_bundle_file(ok), "{ok}");
    }
    for bad in [
        "other.json",
        "mats/1.gcode",
        "mats/01.txt",
        "mats/a1.gcode",
        "mats/01.gcode/x",
        "previews/01.png",
        "evil/manifest.json",
        "mats/.gcode",
    ] {
        assert!(!is_bundle_file(bad), "{bad}");
    }
}

// ---- validation ----

#[test]
fn a_complete_manifest_is_valid() {
    sample().validate().unwrap();
}

#[test]
fn newer_schemas_are_refused() {
    let mut m = sample();
    m.schema_version = SCHEMA_VERSION + 1;
    assert_eq!(
        m.validate().unwrap_err(),
        ManifestError::TooNew {
            found: SCHEMA_VERSION + 1,
            supported: SCHEMA_VERSION
        }
    );
}

#[test]
fn mats_must_be_numbered_in_run_order() {
    let mut m = sample();
    m.mats[0].index = 2;
    assert!(matches!(
        m.validate().unwrap_err(),
        ManifestError::MatOrder { .. }
    ));
}

#[test]
fn a_mat_without_a_file_entry_is_invalid() {
    let mut m = sample();
    m.files.remove("mats/01.gcode");
    assert_eq!(
        m.validate().unwrap_err(),
        ManifestError::MissingFileEntry("mats/01.gcode".to_owned())
    );
}

#[test]
fn the_profile_copy_is_required() {
    let mut m = sample();
    m.files.remove("profile.json");
    assert_eq!(
        m.validate().unwrap_err(),
        ManifestError::MissingFileEntry("profile.json".to_owned())
    );
}

#[test]
fn a_preview_without_a_file_entry_is_invalid() {
    let mut m = sample();
    m.files.remove("previews/01.svg");
    assert_eq!(
        m.validate().unwrap_err(),
        ManifestError::MissingFileEntry("previews/01.svg".to_owned())
    );
}

#[test]
fn hashes_must_be_64_lowercase_hex_digits() {
    for bad in ["", "abc", &"G".repeat(64), &"A".repeat(64)] {
        let mut m = sample();
        m.files.insert("profile.json".to_owned(), bad.to_owned());
        assert_eq!(
            m.validate().unwrap_err(),
            ManifestError::BadHash("profile.json".to_owned()),
            "{bad:?}"
        );
    }
}

#[test]
fn files_outside_the_layout_are_invalid() {
    let mut m = sample();
    m.files.insert("secrets.txt".to_owned(), sha256_hex(b"x"));
    assert_eq!(
        m.validate().unwrap_err(),
        ManifestError::UnexpectedFile("secrets.txt".to_owned())
    );
}

#[test]
fn the_manifest_cannot_list_itself() {
    let mut m = sample();
    m.files.insert("manifest.json".to_owned(), sha256_hex(b"x"));
    assert_eq!(
        m.validate().unwrap_err(),
        ManifestError::UnexpectedFile("manifest.json".to_owned())
    );
}

#[test]
fn a_manifest_needs_a_job_id_and_at_least_one_mat() {
    let mut m = sample();
    m.job_id.clear();
    assert_eq!(m.validate().unwrap_err(), ManifestError::MissingJobId);
    let mut m = sample();
    m.mats.clear();
    assert_eq!(m.validate().unwrap_err(), ManifestError::NoMats);
}

// ---- serialisation and the published schema ----

#[test]
fn round_trips_through_json() {
    let m = sample();
    let json = serde_json::to_string_pretty(&m).unwrap();
    assert_eq!(serde_json::from_str::<Manifest>(&json).unwrap(), m);
}

#[test]
fn refs_are_written_as_id_at_version_strings() {
    let v: serde_json::Value = serde_json::to_value(sample()).unwrap();
    assert_eq!(v["profile"], "generic.cutter-medium@1.0.0");
    assert_eq!(v["mats"][0]["material"], "community.cardstock-80lb@2.1.0");
    assert_eq!(
        v["mats"][0]["tools"][0]["tool"],
        "pen.fineliner-black-0.4@1.0.0"
    );
    assert_eq!(v["schema_version"], 1);
}

#[test]
fn the_published_schema_accepts_a_real_manifest_and_rejects_a_broken_one() {
    let schema = serde_json::to_value(schema()).unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    let validator = jsonschema::validator_for(&schema).unwrap();
    let good = serde_json::to_value(sample()).unwrap();
    assert!(validator.is_valid(&good));

    let mut missing = good.clone();
    missing.as_object_mut().unwrap().remove("job_id");
    assert!(!validator.is_valid(&missing));

    let mut wrong_type = good;
    wrong_type["schema_version"] = serde_json::json!("one");
    assert!(!validator.is_valid(&wrong_type));
}
