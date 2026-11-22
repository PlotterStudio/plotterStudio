//! Ground truth for the device host. The host finds nothing by itself: every
//! device comes from a plugin's `discover`, so with no plugin installed nothing is ever found. A
//! plugin is trusted only for what its manifest filters allow, and the host then asks the plugin's
//! `identify` to confirm the model. The first connection to a device asks the user; the answer is
//! kept per plugin and serial number and can be revoked. The device list updates on hot-plug.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;

use super::*;
use crate::sim::{SimConfig, SimulatedMachine};
use crate::{ConnectionState, MachineState};

#[derive(Default)]
struct Calls(Mutex<Vec<String>>);

impl Calls {
    fn push(&self, s: &str) {
        self.0.lock().unwrap().push(s.to_owned());
    }
    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

struct FakePlugin {
    manifest: PluginManifest,
    recognises: Vec<&'static str>,
    calls: Arc<Calls>,
    fail_open: bool,
    finds: Found,
}

/// What a fake plugin's `discover` reports; tests change it between refreshes to plug and unplug.
#[derive(Clone, Default)]
struct Found(Arc<Mutex<Vec<DiscoveredDevice>>>);

impl Found {
    fn of(devices: Vec<DiscoveredDevice>) -> Self {
        Self(Arc::new(Mutex::new(devices)))
    }
    fn get(&self) -> Vec<DiscoveredDevice> {
        self.0.lock().unwrap().clone()
    }
    fn push(&self, device: DiscoveredDevice) {
        self.0.lock().unwrap().push(device);
    }
    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

#[async_trait]
impl Plugin for FakePlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn discover(&self) -> Vec<DiscoveredDevice> {
        self.calls.push(&format!("{}:discover", self.manifest.id));
        self.finds.get()
    }

    async fn identify(&self, device: &DiscoveredDevice) -> Option<DeviceInfo> {
        self.calls
            .push(&format!("{}:identify:{}", self.manifest.id, device.id.0));
        self.recognises
            .contains(&device.name.as_str())
            .then(|| DeviceInfo {
                model: device.name.clone(),
                serial_number: device.serial.clone(),
                firmware: None,
                profile_id: Some("generic.cutter-medium".into()),
                transport: device.transport,
            })
    }

    async fn open(&self, device: &DiscoveredDevice) -> Result<Box<dyn Driver>, DeviceError> {
        self.calls
            .push(&format!("{}:open:{}", self.manifest.id, device.id.0));
        if self.fail_open {
            return Err(DeviceError::new("open-failed", "no such port"));
        }
        Ok(Box::new(SimulatedMachine::new(SimConfig::default())))
    }
}

fn usb_filter(vid: u16, pid: u16) -> DeviceFilter {
    DeviceFilter {
        transport: TransportKind::Usb,
        usb: Some((vid, pid)),
    }
}

fn plugin(
    id: &str,
    name: &str,
    filters: Vec<DeviceFilter>,
    recognises: &[&'static str],
    calls: &Arc<Calls>,
    finds: &Found,
) -> Arc<FakePlugin> {
    Arc::new(FakePlugin {
        manifest: PluginManifest {
            id: id.into(),
            name: name.into(),
            filters,
            internet_hosts: Vec::new(),
        },
        recognises: recognises.to_vec(),
        calls: Arc::clone(calls),
        fail_open: false,
        finds: finds.clone(),
    })
}

fn usb(id: &str, name: &str, vid: u16, pid: u16, serial: Option<&str>) -> DiscoveredDevice {
    DiscoveredDevice {
        id: DeviceId(id.into()),
        name: name.into(),
        transport: TransportKind::Usb,
        usb: Some((vid, pid)),
        address: None,
        serial: serial.map(str::to_owned),
    }
}

fn host_with(devices: Vec<DiscoveredDevice>) -> (DeviceHost, Found, Arc<Calls>) {
    let calls = Arc::new(Calls::default());
    let found = Found::of(devices);
    let host = DeviceHost::new(PermissionStore::default(), HandleConfig::default());
    host.add_plugin(plugin(
        "acme",
        "Generic Cutter family",
        vec![usb_filter(0x1234, 0x0001)],
        &["Generic Cutter M"],
        &calls,
        &found,
    ));
    (host, found, calls)
}

#[tokio::test]
async fn a_host_with_no_plugins_finds_nothing() {
    let host = DeviceHost::new(PermissionStore::default(), HandleConfig::default());
    host.refresh().await;
    assert!(host.devices().borrow().is_empty());
    assert_eq!(host.unclaimed().len(), 0);
}

#[tokio::test]
async fn nothing_is_discovered_until_a_refresh_asks_the_plugins() {
    let (_host, _, calls) = host_with(vec![usb("usb:1", "Generic Cutter M", 0x1234, 1, None)]);
    assert!(calls.all().is_empty(), "adding a plugin runs none of it");
}

#[tokio::test]
async fn every_refresh_asks_each_plugin_to_discover_once() {
    let calls = Arc::new(Calls::default());
    let host = DeviceHost::new(PermissionStore::default(), HandleConfig::default());
    for id in ["a", "b"] {
        host.add_plugin(plugin(
            id,
            id,
            vec![usb_filter(1, 1)],
            &[],
            &calls,
            &Found::default(),
        ));
    }
    host.refresh().await;
    host.refresh().await;
    let mut discovers: Vec<String> = calls
        .all()
        .into_iter()
        .filter(|c| c.ends_with(":discover"))
        .collect();
    discovers.sort();
    assert_eq!(
        discovers,
        ["a:discover", "a:discover", "b:discover", "b:discover"]
    );
}

#[tokio::test]
async fn a_device_matching_a_plugins_filters_is_listed_with_what_the_plugin_says() {
    let (host, _, _) = host_with(vec![usb(
        "usb:1",
        "Generic Cutter M",
        0x1234,
        0x0001,
        Some("SN1"),
    )]);
    host.refresh().await;
    let list = host.devices().borrow().clone();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, DeviceId("usb:1".into()));
    assert_eq!(list[0].plugin_id, "acme");
    assert_eq!(list[0].info.model, "Generic Cutter M");
    assert!(!list[0].approved);
    assert_eq!(
        list[0].prompt,
        "Allow Generic Cutter family to use Generic Cutter M on USB?"
    );
}

#[tokio::test]
async fn a_device_a_plugin_reports_outside_its_manifest_filters_is_not_trusted() {
    let (host, _, calls) = host_with(vec![
        usb("usb:1", "Generic Cutter M", 0x1234, 0x0001, Some("A")),
        usb("usb:2", "Something else", 0x9999, 0x0001, Some("B")),
    ]);
    host.refresh().await;
    let identified: Vec<String> = calls
        .all()
        .into_iter()
        .filter(|c| c.contains(":identify:"))
        .collect();
    assert_eq!(
        identified,
        vec!["acme:identify:usb:1"],
        "a device outside the manifest is never identified or listed"
    );
    assert_eq!(host.devices().borrow().len(), 1);
}

#[tokio::test]
async fn a_matching_device_the_plugin_does_not_recognise_is_not_listed() {
    let (host, _, _) = host_with(vec![usb(
        "usb:1",
        "Unknown model",
        0x1234,
        0x0001,
        Some("A"),
    )]);
    host.refresh().await;
    assert_eq!(host.devices().borrow().len(), 0);
}

#[tokio::test]
async fn the_first_plugin_that_recognises_a_device_gets_it() {
    let calls = Arc::new(Calls::default());
    let host = DeviceHost::new(PermissionStore::default(), HandleConfig::default());
    let both = Found::of(vec![usb("usb:1", "Dual", 0x1234, 0x0001, Some("S"))]);
    host.add_plugin(plugin(
        "first",
        "First",
        vec![usb_filter(0x1234, 0x0001)],
        &[],
        &calls,
        &both,
    ));
    host.add_plugin(plugin(
        "second",
        "Second",
        vec![usb_filter(0x1234, 0x0001)],
        &["Dual"],
        &calls,
        &both,
    ));
    host.refresh().await;
    let list = host.devices().borrow().clone();
    assert_eq!(list[0].plugin_id, "second");
    let identified: Vec<String> = calls
        .all()
        .into_iter()
        .filter(|c| c.contains(":identify:"))
        .collect();
    assert_eq!(
        identified,
        vec!["first:identify:usb:1", "second:identify:usb:1"]
    );
}

#[tokio::test]
async fn a_plugin_cannot_claim_a_device_only_another_plugin_found() {
    let calls = Arc::new(Calls::default());
    let host = DeviceHost::new(PermissionStore::default(), HandleConfig::default());
    host.add_plugin(plugin(
        "finder",
        "Finder",
        vec![usb_filter(0x1234, 0x0001)],
        &[],
        &calls,
        &Found::of(vec![usb("usb:1", "Dual", 0x1234, 0x0001, Some("S"))]),
    ));
    host.add_plugin(plugin(
        "bystander",
        "Bystander",
        vec![usb_filter(0x1234, 0x0001)],
        &["Dual"],
        &calls,
        &Found::default(),
    ));
    host.refresh().await;
    assert!(
        host.devices().borrow().is_empty(),
        "a plugin only ever hears about devices it found itself"
    );
    assert!(
        !calls
            .all()
            .iter()
            .any(|c| c.starts_with("bystander:identify"))
    );
}

#[tokio::test]
async fn a_filter_without_ids_matches_every_device_on_that_transport() {
    let calls = Arc::new(Calls::default());
    let host = DeviceHost::new(PermissionStore::default(), HandleConfig::default());
    host.add_plugin(plugin(
        "p",
        "P",
        vec![DeviceFilter {
            transport: TransportKind::Usb,
            usb: None,
        }],
        &["Any"],
        &calls,
        &Found::of(vec![usb("usb:1", "Any", 1, 2, None)]),
    ));
    host.refresh().await;
    assert_eq!(host.devices().borrow().len(), 1);
}

#[tokio::test]
async fn the_list_follows_hot_plugging() {
    let (host, source, _) = host_with(vec![usb(
        "usb:1",
        "Generic Cutter M",
        0x1234,
        0x0001,
        Some("A"),
    )]);
    let mut rx = host.devices();
    host.refresh().await;
    assert!(rx.has_changed().unwrap());
    rx.borrow_and_update();
    source.push(usb("usb:0", "Generic Cutter M", 0x1234, 0x0001, Some("B")));
    host.refresh().await;
    assert!(rx.has_changed().unwrap());
    let ids: Vec<String> = rx
        .borrow_and_update()
        .iter()
        .map(|e| e.id.0.clone())
        .collect();
    assert_eq!(ids, vec!["usb:0", "usb:1"], "sorted by id");
    source.clear();
    host.refresh().await;
    assert_eq!(rx.borrow().len(), 0);
}

#[tokio::test]
async fn connecting_needs_the_users_approval_first() {
    let (host, _, calls) = host_with(vec![usb(
        "usb:1",
        "Generic Cutter M",
        0x1234,
        0x0001,
        Some("SN1"),
    )]);
    host.refresh().await;
    let id = DeviceId("usb:1".into());
    match host.connect(&id).await {
        Err(ConnectError::NotApproved { prompt }) => {
            assert_eq!(
                prompt,
                "Allow Generic Cutter family to use Generic Cutter M on USB?"
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(
        !calls.all().iter().any(|c| c.contains(":open:")),
        "the plugin was never given the device"
    );
}

#[tokio::test]
async fn after_approval_the_device_connects_and_the_answer_is_remembered() {
    let (host, _, _) = host_with(vec![usb(
        "usb:1",
        "Generic Cutter M",
        0x1234,
        0x0001,
        Some("SN1"),
    )]);
    host.refresh().await;
    let id = DeviceId("usb:1".into());
    host.approve(&id);
    assert!(host.permissions().is_device_allowed("acme", "SN1"));
    assert!(
        host.devices().borrow()[0].approved,
        "the list shows it as approved"
    );

    let handle = host.connect(&id).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while handle.status().connection != ConnectionState::Connected
        || handle.status().state != MachineState::Idle
    {
        assert!(Instant::now() < deadline, "never connected");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn revoking_makes_the_next_connection_ask_again() {
    let (host, _, _) = host_with(vec![usb(
        "usb:1",
        "Generic Cutter M",
        0x1234,
        0x0001,
        Some("SN1"),
    )]);
    host.refresh().await;
    let id = DeviceId("usb:1".into());
    host.approve(&id);
    host.revoke(&id);
    assert!(matches!(
        host.connect(&id).await,
        Err(ConnectError::NotApproved { .. })
    ));
}

#[tokio::test]
async fn approval_is_per_serial_number_so_another_unit_asks_again() {
    let (host, _, _) = host_with(vec![
        usb("usb:1", "Generic Cutter M", 0x1234, 0x0001, Some("SN1")),
        usb("usb:2", "Generic Cutter M", 0x1234, 0x0001, Some("SN2")),
    ]);
    host.refresh().await;
    host.approve(&DeviceId("usb:1".into()));
    assert!(host.connect(&DeviceId("usb:1".into())).await.is_ok());
    assert!(matches!(
        host.connect(&DeviceId("usb:2".into())).await,
        Err(ConnectError::NotApproved { .. })
    ));
}

#[tokio::test]
async fn a_device_with_no_serial_number_is_approved_by_its_id() {
    let (host, _, _) = host_with(vec![usb("usb:7", "Generic Cutter M", 0x1234, 0x0001, None)]);
    host.refresh().await;
    let id = DeviceId("usb:7".into());
    host.approve(&id);
    assert!(host.permissions().is_device_allowed("acme", "usb:7"));
    assert!(host.connect(&id).await.is_ok());
}

#[tokio::test]
async fn an_unknown_device_cannot_be_connected() {
    let (host, _, _) = host_with(vec![]);
    host.refresh().await;
    assert!(matches!(
        host.connect(&DeviceId("nope".into())).await,
        Err(ConnectError::NotFound(_))
    ));
}

#[tokio::test]
async fn a_plugin_that_cannot_open_the_device_reports_why() {
    let calls = Arc::new(Calls::default());
    let host = DeviceHost::new(PermissionStore::default(), HandleConfig::default());
    host.add_plugin(Arc::new(FakePlugin {
        manifest: PluginManifest {
            id: "p".into(),
            name: "P".into(),
            filters: vec![usb_filter(1, 1)],
            internet_hosts: Vec::new(),
        },
        recognises: vec!["M"],
        calls,
        fail_open: true,
        finds: Found::of(vec![usb("usb:1", "M", 1, 1, Some("S"))]),
    }));
    host.refresh().await;
    let id = DeviceId("usb:1".into());
    host.approve(&id);
    match host.connect(&id).await {
        Err(ConnectError::Open(e)) => assert_eq!(e.code, "open-failed"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn refreshing_twice_gives_the_same_list() {
    let (host, _, _) = host_with(vec![usb(
        "usb:1",
        "Generic Cutter M",
        0x1234,
        0x0001,
        Some("A"),
    )]);
    host.refresh().await;
    let first = host.devices().borrow().clone();
    host.refresh().await;
    assert_eq!(*host.devices().borrow(), first);
}

#[test]
fn filters_match_on_transport_and_optionally_on_usb_ids() {
    let device = usb("u", "x", 0x1234, 0x0001, None);
    assert!(usb_filter(0x1234, 0x0001).matches(&device));
    assert!(!usb_filter(0x1234, 0x0002).matches(&device));
    assert!(
        !DeviceFilter {
            transport: TransportKind::Serial,
            usb: None
        }
        .matches(&device)
    );
    assert!(
        DeviceFilter {
            transport: TransportKind::Usb,
            usb: None
        }
        .matches(&device)
    );
}

// ---- internet hosts a plugin declares ----

struct CloudPlugin {
    manifest: PluginManifest,
    allowed: Mutex<Vec<String>>,
    used: Vec<String>,
}

#[async_trait]
impl Plugin for CloudPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    async fn discover(&self) -> Vec<DiscoveredDevice> {
        Vec::new()
    }
    async fn identify(&self, _: &DiscoveredDevice) -> Option<DeviceInfo> {
        None
    }
    async fn open(&self, _: &DiscoveredDevice) -> Result<Box<dyn Driver>, DeviceError> {
        Err(DeviceError::new("unused", "unused"))
    }
    fn set_host_allowed(&self, host: &str, allowed: bool) {
        let mut list = self.allowed.lock().unwrap();
        list.retain(|h| h != host);
        if allowed {
            list.push(host.to_owned());
        }
    }
    fn internet_in_use(&self) -> Vec<String> {
        self.used.clone()
    }
}

fn cloud(used: &[&str]) -> Arc<CloudPlugin> {
    Arc::new(CloudPlugin {
        manifest: PluginManifest {
            id: "vendor.cloud".into(),
            name: "Vendor Cloud".into(),
            filters: Vec::new(),
            internet_hosts: vec!["api.vendor.example".into(), "cdn.vendor.example".into()],
        },
        allowed: Mutex::new(Vec::new()),
        used: used.iter().map(|s| (*s).to_owned()).collect(),
    })
}

fn host_of(store: PermissionStore) -> DeviceHost {
    DeviceHost::new(store, HandleConfig::default())
}

#[test]
fn declared_hosts_start_blocked_and_are_listed_with_their_plugin() {
    let host = host_of(PermissionStore::default());
    host.add_plugin(cloud(&[]));
    let hosts = host.declared_hosts();
    assert_eq!(
        hosts,
        vec![
            DeclaredHost {
                plugin_id: "vendor.cloud".into(),
                plugin_name: "Vendor Cloud".into(),
                host: "api.vendor.example".into(),
                allowed: false
            },
            DeclaredHost {
                plugin_id: "vendor.cloud".into(),
                plugin_name: "Vendor Cloud".into(),
                host: "cdn.vendor.example".into(),
                allowed: false
            },
        ]
    );
}

#[test]
fn allowing_a_declared_host_updates_the_store_and_the_plugin_and_revoking_undoes_it() {
    let host = host_of(PermissionStore::default());
    let plugin = cloud(&[]);
    host.add_plugin(plugin.clone());
    assert!(host.allow_host("vendor.cloud", "api.vendor.example"));
    assert!(
        host.permissions()
            .is_host_allowed("vendor.cloud", "api.vendor.example")
    );
    assert_eq!(
        *plugin.allowed.lock().unwrap(),
        vec!["api.vendor.example".to_owned()]
    );
    assert!(host.declared_hosts()[0].allowed);
    assert!(host.revoke_host("vendor.cloud", "api.vendor.example"));
    assert!(
        !host
            .permissions()
            .is_host_allowed("vendor.cloud", "api.vendor.example")
    );
    assert!(plugin.allowed.lock().unwrap().is_empty());
}

#[test]
fn a_host_the_manifest_does_not_declare_cannot_be_allowed() {
    let host = host_of(PermissionStore::default());
    host.add_plugin(cloud(&[]));
    assert!(!host.allow_host("vendor.cloud", "evil.example"));
    assert!(!host.allow_host("nobody", "api.vendor.example"));
    assert!(
        !host
            .permissions()
            .is_host_allowed("vendor.cloud", "evil.example")
    );
}

#[test]
fn approvals_saved_earlier_are_applied_when_the_plugin_is_added() {
    let mut store = PermissionStore::default();
    store.allow_host("vendor.cloud", "cdn.vendor.example");
    let host = host_of(store);
    let plugin = cloud(&[]);
    host.add_plugin(plugin.clone());
    assert_eq!(
        *plugin.allowed.lock().unwrap(),
        vec!["cdn.vendor.example".to_owned()]
    );
}

#[test]
fn hosts_in_use_are_listed_by_plugin_name() {
    let host = host_of(PermissionStore::default());
    host.add_plugin(cloud(&["api.vendor.example"]));
    assert_eq!(
        host.internet_in_use(),
        vec![("Vendor Cloud".to_owned(), "api.vendor.example".to_owned())]
    );
}

#[tokio::test]
async fn devices_a_plugin_found_but_could_not_confirm_are_reported_so_the_user_can_see_why() {
    let (host, _, _) = host_with(vec![
        usb("usb:1", "Generic Cutter M", 0x1234, 0x0001, Some("A")),
        usb("usb:2", "Some camera", 0x9999, 0x0002, Some("B")),
        usb("usb:3", "Unknown model", 0x1234, 0x0001, Some("C")),
    ]);
    host.refresh().await;
    let ids: Vec<String> = host.unclaimed().into_iter().map(|d| d.id.0).collect();
    assert_eq!(
        ids,
        vec!["usb:3".to_owned()],
        "the camera is outside the manifest, so it is dropped, not reported"
    );
}

// ---- a slow or stuck port must not hide the others ----

fn serial_port(name: &str) -> DiscoveredDevice {
    DiscoveredDevice {
        id: DeviceId(format!("serial:{name}")),
        name: name.into(),
        transport: TransportKind::Serial,
        usb: None,
        address: Some(name.into()),
        serial: None,
    }
}

/// Recognises `"COM5"`; for the ports named `"COM6…"` it takes `slow_for` to answer (a Bluetooth port that never
/// connects), `None` meaning it never does.
struct SlowPlugin {
    manifest: PluginManifest,
    slow_for: Option<Duration>,
    ports: Vec<DiscoveredDevice>,
}

#[async_trait]
impl Plugin for SlowPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
    async fn discover(&self) -> Vec<DiscoveredDevice> {
        self.ports.clone()
    }
    async fn identify(&self, device: &DiscoveredDevice) -> Option<DeviceInfo> {
        if device.name.starts_with("COM6") {
            match self.slow_for {
                Some(wait) => tokio::time::sleep(wait).await,
                None => std::future::pending::<()>().await,
            }
            return None;
        }
        (device.name == "COM5").then(|| DeviceInfo {
            model: "Generic Cutter S".into(),
            serial_number: None,
            firmware: None,
            profile_id: None,
            transport: device.transport,
        })
    }
    async fn open(&self, _: &DiscoveredDevice) -> Result<Box<dyn Driver>, DeviceError> {
        Err(DeviceError::new("unused", "unused"))
    }
}

fn serial_host(slow_for: Option<Duration>, ports: &[&str]) -> DeviceHost {
    let host = DeviceHost::new(PermissionStore::default(), HandleConfig::default());
    host.add_plugin(Arc::new(SlowPlugin {
        ports: ports.iter().map(|p| serial_port(p)).collect(),
        manifest: PluginManifest {
            id: "slow".into(),
            name: "Slow".into(),
            filters: vec![DeviceFilter {
                transport: TransportKind::Serial,
                usb: None,
            }],
            internet_hosts: Vec::new(),
        },
        slow_for,
    }));
    host
}

#[tokio::test(start_paused = true)]
async fn a_port_that_never_answers_does_not_hide_the_machine_on_another_port() {
    let host = serial_host(None, &["COM1", "COM5", "COM6", "COM7"]);
    tokio::time::timeout(Duration::from_secs(60), host.refresh())
        .await
        .expect("the scan ends even though COM6 never answers");
    let found = host.devices().borrow().clone();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].id, DeviceId("serial:COM5".into()));
    let unclaimed: Vec<String> = host.unclaimed().into_iter().map(|d| d.name).collect();
    assert_eq!(unclaimed, ["COM1", "COM6", "COM7"]);
}

#[tokio::test(start_paused = true)]
async fn ports_are_probed_side_by_side_not_one_after_another() {
    // Three silent ports that each take three seconds to say nothing.
    let host = serial_host(
        Some(Duration::from_secs(3)),
        &["COM6", "COM6b", "COM6c", "COM5"],
    );
    let started = tokio::time::Instant::now();
    host.refresh().await;
    let took = started.elapsed();
    assert!(took < Duration::from_secs(4), "{took:?}");
    assert_eq!(host.devices().borrow().len(), 1);
}

/// Its discovery never returns (a radio that never answers).
struct Hanging(PluginManifest);

#[async_trait]
impl Plugin for Hanging {
    fn manifest(&self) -> &PluginManifest {
        &self.0
    }
    async fn discover(&self) -> Vec<DiscoveredDevice> {
        std::future::pending::<()>().await;
        Vec::new()
    }
    async fn identify(&self, _: &DiscoveredDevice) -> Option<DeviceInfo> {
        None
    }
    async fn open(&self, _: &DiscoveredDevice) -> Result<Box<dyn Driver>, DeviceError> {
        Err(DeviceError::new("unused", "unused"))
    }
}

#[tokio::test(start_paused = true)]
async fn a_plugin_whose_discovery_hangs_does_not_hide_another_plugins_machine() {
    let calls = Arc::new(Calls::default());
    let host = DeviceHost::new(PermissionStore::default(), HandleConfig::default());
    host.add_plugin(Arc::new(Hanging(PluginManifest {
        id: "stuck".into(),
        name: "Stuck".into(),
        filters: vec![usb_filter(1, 1)],
        internet_hosts: Vec::new(),
    })));
    host.add_plugin(plugin(
        "acme",
        "Acme",
        vec![usb_filter(0x1234, 1)],
        &["Generic Cutter M"],
        &calls,
        &Found::of(vec![usb("usb:1", "Generic Cutter M", 0x1234, 1, Some("A"))]),
    ));
    tokio::time::timeout(Duration::from_secs(120), host.refresh())
        .await
        .expect("the refresh ends although one plugin never finishes discovering");
    let list = host.devices().borrow().clone();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].plugin_id, "acme");
}
