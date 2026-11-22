//! Ground truth for the buses a plugin may look at while it discovers. A plugin
//! cannot reach a bus its manifest filters do not name, and the host does not even power a radio
//! or open a socket on its behalf then. Radio and network listening time is capped.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::host::{DeviceFilter, DeviceId};

#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<String>>,
}

fn device(id: &str, transport: TransportKind, usb: Option<(u16, u16)>) -> DiscoveredDevice {
    DiscoveredDevice {
        id: DeviceId(id.to_owned()),
        name: id.to_owned(),
        transport,
        usb,
        address: None,
        serial: None,
    }
}

impl Buses for Fake {
    fn serial_ports(&self) -> Vec<DiscoveredDevice> {
        self.calls.lock().unwrap().push("serial".into());
        vec![
            device("serial:COM3", TransportKind::Serial, Some((0x1234, 7))),
            device("serial:COM4", TransportKind::Serial, Some((0x0403, 1))),
            device("serial:COM5", TransportKind::Serial, None),
        ]
    }
    fn usb_devices(&self) -> Vec<DiscoveredDevice> {
        self.calls.lock().unwrap().push("usb".into());
        vec![
            device("usb:a", TransportKind::Usb, Some((0x1234, 7))),
            device("usb:b", TransportKind::Usb, Some((0x0403, 1))),
        ]
    }
    fn ble_devices(&self, listen_for: Duration) -> Vec<DiscoveredDevice> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("ble {}", listen_for.as_millis()));
        vec![device("ble:1", TransportKind::BluetoothLe, None)]
    }
    fn mdns_services(&self, service_type: &str, listen_for: Duration) -> Vec<DiscoveredDevice> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("mdns {service_type} {}", listen_for.as_millis()));
        vec![device("mdns:x", TransportKind::Tcp, None)]
    }
}

fn filter(transport: TransportKind, usb: Option<(u16, u16)>) -> DeviceFilter {
    DeviceFilter { transport, usb }
}

fn scoped(filters: Vec<DeviceFilter>) -> (ScopedBuses, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    (ScopedBuses::new(fake.clone(), filters), fake)
}

fn ids(devices: &[DiscoveredDevice]) -> Vec<&str> {
    devices.iter().map(|d| d.id.0.as_str()).collect()
}

#[test]
fn a_plugin_with_no_filters_can_look_at_nothing_and_nothing_is_touched() {
    let (bus, fake) = scoped(Vec::new());
    assert_eq!(ids(&bus.serial_ports()), Vec::<&str>::new());
    assert_eq!(ids(&bus.usb_devices()), Vec::<&str>::new());
    assert_eq!(ids(&bus.ble_devices(1000)), Vec::<&str>::new());
    assert_eq!(
        ids(&bus.mdns_services("_plotter._tcp.local.", 1000)),
        Vec::<&str>::new()
    );
    assert!(fake.calls.lock().unwrap().is_empty());
}

#[test]
fn a_serial_filter_without_ids_shows_every_serial_port_and_nothing_else() {
    let (bus, fake) = scoped(vec![filter(TransportKind::Serial, None)]);
    assert_eq!(
        ids(&bus.serial_ports()),
        ["serial:COM3", "serial:COM4", "serial:COM5"]
    );
    assert_eq!(ids(&bus.usb_devices()), Vec::<&str>::new());
    assert_eq!(*fake.calls.lock().unwrap(), ["serial"]);
}

#[test]
fn a_filter_with_usb_ids_shows_only_ports_with_those_ids() {
    let (bus, _) = scoped(vec![filter(TransportKind::Serial, Some((0x1234, 7)))]);
    assert_eq!(ids(&bus.serial_ports()), ["serial:COM3"]);
    let (bus, _) = scoped(vec![filter(TransportKind::Usb, Some((0x0403, 1)))]);
    assert_eq!(ids(&bus.usb_devices()), ["usb:b"]);
}

#[test]
fn bluetooth_needs_a_bluetooth_filter() {
    let (bus, fake) = scoped(vec![filter(TransportKind::Serial, None)]);
    assert_eq!(ids(&bus.ble_devices(500)), Vec::<&str>::new());
    assert!(fake.calls.lock().unwrap().is_empty(), "no radio was used");
    let (bus, _) = scoped(vec![filter(TransportKind::BluetoothLe, None)]);
    assert_eq!(ids(&bus.ble_devices(500)), ["ble:1"]);
}

#[test]
fn network_browsing_needs_a_tcp_filter_and_a_plain_service_type() {
    let (bus, fake) = scoped(vec![filter(TransportKind::Serial, None)]);
    assert_eq!(
        ids(&bus.mdns_services("_plotter._tcp.local.", 500)),
        Vec::<&str>::new()
    );
    assert!(fake.calls.lock().unwrap().is_empty());

    let (bus, fake) = scoped(vec![filter(TransportKind::Tcp, None)]);
    assert_eq!(
        ids(&bus.mdns_services("_plotter._tcp.local.", 500)),
        ["mdns:x"]
    );
    for bad in [
        "",
        "plotter",
        "_a._tcp.local.\n",
        "_a._tcp.example.com.",
        "*",
    ] {
        assert!(bus.mdns_services(bad, 500).is_empty(), "{bad:?}");
    }
    assert_eq!(
        fake.calls.lock().unwrap().len(),
        1,
        "bad names reach nothing"
    );
}

#[test]
fn listening_time_is_capped() {
    let (bus, fake) = scoped(vec![
        filter(TransportKind::BluetoothLe, None),
        filter(TransportKind::Tcp, None),
    ]);
    let _ = bus.ble_devices(u32::MAX);
    let _ = bus.mdns_services("_plotter._tcp.local.", 60_000);
    let cap = MAX_LISTEN.as_millis();
    assert_eq!(
        *fake.calls.lock().unwrap(),
        [
            format!("ble {cap}"),
            format!("mdns _plotter._tcp.local. {cap}")
        ]
    );
}
