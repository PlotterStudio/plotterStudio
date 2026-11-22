//! Ground truth for the published schemas: `cargo xtask schemas` writes JSON Schema
//! files with stable `$id` URLs to `schemas/<kind>/v1.json`, and CI fails if the committed files
//! differ from the generated ones.

use std::path::Path;

use super::*;

#[test]
fn one_file_per_library_kind_under_schemas() {
    let files = generate();
    let names: Vec<String> = files
        .iter()
        .map(|f| f.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(
        names,
        vec![
            "schemas/tool/v1.json",
            "schemas/material/v1.json",
            "schemas/profile/v1.json"
        ]
    );
}

#[test]
fn each_schema_has_the_stable_id_the_published_url_names() {
    for file in generate() {
        let value: serde_json::Value = serde_json::from_str(&file.text).unwrap();
        let id = value["$id"].as_str().unwrap();
        let kind = file
            .path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            id,
            format!("https://plotter-studio.github.io/schemas/{kind}/v1.json")
        );
        assert_eq!(
            value["$schema"].as_str().map(|s| s.contains("2020-12")),
            Some(true),
            "{kind}: draft 2020-12"
        );
    }
}

#[test]
fn the_output_is_stable_and_ends_with_a_newline() {
    let (a, b) = (generate(), generate());
    assert_eq!(a, b);
    assert!(
        a.iter()
            .all(|f| f.text.ends_with('\n') && !f.text.ends_with("\n\n"))
    );
}

#[test]
fn a_schema_describes_the_fields_of_its_type() {
    let tool = &generate()[0];
    let value: serde_json::Value = serde_json::from_str(&tool.text).unwrap();
    let props = value["properties"].as_object().unwrap();
    for field in [
        "id",
        "version",
        "name",
        "family",
        "drive",
        "operations",
        "parameters",
    ] {
        assert!(props.contains_key(field), "{field}");
    }
}

#[test]
fn a_matching_tree_has_no_differences() {
    let dir = tempfile::tempdir().unwrap();
    write_all(dir.path(), &generate()).unwrap();
    assert_eq!(differences(dir.path(), &generate()), Vec::<String>::new());
}

#[test]
fn a_changed_missing_or_extra_file_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    write_all(dir.path(), &generate()).unwrap();
    std::fs::write(dir.path().join("schemas/tool/v1.json"), "{}\n").unwrap();
    std::fs::remove_file(dir.path().join("schemas/profile/v1.json")).unwrap();
    std::fs::write(dir.path().join("schemas/material/v2.json"), "{}\n").unwrap();
    let diffs = differences(dir.path(), &generate());
    assert_eq!(diffs.len(), 3, "{diffs:?}");
    assert!(
        diffs
            .iter()
            .any(|d| d.contains("tool/v1.json") && d.contains("differs"))
    );
    assert!(
        diffs
            .iter()
            .any(|d| d.contains("profile/v1.json") && d.contains("missing"))
    );
    assert!(
        diffs
            .iter()
            .any(|d| d.contains("material/v2.json") && d.contains("unexpected"))
    );
}

#[test]
fn the_committed_schemas_match_the_generated_ones() {
    // The repository root is the parent of this crate.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    assert_eq!(
        differences(root, &generate()),
        Vec::<String>::new(),
        "run `cargo xtask schemas` and commit the result"
    );
}
