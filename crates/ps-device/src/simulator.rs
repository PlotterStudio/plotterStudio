//! The built-in simulated machine as a discoverable device.
//!
//! It lets the app and the command line run the whole machine flow (discovery, the approval
//! prompt, a connection, a job with its pauses) without hardware, and gives plugin authors a
//! reference for how a device plugin looks to the host.

use async_trait::async_trait;

use crate::driver::Driver;
use crate::host::{DeviceFilter, DeviceId, DiscoveredDevice, Plugin, PluginManifest};
use crate::sim::{SimConfig, SimulatedMachine};
use crate::types::{DeviceError, DeviceInfo, TransportKind};

/// The id the simulated machine is listed under.
pub const SIMULATOR_ID: &str = "sim";
/// The plugin id of the simulator.
pub const SIMULATOR_PLUGIN: &str = "plotter.simulator";

/// Drives the simulated machine.
#[derive(Debug)]
pub struct SimulatorPlugin {
    manifest: PluginManifest,
}

impl Default for SimulatorPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulatorPlugin {
    /// The simulator plugin.
    #[must_use]
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest {
                id: SIMULATOR_PLUGIN.to_owned(),
                name: "Built-in simulator".to_owned(),
                filters: vec![DeviceFilter {
                    transport: TransportKind::Tcp,
                    usb: None,
                }],
                internet_hosts: Vec::new(),
            },
        }
    }
}

#[async_trait]
impl Plugin for SimulatorPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn discover(&self) -> Vec<DiscoveredDevice> {
        vec![DiscoveredDevice {
            id: DeviceId(SIMULATOR_ID.to_owned()),
            name: "Simulated machine".to_owned(),
            transport: TransportKind::Tcp,
            usb: None,
            address: Some("in-process".to_owned()),
            serial: Some("SIM-0001".to_owned()),
        }]
    }

    async fn identify(&self, device: &DiscoveredDevice) -> Option<DeviceInfo> {
        (device.id.0 == SIMULATOR_ID).then(|| DeviceInfo {
            model: "Simulated machine".to_owned(),
            serial_number: device.serial.clone(),
            firmware: Some("sim".to_owned()),
            profile_id: None,
            transport: device.transport,
        })
    }

    async fn open(&self, _device: &DiscoveredDevice) -> Result<Box<dyn Driver>, DeviceError> {
        Ok(Box::new(SimulatedMachine::new(SimConfig {
            profile_id: None,
            max_inflight: 32,
            line_ms: 1,
            ..SimConfig::default()
        })))
    }
}

#[cfg(test)]
mod tests;
