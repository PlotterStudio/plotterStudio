//! Ground truth for the library snapshot: references resolve by id and
//! version; a missing version falls back to the newest installed one with a warning; a missing
//! id blocks export; local search; the pen inventory.

use serde_json::json;

use super::*;
use crate::testdata::{MATERIAL, PEN, PROFILE, TOOL};

fn entry(kind: LibraryKind, text: &str) -> Entry {
    Entry::from_json(kind, text).unwrap()
}

fn tool_with(version: &str, name: &str) -> Entry {
    let mut v: serde_json::Value = serde_json::from_str(TOOL).unwrap();
    v["version"] = json!(version);
    v["name"] = json!(name);
    entry(LibraryKind::Tool, &v.to_string())
}

fn snap() -> LibrarySnapshot {
    LibrarySnapshot::from_entries([
        tool_with("1.0.0", "Fine-point blade"),
        tool_with("1.2.0", "Fine-point blade v1.2"),
        entry(LibraryKind::Tool, PEN),
        entry(LibraryKind::Material, MATERIAL),
        entry(LibraryKind::Profile, PROFILE),
    ])
}

fn r(text: &str) -> LibRef {
    text.parse().unwrap()
}

#[test]
fn an_exact_reference_resolves_without_warning() {
    let s = snap();
    let found = s.resolve_tool(&r("generic.drag-blade@1.0.0")).unwrap();
    assert_eq!(found.item.name, "Fine-point blade");
    assert_eq!(found.warning, None);
}

#[test]
fn a_missing_version_falls_back_to_the_newest_with_a_warning() {
    let s = snap();
    let found = s.resolve_tool(&r("generic.drag-blade@0.9.0")).unwrap();
    assert_eq!(found.item.version.to_string(), "1.2.0");
    assert_eq!(
        found.warning,
        Some(ResolveWarning::VersionSubstituted {
            wanted: r("generic.drag-blade@0.9.0"),
            used: r("generic.drag-blade@1.2.0"),
        })
    );
}

#[test]
fn a_missing_id_is_an_error_that_blocks_export() {
    let s = snap();
    let err = s.resolve_tool(&r("nobody.nothing@1.0.0")).unwrap_err();
    assert_eq!(err, ResolveError::Missing(r("nobody.nothing@1.0.0")));
    assert!(err.to_string().contains("nobody.nothing"));
}

#[test]
fn materials_and_profiles_resolve_the_same_way() {
    let s = snap();
    assert_eq!(
        s.resolve_material(&r("community.cardstock-80lb@2.1.0"))
            .unwrap()
            .warning,
        None
    );
    assert_eq!(
        s.resolve_profile(&r("generic.cutter-small@1.0.0"))
            .unwrap()
            .item
            .name,
        "Generic Cutter S"
    );
    assert!(
        s.resolve_profile(&r("generic.cutter-small@9.0.0"))
            .unwrap()
            .warning
            .is_some()
    );
    assert!(s.resolve_material(&r("none@1.0.0")).is_err());
}

#[test]
fn the_kind_matters() {
    let s = snap();
    // A tool id is not a material id.
    assert!(s.resolve_material(&r("generic.drag-blade@1.0.0")).is_err());
}

#[test]
fn listing_returns_the_newest_of_each_id() {
    let s = snap();
    let tools = s.tools();
    let ids: Vec<(&str, String)> = tools
        .iter()
        .map(|t| (t.id.as_str(), t.version.to_string()))
        .collect();
    assert_eq!(
        ids,
        [
            ("generic.drag-blade", "1.2.0".to_owned()),
            ("pen.fineliner-black-0.4", "1.0.0".to_owned())
        ]
    );
}

#[test]
fn search_matches_id_name_brand_and_category_ignoring_case() {
    let s = snap();
    assert_eq!(
        s.search(LibraryKind::Tool, "FINE"),
        ["generic.drag-blade", "pen.fineliner-black-0.4"]
    );
    assert_eq!(
        s.search(LibraryKind::Tool, "liner"),
        ["pen.fineliner-black-0.4"]
    );
    assert_eq!(
        s.search(LibraryKind::Material, "paper"),
        ["community.cardstock-80lb"]
    );
    assert_eq!(
        s.search(LibraryKind::Material, "generic"),
        ["community.cardstock-80lb"]
    );
    assert_eq!(
        s.search(LibraryKind::Material, "Karton"),
        ["community.cardstock-80lb"]
    ); // translated name
    assert_eq!(s.search(LibraryKind::Material, "vinyl").len(), 0);
}

#[test]
fn an_empty_query_lists_everything() {
    let s = snap();
    assert_eq!(s.search(LibraryKind::Tool, "").len(), 2);
    assert_eq!(s.search(LibraryKind::Tool, "   ").len(), 2);
}

#[test]
fn only_pens_marked_owned_are_in_the_inventory() {
    let mut owned: serde_json::Value = serde_json::from_str(PEN).unwrap();
    owned["pen"]["owned"] = json!(true);
    let s = LibrarySnapshot::from_entries([
        entry(LibraryKind::Tool, PEN),
        tool_with("1.0.0", "Blade"),
        {
            let mut other = owned.clone();
            other["id"] = json!("pen.gel-red");
            other["pen"]["colour"] = json!({"hex": "#C42B2B", "name": "Red"});
            entry(LibraryKind::Tool, &other.to_string())
        },
    ]);
    let pens = s.owned_pens();
    assert_eq!(pens.len(), 1);
    assert_eq!(pens[0].id, "pen.gel-red");
}

#[test]
fn a_snapshot_is_cheap_to_share_between_threads() {
    fn needs_send_sync<T: Send + Sync>() {}
    needs_send_sync::<LibrarySnapshot>();
    let s = snap();
    let a = s.resolve_tool(&r("generic.drag-blade@1.0.0")).unwrap();
    let b = s.resolve_tool(&r("generic.drag-blade@1.0.0")).unwrap();
    assert!(Arc::ptr_eq(&a.item, &b.item));
}

#[test]
fn a_library_builds_a_snapshot_with_overlays_applied() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = Library::open(dir.path()).unwrap();
    lib.install(&tool_with("1.0.0", "Blade"), None).unwrap();
    let id = "generic.drag-blade";
    let mut edited =
        serde_json::to_value(match lib.package(LibraryKind::Tool, id).unwrap().unwrap() {
            Entry::Tool(t) => t,
            _ => unreachable!(),
        })
        .unwrap();
    edited["parameters"]["force"]["default"] = json!(111);
    lib.edit(LibraryKind::Tool, id, &edited, "t", "w").unwrap();
    let s = lib.snapshot().unwrap();
    let found = s.resolve_tool(&r("generic.drag-blade@1.0.0")).unwrap();
    assert_eq!(found.item.parameters["force"].default, 111.0);
}
