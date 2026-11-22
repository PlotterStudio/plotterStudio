//! Ground truth for the data directory: the platform's own data folder, named
//! `PlotterStudio` (for example `~/Library/Application Support/PlotterStudio` on macOS).

use super::*;

#[test]
fn the_default_root_is_a_platform_folder_named_plotter_studio() {
    let root = default_root().expect("this platform has a data directory");
    assert!(root.is_absolute(), "{root:?}");
    assert!(root.to_string_lossy().contains("PlotterStudio"), "{root:?}");
}

#[test]
fn the_default_root_is_stable() {
    assert_eq!(default_root(), default_root());
}

#[test]
fn the_default_library_can_be_opened_under_any_root() {
    let dir = tempfile::tempdir().unwrap();
    let lib = open_at(dir.path()).unwrap();
    assert!(lib.cache_dir().ends_with("cache"));
}

#[test]
fn the_settings_file_lives_in_the_platform_config_folder() {
    let path = default_settings_path().expect("this platform has a config directory");
    assert!(path.is_absolute(), "{path:?}");
    assert_eq!(path.file_name().unwrap(), "settings.toml");
    assert!(path.to_string_lossy().contains("PlotterStudio"), "{path:?}");
}

#[test]
fn autosaves_and_crash_reports_live_under_the_data_folder() {
    let root = default_root().unwrap();
    assert_eq!(default_state_dir().unwrap(), root.join("state"));
}

#[test]
fn installed_device_plugins_live_in_a_plugins_folder_under_the_data_root() {
    let root = default_root().unwrap();
    assert_eq!(default_plugins_dir().unwrap(), root.join("plugins"));
}
