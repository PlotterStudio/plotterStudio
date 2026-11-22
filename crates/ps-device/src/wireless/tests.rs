//! Ground truth for wireless discovery: machines announced over mDNS appear only
//! when they are on the local network and name a port, and a Bluetooth LE peripheral is described
//! by its name, address and services. Radios cannot be exercised without hardware, so what is
//! checked is the conversion from what they report to what the host lists, and that scanning
//! without a radio fails quietly.

use std::net::IpAddr;
use std::time::Duration;

use super::*;
use crate::types::TransportKind;

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

#[test]
fn a_local_service_becomes_a_tcp_device_with_its_address() {
    let d =
        device_from_service("Plotter._plotter._tcp.local.", &[ip("192.168.1.40")], 8080).unwrap();
    assert_eq!(d.transport, TransportKind::Tcp);
    assert_eq!(d.address.as_deref(), Some("192.168.1.40:8080"));
    assert_eq!(d.name, "Plotter");
    assert_eq!(d.id.0, "mdns:Plotter._plotter._tcp.local.");
}

#[test]
fn a_service_with_no_local_address_is_ignored() {
    assert!(device_from_service("X._x._tcp.local.", &[ip("8.8.8.8")], 80).is_none());
    assert!(device_from_service("X._x._tcp.local.", &[], 80).is_none());
}

#[test]
fn the_first_local_address_is_used_when_several_are_given() {
    let d = device_from_service("X._x._tcp.local.", &[ip("8.8.8.8"), ip("10.0.0.7")], 23).unwrap();
    assert_eq!(d.address.as_deref(), Some("10.0.0.7:23"));
}

#[test]
fn a_service_without_a_port_is_ignored() {
    assert!(device_from_service("X._x._tcp.local.", &[ip("10.0.0.7")], 0).is_none());
}

#[test]
fn ipv6_addresses_are_written_with_brackets() {
    let d = device_from_service("X._x._tcp.local.", &[ip("fe80::1")], 80).unwrap();
    assert_eq!(d.address.as_deref(), Some("[fe80::1]:80"));
}

#[test]
fn the_instance_name_is_the_part_before_the_service_type() {
    assert_eq!(
        instance_name("My Plotter._plotter._tcp.local."),
        "My Plotter"
    );
    assert_eq!(instance_name("bare"), "bare");
}

#[test]
fn a_ble_peripheral_is_described_by_name_address_and_services() {
    let p = BleInfo {
        address: "AA:BB:CC:DD:EE:FF".into(),
        name: Some("Pen Plotter".into()),
        services: vec!["0000ffe0-0000-1000-8000-00805f9b34fb".into()],
    };
    let d = p.to_discovered();
    assert_eq!(d.transport, TransportKind::BluetoothLe);
    assert_eq!(d.id.0, "ble:AA:BB:CC:DD:EE:FF");
    assert_eq!(d.name, "Pen Plotter");
    assert_eq!(d.address.as_deref(), Some("AA:BB:CC:DD:EE:FF"));
    let unnamed = BleInfo {
        address: "11:22".into(),
        name: None,
        services: Vec::new(),
    };
    assert_eq!(unnamed.to_discovered().name, "Bluetooth device 11:22");
}

#[test]
fn a_short_uuid_expands_to_the_bluetooth_base() {
    assert_eq!(
        parse_uuid("ffe1").unwrap().to_string(),
        "0000ffe1-0000-1000-8000-00805f9b34fb"
    );
    assert_eq!(
        parse_uuid("6E400001-B5A3-F393-E0A9-E50E24DCCA9E")
            .unwrap()
            .to_string(),
        "6e400001-b5a3-f393-e0a9-e50e24dcca9e"
    );
    assert!(parse_uuid("not a uuid").is_none());
}

#[test]
fn scanning_without_a_radio_or_network_returns_nothing_rather_than_failing() {
    // Whatever this machine has, these must not panic.
    let _ = browse_mdns(&["_plotter._tcp.local."], Duration::from_millis(50));
    let _ = scan_ble(Duration::from_millis(50));
}

// ---- scanning from inside a runtime ----

/// The app scans from inside the device host's async runtime. A scan that starts a runtime of its
/// own there panics ("Cannot start a runtime from within a runtime"), which took the whole app
/// down the first time Bluetooth scanning was turned on.
#[tokio::test]
async fn scanning_for_bluetooth_inside_a_runtime_does_not_panic() {
    // Whatever the computer's radio does, the scan returns instead of panicking.
    let _ = scan_ble(Duration::from_millis(1));
}

#[tokio::test]
async fn the_system_bus_can_also_scan_inside_a_runtime() {
    let found = crate::bus::Buses::ble_devices(&crate::bus::SystemBuses, Duration::from_millis(1));
    // Nothing to assert about what is nearby; every entry is a Bluetooth device.
    assert!(
        found
            .iter()
            .all(|d| d.transport == TransportKind::BluetoothLe)
    );
}
