//! Wireless discovery and links: machines announced over mDNS on the local
//! network, and Bluetooth LE peripherals with a GATT transport.
//!
//! Scanning is a deliberate act by the user's request; nothing here runs on its own. A radio or a
//! network that is missing or refuses just yields no devices.

use std::net::IpAddr;
use std::time::{Duration, Instant};

use futures::StreamExt as _;

use crate::host::{DeviceId, DiscoveredDevice};
use crate::link::is_local_address;
use crate::types::TransportKind;
use crate::wasm::BlockingTransport;

/// The part of an mDNS full name before the service type: the machine's own name.
#[must_use]
pub fn instance_name(full_name: &str) -> &str {
    full_name
        .split_once("._")
        .map_or(full_name, |(instance, _)| instance)
}

/// A device for a resolved mDNS service: the first address on the local network and the port.
/// `None` when no address is local or the service names no port.
#[must_use]
pub fn device_from_service(
    full_name: &str,
    addresses: &[IpAddr],
    port: u16,
) -> Option<DiscoveredDevice> {
    if port == 0 {
        return None;
    }
    let ip = addresses.iter().find(|a| is_local_address(**a))?;
    let address = match ip {
        IpAddr::V4(v4) => format!("{v4}:{port}"),
        IpAddr::V6(v6) => format!("[{v6}]:{port}"),
    };
    Some(DiscoveredDevice {
        id: DeviceId(format!("mdns:{full_name}")),
        name: instance_name(full_name).to_owned(),
        transport: TransportKind::Tcp,
        usb: None,
        address: Some(address),
        serial: None,
    })
}

/// Browses the local network for `service_types` (such as `_plotter._tcp.local.`) for
/// `listen_for` and returns the machines that answered; empty when the network refuses.
#[must_use]
pub fn browse_mdns(service_types: &[&str], listen_for: Duration) -> Vec<DiscoveredDevice> {
    let Ok(daemon) = mdns_sd::ServiceDaemon::new() else {
        return Vec::new();
    };
    let receivers: Vec<_> = service_types
        .iter()
        .filter_map(|ty| daemon.browse(ty).ok())
        .collect();
    let deadline = Instant::now() + listen_for;
    let mut found: Vec<DiscoveredDevice> = Vec::new();
    while Instant::now() < deadline {
        for receiver in &receivers {
            while let Ok(event) = receiver.try_recv() {
                if let mdns_sd::ServiceEvent::ServiceResolved(service) = event {
                    let addresses: Vec<IpAddr> = service
                        .get_addresses()
                        .iter()
                        .map(mdns_sd::ScopedIp::to_ip_addr)
                        .collect();
                    if let Some(device) =
                        device_from_service(service.get_fullname(), &addresses, service.get_port())
                        && !found.iter().any(|d| d.id == device.id)
                    {
                        found.push(device);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = daemon.shutdown();
    found
}

/// A Bluetooth LE peripheral seen while scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BleInfo {
    /// Its address.
    pub address: String,
    /// Its advertised name, if any.
    pub name: Option<String>,
    /// The services it advertises, as UUID text.
    pub services: Vec<String>,
}

impl BleInfo {
    /// The host's view of this peripheral.
    #[must_use]
    pub fn to_discovered(&self) -> DiscoveredDevice {
        DiscoveredDevice {
            id: DeviceId(format!("ble:{}", self.address)),
            name: self
                .name
                .clone()
                .unwrap_or_else(|| format!("Bluetooth device {}", self.address)),
            transport: TransportKind::BluetoothLe,
            usb: None,
            address: Some(self.address.clone()),
            serial: None,
        }
    }
}

/// A UUID from its full text or from a 16-bit short form on the Bluetooth base UUID.
#[must_use]
pub fn parse_uuid(text: &str) -> Option<uuid::Uuid> {
    let text = text.trim();
    if text.len() == 4 && text.chars().all(|c| c.is_ascii_hexdigit()) {
        return uuid::Uuid::parse_str(&format!("0000{text}-0000-1000-8000-00805f9b34fb")).ok();
    }
    uuid::Uuid::parse_str(text).ok()
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())
}

async fn first_adapter() -> Result<btleplug::platform::Adapter, String> {
    use btleplug::api::Manager as _;
    let manager = btleplug::platform::Manager::new()
        .await
        .map_err(|e| e.to_string())?;
    manager
        .adapters()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .next()
        .ok_or_else(|| "this computer has no Bluetooth adapter".to_owned())
}

/// Scans for Bluetooth LE peripherals for `listen_for`; empty when there is no usable radio.
#[must_use]
pub fn scan_ble(listen_for: Duration) -> Vec<BleInfo> {
    // The scan needs a runtime of its own, and starting one from a thread that already drives a
    // runtime (the device host's) panics. A thread of its own has no runtime to clash with.
    std::thread::spawn(move || scan_ble_here(listen_for))
        .join()
        .unwrap_or_default()
}

fn scan_ble_here(listen_for: Duration) -> Vec<BleInfo> {
    use btleplug::api::{Central as _, Peripheral as _, ScanFilter};
    let Ok(rt) = runtime() else {
        return Vec::new();
    };
    rt.block_on(async {
        let adapter = first_adapter().await?;
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|e| e.to_string())?;
        tokio::time::sleep(listen_for).await;
        let mut out = Vec::new();
        for peripheral in adapter.peripherals().await.map_err(|e| e.to_string())? {
            if let Ok(Some(props)) = peripheral.properties().await {
                out.push(BleInfo {
                    address: props.address.to_string(),
                    name: props.local_name.or(props.advertisement_name),
                    services: props.services.iter().map(ToString::to_string).collect(),
                });
            }
        }
        let _ = adapter.stop_scan().await;
        Ok::<_, String>(out)
    })
    .unwrap_or_default()
}

/// A connected Bluetooth LE peripheral: writes and notifications on GATT characteristics.
pub struct BleTransport {
    rt: tokio::runtime::Runtime,
    peripheral: btleplug::platform::Peripheral,
    notifications:
        std::pin::Pin<Box<dyn futures::Stream<Item = btleplug::api::ValueNotification> + Send>>,
    subscribed: Vec<uuid::Uuid>,
}

impl std::fmt::Debug for BleTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BleTransport").finish_non_exhaustive()
    }
}

impl BleTransport {
    /// Connects to the peripheral with this address.
    ///
    /// # Errors
    ///
    /// A message when there is no radio, the peripheral is not found, or it cannot be connected.
    pub fn connect(address: &str, scan_for: Duration) -> Result<Self, String> {
        use btleplug::api::{Central as _, Peripheral as _, ScanFilter};
        let rt = runtime()?;
        let (peripheral, notifications) = rt.block_on(async {
            let adapter = first_adapter().await?;
            adapter
                .start_scan(ScanFilter::default())
                .await
                .map_err(|e| e.to_string())?;
            tokio::time::sleep(scan_for).await;
            let mut found = None;
            for p in adapter.peripherals().await.map_err(|e| e.to_string())? {
                if p.address().to_string().eq_ignore_ascii_case(address) {
                    found = Some(p);
                    break;
                }
            }
            let _ = adapter.stop_scan().await;
            let peripheral =
                found.ok_or_else(|| format!("no Bluetooth device {address} in range"))?;
            peripheral.connect().await.map_err(|e| e.to_string())?;
            peripheral
                .discover_services()
                .await
                .map_err(|e| e.to_string())?;
            let notifications = peripheral
                .notifications()
                .await
                .map_err(|e| e.to_string())?;
            Ok::<_, String>((peripheral, notifications))
        })?;
        Ok(Self {
            rt,
            peripheral,
            notifications,
            subscribed: Vec::new(),
        })
    }

    fn characteristic(&self, text: &str) -> Result<btleplug::api::Characteristic, String> {
        use btleplug::api::Peripheral as _;
        let wanted = parse_uuid(text).ok_or_else(|| format!("{text:?} is not a UUID"))?;
        self.peripheral
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == wanted)
            .ok_or_else(|| format!("the device has no characteristic {wanted}"))
    }
}

impl BlockingTransport for BleTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::BluetoothLe
    }

    fn write(&mut self, _data: &[u8]) -> Result<(), String> {
        Err("a Bluetooth LE link writes to characteristics, not raw bytes".to_owned())
    }

    fn read(&mut self, _max: usize, _timeout: Duration) -> Result<Vec<u8>, String> {
        Err("a Bluetooth LE link reads notifications, not raw bytes".to_owned())
    }

    fn ble_write(&mut self, characteristic: &str, data: &[u8]) -> Result<(), String> {
        use btleplug::api::{Peripheral as _, WriteType};
        let c = self.characteristic(characteristic)?;
        self.rt
            .block_on(self.peripheral.write(&c, data, WriteType::WithoutResponse))
            .map_err(|e| e.to_string())
    }

    fn ble_notify(&mut self, characteristic: &str, timeout: Duration) -> Result<Vec<u8>, String> {
        use btleplug::api::Peripheral as _;
        let c = self.characteristic(characteristic)?;
        if !self.subscribed.contains(&c.uuid) {
            self.rt
                .block_on(self.peripheral.subscribe(&c))
                .map_err(|e| e.to_string())?;
            self.subscribed.push(c.uuid);
        }
        let wanted = c.uuid;
        let next = async {
            while let Some(n) = self.notifications.next().await {
                if n.uuid == wanted {
                    return Some(n.value);
                }
            }
            None
        };
        Ok(self
            .rt
            .block_on(async { tokio::time::timeout(timeout, next).await })
            .ok()
            .flatten()
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests;
