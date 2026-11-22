//! Ground truth for saving safely: write to a temporary file in the same directory,
//! fsync, then rename over the target, so a crash during a save leaves the previous file intact.
//! The previous version is kept as `<name>.pstudio.bak` (configurable).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::*;

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

const KEEP: SaveOptions = SaveOptions { keep_backup: true };
const NO_BACKUP: SaveOptions = SaveOptions { keep_backup: false };

#[test]
fn a_new_file_is_written_whole() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("card.pstudio");
    save_bytes(&target, b"first", KEEP).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"first");
    assert_eq!(
        names(dir.path()),
        vec!["card.pstudio"],
        "no backup without a previous version"
    );
}

#[test]
fn saving_over_a_file_keeps_the_previous_version_as_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("card.pstudio");
    save_bytes(&target, b"one", KEEP).unwrap();
    save_bytes(&target, b"two", KEEP).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"two");
    assert_eq!(fs::read(backup_path(&target)).unwrap(), b"one");
    assert_eq!(names(dir.path()), vec!["card.pstudio", "card.pstudio.bak"]);
}

#[test]
fn the_backup_is_always_the_version_just_before() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("card.pstudio");
    for v in [&b"one"[..], b"two", b"three"] {
        save_bytes(&target, v, KEEP).unwrap();
    }
    assert_eq!(fs::read(backup_path(&target)).unwrap(), b"two");
}

#[test]
fn the_backup_can_be_turned_off() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("card.pstudio");
    save_bytes(&target, b"one", NO_BACKUP).unwrap();
    save_bytes(&target, b"two", NO_BACKUP).unwrap();
    assert_eq!(names(dir.path()), vec!["card.pstudio"]);
}

#[test]
fn a_failed_write_leaves_the_previous_file_and_no_litter() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("card.pstudio");
    save_bytes(&target, b"precious", KEEP).unwrap();
    let result = save_with(&target, KEEP, |f| {
        f.write_all(b"half of a new fi")?;
        Err(std::io::Error::other("disk full"))
    });
    assert!(matches!(result, Err(SaveError::Io(_))), "{result:?}");
    assert_eq!(fs::read(&target).unwrap(), b"precious");
    assert_eq!(
        names(dir.path()),
        vec!["card.pstudio"],
        "the temporary file is gone"
    );
}

#[test]
fn a_failed_first_save_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("card.pstudio");
    let result = save_with(&target, KEEP, |_| Err(std::io::Error::other("nope")));
    assert!(result.is_err());
    assert_eq!(names(dir.path()), Vec::<String>::new());
}

#[test]
fn saving_into_a_missing_directory_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("nope").join("card.pstudio");
    assert!(matches!(
        save_bytes(&target, b"x", KEEP),
        Err(SaveError::Io(_))
    ));
}

#[test]
fn backup_paths_append_bak_to_the_whole_name() {
    assert_eq!(
        backup_path(Path::new("/a/b/card.pstudio")),
        PathBuf::from("/a/b/card.pstudio.bak")
    );
    assert_eq!(backup_path(Path::new("plain")), PathBuf::from("plain.bak"));
}

#[test]
fn an_empty_file_can_be_saved() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("e.pstudio");
    save_bytes(&target, b"", KEEP).unwrap();
    assert_eq!(fs::read(&target).unwrap().len(), 0);
}
