//! Ground truth for installed WebAssembly plugins in the device host: a package
//! becomes a plugin whose manifest filters decide which buses it may look at, whose component
//! finds and confirms its own machines, and whose driver the machine handle runs. The buses and
//! transports are fakes (a list of ports, the simulated serial machine), so a whole GRBL plugin
//! runs through the host with no hardware.

use std::sync::Arc;

use super::*;
use crate::handle::HandleConfig;
use crate::host::{DeviceFilter, DeviceHost, DeviceId, DiscoveredDevice, PluginManifest};
use crate::package::{SimulatedSerial, pack};
use crate::permissions::PermissionStore;
use crate::types::TransportKind;
use crate::wasm::tests_support::{FakeBuses, grbl};

fn serial(id: &str) -> DiscoveredDevice {
    DiscoveredDevice {
        id: DeviceId(id.to_owned()),
        name: "USB-SERIAL".into(),
        transport: TransportKind::Serial,
        usb: None,
        address: Some("COM7".into()),
        serial: Some("A1".into()),
    }
}

fn tcp(id: &str) -> DiscoveredDevice {
    DiscoveredDevice {
        id: DeviceId(id.to_owned()),
        name: "network".into(),
        transport: TransportKind::Tcp,
        usb: None,
        address: Some("192.168.1.9:23".into()),
        serial: None,
    }
}

fn manifest() -> PluginManifest {
    PluginManifest {
        id: "acme.grbl".into(),
        name: "Acme GRBL".into(),
        filters: vec![DeviceFilter {
            transport: TransportKind::Serial,
            usb: None,
        }],
        internet_hosts: Vec::new(),
    }
}

/// Every device opens as the simulated GRBL machine.
struct Simulated;

impl TransportFactory for Simulated {
    fn open(&self, _device: &DiscoveredDevice) -> Result<Box<dyn BlockingTransport>, DeviceError> {
        Ok(Box::new(SimulatedSerial::default()))
    }
}

/// No device can be opened.
struct Unplugged;

impl TransportFactory for Unplugged {
    fn open(&self, device: &DiscoveredDevice) -> Result<Box<dyn BlockingTransport>, DeviceError> {
        Err(DeviceError::new(
            "open-failed",
            format!("{} is not there", device.id.0),
        ))
    }
}

/// A plugin whose serial bus lists `ports`.
fn plugin_seeing(
    ports: Vec<DiscoveredDevice>,
    factory: Arc<dyn TransportFactory>,
) -> Arc<WasmPlugin> {
    let package = pack(&manifest(), &grbl(), "1.0.0").unwrap();
    let host = Arc::new(WasmHost::new(WasmLimits::default()).unwrap());
    let plugin = WasmPlugin::from_package(&package, host, factory)
        .unwrap()
        .with_buses(Arc::new(FakeBuses {
            serial: ports,
            ..FakeBuses::default()
        }));
    Arc::new(plugin)
}

fn plugin(factory: Arc<dyn TransportFactory>) -> Arc<WasmPlugin> {
    plugin_seeing(Vec::new(), factory)
}

fn device_host(devices: Vec<DiscoveredDevice>, factory: Arc<dyn TransportFactory>) -> DeviceHost {
    let host = DeviceHost::new(PermissionStore::default(), HandleConfig::default());
    host.add_plugin(plugin_seeing(devices, factory));
    host
}

#[test]
fn a_plugin_takes_its_manifest_from_the_package() {
    let p = plugin(Arc::new(Simulated));
    assert_eq!(p.manifest(), &manifest());
    assert_eq!(p.version(), "1.0.0");
}

#[test]
fn a_package_that_is_not_one_is_refused() {
    let host = Arc::new(WasmHost::new(WasmLimits::default()).unwrap());
    assert!(WasmPlugin::from_package(b"nope", host, Arc::new(Simulated)).is_err());
}

#[tokio::test]
async fn a_matching_device_is_listed_as_what_the_component_says_it_is() {
    let host = device_host(vec![serial("serial:COM7")], Arc::new(Simulated));
    host.refresh().await;
    let list = host.devices().borrow().clone();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].info.model, "GRBL 1.1h");
    assert_eq!(list[0].plugin_id, "acme.grbl");
    assert!(!list[0].approved);
}

#[tokio::test]
async fn a_device_the_manifest_does_not_allow_is_never_shown_to_the_plugin() {
    // The serial bus lists a network device; the manifest names only serial ports.
    let host = device_host(vec![tcp("tcp:1")], Arc::new(Simulated));
    host.refresh().await;
    assert!(host.devices().borrow().is_empty());
}

#[tokio::test]
async fn an_installed_plugin_finds_its_machine_with_no_help_from_the_app() {
    // The host has no discovery source of its own: the port comes from the plugin's own look at
    // the serial bus.
    let host = device_host(vec![serial("serial:COM7")], Arc::new(Simulated));
    assert!(
        host.devices().borrow().is_empty(),
        "nothing is found before a refresh"
    );
    host.refresh().await;
    assert_eq!(host.devices().borrow().len(), 1);
}

#[tokio::test]
async fn a_host_without_the_plugin_finds_nothing_even_with_a_machine_plugged_in() {
    let host = DeviceHost::new(PermissionStore::default(), HandleConfig::default());
    host.refresh().await;
    assert!(host.devices().borrow().is_empty());
}

#[tokio::test]
async fn a_plugin_whose_discovery_fails_finds_nothing_instead_of_failing_the_refresh() {
    let package = pack(&manifest(), b"not a component", "1.0.0").unwrap();
    let wasm_host = Arc::new(WasmHost::new(WasmLimits::default()).unwrap());
    let plugin = WasmPlugin::from_package(&package, wasm_host, Arc::new(Simulated)).unwrap();
    assert_eq!(plugin.discover().await.len(), 0);
}

#[tokio::test]
async fn a_device_that_cannot_be_opened_is_not_listed() {
    let host = device_host(vec![serial("serial:COM7")], Arc::new(Unplugged));
    host.refresh().await;
    assert!(host.devices().borrow().is_empty());
}

#[tokio::test]
async fn an_approved_device_connects_and_reports_status() {
    use crate::types::{ConnectionState, MachineState};
    let host = device_host(vec![serial("serial:COM7")], Arc::new(Simulated));
    host.refresh().await;
    let id = DeviceId("serial:COM7".into());
    assert!(host.connect(&id).await.is_err(), "not approved yet");
    host.approve(&id);
    let handle = host.connect(&id).await.expect("connects");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let s = handle.status();
        if s.connection == ConnectionState::Connected && s.state != MachineState::Unknown {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "never reported: {s:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn opening_a_device_the_factory_cannot_open_reports_why() {
    let p = plugin(Arc::new(Unplugged));
    let error = p.open(&serial("serial:COM7")).await.err().expect("fails");
    assert_eq!(error.code, "open-failed");
}

// ---- the default factory ----

#[test]
fn the_default_factory_refuses_what_it_cannot_reach() {
    let f = SystemTransports;
    let mut no_address = serial("s");
    no_address.address = None;
    assert!(f.open(&no_address).is_err());
    let mut internet = tcp("t");
    internet.address = Some("8.8.8.8:23".into());
    assert!(f.open(&internet).is_err());
    let mut bad = tcp("t");
    bad.address = Some("not an address".into());
    assert!(f.open(&bad).is_err());
    let mut ble = serial("b");
    ble.transport = TransportKind::BluetoothLe;
    ble.address = None;
    assert_eq!(f.open(&ble).err().unwrap().code, "no-address");
}

#[test]
fn the_default_factory_opens_network_links_only_to_the_local_network() {
    let f = SystemTransports;
    for transport in [
        TransportKind::Udp,
        TransportKind::Http,
        TransportKind::WebSocket,
        TransportKind::Tcp,
    ] {
        let mut internet = tcp("t");
        internet.transport = transport;
        internet.address = Some("8.8.8.8:80".into());
        let error = f
            .open(&internet)
            .err()
            .unwrap_or_else(|| panic!("{transport:?} opened"));
        assert_eq!(error.code, "open-failed", "{transport:?}");
    }
}

#[test]
fn the_default_factory_opens_a_udp_link_to_a_local_machine() {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut device = tcp("u");
    device.transport = TransportKind::Udp;
    device.address = Some(socket.local_addr().unwrap().to_string());
    let link = SystemTransports.open(&device).expect("opens");
    assert_eq!(link.kind(), TransportKind::Udp);
}

#[test]
fn the_default_factory_reports_a_missing_usb_device_cleanly() {
    let mut device = serial("usb:ffff:fffe");
    device.transport = TransportKind::Usb;
    device.usb = Some((0xFFFF, 0xFFFE));
    device.address = None;
    assert_eq!(
        SystemTransports.open(&device).err().unwrap().code,
        "open-failed"
    );
    device.usb = None;
    assert_eq!(
        SystemTransports.open(&device).err().unwrap().code,
        "no-address"
    );
}
