//! Installed WebAssembly plugins as device-host plugins.
//!
//! A [`WasmPlugin`] is made from a `.psdevice` package: its manifest says which buses it may look
//! at, its component decides what to look for there and what a candidate is, and its driver is what
//! the machine handle runs. The host lists the buses (through [`Buses`]) and opens each device's
//! transport itself (through a [`TransportFactory`]), so the plugin never names a port or an
//! address it was not shown.

use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;

use crate::bus::{Buses, ScopedBuses, SystemBuses};
use crate::driver::Driver;
use crate::host::{DiscoveredDevice, Plugin, PluginManifest};
use crate::link::{
    HttpTransport, SerialTransport, TcpTransport, UdpTransport, UsbTransport, WebSocketTransport,
};
use crate::package::{PackageError, unpack};
use crate::tls::{HostGate, InternetAccess, PublicHttpsOpener};
use crate::types::{DeviceError, DeviceInfo, TransportKind};
use crate::wasm::{BlockingTransport, WasmHost, WasmLimits};
use crate::wireless::BleTransport;

/// The baud rate serial machines are opened at unless a plugin's device says otherwise.
const DEFAULT_BAUD: u32 = 115_200;

/// Opens the byte link for a discovered device.
pub trait TransportFactory: Send + Sync {
    /// Opens the device's transport.
    ///
    /// # Errors
    ///
    /// A [`DeviceError`] when the device cannot be reached.
    fn open(&self, device: &DiscoveredDevice) -> Result<Box<dyn BlockingTransport>, DeviceError>;
}

/// Opens a network link to `host:port` from a device address.
fn network<T: BlockingTransport + 'static>(
    address: &str,
    open: impl FnOnce(SocketAddr, &[u16]) -> Result<T, String>,
) -> Result<Box<dyn BlockingTransport>, DeviceError> {
    let addr: SocketAddr = address
        .parse()
        .map_err(|_| DeviceError::new("bad-address", format!("{address:?} is not host:port")))?;
    open(addr, &[addr.port()])
        .map(|t| Box::new(t) as Box<dyn BlockingTransport>)
        .map_err(|m| DeviceError::new("open-failed", m))
}

/// The transports this build can open: serial ports and TCP sockets on the local network.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTransports;

impl TransportFactory for SystemTransports {
    fn open(&self, device: &DiscoveredDevice) -> Result<Box<dyn BlockingTransport>, DeviceError> {
        if device.transport == TransportKind::BluetoothLe {
            let address = device
                .address
                .as_deref()
                .ok_or_else(|| DeviceError::new("no-address", "the device has no address"))?;
            return BleTransport::connect(address, std::time::Duration::from_secs(2))
                .map(|t| Box::new(t) as Box<dyn BlockingTransport>)
                .map_err(|m| DeviceError::new("open-failed", m));
        }
        if device.transport == TransportKind::Usb {
            let (vendor, product) = device
                .usb
                .ok_or_else(|| DeviceError::new("no-address", "the USB device has no ids"))?;
            return UsbTransport::open(vendor, product, device.serial.as_deref())
                .map(|t| Box::new(t) as Box<dyn BlockingTransport>)
                .map_err(|m| DeviceError::new("open-failed", m));
        }
        let address = device
            .address
            .as_deref()
            .ok_or_else(|| DeviceError::new("no-address", "the device has no port or address"))?;
        match device.transport {
            TransportKind::Serial => SerialTransport::open(address, DEFAULT_BAUD)
                .map(|t| Box::new(t) as Box<dyn BlockingTransport>)
                .map_err(|m| DeviceError::new("open-failed", m)),
            TransportKind::Tcp => {
                let addr: SocketAddr = address.parse().map_err(|_| {
                    DeviceError::new("bad-address", format!("{address:?} is not host:port"))
                })?;
                TcpTransport::connect(addr, &[addr.port()])
                    .map(|t| Box::new(t) as Box<dyn BlockingTransport>)
                    .map_err(|m| DeviceError::new("open-failed", m))
            }
            TransportKind::Udp => network(address, UdpTransport::connect),
            TransportKind::Http => network(address, HttpTransport::connect),
            TransportKind::WebSocket => network(address, WebSocketTransport::connect),
            other => Err(DeviceError::new(
                "transport-unsupported",
                format!("{other:?} links are not available in this build"),
            )),
        }
    }
}

/// A device plugin that runs a WebAssembly component.
pub struct WasmPlugin {
    manifest: PluginManifest,
    version: String,
    wasm: Vec<u8>,
    host: Arc<WasmHost>,
    transports: Arc<dyn TransportFactory>,
    buses: Arc<dyn Buses>,
    internet: InternetAccess,
}

impl std::fmt::Debug for WasmPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmPlugin")
            .field("manifest", &self.manifest)
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl WasmPlugin {
    /// Makes a plugin from a `.psdevice` package.
    ///
    /// # Errors
    ///
    /// A [`PackageError`] when the package is not valid.
    pub fn from_package(
        package: &[u8],
        host: Arc<WasmHost>,
        transports: Arc<dyn TransportFactory>,
    ) -> Result<Self, PackageError> {
        Self::from_package_with(package, host, |_| transports)
    }

    /// Makes a plugin whose transport factory is built from its own manifest, for factories that
    /// need the plugin id (certificate pins are kept per plugin).
    ///
    /// # Errors
    ///
    /// A [`PackageError`] when the package is not valid.
    pub fn from_package_with(
        package: &[u8],
        host: Arc<WasmHost>,
        transports: impl FnOnce(&PluginManifest) -> Arc<dyn TransportFactory>,
    ) -> Result<Self, PackageError> {
        let unpacked = unpack(package)?;
        let transports = transports(&unpacked.manifest);
        // Only the hosts the manifest declares can ever be allowed; each starts blocked.
        let internet = InternetAccess::new(
            HostGate::new(unpacked.manifest.internet_hosts.clone(), Vec::new()),
            Arc::new(PublicHttpsOpener),
        );
        Ok(Self {
            manifest: unpacked.manifest,
            version: unpacked.version,
            wasm: unpacked.wasm,
            host,
            transports,
            buses: Arc::new(SystemBuses),
            internet,
        })
    }

    /// Lets the plugin look at `buses` instead of the system's, for tests.
    #[must_use]
    pub fn with_buses(mut self, buses: Arc<dyn Buses>) -> Self {
        self.buses = buses;
        self
    }

    /// Makes a plugin with the default limits and the system's transports.
    ///
    /// # Errors
    ///
    /// A message when the package is invalid or the engine cannot start.
    pub fn install(package: &[u8]) -> Result<Self, String> {
        let host = WasmHost::new(WasmLimits::default()).map_err(|e| e.to_string())?;
        Self::from_package(package, Arc::new(host), Arc::new(SystemTransports))
            .map_err(|e| e.to_string())
    }

    /// The plugin's own version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }
}

#[async_trait]
impl Plugin for WasmPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn discover(&self) -> Vec<DiscoveredDevice> {
        // Listing a bus can block (a radio scan), and the plugin runs on a blocking thread inside
        // `WasmHost::discover`, so nothing here stalls the async threads.
        let scope = ScopedBuses::new(Arc::clone(&self.buses), self.manifest.filters.clone());
        match self.host.discover(&self.wasm, scope).await {
            Ok(found) => found,
            Err(error) => {
                tracing::warn!(plugin = %self.manifest.id, %error, "discovery failed");
                Vec::new()
            }
        }
    }

    async fn identify(&self, device: &DiscoveredDevice) -> Option<DeviceInfo> {
        // Opening a port can block for a long time (a Bluetooth serial port whose machine is
        // away), so it happens off the async threads, where it cannot stall other devices.
        let transports = Arc::clone(&self.transports);
        let target = device.clone();
        let transport = tokio::task::spawn_blocking(move || transports.open(&target).ok())
            .await
            .ok()??;
        self.host.identify(&self.wasm, transport).await
    }

    async fn open(&self, device: &DiscoveredDevice) -> Result<Box<dyn Driver>, DeviceError> {
        let transport = self.transports.open(device)?;
        let driver = self
            .host
            .open_with(&self.wasm, transport, Some(self.internet.clone()))
            .await?;
        Ok(Box::new(driver))
    }

    fn set_host_allowed(&self, host: &str, allowed: bool) {
        self.internet.set_allowed(host, allowed);
    }

    fn internet_in_use(&self) -> Vec<String> {
        self.internet.in_use()
    }
}

#[cfg(test)]
mod tests;
