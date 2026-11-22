//! Ground truth for the built-in simulator as a discoverable device: the way to try the whole
//! machine flow (discovery, approval, connection, a run) without hardware.

use super::*;
use crate::handle::HandleConfig;
use crate::permissions::PermissionStore;
use crate::types::{ConnectionState, MachineState};
use crate::{DeviceHost, DeviceId};

fn host() -> DeviceHost {
    let host = DeviceHost::new(PermissionStore::default(), HandleConfig::default());
    host.add_plugin(std::sync::Arc::new(SimulatorPlugin::new()));
    host
}

#[tokio::test]
async fn the_plugin_discovers_exactly_one_simulated_machine() {
    let found = SimulatorPlugin::new().discover().await;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, DeviceId(SIMULATOR_ID.to_owned()));
    assert_eq!(found[0].name, "Simulated machine");
}

#[tokio::test]
async fn the_host_lists_it_and_asks_for_approval() {
    let host = host();
    host.refresh().await;
    let list = host.devices().borrow().clone();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].info.model, "Simulated machine");
    assert!(!list[0].approved);
}

#[tokio::test]
async fn an_approved_simulator_connects_and_goes_idle() {
    let host = host();
    host.refresh().await;
    let id = DeviceId(SIMULATOR_ID.to_owned());
    host.approve(&id);
    let handle = host.connect(&id).await.unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let s = handle.status();
        if s.connection == ConnectionState::Connected && s.state == MachineState::Idle {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "never idle");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn the_plugin_only_recognises_its_own_device() {
    let plugin = SimulatorPlugin::new();
    let mut other = plugin.discover().await.remove(0);
    other.id = DeviceId("usb:1".to_owned());
    assert!(plugin.identify(&other).await.is_none());
}

#[test]
fn the_manifest_asks_for_no_internet() {
    let plugin = SimulatorPlugin::new();
    assert_eq!(plugin.manifest().id, "plotter.simulator");
    assert_eq!(plugin.manifest().internet_hosts.len(), 0);
}
