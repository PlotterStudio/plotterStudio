//! Ground truth for autosave and crash recovery. Autosave every 30 s when the document
//! changed, and on every mode switch, into `<data>/autosave/<project-id>/`, keeping the last 5. A
//! session lock file holds the process id and a clean-exit flag; on start, a lock without the flag
//! and with a dead process means a crash. A panic writes a local crash report and nothing is
//! uploaded.

use std::fs;
use std::path::Path;

use super::*;

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

// ---- when to autosave ----

#[test]
fn nothing_is_saved_when_nothing_changed() {
    let clock = AutosaveClock {
        last_ms: 0,
        dirty: false,
    };
    assert!(!clock.due(1_000_000, 30_000, false));
}

#[test]
fn a_changed_document_is_saved_once_the_interval_has_passed() {
    let clock = AutosaveClock {
        last_ms: 10_000,
        dirty: true,
    };
    assert!(!clock.due(39_999, 30_000, false));
    assert!(clock.due(40_000, 30_000, false));
    assert!(clock.due(90_000, 30_000, false));
}

#[test]
fn a_mode_switch_saves_a_changed_document_at_once() {
    let clock = AutosaveClock {
        last_ms: 10_000,
        dirty: true,
    };
    assert!(clock.due(10_001, 30_000, true));
    let clean = AutosaveClock {
        last_ms: 10_000,
        dirty: false,
    };
    assert!(!clean.due(10_001, 30_000, true), "still nothing to save");
}

#[test]
fn saving_resets_the_clock_and_editing_dirties_it() {
    let mut clock = AutosaveClock {
        last_ms: 0,
        dirty: true,
    };
    clock.saved(50_000);
    assert_eq!(
        clock,
        AutosaveClock {
            last_ms: 50_000,
            dirty: false
        }
    );
    clock.edited();
    assert!(clock.dirty);
}

// ---- autosave files ----

fn saver(dir: &Path) -> Autosaver {
    Autosaver::new(dir, 5)
}

#[test]
fn an_autosave_goes_into_a_folder_for_its_project() {
    let dir = tempfile::tempdir().unwrap();
    let path = saver(dir.path())
        .write("proj-1", 1_700_000_000_000, b"data")
        .unwrap();
    assert!(path.starts_with(dir.path().join("autosave").join("proj-1")));
    assert_eq!(fs::read(&path).unwrap(), b"data");
    assert_eq!(path.extension().unwrap(), "pstudio");
}

#[test]
fn only_the_newest_five_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let s = saver(dir.path());
    for i in 0..8_u64 {
        s.write("p", 1_000 + i, format!("v{i}").as_bytes()).unwrap();
    }
    let list = s.list("p");
    assert_eq!(list.len(), 5);
    let times: Vec<u64> = list.iter().map(|e| e.time_ms).collect();
    assert_eq!(
        times,
        vec![1_007, 1_006, 1_005, 1_004, 1_003],
        "newest first"
    );
    assert_eq!(fs::read(&list[0].path).unwrap(), b"v7");
}

#[test]
fn the_latest_autosave_is_the_newest() {
    let dir = tempfile::tempdir().unwrap();
    let s = saver(dir.path());
    assert!(s.latest("p").is_none());
    s.write("p", 5, b"a").unwrap();
    s.write("p", 9, b"b").unwrap();
    s.write("p", 7, b"c").unwrap();
    assert_eq!(s.latest("p").unwrap().time_ms, 9);
}

#[test]
fn projects_do_not_share_autosaves() {
    let dir = tempfile::tempdir().unwrap();
    let s = saver(dir.path());
    s.write("one", 1, b"1").unwrap();
    s.write("two", 2, b"2").unwrap();
    assert_eq!(s.list("one").len(), 1);
    assert_eq!(s.list("two").len(), 1);
    s.clear("one").unwrap();
    assert_eq!(s.list("one").len(), 0);
    assert_eq!(s.list("two").len(), 1);
}

#[test]
fn a_project_id_cannot_climb_out_of_the_autosave_folder() {
    let dir = tempfile::tempdir().unwrap();
    let s = saver(dir.path());
    for bad in ["../evil", "a/b", "", "..", "a\\b"] {
        assert!(s.write(bad, 1, b"x").is_err(), "{bad:?}");
    }
}

#[test]
fn foreign_files_in_the_folder_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let s = saver(dir.path());
    s.write("p", 3, b"x").unwrap();
    fs::write(
        dir.path().join("autosave").join("p").join("notes.txt"),
        "hi",
    )
    .unwrap();
    assert_eq!(s.list("p").len(), 1);
}

#[test]
fn listing_a_project_with_no_autosaves_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(saver(dir.path()).list("never").len(), 0);
}

// ---- session lock ----

struct Probe(&'static [u32]);

impl ProcessProbe for Probe {
    fn is_alive(&self, pid: u32) -> bool {
        self.0.contains(&pid)
    }
}

#[test]
fn no_lock_file_means_a_fresh_start() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        inspect(&dir.path().join("session.lock"), &Probe(&[])),
        Recovery::Fresh
    );
}

#[test]
fn a_lock_with_a_dead_process_and_no_clean_exit_is_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let lock = dir.path().join("session.lock");
    acquire(&lock, 4242, 1_000).unwrap();
    match inspect(&lock, &Probe(&[1, 2, 3])) {
        Recovery::Crashed(info) => {
            assert_eq!(info.pid, 4242);
            assert_eq!(info.started_ms, 1_000);
            assert!(!info.clean_exit);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_lock_whose_process_is_still_running_is_another_instance() {
    let dir = tempfile::tempdir().unwrap();
    let lock = dir.path().join("session.lock");
    acquire(&lock, 4242, 1_000).unwrap();
    assert!(matches!(inspect(&lock, &Probe(&[4242])), Recovery::Running(i) if i.pid == 4242));
}

#[test]
fn a_clean_exit_is_not_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let lock = dir.path().join("session.lock");
    acquire(&lock, 4242, 1_000).unwrap();
    mark_clean_exit(&lock).unwrap();
    assert_eq!(inspect(&lock, &Probe(&[])), Recovery::CleanExit);
}

#[test]
fn a_corrupt_lock_is_treated_as_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let lock = dir.path().join("session.lock");
    fs::write(&lock, "{ half a file").unwrap();
    assert!(matches!(inspect(&lock, &Probe(&[])), Recovery::Crashed(_)));
}

#[test]
fn acquiring_replaces_an_old_lock() {
    let dir = tempfile::tempdir().unwrap();
    let lock = dir.path().join("session.lock");
    acquire(&lock, 1, 10).unwrap();
    mark_clean_exit(&lock).unwrap();
    acquire(&lock, 2, 20).unwrap();
    assert!(
        matches!(inspect(&lock, &Probe(&[])), Recovery::Crashed(i) if i.pid == 2 && !i.clean_exit)
    );
}

#[test]
fn marking_a_clean_exit_with_no_lock_is_harmless() {
    let dir = tempfile::tempdir().unwrap();
    assert!(mark_clean_exit(&dir.path().join("session.lock")).is_ok());
}

// ---- crash reports ----

fn report() -> CrashReport {
    CrashReport {
        app_version: "0.1.0".into(),
        os: "windows 11".into(),
        time_ms: 1_700_000_000_123,
        message: "index out of bounds".into(),
        backtrace: "0: ps_app::main\n1: std::rt".into(),
        log_lines: (1..=300).map(|i| format!("line {i}")).collect(),
    }
}

#[test]
fn a_report_carries_version_os_backtrace_and_recent_log_lines() {
    let text = format_report(&report(), 5);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "Plotter Studio crash report");
    assert!(text.contains("version: 0.1.0"));
    assert!(text.contains("os: windows 11"));
    assert!(text.contains("message: index out of bounds"));
    assert!(text.contains("0: ps_app::main"));
    // Only the most recent log lines are kept, oldest of them first.
    assert!(text.contains("line 296\nline 297\nline 298\nline 299\nline 300"));
    assert!(!text.contains("line 295"));
}

#[test]
fn a_report_says_that_nothing_is_uploaded() {
    assert!(format_report(&report(), 5).contains("never uploaded"));
}

#[test]
fn reports_are_written_into_the_crash_folder_without_clobbering() {
    let dir = tempfile::tempdir().unwrap();
    let crashes = dir.path().join("crashes");
    let first = write_report(&crashes, &report(), 50).unwrap();
    let second = write_report(&crashes, &report(), 50).unwrap();
    assert_ne!(first, second);
    assert!(
        first
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("crash-1700000000123")
    );
    assert_eq!(names(&crashes).len(), 2);
    assert!(
        fs::read_to_string(&first)
            .unwrap()
            .starts_with("Plotter Studio crash report")
    );
}
