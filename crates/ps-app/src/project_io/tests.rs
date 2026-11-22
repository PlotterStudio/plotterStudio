//! Ground truth for opening and saving projects from the app: the same `.pstudio` format
//! and safe save as everywhere else. A file written by a newer app opens read-only and is never
//! saved over; saving records the file in the recent list; the window title shows the project and
//! whether it has unsaved changes.

use ps_geometry::Point;
use ps_library::Settings;
use ps_model::{Project, ReadOnly};
use semver::Version;

use super::*;
use crate::commands::Mods;
use crate::state::AppState;
use crate::tools::CanvasEvent;

const NONE: Mods = Mods {
    command: false,
    shift: false,
    alt: false,
};

fn drawn() -> AppState {
    let mut s = AppState::new(Settings::default());
    s.run_command("tool.rectangle", 0);
    s.canvas_event(CanvasEvent::Press(Point::new(10.0, 10.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(Point::new(50.0, 40.0)), NONE, 0);
    s
}

#[test]
fn a_saved_project_opens_to_the_same_document() {
    let mut s = drawn();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("card.pstudio");
    save_project(&mut s, &path).unwrap();
    let opened = open_project(&path).unwrap();
    assert_eq!(opened.document, *s.document());
    assert!(opened.read_only.is_none());
}

#[test]
fn saving_marks_the_project_saved_remembers_the_path_and_updates_recent_files() {
    let mut s = drawn();
    assert!(s.is_dirty());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("card.pstudio");
    save_project(&mut s, &path).unwrap();
    assert!(!s.is_dirty());
    assert_eq!(s.project_path(), Some(path.as_path()));
    assert_eq!(s.settings().recent_files[0], path.to_string_lossy());
}

#[test]
fn the_title_shows_the_name_and_unsaved_changes() {
    let mut s = AppState::new(Settings::default());
    assert_eq!(s.window_title(), "Untitled — Plotter Studio");
    s.run_command("tool.rectangle", 0);
    s.canvas_event(CanvasEvent::Press(Point::new(0.0, 0.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(Point::new(20.0, 20.0)), NONE, 0);
    assert_eq!(s.window_title(), "Untitled* — Plotter Studio");
    let dir = tempfile::tempdir().unwrap();
    save_project(&mut s, &dir.path().join("card.pstudio")).unwrap();
    assert_eq!(s.window_title(), "card — Plotter Studio");
}

#[test]
fn a_second_save_keeps_the_previous_version_as_a_backup() {
    let mut s = drawn();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("card.pstudio");
    save_project(&mut s, &path).unwrap();
    s.run_command("nudge.right", 10);
    save_project(&mut s, &path).unwrap();
    assert!(dir.path().join("card.pstudio.bak").exists());
}

#[test]
fn opening_a_missing_or_damaged_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        open_project(&dir.path().join("nope.pstudio")),
        Err(ProjectIoError::Io(_))
    ));
    let bad = dir.path().join("bad.pstudio");
    std::fs::write(&bad, b"not a zip").unwrap();
    assert!(matches!(
        open_project(&bad),
        Err(ProjectIoError::Invalid(_))
    ));
}

#[test]
fn adopting_an_opened_project_replaces_the_state_and_tracks_its_file() {
    let mut s = drawn();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("card.pstudio");
    save_project(&mut s, &path).unwrap();

    let mut fresh = AppState::new(Settings::default());
    let opened = open_project(&path).unwrap();
    fresh.adopt_project(opened, &path);
    assert_eq!(*fresh.document(), *s.document());
    assert_eq!(fresh.project_path(), Some(path.as_path()));
    assert!(!fresh.is_dirty());
    assert!(!fresh.can_undo());
    assert_eq!(fresh.settings().recent_files[0], path.to_string_lossy());
}

#[test]
fn a_read_only_project_is_never_saved_over() {
    let mut s = drawn();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("card.pstudio");
    save_project(&mut s, &path).unwrap();
    let mut opened = open_project(&path).unwrap();
    opened.read_only = Some(ReadOnly::NewerSchema {
        found: 99,
        supported: 1,
    });
    let before = std::fs::read(&path).unwrap();
    let mut other = AppState::new(Settings::default());
    other.adopt_project(opened, &path);
    assert!(other.read_only().is_some());
    assert!(matches!(
        save_project(&mut other, &path),
        Err(ProjectIoError::ReadOnly)
    ));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "the file is untouched"
    );
}

#[test]
fn unknown_fields_survive_a_round_trip_through_the_app() {
    // The shell of the opened project (thumbnails, embedded files, unknown fields) is kept and
    // written back with the edited document.
    let mut s = drawn();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.pstudio");
    save_project(&mut s, &path).unwrap();
    let mut opened = open_project(&path).unwrap();
    opened.shell.template = true;
    let mut other = AppState::new(Settings::default());
    other.adopt_project(opened, &path);
    save_project(&mut other, &path).unwrap();
    let again = open_project(&path).unwrap();
    assert!(again.shell.template, "the template flag was kept");
}

#[test]
fn the_project_written_is_a_plain_pstudio_file() {
    let mut s = drawn();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("card.pstudio");
    save_project(&mut s, &path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let version = Version::new(0, 1, 0);
    let loaded = Project::from_bytes(&bytes, &version).unwrap();
    assert_eq!(loaded.project.document, *s.document());
}
