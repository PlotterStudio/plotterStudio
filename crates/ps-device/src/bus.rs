//! The system buses a plugin may look at while it discovers.
//!
//! A plugin cannot enumerate serial ports, USB, Bluetooth or the network itself: the sandbox gives
//! it no authority. The host lends it the lists through the `discovery` interface instead, scoped
//! by the filters in its manifest. A plugin that names no serial filter cannot see serial ports, and
//! the host then does not touch the bus at all (no radio is powered, no socket opened). What to
//! look for, and which of the things found are its machines, is the plugin's own decision.

use std::sync::Arc;
use std::time::Duration;

use crate::host::{DeviceFilter, DiscoveredDevice};
use crate::link::{SerialPortInfo, UsbInfo, list_serial_ports, list_usb_devices};
use crate::types::TransportKind;
use crate::wireless::{BleInfo, browse_mdns, scan_ble};

/// The longest a plugin may make the host listen on a radio or the network in one call.
pub const MAX_LISTEN: Duration = Duration::from_secs(3);

/// The system's buses, as the host sees them. Tests replace it with a fixed list.
pub trait Buses: Send + Sync {
    /// The serial ports the system lists, with USB ids where it knows them.
    fn serial_ports(&self) -> Vec<DiscoveredDevice>;

    /// The USB devices the system lists.
    fn usb_devices(&self) -> Vec<DiscoveredDevice>;

    /// The Bluetooth LE peripherals heard within `listen_for`.
    fn ble_devices(&self, listen_for: Duration) -> Vec<DiscoveredDevice>;

    /// The machines on the local network announcing `service_type` within `listen_for`.
    fn mdns_services(&self, service_type: &str, listen_for: Duration) -> Vec<DiscoveredDevice>;
}

/// The real buses of this computer.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemBuses;

impl Buses for SystemBuses {
    fn serial_ports(&self) -> Vec<DiscoveredDevice> {
        list_serial_ports()
            .iter()
            .map(SerialPortInfo::to_discovered)
            .collect()
    }

    fn usb_devices(&self) -> Vec<DiscoveredDevice> {
        list_usb_devices()
            .iter()
            .map(UsbInfo::to_discovered)
            .collect()
    }

    fn ble_devices(&self, listen_for: Duration) -> Vec<DiscoveredDevice> {
        scan_ble(listen_for)
            .iter()
            .map(BleInfo::to_discovered)
            .collect()
    }

    fn mdns_services(&self, service_type: &str, listen_for: Duration) -> Vec<DiscoveredDevice> {
        browse_mdns(&[service_type], listen_for)
    }
}

/// The buses one plugin may look at: what [`Buses`] offers, narrowed by its manifest filters.
#[derive(Clone)]
pub struct ScopedBuses {
    buses: Arc<dyn Buses>,
    filters: Vec<DeviceFilter>,
}

impl std::fmt::Debug for ScopedBuses {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopedBuses")
            .field("filters", &self.filters)
            .finish_non_exhaustive()
    }
}

/// A listening time from a plugin, capped at [`MAX_LISTEN`].
fn listen(ms: u32) -> Duration {
    Duration::from_millis(u64::from(ms)).min(MAX_LISTEN)
}

/// True for names like `_plotter._tcp.local.`: nothing a plugin could use to aim the lookup at
/// anything but the local network's service announcements.
fn plain_service_type(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('_') else {
        return false;
    };
    let Some(label) = rest
        .strip_suffix("._tcp.local.")
        .or_else(|| rest.strip_suffix("._udp.local."))
    else {
        return false;
    };
    !label.is_empty()
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl ScopedBuses {
    /// The buses `filters` allow, over `buses`.
    #[must_use]
    pub fn new(buses: Arc<dyn Buses>, filters: Vec<DeviceFilter>) -> Self {
        Self { buses, filters }
    }

    fn allows(&self, transport: TransportKind) -> bool {
        self.filters.iter().any(|f| f.transport == transport)
    }

    fn visible(&self, found: Vec<DiscoveredDevice>) -> Vec<DiscoveredDevice> {
        found
            .into_iter()
            .filter(|d| self.filters.iter().any(|f| f.matches(d)))
            .collect()
    }

    /// The serial ports the manifest allows.
    #[must_use]
    pub fn serial_ports(&self) -> Vec<DiscoveredDevice> {
        if !self.allows(TransportKind::Serial) {
            return Vec::new();
        }
        self.visible(self.buses.serial_ports())
    }

    /// The USB devices the manifest allows.
    #[must_use]
    pub fn usb_devices(&self) -> Vec<DiscoveredDevice> {
        if !self.allows(TransportKind::Usb) {
            return Vec::new();
        }
        self.visible(self.buses.usb_devices())
    }

    /// Bluetooth LE peripherals heard for `listen_ms` (capped), if the manifest has a Bluetooth
    /// filter.
    #[must_use]
    pub fn ble_devices(&self, listen_ms: u32) -> Vec<DiscoveredDevice> {
        if !self.allows(TransportKind::BluetoothLe) {
            return Vec::new();
        }
        self.visible(self.buses.ble_devices(listen(listen_ms)))
    }

    /// Machines on the local network announcing `service_type` for `listen_ms` (capped), if the
    /// manifest has a TCP filter and the name is a plain service type.
    #[must_use]
    pub fn mdns_services(&self, service_type: &str, listen_ms: u32) -> Vec<DiscoveredDevice> {
        if !self.allows(TransportKind::Tcp) || !plain_service_type(service_type) {
            return Vec::new();
        }
        self.visible(self.buses.mdns_services(service_type, listen(listen_ms)))
    }
}

#[cfg(test)]
mod tests;
