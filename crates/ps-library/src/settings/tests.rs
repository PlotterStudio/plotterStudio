//! Ground truth for app settings: a `settings.toml` in the platform config directory
//! holding units, locale and decimal separator, theme, shortcuts, default machine, autosave
//! interval, network opt-ins (both off by default), GPU quality and recent files. Settings never
//! touch project files.

use super::*;

#[test]
fn the_defaults_are_private_and_conventional() {
    let s = Settings::default();
    assert_eq!(s.units, DisplayUnits::Millimetres);
    assert_eq!(s.theme, Theme::System);
    assert_eq!(s.locale, "system");
    assert_eq!(s.decimal_separator, DecimalSeparator::Auto);
    assert_eq!(s.autosave_interval_s, 30);
    assert_eq!(s.gpu_quality, GpuQuality::High);
    assert_eq!(s.default_machine, None);
    assert!(s.keep_backup);
    assert_eq!(s.recent_files.len(), 0);
    assert_eq!(s.shortcuts.len(), 0);
}

#[test]
fn both_network_features_are_off_by_default() {
    let n = Settings::default().network;
    assert!(!n.update_check);
    assert!(!n.material_index);
}

#[test]
fn settings_round_trip_through_toml() {
    let mut s = Settings {
        units: DisplayUnits::Inches,
        theme: Theme::Dark,
        locale: "de-DE".into(),
        default_machine: Some("generic.cutter-medium".into()),
        ..Settings::default()
    };
    s.network.update_check = true;
    s.shortcuts.insert("file.save".into(), "Ctrl+S".into());
    s.push_recent("a.pstudio");
    let text = s.to_toml().unwrap();
    assert_eq!(Settings::from_toml(&text).unwrap(), s);
}

#[test]
fn a_partial_file_fills_in_the_defaults() {
    let s = Settings::from_toml("theme = \"light\"\n").unwrap();
    assert_eq!(s.theme, Theme::Light);
    assert_eq!(s.autosave_interval_s, 30);
    assert!(!s.network.update_check);
}

#[test]
fn an_empty_file_is_the_defaults() {
    assert_eq!(Settings::from_toml("").unwrap(), Settings::default());
}

#[test]
fn unknown_settings_from_a_newer_version_are_ignored() {
    let s = Settings::from_toml("theme = \"dark\"\nfuture_option = 7\n[future_table]\nx = 1\n")
        .unwrap();
    assert_eq!(s.theme, Theme::Dark);
}

#[test]
fn broken_toml_is_an_error_and_a_wrong_type_is_too() {
    assert!(Settings::from_toml("theme = ").is_err());
    assert!(Settings::from_toml("autosave_interval_s = \"often\"").is_err());
    assert!(Settings::from_toml("theme = \"purple\"").is_err());
}

#[test]
fn the_autosave_interval_is_kept_within_sensible_bounds() {
    assert_eq!(
        Settings::from_toml("autosave_interval_s = 0")
            .unwrap()
            .autosave_interval_s,
        5
    );
    assert_eq!(
        Settings::from_toml("autosave_interval_s = 999999")
            .unwrap()
            .autosave_interval_s,
        3_600
    );
    assert_eq!(
        Settings::from_toml("autosave_interval_s = 45")
            .unwrap()
            .autosave_interval_s,
        45
    );
}

// ---- files ----

#[test]
fn a_missing_file_gives_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        Settings::load(&dir.path().join("settings.toml")).unwrap(),
        Settings::default()
    );
}

#[test]
fn saving_then_loading_gives_the_same_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    let mut s = Settings {
        theme: Theme::Dark,
        ..Settings::default()
    };
    s.push_recent("x.pstudio");
    s.save(&path).unwrap();
    assert_eq!(Settings::load(&path).unwrap(), s);
}

#[test]
fn saving_creates_the_config_folder_and_leaves_no_litter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config").join("settings.toml");
    Settings::default().save(&path).unwrap();
    let names: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().collect();
    assert_eq!(names.len(), 1);
}

#[test]
fn a_damaged_file_falls_back_to_the_defaults_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "theme = [").unwrap();
    let (s, problem) = Settings::load_or_default(&path);
    assert_eq!(s, Settings::default());
    assert!(problem.is_some());
    let (ok, none) = Settings::load_or_default(&dir.path().join("missing.toml"));
    assert_eq!(ok, Settings::default());
    assert!(none.is_none());
}

// ---- recent files ----

#[test]
fn recent_files_are_newest_first_without_duplicates() {
    let mut s = Settings::default();
    for f in ["a", "b", "c", "a"] {
        s.push_recent(f);
    }
    assert_eq!(s.recent_files, vec!["a", "c", "b"]);
}

#[test]
fn only_the_ten_most_recent_files_are_kept() {
    let mut s = Settings::default();
    for i in 0..15 {
        s.push_recent(&format!("f{i}"));
    }
    assert_eq!(s.recent_files.len(), 10);
    assert_eq!(s.recent_files[0], "f14");
    assert_eq!(s.recent_files[9], "f5");
}

#[test]
fn a_recent_file_can_be_forgotten() {
    let mut s = Settings::default();
    s.push_recent("a");
    s.push_recent("b");
    s.forget_recent("a");
    assert_eq!(s.recent_files, vec!["b"]);
    s.forget_recent("not there");
    assert_eq!(s.recent_files, vec!["b"]);
}

// ---- decimal separator ----

#[test]
fn an_explicit_decimal_separator_wins() {
    assert_eq!(DecimalSeparator::Dot.resolve("de-DE"), '.');
    assert_eq!(DecimalSeparator::Comma.resolve("en-US"), ',');
}

#[test]
fn the_automatic_separator_follows_the_locale() {
    for comma in [
        "de-DE", "fr-FR", "es-ES", "it-IT", "pt-BR", "nl-NL", "ru-RU", "pl-PL", "sv-SE",
    ] {
        assert_eq!(DecimalSeparator::Auto.resolve(comma), ',', "{comma}");
    }
    for dot in ["en-US", "en-GB", "ja-JP", "zh-CN", "system", ""] {
        assert_eq!(DecimalSeparator::Auto.resolve(dot), '.', "{dot}");
    }
}

// ---- the first-run notice ----

#[test]
fn the_first_run_notice_has_not_been_seen_by_default() {
    assert!(!Settings::default().first_run_seen);
}

#[test]
fn having_seen_the_first_run_notice_is_remembered() {
    let s = Settings {
        first_run_seen: true,
        ..Settings::default()
    };
    let text = s.to_toml().unwrap();
    assert!(Settings::from_toml(&text).unwrap().first_run_seen);
}

#[test]
fn an_older_settings_file_without_the_flag_still_counts_as_first_run() {
    let s = Settings::from_toml("units = \"inches\"\n").unwrap();
    assert!(!s.first_run_seen);
}

// ---- screen calibration ----

#[test]
fn the_screen_calibration_defaults_to_one_and_is_kept_within_sensible_bounds() {
    assert!((Settings::default().screen_calibration - 1.0).abs() < f64::EPSILON);
    let low = Settings::from_toml("screen_calibration = 0.01").unwrap();
    assert!((low.screen_calibration - MIN_SCREEN_CALIBRATION).abs() < f64::EPSILON);
    let high = Settings::from_toml("screen_calibration = 50.0").unwrap();
    assert!((high.screen_calibration - MAX_SCREEN_CALIBRATION).abs() < f64::EPSILON);
    let nan = Settings::from_toml("screen_calibration = nan").unwrap();
    assert!((nan.screen_calibration - 1.0).abs() < f64::EPSILON);
}

#[test]
fn the_screen_calibration_survives_a_round_trip() {
    let s = Settings {
        screen_calibration: 1.0625,
        ..Settings::default()
    };
    let back = Settings::from_toml(&s.to_toml().unwrap()).unwrap();
    assert!((back.screen_calibration - 1.0625).abs() < 1e-12);
}

#[test]
fn a_ruler_measurement_gives_the_new_calibration() {
    // A 100 mm bar drawn at calibration 1.0 measured 95 mm: the screen draws too small, so the
    // scale must grow by 100 / 95.
    let c = calibration_from_measurement(1.0, 95.0).unwrap();
    assert!((c - 100.0 / 95.0).abs() < 1e-12);
    // Measured again after that correction, 100 mm: nothing changes.
    assert!((calibration_from_measurement(c, 100.0).unwrap() - c).abs() < 1e-12);
}

#[test]
fn a_nonsense_measurement_is_refused() {
    for bad in [0.0, -5.0, f64::NAN, f64::INFINITY, 1.0, 5_000.0] {
        // 1 mm and 5 m would give a calibration outside the allowed range.
        assert_eq!(calibration_from_measurement(1.0, bad), None, "{bad}");
    }
}

#[test]
fn the_automation_server_is_off_local_and_without_a_token_by_default() {
    let a = Settings::default().automation;
    assert!(!a.mcp_enabled);
    assert!(
        !a.mcp_allow_network,
        "only this computer, until the user says otherwise"
    );
    assert_eq!(a.mcp_port, DEFAULT_MCP_PORT);
    assert_eq!(a.mcp_token, "");
}

#[test]
fn the_access_token_never_appears_in_debug_output() {
    let mut s = Settings::default();
    s.automation.mcp_token = "s3cret-token-value".to_owned();
    let shown = format!("{s:?}");
    assert!(!shown.contains("s3cret-token-value"), "{shown}");
}

#[test]
fn the_automation_settings_round_trip_through_toml() {
    let s = Settings {
        automation: Automation {
            mcp_enabled: true,
            mcp_port: 9100,
            mcp_allow_network: true,
            mcp_token: "abc123".to_owned(),
        },
        ..Settings::default()
    };
    let text = s.to_toml().unwrap();
    assert!(text.contains("[automation]"), "{text}");
    assert_eq!(Settings::from_toml(&text).unwrap(), s);
}

#[test]
fn a_port_the_app_should_not_listen_on_is_replaced_by_the_default() {
    for port in [0_u16, 80, 1023] {
        let s = Settings::from_toml(&format!("[automation]\nmcp_port = {port}\n")).unwrap();
        assert_eq!(s.automation.mcp_port, DEFAULT_MCP_PORT, "port {port}");
    }
    let s = Settings::from_toml("[automation]\nmcp_port = 9100\n").unwrap();
    assert_eq!(s.automation.mcp_port, 9100);
}
