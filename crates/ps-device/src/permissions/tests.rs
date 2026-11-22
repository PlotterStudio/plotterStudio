//! Ground truth for device permissions. The first connection to a device asks the
//! user ("Allow Generic Cutter family to use Generic Cutter M on USB?"). The answer is stored per
//! plugin and device serial number in settings and can be revoked. Internet hosts a manifest
//! declares are blocked until the user allows each one.

use super::*;
use crate::TransportKind;

#[test]
fn nothing_is_allowed_until_the_user_says_so() {
    let s = PermissionStore::default();
    assert!(!s.is_device_allowed("acme.cutter", "SN1"));
    assert!(!s.is_host_allowed("acme.cutter", "example.com"));
}

#[test]
fn an_approval_covers_one_plugin_and_one_device() {
    let mut s = PermissionStore::default();
    s.allow_device("acme.cutter", "SN1");
    assert!(s.is_device_allowed("acme.cutter", "SN1"));
    assert!(!s.is_device_allowed("acme.cutter", "SN2"), "another device");
    assert!(!s.is_device_allowed("grbl.serial", "SN1"), "another plugin");
}

#[test]
fn approvals_can_be_revoked() {
    let mut s = PermissionStore::default();
    s.allow_device("p", "a");
    s.allow_device("p", "b");
    s.revoke_device("p", "a");
    assert!(!s.is_device_allowed("p", "a"));
    assert!(s.is_device_allowed("p", "b"));
    s.revoke_device("p", "never-allowed");
}

#[test]
fn internet_hosts_are_allowed_per_plugin_and_separately_from_devices() {
    let mut s = PermissionStore::default();
    s.allow_host("p", "cloud.example.com");
    assert!(s.is_host_allowed("p", "cloud.example.com"));
    assert!(!s.is_host_allowed("p", "other.example.com"));
    assert!(!s.is_host_allowed("q", "cloud.example.com"));
    assert!(!s.is_device_allowed("p", "cloud.example.com"));
    s.revoke_host("p", "cloud.example.com");
    assert!(!s.is_host_allowed("p", "cloud.example.com"));
}

#[test]
fn the_listings_are_sorted_for_the_settings_screen() {
    let mut s = PermissionStore::default();
    s.allow_device("b", "2");
    s.allow_device("a", "9");
    s.allow_device("a", "1");
    assert_eq!(
        s.devices(),
        vec![
            ("a".to_owned(), "1".to_owned()),
            ("a".to_owned(), "9".to_owned()),
            ("b".to_owned(), "2".to_owned())
        ]
    );
    s.allow_host("z", "h2");
    s.allow_host("z", "h1");
    assert_eq!(
        s.hosts(),
        vec![
            ("z".to_owned(), "h1".to_owned()),
            ("z".to_owned(), "h2".to_owned())
        ]
    );
}

#[test]
fn the_question_names_the_plugin_the_model_and_the_connection() {
    assert_eq!(
        approval_prompt(
            "Generic Cutter family",
            "Generic Cutter M",
            TransportKind::Usb
        ),
        "Allow Generic Cutter family to use Generic Cutter M on USB?"
    );
    assert_eq!(
        approval_prompt("GRBL", "Plotter", TransportKind::Serial),
        "Allow GRBL to use Plotter on a serial port?"
    );
    assert_eq!(
        approval_prompt("X", "Y", TransportKind::BluetoothLe),
        "Allow X to use Y on Bluetooth?"
    );
    for net in [
        TransportKind::Tcp,
        TransportKind::Udp,
        TransportKind::WebSocket,
        TransportKind::Http,
    ] {
        assert!(
            approval_prompt("X", "Y", net).ends_with("on the network?"),
            "{net:?}"
        );
    }
}

#[test]
fn the_store_round_trips_through_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("permissions.json");
    let mut s = PermissionStore::default();
    s.allow_device("p", "SN1");
    s.allow_host("p", "h.example.com");
    s.save(&path).unwrap();
    assert_eq!(PermissionStore::load(&path).unwrap(), s);
}

#[test]
fn a_missing_file_is_an_empty_store_and_a_damaged_one_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        PermissionStore::load(&dir.path().join("nope.json")).unwrap(),
        PermissionStore::default()
    );
    let bad = dir.path().join("bad.json");
    std::fs::write(&bad, "{ not json").unwrap();
    assert!(PermissionStore::load(&bad).is_err());
}

#[test]
fn saving_creates_the_folder_and_leaves_no_litter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state").join("permissions.json");
    PermissionStore::default().save(&path).unwrap();
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        1
    );
}

// ---- certificate pins ----

#[test]
fn a_certificate_pin_is_kept_per_plugin_and_device_and_can_be_cleared() {
    let mut store = PermissionStore::default();
    assert_eq!(store.pin("p", "serial-1"), None);
    store.set_pin("p", "serial-1", "aa11");
    assert_eq!(store.pin("p", "serial-1"), Some("aa11"));
    assert_eq!(store.pin("p", "serial-2"), None);
    assert_eq!(store.pin("q", "serial-1"), None);
    store.clear_pin("p", "serial-1");
    assert_eq!(store.pin("p", "serial-1"), None);
}

#[test]
fn pins_survive_a_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("permissions.json");
    let mut store = PermissionStore::default();
    store.set_pin("p", "s", "cafe");
    store.save(&path).unwrap();
    assert_eq!(
        PermissionStore::load(&path).unwrap().pin("p", "s"),
        Some("cafe")
    );
}

#[test]
fn an_older_file_without_pins_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("permissions.json");
    std::fs::write(&path, r#"{"devices":[["p","s"]],"hosts":[]}"#).unwrap();
    let store = PermissionStore::load(&path).unwrap();
    assert!(store.is_device_allowed("p", "s"));
}
