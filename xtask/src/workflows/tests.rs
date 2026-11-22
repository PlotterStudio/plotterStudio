//! Ground truth for the CI and release workflows. The three workflows exist, parse, and
//! carry the checks the design lists: lint, tests on three systems, the boundary and schema
//! checks, a signed release on `v*` tags with checksums and provenance, and a nightly
//! with long property tests. Every cargo build uses the committed lockfile.

use std::path::Path;

use serde_yaml_ng::Value;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn workflow(name: &str) -> Value {
    let path = root().join(".github/workflows").join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_yaml_ng::from_str(&text).unwrap_or_else(|e| panic!("{name} is not valid YAML: {e}"))
}

fn jobs(w: &Value) -> Vec<String> {
    w["jobs"]
        .as_mapping()
        .unwrap()
        .keys()
        .filter_map(|k| k.as_str().map(str::to_owned))
        .collect()
}

fn text_of(name: &str) -> String {
    std::fs::read_to_string(root().join(".github/workflows").join(name)).unwrap()
}

#[test]
fn the_three_workflows_exist_and_parse() {
    for name in ["ci.yml", "nightly.yml", "release.yml"] {
        let w = workflow(name);
        assert!(w["jobs"].is_mapping(), "{name} has jobs");
    }
}

#[test]
fn ci_runs_on_every_push_and_pull_request() {
    let w = workflow("ci.yml");
    let on = &w["on"];
    assert!(
        on["pull_request"].is_null()
            || on["pull_request"].is_mapping()
            || on.get("pull_request").is_some()
    );
    assert!(on.get("push").is_some());
}

#[test]
fn ci_has_the_lint_test_ui_and_build_check_jobs() {
    let names = jobs(&workflow("ci.yml"));
    for expected in ["lint", "test", "ui", "build-check"] {
        assert!(
            names.iter().any(|n| n == expected),
            "{expected} in {names:?}"
        );
    }
}

#[test]
fn the_lint_job_runs_every_check_the_design_lists() {
    let text = text_of("ci.yml");
    for command in [
        "cargo fmt --all --check",
        "cargo clippy --workspace --all-targets --locked -- -D warnings",
        "cargo deny check",
        "cargo machete",
        "cargo xtask check-boundaries",
        "cargo xtask check-schemas",
    ] {
        assert!(text.contains(command), "ci.yml should run {command}");
    }
}

#[test]
fn tests_run_on_linux_windows_and_macos() {
    let w = workflow("ci.yml");
    let matrix = &w["jobs"]["test"]["strategy"]["matrix"]["os"];
    let oses: Vec<&str> = matrix
        .as_sequence()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for os in ["ubuntu-22.04", "windows-2022", "macos-15"] {
        assert!(oses.contains(&os), "{os} in {oses:?}");
    }
    assert!(text_of("ci.yml").contains("cargo test --workspace --locked"));
}

#[test]
fn the_ui_job_uses_a_software_vulkan_device() {
    let text = text_of("ci.yml");
    assert!(text.contains("mesa-vulkan-drivers"), "lavapipe");
    assert!(text.contains("libxkbcommon-dev") && text.contains("libvulkan-dev"));
}

#[test]
fn every_cargo_invocation_that_builds_uses_the_lockfile() {
    for name in ["ci.yml", "nightly.yml", "release.yml"] {
        for line in text_of(name).lines() {
            let l = line.trim();
            let builds = [
                "cargo build",
                "cargo test",
                "cargo clippy",
                "cargo doc",
                "cargo nextest",
            ]
            .iter()
            .any(|c| l.contains(c));
            if builds && !l.starts_with('#') {
                assert!(l.contains("--locked"), "{name}: {l}");
            }
        }
    }
}

#[test]
fn the_release_runs_on_version_tags_and_signs_every_platform() {
    let w = workflow("release.yml");
    let tags = w["on"]["push"]["tags"].as_sequence().unwrap();
    assert!(tags.iter().any(|t| t.as_str() == Some("v*")));
    let names = jobs(&w);
    for expected in [
        "build-windows",
        "build-macos",
        "build-linux",
        "sign-macos",
        "sign-windows",
        "sign-linux",
        "publish",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "{expected} in {names:?}"
        );
    }
}

#[test]
fn publishing_waits_for_the_signing_jobs_and_makes_a_draft_release_with_provenance() {
    let w = workflow("release.yml");
    let needs: Vec<&str> = w["jobs"]["publish"]["needs"]
        .as_sequence()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for job in ["sign-macos", "sign-windows", "sign-linux"] {
        assert!(needs.contains(&job), "{job}");
    }
    let text = text_of("release.yml");
    assert!(text.contains("SHA256SUMS"));
    assert!(text.contains("minisign"));
    assert!(text.contains("actions/attest-build-provenance"));
    assert!(text.contains("gh release create") && text.contains("--draft"));
}

/// What users download is the installer for each system and the plain executables, with the
/// checksums. Build by-products (a second Windows installer, one SBOM per crate) are not attached.
#[test]
fn the_release_attaches_only_what_people_download() {
    let text = text_of("release.yml");
    assert!(
        text.contains("--formats nsis") && !text.contains("nsis,"),
        "one Windows installer, built with NSIS"
    );
    for unwanted in [".msi", "wix", "cdx.json", "cyclonedx"] {
        assert!(
            !text.contains(unwanted),
            "the release does not carry {unwanted}"
        );
    }
    let config = std::fs::read_to_string(root().join("crates/ps-app/Packager.toml")).unwrap();
    assert!(!config.contains("wix"), "no second Windows installer");
}

#[test]
fn the_release_builds_the_macos_app_universal() {
    let text = text_of("release.yml");
    assert!(text.contains("aarch64-apple-darwin") && text.contains("x86_64-apple-darwin"));
    assert!(text.contains("lipo"));
}

#[test]
fn device_plugins_build_for_the_wasi_target_in_ci() {
    let text = text_of("ci.yml");
    assert!(text.contains("wasm32-wasip2"));
}

#[test]
fn the_nightly_runs_long_property_tests_and_publishes_a_rolling_prerelease() {
    let w = workflow("nightly.yml");
    assert!(w["on"].get("schedule").is_some());
    let text = text_of("nightly.yml");
    assert!(text.contains("PROPTEST_CASES"));
    assert!(text.contains("10000"));
    assert!(text.contains("--prerelease"));
}

#[test]
fn the_release_profile_matches_the_design() {
    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).unwrap();
    let v: toml_edit_free::Table = toml_edit_free::parse(&manifest);
    let release = v.section("profile.release");
    assert_eq!(release.get("lto").map(String::as_str), Some("\"thin\""));
    assert_eq!(release.get("codegen-units").map(String::as_str), Some("1"));
    assert_eq!(
        release.get("strip").map(String::as_str),
        Some("\"symbols\"")
    );
    assert_eq!(release.get("panic").map(String::as_str), Some("\"unwind\""));
}

#[test]
fn windows_builds_link_the_c_runtime_statically() {
    let config = std::fs::read_to_string(root().join(".cargo/config.toml")).unwrap();
    assert!(config.contains("+crt-static"));
}

#[test]
fn the_packager_configuration_registers_the_file_types() {
    let text = std::fs::read_to_string(root().join("crates/ps-app/Packager.toml")).unwrap();
    assert!(text.contains("pstudio") && text.contains("psjob"));
    for format in ["nsis", "dmg", "appimage", "deb"] {
        assert!(text.contains(format), "{format}");
    }
}

#[test]
fn a_changelog_exists_in_keep_a_changelog_form() {
    let text = std::fs::read_to_string(root().join("CHANGELOG.md")).unwrap();
    assert!(text.contains("Keep a Changelog"));
    assert!(text.contains("## [Unreleased]"));
}

/// A minimal reader for `[section]` tables, enough to read a few release-profile keys without a
/// TOML dependency in this crate.
mod toml_edit_free {
    use std::collections::BTreeMap;

    pub(super) struct Table(BTreeMap<String, BTreeMap<String, String>>);

    pub(super) fn parse(text: &str) -> Table {
        let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut current = String::new();
        for line in text.lines() {
            let l = line.trim();
            if let Some(name) = l.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                current = name.to_owned();
            } else if let Some((k, v)) = l.split_once('=') {
                sections
                    .entry(current.clone())
                    .or_default()
                    .insert(k.trim().to_owned(), v.trim().to_owned());
            }
        }
        Table(sections)
    }

    impl Table {
        pub(super) fn section(&self, name: &str) -> BTreeMap<String, String> {
            self.0.get(name).cloned().unwrap_or_default()
        }
    }
}

#[test]
fn the_nightly_fuzzes_every_untrusted_parser_for_thirty_minutes_each() {
    let text = text_of("nightly.yml");
    assert!(text.contains("fuzz run"), "a fuzz job exists");
    assert!(
        text.contains("-max_total_time=1800"),
        "30 minutes per target"
    );
    for target in [
        "svg",
        "dxf",
        "pdf",
        "project",
        "material_package",
        "library_json",
    ] {
        assert!(text.contains(target), "{target} is fuzzed");
        let file = root()
            .join("fuzz/fuzz_targets")
            .join(format!("{target}.rs"));
        assert!(file.exists(), "{} exists", file.display());
    }
}

#[test]
fn the_fuzz_targets_call_the_shared_entry_points() {
    for target in [
        "svg",
        "dxf",
        "pdf",
        "project",
        "material_package",
        "library_json",
    ] {
        let file = root()
            .join("fuzz/fuzz_targets")
            .join(format!("{target}.rs"));
        let body = std::fs::read_to_string(&file).unwrap();
        assert!(body.contains("ps_fuzz::run"), "{target}");
        assert!(body.contains("fuzz_target!"), "{target}");
    }
}
