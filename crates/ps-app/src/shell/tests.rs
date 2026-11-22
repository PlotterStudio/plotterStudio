//! Ground truth for the shell layer: what happens when a command asks for a dialog or a file. The
//! dialogs are a trait, so these tests answer them with a script instead of opening windows.

use std::path::PathBuf;
use std::time::Duration;

use ps_geometry::Point;
use ps_library::Settings;
use ps_model::OpKind;

use super::*;
use crate::commands::Mods;
use crate::tools::CanvasEvent;
use crate::ui::App;

const NONE: Mods = Mods {
    command: false,
    shift: false,
    alt: false,
};

#[derive(Default)]
struct Script {
    open: Option<PathBuf>,
    import: Option<PathBuf>,
    save: Option<PathBuf>,
    folder: Option<PathBuf>,
    package: Option<PathBuf>,
    asked: Vec<&'static str>,
}

impl Dialogs for Script {
    fn open_project(&mut self) -> Option<PathBuf> {
        self.asked.push("open");
        self.open.take()
    }
    fn import_drawing(&mut self) -> Option<PathBuf> {
        self.asked.push("import");
        self.import.take()
    }
    fn save_project(&mut self, _suggested: &str) -> Option<PathBuf> {
        self.asked.push("save");
        self.save.take()
    }
    fn pick_folder(&mut self) -> Option<PathBuf> {
        self.asked.push("folder");
        self.folder.take()
    }
    fn pick_package(&mut self) -> Option<PathBuf> {
        self.asked.push("package");
        self.package.take()
    }
}

fn app() -> App {
    App::new(Settings::default(), "en")
}

fn draw(a: &mut App) {
    a.state_mut().run_command("tool.rectangle", 0);
    a.state_mut()
        .canvas_event(CanvasEvent::Press(Point::new(20.0, 20.0)), NONE, 0);
    a.state_mut()
        .canvas_event(CanvasEvent::Release(Point::new(80.0, 60.0)), NONE, 0);
}

fn compiled() -> App {
    let mut a = app();
    draw(&mut a);
    a.state_mut()
        .assign_operation(OpKind::Cut, "generic.drag-blade@1.0.0", 0);
    a.state_mut().tick(100);
    assert!(a.state_mut().wait_for_compile(Duration::from_secs(60)));
    a
}

fn objects(a: &App) -> usize {
    a.state().current_mat().objects.len()
}

#[test]
fn a_new_project_replaces_the_document() {
    let mut a = app();
    draw(&mut a);
    assert_eq!(objects(&a), 1);
    handle_effect(&mut a, Effect::NewProject, &mut Script::default(), 0);
    assert_eq!(objects(&a), 0);
    assert!(!a.state().can_undo());
}

#[test]
fn saving_an_untitled_project_asks_where_then_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("card.pstudio");
    let mut a = app();
    draw(&mut a);
    let mut d = Script {
        save: Some(path.clone()),
        ..Script::default()
    };
    handle_effect(&mut a, Effect::Save, &mut d, 0);
    assert_eq!(d.asked, vec!["save"]);
    assert!(path.exists());
    assert!(!a.state().is_dirty());
}

#[test]
fn saving_a_project_with_a_file_does_not_ask() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("card.pstudio");
    let mut a = app();
    draw(&mut a);
    let mut d = Script {
        save: Some(path),
        ..Script::default()
    };
    handle_effect(&mut a, Effect::Save, &mut d, 0);
    a.state_mut().run_command("nudge.right", 5);
    let mut d2 = Script::default();
    handle_effect(&mut a, Effect::Save, &mut d2, 0);
    assert!(d2.asked.is_empty(), "{:?}", d2.asked);
    assert!(!a.state().is_dirty());
}

#[test]
fn save_as_always_asks() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = app();
    draw(&mut a);
    let first = dir.path().join("a.pstudio");
    handle_effect(
        &mut a,
        Effect::Save,
        &mut Script {
            save: Some(first),
            ..Script::default()
        },
        0,
    );
    let second = dir.path().join("b.pstudio");
    let mut d = Script {
        save: Some(second.clone()),
        ..Script::default()
    };
    handle_effect(&mut a, Effect::SaveAs, &mut d, 0);
    assert_eq!(d.asked, vec!["save"]);
    assert!(second.exists());
    assert_eq!(a.state().project_path(), Some(second.as_path()));
}

#[test]
fn cancelling_the_save_dialog_changes_nothing() {
    let mut a = app();
    draw(&mut a);
    handle_effect(&mut a, Effect::Save, &mut Script::default(), 0);
    assert!(a.state().is_dirty());
    assert!(a.state().project_path().is_none());
}

#[test]
fn opening_a_project_loads_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("card.pstudio");
    let mut a = app();
    draw(&mut a);
    handle_effect(
        &mut a,
        Effect::Save,
        &mut Script {
            save: Some(path.clone()),
            ..Script::default()
        },
        0,
    );
    let mut b = app();
    handle_effect(
        &mut b,
        Effect::OpenProject,
        &mut Script {
            open: Some(path),
            ..Script::default()
        },
        0,
    );
    assert_eq!(objects(&b), 1);
    assert!(!b.state().is_dirty());
}

#[test]
fn opening_a_bad_file_leaves_the_project_alone_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.pstudio");
    std::fs::write(&bad, b"nonsense").unwrap();
    let mut a = app();
    draw(&mut a);
    handle_effect(
        &mut a,
        Effect::OpenProject,
        &mut Script {
            open: Some(bad),
            ..Script::default()
        },
        0,
    );
    assert_eq!(objects(&a), 1, "the project was not replaced");
    let notice = a.notice.clone().expect("a notice");
    assert!(notice.contains("not a valid project"), "{notice}");
}

#[test]
fn exporting_asks_for_a_folder_and_reports_what_was_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = compiled();
    let mut d = Script {
        folder: Some(dir.path().to_path_buf()),
        ..Script::default()
    };
    handle_effect(&mut a, Effect::Export, &mut d, 1_700_000_000);
    assert_eq!(d.asked, vec!["folder"]);
    let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(files.len(), 2, "a bundle and one G-code file");
    assert_eq!(a.export_status.as_deref(), Some("Exported 2 files"));
}

#[test]
fn the_export_is_stamped_with_the_time_it_happens() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = compiled();
    handle_effect(
        &mut a,
        Effect::Export,
        &mut Script {
            folder: Some(dir.path().to_path_buf()),
            ..Script::default()
        },
        1_700_000_000,
    );
    let bundle = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "psjob"))
        .unwrap();
    let opened = ps_job::Bundle::from_bytes(&std::fs::read(bundle).unwrap()).unwrap();
    assert_eq!(opened.manifest.export_time, "2023-11-14T22:13:20Z");
}

#[test]
fn cancelling_the_folder_dialog_exports_nothing() {
    let mut a = compiled();
    handle_effect(&mut a, Effect::Export, &mut Script::default(), 0);
    assert!(a.export_status.is_none());
}

#[test]
fn a_failed_export_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = compiled();
    a.state_mut().run_command("nudge.right", 9); // the job no longer matches
    handle_effect(
        &mut a,
        Effect::Export,
        &mut Script {
            folder: Some(dir.path().to_path_buf()),
            ..Script::default()
        },
        0,
    );
    // The command is refused before the shell sees it, so call the export directly too.
    let result = export_to_folder(&mut a, dir.path(), 0);
    assert!(result.is_err());
    assert!(
        a.export_status
            .as_deref()
            .unwrap()
            .starts_with("Export failed:")
    );
}

#[test]
fn the_export_stem_is_the_project_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("birthday.pstudio");
    let mut a = compiled();
    handle_effect(
        &mut a,
        Effect::Save,
        &mut Script {
            save: Some(path),
            ..Script::default()
        },
        0,
    );
    let out = tempfile::tempdir().unwrap();
    handle_effect(
        &mut a,
        Effect::Export,
        &mut Script {
            folder: Some(out.path().to_path_buf()),
            ..Script::default()
        },
        0,
    );
    assert!(out.path().join("birthday.psjob").exists());
}

#[test]
fn sending_to_a_machine_is_reported_as_not_available_without_a_connection() {
    let mut a = compiled();
    handle_effect(&mut a, Effect::SendToMachine, &mut Script::default(), 0);
    assert_eq!(
        a.notice.as_deref(),
        Some("Connect a machine in the machine panel first")
    );
}

// ---- import ----

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200mm" height="200mm" viewBox="0 0 200 200"><rect x="10" y="10" width="40" height="30" fill="none" stroke="#FF0000" stroke-width="0.5"/></svg>"##;

#[test]
fn importing_asks_for_a_drawing_and_adds_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("card.svg");
    std::fs::write(&path, SVG).unwrap();
    let mut a = app();
    let mut d = Script {
        import: Some(path),
        ..Script::default()
    };
    handle_effect(&mut a, Effect::Import, &mut d, 0);
    assert_eq!(d.asked, vec!["import"]);
    assert_eq!(objects(&a), 1);
    assert_eq!(a.notice.as_deref(), Some("Imported card.svg: 1 object"));
}

#[test]
fn cancelling_the_import_dialog_changes_nothing() {
    let mut a = app();
    handle_effect(&mut a, Effect::Import, &mut Script::default(), 0);
    assert_eq!(objects(&a), 0);
    assert!(a.notice.is_none());
}

#[test]
fn a_file_that_cannot_be_imported_is_reported_and_leaves_the_project_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.txt");
    std::fs::write(&path, "hello").unwrap();
    let mut a = app();
    handle_effect(
        &mut a,
        Effect::Import,
        &mut Script {
            import: Some(path),
            ..Script::default()
        },
        0,
    );
    assert_eq!(objects(&a), 0);
    assert!(a.notice.as_deref().unwrap().starts_with("Could not import"));
}

// ---- installing a material package ----

fn material_package() -> Vec<u8> {
    use ps_library::{Entry, LibraryInfo, builtin_entries, export_package};
    let mut m = builtin_entries()
        .unwrap()
        .into_iter()
        .find_map(|e| match e {
            Entry::Material(m) => Some(m),
            _ => None,
        })
        .unwrap();
    m.id = "acme.glitter".to_owned();
    m.name = "Glitter card".to_owned();
    let info = LibraryInfo {
        schema_version: 1,
        id: "acme.pack".into(),
        name: "Acme".into(),
        version: "1.0.0".into(),
        license: "CC0-1.0".into(),
    };
    export_package(&info, &[(m, std::collections::BTreeMap::new())]).unwrap()
}

#[test]
fn installing_a_package_asks_for_a_file_and_adds_its_materials() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("glitter.psmaterials");
    std::fs::write(&file, material_package()).unwrap();
    let mut a = app();
    a.open_library(&dir.path().join("lib")).unwrap();
    let mut d = Script {
        package: Some(file),
        ..Script::default()
    };
    handle_effect(&mut a, Effect::InstallPackage, &mut d, 0);
    assert_eq!(d.asked, vec!["package"]);
    assert!(
        a.state()
            .material_choices()
            .iter()
            .any(|m| m.name == "Glitter card")
    );
}

#[test]
fn cancelling_the_package_dialog_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = app();
    a.open_library(dir.path()).unwrap();
    let before = a.state().material_choices().len();
    handle_effect(&mut a, Effect::InstallPackage, &mut Script::default(), 0);
    assert_eq!(a.state().material_choices().len(), before);
}

#[test]
fn an_unreadable_package_file_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = app();
    a.open_library(&dir.path().join("lib")).unwrap();
    let mut d = Script {
        package: Some(dir.path().join("missing.psmaterials")),
        ..Script::default()
    };
    handle_effect(&mut a, Effect::InstallPackage, &mut d, 0);
    assert!(
        a.notice
            .as_deref()
            .is_some_and(|n| n.contains("missing.psmaterials"))
    );
}
