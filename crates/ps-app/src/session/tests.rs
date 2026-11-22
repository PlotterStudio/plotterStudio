//! Ground truth for autosave and crash recovery. Autosave every 30 s when the document
//! changed, and on every mode switch, into `<data>/autosave/<project-id>/`, keeping the last 5. A
//! session lock holds the process id and a clean-exit flag: a lock without a clean exit and with a
//! dead process means a crash, and the app offers the newest autosave. A panic writes the autosave
//! at once and a local crash report; nothing is uploaded.

use std::path::Path;

use ps_geometry::Point;
use ps_library::Settings;
use ps_model::ProcessProbe;

use super::*;
use crate::commands::Mods;
use crate::project_io::open_project;
use crate::state::AppState;
use crate::tools::CanvasEvent;

const NONE: Mods = Mods {
    command: false,
    shift: false,
    alt: false,
};

struct Alive(bool);

impl ProcessProbe for Alive {
    fn is_alive(&self, _pid: u32) -> bool {
        self.0
    }
}

fn drawn() -> AppState {
    let mut s = AppState::new(Settings::default());
    s.run_command("tool.rectangle", 0);
    s.canvas_event(CanvasEvent::Press(Point::new(10.0, 10.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(Point::new(50.0, 40.0)), NONE, 0);
    s
}

fn files_in(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, Iterator::count)
}

#[test]
fn the_first_run_has_nothing_to_recover() {
    let dir = tempfile::tempdir().unwrap();
    let (_, startup) = SessionGuard::start(dir.path(), &Alive(false), 100, 0);
    assert_eq!(startup, Startup::Fresh);
}

#[test]
fn a_clean_exit_leaves_nothing_to_recover_next_time() {
    let dir = tempfile::tempdir().unwrap();
    let (guard, _) = SessionGuard::start(dir.path(), &Alive(false), 100, 0);
    guard.finish();
    let (_, startup) = SessionGuard::start(dir.path(), &Alive(false), 101, 10);
    assert_eq!(startup, Startup::Fresh);
}

#[test]
fn a_crash_is_detected_and_the_newest_autosave_is_offered() {
    let dir = tempfile::tempdir().unwrap();
    let s = drawn();
    let (mut guard, _) = SessionGuard::start(dir.path(), &Alive(false), 100, 0);
    guard.tick(&s, 30, 40_000, false).unwrap();
    // The process dies without calling finish().
    drop(guard);
    let (_, startup) = SessionGuard::start(dir.path(), &Alive(false), 101, 50_000);
    let Startup::Crashed { autosave } = startup else {
        panic!("{startup:?}");
    };
    let entry = autosave.expect("an autosave to restore");
    let restored = open_project(&entry.path).unwrap();
    assert_eq!(restored.document, *s.document());
    assert!(entry.time_ms >= 40_000);
}

#[test]
fn a_crash_with_no_autosave_offers_nothing_to_restore() {
    let dir = tempfile::tempdir().unwrap();
    let (guard, _) = SessionGuard::start(dir.path(), &Alive(false), 100, 0);
    drop(guard);
    let (_, startup) = SessionGuard::start(dir.path(), &Alive(false), 101, 10);
    assert_eq!(startup, Startup::Crashed { autosave: None });
}

#[test]
fn a_lock_held_by_a_running_process_means_another_instance() {
    let dir = tempfile::tempdir().unwrap();
    let (guard, _) = SessionGuard::start(dir.path(), &Alive(true), 100, 0);
    drop(guard);
    let (_, startup) = SessionGuard::start(dir.path(), &Alive(true), 101, 10);
    assert_eq!(startup, Startup::AnotherInstance);
}

#[test]
fn an_unchanged_document_is_never_autosaved() {
    let dir = tempfile::tempdir().unwrap();
    let s = AppState::new(Settings::default());
    let (mut guard, _) = SessionGuard::start(dir.path(), &Alive(false), 100, 0);
    assert!(!guard.tick(&s, 30, 1_000_000, false).unwrap());
    assert_eq!(files_in(&dir.path().join("autosave")), 0);
}

#[test]
fn a_changed_document_is_autosaved_after_the_interval() {
    let dir = tempfile::tempdir().unwrap();
    let s = drawn();
    let (mut guard, _) = SessionGuard::start(dir.path(), &Alive(false), 100, 0);
    assert!(
        !guard.tick(&s, 30, 10_000, false).unwrap(),
        "only 10 s since the start"
    );
    assert!(guard.tick(&s, 30, 31_000, false).unwrap());
    assert!(
        !guard.tick(&s, 30, 32_000, false).unwrap(),
        "saved already; nothing new"
    );
}

#[test]
fn switching_mode_autosaves_a_changed_document_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let s = drawn();
    let (mut guard, _) = SessionGuard::start(dir.path(), &Alive(false), 100, 0);
    assert!(guard.tick(&s, 30, 1_000, true).unwrap());
}

#[test]
fn a_further_edit_is_autosaved_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = drawn();
    let (mut guard, _) = SessionGuard::start(dir.path(), &Alive(false), 100, 0);
    assert!(guard.tick(&s, 30, 31_000, false).unwrap());
    s.run_command("nudge.right", 40_000);
    assert!(guard.tick(&s, 30, 62_000, false).unwrap());
}

#[test]
fn only_the_last_five_autosaves_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = drawn();
    let (mut guard, _) = SessionGuard::start(dir.path(), &Alive(false), 100, 0);
    for i in 1..=8_u64 {
        s.run_command("nudge.right", i);
        assert!(guard.tick(&s, 30, i * 31_000, false).unwrap());
    }
    let project_dir = std::fs::read_dir(dir.path().join("autosave"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(files_in(&project_dir), 5);
}

#[test]
fn the_autosave_folder_follows_the_project_file() {
    assert_eq!(project_id(None), "untitled");
    let a = project_id(Some(Path::new("/projects/a.pstudio")));
    let b = project_id(Some(Path::new("/projects/b.pstudio")));
    assert_ne!(a, b);
    assert_eq!(a, project_id(Some(Path::new("/projects/a.pstudio"))));
    assert!(
        a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "{a}"
    );
}

#[test]
fn saving_properly_clears_the_projects_autosaves() {
    let dir = tempfile::tempdir().unwrap();
    let s = drawn();
    let (mut guard, _) = SessionGuard::start(dir.path(), &Alive(false), 100, 0);
    guard.tick(&s, 30, 31_000, false).unwrap();
    guard.saved(&s);
    let (_, startup) = {
        drop(guard);
        SessionGuard::start(dir.path(), &Alive(false), 101, 100_000)
    };
    assert_eq!(
        startup,
        Startup::Crashed { autosave: None },
        "nothing newer than the saved file"
    );
}

#[test]
fn a_panic_writes_the_autosave_and_a_local_report() {
    let dir = tempfile::tempdir().unwrap();
    let s = drawn();
    let (crashed, _) = SessionGuard::start(dir.path(), &Alive(false), 1, 1);
    drop(crashed);
    let snapshot = Snapshot::default();
    snapshot.update(&s);
    let report_path = write_panic_files(
        dir.path(),
        &snapshot,
        "index out of bounds",
        "frame 0\nframe 1",
        &["opened project".to_owned(), "compiled".to_owned()],
        "0.1.0",
        "windows",
        123_456,
    )
    .unwrap();
    let text = std::fs::read_to_string(&report_path).unwrap();
    assert!(text.contains("index out of bounds"));
    assert!(text.contains("never uploaded"));
    assert!(text.contains("compiled"));
    assert!(report_path.starts_with(dir.path().join("crash")));
    // The unsaved work was written as an autosave; the dead session left its lock behind.
    let (_, startup) = SessionGuard::start(dir.path(), &Alive(false), 2, 2);
    let Startup::Crashed {
        autosave: Some(entry),
    } = startup
    else {
        panic!("{startup:?}");
    };
    assert_eq!(open_project(&entry.path).unwrap().document, *s.document());
}

#[test]
fn a_panic_before_anything_is_open_still_writes_the_report() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot = Snapshot::default();
    let path =
        write_panic_files(dir.path(), &snapshot, "boom", "", &[], "0.1.0", "linux", 1).unwrap();
    assert!(path.exists());
}
