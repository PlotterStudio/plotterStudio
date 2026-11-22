//! Ground truth for the log file: lines written through `tracing` end up in a dated file in the
//! folder, and an unusable folder is reported, not fatal.

use super::*;

#[test]
fn events_are_written_to_a_log_file_in_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let (writer, guard) = file_writer(dir.path()).unwrap();
    tracing::subscriber::with_default(subscriber(writer), || {
        tracing::info!(machine = "acme.mill", "the machine was chosen");
        tracing::debug!("too detailed for the default level");
    });
    drop(guard);
    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    let name = files[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("plotter-studio.")
            && files[0]
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("log")),
        "{name}"
    );
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert!(text.contains("the machine was chosen"), "{text}");
    assert!(text.contains("acme.mill"), "fields are written: {text}");
    assert!(text.contains("INFO"), "{text}");
    assert!(
        !text.contains("too detailed"),
        "the default level is info: {text}"
    );
    assert!(
        !text.contains('\u{1b}'),
        "no colour codes in a file: {text}"
    );
}

#[test]
fn a_folder_that_cannot_be_used_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a-file");
    std::fs::write(&file, b"x").unwrap();
    assert!(file_writer(&file).is_err(), "a file is not a folder");
}
