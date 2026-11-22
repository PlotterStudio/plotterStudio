//! Golden-file tests: the same drawing, rules and machine must give byte-identical
//! G-code on every operating system. Each `goldens/inputs/<name>.svg` is exported and compared
//! with `goldens/expected/<name>-01.gcode`. `cargo xtask goldens --bless` (or `PS_BLESS=1`) rewrites
//! the expected files; review the diff before committing it.

use std::fs;
use std::path::{Path, PathBuf};

const MACHINE: &str = "generic.cutter-small";
const TIME: &str = "2026-01-01T00:00:00Z";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens")
}

fn export(name: &str) -> String {
    let inputs = root().join("inputs");
    let out = tempfile::tempdir().unwrap();
    let args: Vec<String> = [
        "pstudio",
        "export",
        &inputs.join(format!("{name}.svg")).to_string_lossy(),
        "--machine",
        MACHINE,
        "--rules",
        &inputs.join("rules.json").to_string_lossy(),
        "--out",
        &out.path().to_string_lossy(),
        "--export-time",
        TIME,
        "--format",
        "gcode",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    let (mut o, mut e) = (Vec::new(), Vec::new());
    let code = ps_cli::run(
        &args,
        &mut ps_cli::Io {
            out: &mut o,
            err: &mut e,
            now_unix_secs: 1_700_000_000,
        },
    );
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&e));
    fs::read_to_string(out.path().join(format!("{name}-01.gcode"))).unwrap()
}

/// The header names the app version, and the job id is derived from it; goldens must not change
/// with every release, so both are replaced by markers before comparing.
fn without_version(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.starts_with("; plotter-studio ") {
                "; plotter-studio VERSION"
            } else if line.starts_with("; job ") {
                "; job JOB"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn check(name: &str) {
    let got = without_version(&export(name));
    let expected_path = root().join("expected").join(format!("{name}-01.gcode"));
    if std::env::var_os("PS_BLESS").is_some() {
        fs::write(&expected_path, &got).unwrap();
        return;
    }
    let expected = fs::read_to_string(&expected_path)
        .unwrap_or_else(|_| panic!("no golden for {name}; run `cargo xtask goldens --bless`"));
    assert_eq!(
        got.replace("\r\n", "\n"),
        expected.replace("\r\n", "\n"),
        "{name} changed; if intended, run `cargo xtask goldens --bless` and review the diff"
    );
}

#[test]
fn a_card_with_a_cut_and_a_draw_matches_its_golden() {
    check("card");
}

#[test]
fn curves_match_their_golden() {
    check("curves");
}

#[test]
fn rows_of_shapes_match_their_golden() {
    check("rows");
}

#[test]
fn nested_transforms_and_use_references_match_their_golden() {
    check("transforms");
}

#[test]
fn every_path_command_matches_its_golden() {
    check("pathcommands");
}

#[test]
fn exporting_twice_gives_identical_text() {
    assert_eq!(export("curves"), export("curves"));
}
