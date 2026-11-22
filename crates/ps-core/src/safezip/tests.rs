//! Ground truth for the safe zip reader: reject absolute paths, `..`
//! components, symlinks, names outside the allowlist, more than 10 000 entries, a declared total
//! above 500 MB, and zip bombs (compression ratio above 200:1).

use std::io::{Cursor, Write};

use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use super::*;

fn allow_all(_: &str) -> bool {
    true
}

fn build(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in entries {
        w.start_file(*name, opts).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

// ---- pure helper: path safety ----

#[test]
fn ordinary_relative_paths_are_safe() {
    for name in [
        "library.json",
        "wood/material.json",
        "a/b/c.png",
        "mats/01.gcode",
    ] {
        assert!(is_safe_path(name), "{name}");
    }
}

#[test]
fn unsafe_paths_are_recognised() {
    for name in [
        "",
        "/etc/passwd",
        "../escape.json",
        "a/../b.json",
        "a/..",
        "./a.json",
        "a/./b.json",
        "a//b.json",
        "a\\b.json",
        "C:/windows/x",
        "c:x",
        "a/",
    ] {
        assert!(!is_safe_path(name), "{name:?} must be rejected");
    }
}

// ---- reading ----

#[test]
fn a_normal_archive_is_read_into_a_sorted_map() {
    let bytes = build(&[("b.json", b"{}"), ("a/c.json", b"[1]")]);
    let files = read_archive(&bytes, &ZipLimits::default(), &allow_all).unwrap();
    let names: Vec<&str> = files.keys().map(String::as_str).collect();
    assert_eq!(names, ["a/c.json", "b.json"]);
    assert_eq!(files["a/c.json"], b"[1]");
}

#[test]
fn directory_entries_are_skipped() {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    w.add_directory("wood/", SimpleFileOptions::default())
        .unwrap();
    w.start_file("wood/m.json", SimpleFileOptions::default())
        .unwrap();
    w.write_all(b"{}").unwrap();
    let bytes = w.finish().unwrap().into_inner();
    let files = read_archive(&bytes, &ZipLimits::default(), &allow_all).unwrap();
    assert_eq!(
        files.keys().map(String::as_str).collect::<Vec<_>>(),
        ["wood/m.json"]
    );
}

#[test]
fn garbage_is_an_invalid_archive() {
    let err = read_archive(b"this is not a zip", &ZipLimits::default(), &allow_all).unwrap_err();
    assert!(matches!(err, ZipError::Invalid(_)), "{err:?}");
}

#[test]
fn absolute_and_parent_paths_are_rejected() {
    for name in ["/abs.json", "../up.json", "a/../b.json"] {
        let bytes = build(&[(name, b"x")]);
        let err = read_archive(&bytes, &ZipLimits::default(), &allow_all).unwrap_err();
        assert!(matches!(err, ZipError::UnsafePath(_)), "{name}: {err:?}");
    }
}

#[test]
fn symlink_entries_are_rejected() {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    w.add_symlink("link.json", "/etc/passwd", SimpleFileOptions::default())
        .unwrap();
    let bytes = w.finish().unwrap().into_inner();
    let err = read_archive(&bytes, &ZipLimits::default(), &allow_all).unwrap_err();
    assert!(matches!(err, ZipError::Symlink(_)), "{err:?}");
}

#[test]
fn names_outside_the_allowlist_are_rejected() {
    let bytes = build(&[("library.json", b"{}"), ("evil.exe", b"MZ")]);
    let only_json = |n: &str| {
        std::path::Path::new(n)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    };
    let err = read_archive(&bytes, &ZipLimits::default(), &only_json).unwrap_err();
    assert_eq!(err, ZipError::NotAllowed("evil.exe".to_owned()));
}

#[test]
fn too_many_entries_are_rejected() {
    let bytes = build(&[("a", b"1"), ("b", b"2"), ("c", b"3")]);
    let limits = ZipLimits {
        max_entries: 2,
        ..ZipLimits::default()
    };
    assert_eq!(
        read_archive(&bytes, &limits, &allow_all).unwrap_err(),
        ZipError::TooManyEntries
    );
}

#[test]
fn a_declared_total_above_the_limit_is_rejected_before_reading() {
    let bytes = build(&[("a", &[7_u8; 600]), ("b", &[9_u8; 600])]);
    let limits = ZipLimits {
        max_total_bytes: 1_000,
        ..ZipLimits::default()
    };
    assert_eq!(
        read_archive(&bytes, &limits, &allow_all).unwrap_err(),
        ZipError::TooLarge
    );
}

#[test]
fn a_zip_bomb_is_rejected_by_its_compression_ratio() {
    // 10 MB of zeros deflates to roughly 10 kB, a ratio near 1000:1.
    let bomb = vec![0_u8; 10 * 1024 * 1024];
    let bytes = build(&[("zeros.json", &bomb)]);
    let err = read_archive(&bytes, &ZipLimits::default(), &allow_all).unwrap_err();
    assert!(matches!(err, ZipError::ZipBomb(_)), "{err:?}");
}

#[test]
fn a_large_but_ordinary_entry_is_accepted() {
    // Varied text compresses far less than 200:1.
    let text: Vec<u8> = (0..200_000_u32).flat_map(u32::to_le_bytes).collect();
    let bytes = build(&[("data.json", &text)]);
    let files = read_archive(&bytes, &ZipLimits::default(), &allow_all).unwrap();
    assert_eq!(files["data.json"].len(), text.len());
}

#[test]
fn small_compressible_files_are_not_mistaken_for_bombs() {
    // 20 kB of zeros compresses beyond 200:1 but is below the size where the ratio applies.
    let zeros: Vec<u8> = std::iter::repeat_n(0_u8, 20_000).collect();
    let bytes = build(&[("zeros.json", &zeros)]);
    assert!(read_archive(&bytes, &ZipLimits::default(), &allow_all).is_ok());
}

#[test]
fn default_limits_are_the_design_values() {
    let l = ZipLimits::default();
    assert_eq!(l.max_entries, 10_000);
    assert_eq!(l.max_total_bytes, 500 * 1024 * 1024);
    assert_eq!(l.max_ratio, 200);
}
