//! Ground truth for the library manager: the on-disk library holds the built-in
//! entries read-only; a material package the user installs is validated, offered conflict choices,
//! and shows up in the snapshot the app compiles against.

use std::collections::BTreeMap;

use ps_library::{Entry, LibraryInfo, Material, builtin_entries, export_package};

use super::*;

fn material(id: &str, version: &str, name: &str) -> Material {
    let base = builtin_entries()
        .unwrap()
        .into_iter()
        .find_map(|e| match e {
            Entry::Material(m) => Some(m),
            _ => None,
        })
        .unwrap();
    let mut m = base;
    m.id = id.to_owned();
    m.version = version.parse().unwrap();
    m.name = name.to_owned();
    m
}

fn package(materials: &[Material], licence: &str) -> Vec<u8> {
    let info = LibraryInfo {
        schema_version: 1,
        id: "acme.pack".into(),
        name: "Acme pack".into(),
        version: "1.0.0".into(),
        license: licence.into(),
    };
    let items: Vec<_> = materials
        .iter()
        .map(|m| (m.clone(), BTreeMap::new()))
        .collect();
    export_package(&info, &items).unwrap()
}

fn manager() -> (tempfile::TempDir, LibraryManager) {
    let dir = tempfile::tempdir().unwrap();
    let m = LibraryManager::open(dir.path()).unwrap();
    (dir, m)
}

#[test]
fn a_new_library_holds_the_built_in_entries() {
    let (_dir, m) = manager();
    let tools = m.rows(LibraryKind::Tool);
    assert!(
        tools
            .iter()
            .any(|r| r.id == "generic.drag-blade" && r.builtin)
    );
    assert!(
        m.rows(LibraryKind::Profile)
            .iter()
            .any(|r| r.id == "generic.cutter-small")
    );
    assert!(m.rows(LibraryKind::Material).iter().all(|r| r.builtin));
    assert_eq!(
        m.snapshot().unwrap().materials().len(),
        2,
        "the two shipped materials"
    );
}

#[test]
fn opening_twice_does_not_duplicate_or_lose_anything() {
    let dir = tempfile::tempdir().unwrap();
    let first = LibraryManager::open(dir.path())
        .unwrap()
        .rows(LibraryKind::Tool);
    let second = LibraryManager::open(dir.path())
        .unwrap()
        .rows(LibraryKind::Tool);
    assert_eq!(first, second);
}

#[test]
fn rows_are_sorted_by_name() {
    let (_dir, m) = manager();
    let names: Vec<String> = m
        .rows(LibraryKind::Tool)
        .into_iter()
        .map(|r| r.name)
        .collect();
    let mut sorted = names.clone();
    sorted.sort_by_key(|n| n.to_lowercase());
    assert_eq!(names, sorted);
}

#[test]
fn installing_a_package_adds_its_materials_to_the_snapshot() {
    let (_dir, mut m) = manager();
    let bytes = package(
        &[material("acme.glitter", "1.0.0", "Glitter card")],
        "CC0-1.0",
    );
    let report = m.install_package(&bytes, None, false);
    assert_eq!(report.installed, vec!["acme.glitter".to_owned()]);
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert!(
        m.rows(LibraryKind::Material)
            .iter()
            .any(|r| r.id == "acme.glitter" && !r.builtin)
    );
    assert!(
        m.snapshot()
            .unwrap()
            .materials()
            .iter()
            .any(|x| x.id == "acme.glitter")
    );
}

#[test]
fn installing_the_same_version_again_asks_what_to_do() {
    let (_dir, mut m) = manager();
    let bytes = package(
        &[material("acme.glitter", "1.0.0", "Glitter card")],
        "CC0-1.0",
    );
    m.install_package(&bytes, None, false);
    let again = m.install_package(&bytes, None, false);
    assert_eq!(again.installed, Vec::<String>::new());
    assert_eq!(again.needs_answer, vec!["acme.glitter".to_owned()]);
    let keep = m.install_package(&bytes, Some(Choice::KeepBoth), false);
    assert_eq!(keep.installed.len(), 1);
    assert_ne!(keep.installed[0], "acme.glitter");
    let skip = m.install_package(&bytes, Some(Choice::Skip), false);
    assert!(skip.installed.is_empty() && skip.needs_answer.is_empty());
}

#[test]
fn a_newer_version_updates_without_asking() {
    let (_dir, mut m) = manager();
    m.install_package(
        &package(&[material("acme.glitter", "1.0.0", "Glitter")], "CC0-1.0"),
        None,
        false,
    );
    let report = m.install_package(
        &package(
            &[material("acme.glitter", "1.1.0", "Glitter v2")],
            "CC0-1.0",
        ),
        None,
        false,
    );
    assert_eq!(report.installed, vec!["acme.glitter".to_owned()]);
    let row = m
        .rows(LibraryKind::Material)
        .into_iter()
        .find(|r| r.id == "acme.glitter")
        .unwrap();
    assert_eq!(row.version.to_string(), "1.1.0");
    assert_eq!(row.name, "Glitter v2");
}

#[test]
fn a_package_that_is_not_one_reports_a_problem_and_installs_nothing() {
    let (_dir, mut m) = manager();
    let report = m.install_package(b"not a zip", None, false);
    assert_eq!(report.installed, Vec::<String>::new());
    assert_eq!(report.problems.len(), 1);
}

#[test]
fn a_package_with_an_invalid_material_installs_nothing_unless_partial_is_allowed() {
    let (_dir, mut m) = manager();
    let good = material("acme.good", "1.0.0", "Good");
    let mut bad = material("acme.bad", "1.0.0", "Bad");
    bad.thickness_mm = Some(0.0);
    let bytes = package(&[good, bad], "CC0-1.0");
    let strict = m.install_package(&bytes, None, false);
    assert!(strict.installed.is_empty(), "{strict:?}");
    assert_ne!(strict.problems, Vec::<String>::new());
    let partial = m.install_package(&bytes, None, true);
    assert_eq!(partial.installed, vec!["acme.good".to_owned()]);
    assert!(
        !partial.problems.is_empty(),
        "the bad one is still reported"
    );
}

#[test]
fn an_installed_entry_can_be_removed_but_a_built_in_one_cannot() {
    let (_dir, mut m) = manager();
    m.install_package(
        &package(&[material("acme.glitter", "1.0.0", "Glitter")], "CC0-1.0"),
        None,
        false,
    );
    m.uninstall(LibraryKind::Material, "acme.glitter").unwrap();
    assert!(
        m.rows(LibraryKind::Material)
            .iter()
            .all(|r| r.id != "acme.glitter")
    );
    let error = m
        .uninstall(LibraryKind::Tool, "generic.drag-blade")
        .unwrap_err();
    assert!(error.contains("built-in"), "{error}");
}

#[test]
fn the_library_survives_being_reopened() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = LibraryManager::open(dir.path()).unwrap();
    m.install_package(
        &package(&[material("acme.glitter", "1.0.0", "Glitter")], "CC0-1.0"),
        None,
        false,
    );
    let reopened = LibraryManager::open(dir.path()).unwrap();
    assert!(
        reopened
            .rows(LibraryKind::Material)
            .iter()
            .any(|r| r.id == "acme.glitter")
    );
}
