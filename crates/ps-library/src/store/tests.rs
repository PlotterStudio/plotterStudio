//! Ground truth for the library store: `builtin/` read-only, `installed/` by
//! id and version, `overlays/` per id with history, `calibration/` per machine, all written
//! atomically. An effective entry is builtin or installed plus its overlay.

use serde_json::json;

use super::*;
use crate::testdata::{MATERIAL, PEN, PROFILE, TOOL};

fn entry(kind: LibraryKind, text: &str) -> Entry {
    Entry::from_json(kind, text).unwrap()
}

fn tool_v(version: &str) -> Entry {
    let mut v: Value = serde_json::from_str(TOOL).unwrap();
    v["version"] = json!(version);
    entry(LibraryKind::Tool, &v.to_string())
}

fn open() -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path()).unwrap();
    (dir, lib)
}

#[test]
fn opening_creates_the_documented_layout() {
    let (dir, _lib) = open();
    for sub in [
        "library/builtin",
        "library/installed",
        "library/overlays",
        "library/rules",
        "library/calibration",
        "cache/textures",
    ] {
        assert!(dir.path().join(sub).is_dir(), "{sub}");
    }
}

#[test]
fn an_installed_entry_can_be_read_back() {
    let (_d, mut lib) = open();
    let outcome = lib.install(&tool_v("1.0.0"), None).unwrap();
    assert_eq!(outcome, Outcome::Installed);
    let e = lib
        .effective(LibraryKind::Tool, "generic.drag-blade")
        .unwrap()
        .unwrap();
    assert_eq!(e.version().to_string(), "1.0.0");
    assert!(lib.effective(LibraryKind::Tool, "nope").unwrap().is_none());
}

#[test]
fn installing_survives_reopening() {
    let dir = tempfile::tempdir().unwrap();
    Library::open(dir.path())
        .unwrap()
        .install(&tool_v("1.0.0"), None)
        .unwrap();
    let lib = Library::open(dir.path()).unwrap();
    assert!(
        lib.effective(LibraryKind::Tool, "generic.drag-blade")
            .unwrap()
            .is_some()
    );
}

#[test]
fn a_newer_version_updates_in_place_and_drops_the_old_one() {
    let (dir, mut lib) = open();
    lib.install(&tool_v("1.0.0"), None).unwrap();
    assert_eq!(
        lib.install(&tool_v("1.1.0"), None).unwrap(),
        Outcome::Updated
    );
    let e = lib
        .effective(LibraryKind::Tool, "generic.drag-blade")
        .unwrap()
        .unwrap();
    assert_eq!(e.version().to_string(), "1.1.0");
    let folder = dir
        .path()
        .join("library/installed/tools/generic.drag-blade");
    assert_eq!(fs::read_dir(folder).unwrap().count(), 1);
}

#[test]
fn the_same_version_needs_an_answer() {
    let (_d, mut lib) = open();
    lib.install(&tool_v("1.0.0"), None).unwrap();
    assert_eq!(
        lib.install(&tool_v("1.0.0"), None).unwrap_err(),
        StoreError::NeedsAnswer
    );
    assert_eq!(
        lib.install(&tool_v("0.9.0"), None).unwrap_err(),
        StoreError::NeedsAnswer
    );
}

#[test]
fn skip_leaves_everything_alone() {
    let (_d, mut lib) = open();
    lib.install(&tool_v("1.0.0"), None).unwrap();
    assert_eq!(
        lib.install(&tool_v("1.0.0"), Some(Choice::Skip)).unwrap(),
        Outcome::Skipped
    );
}

#[test]
fn replace_overwrites_an_older_or_equal_version() {
    let (_d, mut lib) = open();
    lib.install(&tool_v("2.0.0"), None).unwrap();
    assert_eq!(
        lib.install(&tool_v("1.5.0"), Some(Choice::Replace))
            .unwrap(),
        Outcome::Replaced
    );
    let e = lib
        .effective(LibraryKind::Tool, "generic.drag-blade")
        .unwrap()
        .unwrap();
    assert_eq!(e.version().to_string(), "1.5.0");
}

#[test]
fn keep_both_installs_a_copy_under_a_new_id() {
    let (_d, mut lib) = open();
    lib.install(&tool_v("1.0.0"), None).unwrap();
    let outcome = lib
        .install(&tool_v("1.0.0"), Some(Choice::KeepBoth))
        .unwrap();
    assert_eq!(
        outcome,
        Outcome::InstalledCopy("generic.drag-blade-copy".to_owned())
    );
    assert!(
        lib.effective(LibraryKind::Tool, "generic.drag-blade-copy")
            .unwrap()
            .is_some()
    );
    assert!(
        lib.effective(LibraryKind::Tool, "generic.drag-blade")
            .unwrap()
            .is_some()
    );
}

#[test]
fn builtin_entries_are_visible_and_read_only() {
    let (_d, mut lib) = open();
    lib.add_builtin(&entry(LibraryKind::Profile, PROFILE))
        .unwrap();
    let e = lib
        .effective(LibraryKind::Profile, "generic.cutter-small")
        .unwrap()
        .unwrap();
    assert_eq!(e.id(), "generic.cutter-small");
    assert_eq!(
        lib.uninstall(LibraryKind::Profile, "generic.cutter-small")
            .unwrap_err(),
        StoreError::ReadOnly
    );
}

#[test]
fn installed_entries_can_be_removed() {
    let (_d, mut lib) = open();
    lib.install(&tool_v("1.0.0"), None).unwrap();
    lib.uninstall(LibraryKind::Tool, "generic.drag-blade")
        .unwrap();
    assert!(
        lib.effective(LibraryKind::Tool, "generic.drag-blade")
            .unwrap()
            .is_none()
    );
}

#[test]
fn an_overlay_changes_the_effective_value_but_not_the_package() {
    let (_d, mut lib) = open();
    lib.install(&tool_v("1.0.0"), None).unwrap();
    let mut edited = serde_json::to_value(
        match lib
            .package(LibraryKind::Tool, "generic.drag-blade")
            .unwrap()
            .unwrap()
        {
            Entry::Tool(t) => t,
            _ => unreachable!(),
        },
    )
    .unwrap();
    edited["parameters"]["force"]["default"] = json!(420);
    lib.edit(
        LibraryKind::Tool,
        "generic.drag-blade",
        &edited,
        "t1",
        "ilya",
    )
    .unwrap();

    let Entry::Tool(eff) = lib
        .effective(LibraryKind::Tool, "generic.drag-blade")
        .unwrap()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(eff.parameters["force"].default, 420.0);
    let Entry::Tool(pkg) = lib
        .package(LibraryKind::Tool, "generic.drag-blade")
        .unwrap()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(pkg.parameters["force"].default, 350.0);
}

#[test]
fn reverting_returns_the_package_values() {
    let (_d, mut lib) = open();
    lib.install(&tool_v("1.0.0"), None).unwrap();
    let id = "generic.drag-blade";
    let mut edited =
        serde_json::to_value(match lib.package(LibraryKind::Tool, id).unwrap().unwrap() {
            Entry::Tool(t) => t,
            _ => unreachable!(),
        })
        .unwrap();
    edited["name"] = json!("My blade");
    lib.edit(LibraryKind::Tool, id, &edited, "t1", "ilya")
        .unwrap();
    assert!(lib.has_overlay(LibraryKind::Tool, id));
    lib.revert(LibraryKind::Tool, id, "t2", "ilya").unwrap();
    assert!(!lib.has_overlay(LibraryKind::Tool, id));
    let Entry::Tool(eff) = lib.effective(LibraryKind::Tool, id).unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(eff.name, "Fine-point blade (45°)");
}

#[test]
fn an_overlay_survives_a_package_update() {
    let (_d, mut lib) = open();
    let id = "generic.drag-blade";
    lib.install(&tool_v("1.0.0"), None).unwrap();
    let mut edited =
        serde_json::to_value(match lib.package(LibraryKind::Tool, id).unwrap().unwrap() {
            Entry::Tool(t) => t,
            _ => unreachable!(),
        })
        .unwrap();
    edited["parameters"]["force"]["default"] = json!(420);
    lib.edit(LibraryKind::Tool, id, &edited, "t1", "ilya")
        .unwrap();
    lib.install(&tool_v("1.1.0"), None).unwrap();
    let Entry::Tool(eff) = lib.effective(LibraryKind::Tool, id).unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(eff.version.to_string(), "1.1.0");
    assert_eq!(
        eff.parameters["force"].default, 420.0,
        "tuned value kept across the update"
    );
}

#[test]
fn an_overlay_that_breaks_the_entry_is_refused() {
    let (_d, mut lib) = open();
    let id = "generic.drag-blade";
    lib.install(&tool_v("1.0.0"), None).unwrap();
    let mut edited =
        serde_json::to_value(match lib.package(LibraryKind::Tool, id).unwrap().unwrap() {
            Entry::Tool(t) => t,
            _ => unreachable!(),
        })
        .unwrap();
    edited["parameters"]["force"]["default"] = json!(99_999);
    let err = lib
        .edit(LibraryKind::Tool, id, &edited, "t1", "ilya")
        .unwrap_err();
    assert!(matches!(err, StoreError::Invalid(_)), "{err:?}");
    assert!(!lib.has_overlay(LibraryKind::Tool, id));
}

#[test]
fn undo_steps_back_through_the_history() {
    let (_d, mut lib) = open();
    let id = "generic.drag-blade";
    lib.install(&tool_v("1.0.0"), None).unwrap();
    let base = serde_json::to_value(match lib.package(LibraryKind::Tool, id).unwrap().unwrap() {
        Entry::Tool(t) => t,
        _ => unreachable!(),
    })
    .unwrap();
    for (n, name) in ["A", "B"].into_iter().enumerate() {
        let mut e = base.clone();
        e["name"] = json!(name);
        lib.edit(LibraryKind::Tool, id, &e, &format!("t{n}"), "ilya")
            .unwrap();
    }
    assert!(lib.undo(LibraryKind::Tool, id).unwrap());
    let Entry::Tool(eff) = lib.effective(LibraryKind::Tool, id).unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(eff.name, "A");
}

#[test]
fn listing_shows_origin_and_overlay_state() {
    let (_d, mut lib) = open();
    lib.add_builtin(&entry(LibraryKind::Tool, PEN)).unwrap();
    lib.install(&tool_v("1.0.0"), None).unwrap();
    let list = lib.list(LibraryKind::Tool).unwrap();
    let ids: Vec<(&str, bool)> = list.iter().map(|s| (s.id.as_str(), s.builtin)).collect();
    assert_eq!(
        ids,
        [
            ("generic.drag-blade", false),
            ("pen.fineliner-black-0.4", true)
        ]
    );
    assert!(list.iter().all(|s| !s.has_overlay));
}

#[test]
fn an_installed_entry_shadows_a_builtin_of_the_same_id_when_newer() {
    let (_d, mut lib) = open();
    lib.add_builtin(&tool_v("1.0.0")).unwrap();
    lib.install(&tool_v("1.2.0"), None).unwrap();
    let e = lib
        .effective(LibraryKind::Tool, "generic.drag-blade")
        .unwrap()
        .unwrap();
    assert_eq!(e.version().to_string(), "1.2.0");
}

#[test]
fn calibration_is_stored_per_machine() {
    let (_d, lib) = open();
    assert_eq!(
        lib.calibration("generic.cutter-medium").unwrap(),
        Calibration::default()
    );
    let c = Calibration {
        time_scale: 1.07,
        ..Calibration::default()
    };
    lib.save_calibration("generic.cutter-medium", &c).unwrap();
    assert_eq!(
        lib.calibration("generic.cutter-medium").unwrap().time_scale,
        1.07
    );
    assert_eq!(
        lib.calibration("generic.cutter-large").unwrap(),
        Calibration::default()
    );
}

#[test]
fn bad_calibration_values_are_refused() {
    let (_d, lib) = open();
    let c = Calibration {
        force_scale: -1.0,
        ..Calibration::default()
    };
    assert!(matches!(
        lib.save_calibration("m", &c).unwrap_err(),
        StoreError::Invalid(_)
    ));
}

#[test]
fn materials_install_too() {
    let (_d, mut lib) = open();
    lib.install(&entry(LibraryKind::Material, MATERIAL), None)
        .unwrap();
    assert_eq!(lib.list(LibraryKind::Material).unwrap().len(), 1);
}

#[test]
fn unsafe_ids_cannot_escape_the_library_folder() {
    let (_d, lib) = open();
    assert!(
        lib.effective(LibraryKind::Tool, "../../etc/passwd")
            .is_err()
    );
}
