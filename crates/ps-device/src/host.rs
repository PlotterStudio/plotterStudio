//! The device host: plugin discovery, approval and connection.
//!
//! The host finds nothing by itself. With no plugin installed it cannot see, let alone connect to,
//! any machine; each plugin brings the knowledge of how to find its own hardware in
//! [`Plugin::discover`]. A refresh asks every plugin to discover, drops whatever a plugin reports
//! outside the filters in its manifest, and then asks the plugin's `identify` to confirm the model.
//! The first connection to a device asks the user; the answer is kept per plugin and serial number
//! and can be revoked. The list of devices is a `watch` channel that updates on every refresh, so
//! hot-plugging shows up without polling by the UI.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::watch;

use crate::driver::Driver;
use crate::handle::{HandleConfig, MachineHandle};
use crate::permissions::{PermissionStore, approval_prompt};
use crate::types::{DeviceError, DeviceInfo, TransportKind};

/// Identifies a discovered device for the length of a session.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub String);

/// Something discovery found, before any plugin has looked at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredDevice {
    /// The host's id for it.
    pub id: DeviceId,
    /// A name for it, such as the USB product string.
    pub name: String,
    /// How it is connected.
    pub transport: TransportKind,
    /// USB vendor and product ids, for USB devices.
    pub usb: Option<(u16, u16)>,
    /// A network address, for network devices.
    pub address: Option<String>,
    /// A serial number, when discovery can read one.
    pub serial: Option<String>,
}

/// A device a plugin declares it can drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceFilter {
    /// The transport.
    pub transport: TransportKind,
    /// USB vendor and product ids; `None` matches any USB device.
    pub usb: Option<(u16, u16)>,
}

impl DeviceFilter {
    /// True when a discovered device is one this filter allows the plugin to see.
    #[must_use]
    pub fn matches(&self, device: &DiscoveredDevice) -> bool {
        self.transport == device.transport && self.usb.is_none_or(|ids| device.usb == Some(ids))
    }
}

/// What a plugin declares about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifest {
    /// The plugin's id.
    pub id: String,
    /// The name shown to the user.
    pub name: String,
    /// The devices it may see.
    pub filters: Vec<DeviceFilter>,
    /// Internet hosts it declares (blocked until the user allows each).
    pub internet_hosts: Vec<String>,
}

/// An internet host a plugin's manifest declares, and whether the user has allowed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredHost {
    /// The plugin's id.
    pub plugin_id: String,
    /// The plugin's display name.
    pub plugin_name: String,
    /// The host name.
    pub host: String,
    /// Whether the user has allowed this plugin to reach it.
    pub allowed: bool,
}

/// A device plugin: finds its own hardware, confirms a model and opens a driver for it.
#[async_trait]
pub trait Plugin: Send + Sync {
    /// The plugin's manifest.
    fn manifest(&self) -> &PluginManifest;

    /// Looks for candidate devices on the buses the manifest filters allow. Called only when the
    /// user asks the host to look; may take seconds (a radio scan), so the host bounds it.
    async fn discover(&self) -> Vec<DiscoveredDevice>;

    /// Confirms what a matching device is; `None` when the plugin does not recognise it.
    async fn identify(&self, device: &DiscoveredDevice) -> Option<DeviceInfo>;

    /// Opens a driver for a device the user approved.
    async fn open(&self, device: &DiscoveredDevice) -> Result<Box<dyn Driver>, DeviceError>;

    /// Tells the plugin the user allowed or blocked one of its declared internet hosts.
    fn set_host_allowed(&self, _host: &str, _allowed: bool) {}

    /// The internet hosts the plugin has reached, sorted.
    fn internet_in_use(&self) -> Vec<String> {
        Vec::new()
    }
}

/// A device in the host's list.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceEntry {
    /// The host's id.
    pub id: DeviceId,
    /// The plugin that recognised it.
    pub plugin_id: String,
    /// The plugin's display name.
    pub plugin_name: String,
    /// What the plugin says it is.
    pub info: DeviceInfo,
    /// Whether the user has approved this plugin for this device.
    pub approved: bool,
    /// The question to ask the user if not.
    pub prompt: String,
}

/// Why a connection was not made.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ConnectError {
    /// The host does not know this device.
    #[error("no such device: {0:?}")]
    NotFound(DeviceId),
    /// The user has not approved it yet.
    #[error("{prompt}")]
    NotApproved {
        /// The question to ask.
        prompt: String,
    },
    /// The plugin could not open it.
    #[error("could not open the device: {0}")]
    Open(DeviceError),
}

struct Known {
    entry: DeviceEntry,
    device: DiscoveredDevice,
    plugin: Arc<dyn Plugin>,
}

/// The device host.
pub struct DeviceHost {
    plugins: Mutex<Vec<Arc<dyn Plugin>>>,
    permissions: Arc<Mutex<PermissionStore>>,
    known: Mutex<Vec<Known>>,
    unclaimed: Mutex<Vec<DiscoveredDevice>>,
    list: watch::Sender<Vec<DeviceEntry>>,
    config: HandleConfig,
}

impl std::fmt::Debug for DeviceHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceHost")
            .field("devices", &self.list.borrow().len())
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The key an approval is stored under: the serial number, or the device id when it has none.
fn serial_of(device: &DiscoveredDevice) -> String {
    device.serial.clone().unwrap_or_else(|| device.id.0.clone())
}

/// How long one device gets to be recognised before the host moves on without it.
const IDENTIFY_LIMIT: Duration = Duration::from_secs(4);

/// How long one plugin gets to discover (a radio scan and a network browse, say) before the host
/// moves on without it.
const DISCOVER_LIMIT: Duration = Duration::from_secs(20);

/// A device and the plugins that reported it, in plugin order.
struct Candidate {
    device: DiscoveredDevice,
    reporters: Vec<Arc<dyn Plugin>>,
}

/// The first of the plugins that reported `device` that recognises it, with what it says.
async fn recognise(
    reporters: &[Arc<dyn Plugin>],
    device: &DiscoveredDevice,
) -> Option<(Arc<dyn Plugin>, DeviceInfo)> {
    for plugin in reporters {
        if let Some(info) = plugin.identify(device).await {
            return Some((Arc::clone(plugin), info));
        }
    }
    None
}

/// Asks every plugin to discover side by side, each within [`DISCOVER_LIMIT`], and groups what
/// they report by device id. A plugin is trusted only for devices its manifest filters allow.
async fn discover_all(plugins: &[Arc<dyn Plugin>]) -> Vec<Candidate> {
    let mut asking = tokio::task::JoinSet::new();
    for (index, plugin) in plugins.iter().cloned().enumerate() {
        asking.spawn(async move {
            let found = tokio::time::timeout(DISCOVER_LIMIT, plugin.discover())
                .await
                .unwrap_or_default();
            (index, found)
        });
    }
    let mut reported: Vec<Vec<DiscoveredDevice>> = vec![Vec::new(); plugins.len()];
    while let Some(done) = asking.join_next().await {
        if let Ok((index, found)) = done {
            reported[index] = found;
        }
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    for (plugin, found) in plugins.iter().zip(reported) {
        for device in found {
            if !plugin.manifest().filters.iter().any(|f| f.matches(&device)) {
                continue;
            }
            match candidates.iter_mut().find(|c| c.device.id == device.id) {
                Some(existing) => {
                    if !existing.reporters.iter().any(|p| Arc::ptr_eq(p, plugin)) {
                        existing.reporters.push(Arc::clone(plugin));
                    }
                }
                None => candidates.push(Candidate {
                    device,
                    reporters: vec![Arc::clone(plugin)],
                }),
            }
        }
    }
    candidates.sort_by(|a, b| a.device.id.cmp(&b.device.id));
    candidates
}

impl DeviceHost {
    /// A host with no plugins or discovery sources yet.
    #[must_use]
    pub fn new(permissions: PermissionStore, config: HandleConfig) -> Self {
        let (list, _) = watch::channel(Vec::new());
        Self {
            plugins: Mutex::new(Vec::new()),
            permissions: Arc::new(Mutex::new(permissions)),
            known: Mutex::new(Vec::new()),
            unclaimed: Mutex::new(Vec::new()),
            list,
            config,
        }
    }

    /// Registers a plugin; earlier plugins get the first chance to recognise a device.
    pub fn add_plugin(&self, plugin: Arc<dyn Plugin>) {
        // Hosts the user allowed in an earlier session apply straight away.
        let manifest = plugin.manifest();
        for host in &manifest.internet_hosts {
            if lock(&self.permissions).is_host_allowed(&manifest.id, host) {
                plugin.set_host_allowed(host, true);
            }
        }
        lock(&self.plugins).push(plugin);
    }

    /// Every internet host any plugin declares, with whether the user allowed it.
    #[must_use]
    pub fn declared_hosts(&self) -> Vec<DeclaredHost> {
        let plugins = lock(&self.plugins).clone();
        let store = lock(&self.permissions);
        plugins
            .iter()
            .flat_map(|p| {
                let m = p.manifest();
                m.internet_hosts.iter().map(|h| DeclaredHost {
                    plugin_id: m.id.clone(),
                    plugin_name: m.name.clone(),
                    host: h.clone(),
                    allowed: store.is_host_allowed(&m.id, h),
                })
            })
            .collect()
    }

    fn set_host(&self, plugin_id: &str, host: &str, allowed: bool) -> bool {
        let plugin = lock(&self.plugins)
            .iter()
            .find(|p| p.manifest().id == plugin_id)
            .cloned();
        let Some(plugin) = plugin else {
            return false;
        };
        if !plugin.manifest().internet_hosts.iter().any(|h| h == host) {
            return false;
        }
        {
            let mut store = lock(&self.permissions);
            if allowed {
                store.allow_host(plugin_id, host);
            } else {
                store.revoke_host(plugin_id, host);
            }
        }
        plugin.set_host_allowed(host, allowed);
        true
    }

    /// Allows a plugin to reach one of the hosts it declared; false if it did not declare it.
    pub fn allow_host(&self, plugin_id: &str, host: &str) -> bool {
        self.set_host(plugin_id, host, true)
    }

    /// Blocks a declared host again; false if the plugin did not declare it.
    pub fn revoke_host(&self, plugin_id: &str, host: &str) -> bool {
        self.set_host(plugin_id, host, false)
    }

    /// The internet hosts plugins are using, as `(plugin name, host)`, for the machine panel.
    #[must_use]
    pub fn internet_in_use(&self) -> Vec<(String, String)> {
        lock(&self.plugins)
            .iter()
            .flat_map(|p| {
                let name = p.manifest().name.clone();
                p.internet_in_use()
                    .into_iter()
                    .map(move |h| (name.clone(), h))
            })
            .collect()
    }

    /// The device list, updated on every refresh.
    #[must_use]
    pub fn devices(&self) -> watch::Receiver<Vec<DeviceEntry>> {
        self.list.subscribe()
    }

    /// The live approval book, shared with transport factories that record certificate pins.
    #[must_use]
    pub fn shared_permissions(&self) -> Arc<Mutex<PermissionStore>> {
        Arc::clone(&self.permissions)
    }

    /// A copy of the approvals, for saving to settings.
    #[must_use]
    pub fn permissions(&self) -> PermissionStore {
        lock(&self.permissions).clone()
    }

    /// What plugins found on the last refresh but could not confirm, so the user can be told why
    /// a machine they plugged in is not offered.
    #[must_use]
    pub fn unclaimed(&self) -> Vec<DiscoveredDevice> {
        lock(&self.unclaimed).clone()
    }

    /// Asks every installed plugin to look for its machines and rebuilds the list.
    pub async fn refresh(&self) {
        let plugins: Vec<Arc<dyn Plugin>> = lock(&self.plugins).clone();
        let candidates = discover_all(&plugins).await;
        let found: Vec<DiscoveredDevice> = candidates.iter().map(|c| c.device.clone()).collect();

        // Every device is asked at once, each with its own time limit: one that never answers (a
        // Bluetooth serial port whose machine is away, say) must not hold up or hide the rest.
        let mut asking = tokio::task::JoinSet::new();
        for (index, candidate) in candidates.into_iter().enumerate() {
            asking.spawn(async move {
                let answer = tokio::time::timeout(
                    IDENTIFY_LIMIT,
                    recognise(&candidate.reporters, &candidate.device),
                )
                .await
                .ok()
                .flatten();
                (index, answer)
            });
        }
        let mut answers: Vec<Option<(Arc<dyn Plugin>, DeviceInfo)>> = vec![None; found.len()];
        while let Some(done) = asking.join_next().await {
            if let Ok((index, answer)) = done {
                answers[index] = answer;
            }
        }

        let mut known = Vec::new();
        let mut unclaimed = Vec::new();
        for (device, answer) in found.into_iter().zip(answers) {
            let Some((plugin, info)) = answer else {
                unclaimed.push(device);
                continue;
            };
            let manifest = plugin.manifest();
            let approved =
                lock(&self.permissions).is_device_allowed(&manifest.id, &serial_of(&device));
            known.push(Known {
                entry: DeviceEntry {
                    id: device.id.clone(),
                    plugin_id: manifest.id.clone(),
                    plugin_name: manifest.name.clone(),
                    prompt: approval_prompt(&manifest.name, &info.model, info.transport),
                    info,
                    approved,
                },
                device,
                plugin: Arc::clone(&plugin),
            });
        }
        *lock(&self.unclaimed) = unclaimed;
        let entries: Vec<DeviceEntry> = known.iter().map(|k| k.entry.clone()).collect();
        *lock(&self.known) = known;
        self.list.send_if_modified(|current| {
            let changed = *current != entries;
            if changed {
                *current = entries;
            }
            changed
        });
    }

    fn set_approval(&self, id: &DeviceId, allow: bool) {
        let target = lock(&self.known)
            .iter()
            .find(|k| &k.entry.id == id)
            .map(|k| (k.entry.plugin_id.clone(), serial_of(&k.device)));
        let Some((plugin, serial)) = target else {
            return;
        };
        {
            let mut permissions = lock(&self.permissions);
            if allow {
                permissions.allow_device(&plugin, &serial);
            } else {
                permissions.revoke_device(&plugin, &serial);
            }
        }
        for k in lock(&self.known)
            .iter_mut()
            .filter(|k| k.entry.plugin_id == plugin && serial_of(&k.device) == serial)
        {
            k.entry.approved = allow;
        }
        let entries: Vec<DeviceEntry> = lock(&self.known).iter().map(|k| k.entry.clone()).collect();
        self.list.send_replace(entries);
    }

    /// Records the user's approval of a device for its plugin.
    pub fn approve(&self, id: &DeviceId) {
        self.set_approval(id, true);
    }

    /// Withdraws a device's approval.
    pub fn revoke(&self, id: &DeviceId) {
        self.set_approval(id, false);
    }

    /// Opens a session with an approved device.
    ///
    /// # Errors
    ///
    /// [`ConnectError`] when the device is unknown, not yet approved, or cannot be opened.
    pub async fn connect(&self, id: &DeviceId) -> Result<MachineHandle, ConnectError> {
        let (plugin, device, approved, prompt) = {
            let known = lock(&self.known);
            let k = known
                .iter()
                .find(|k| &k.entry.id == id)
                .ok_or_else(|| ConnectError::NotFound(id.clone()))?;
            (
                Arc::clone(&k.plugin),
                k.device.clone(),
                k.entry.approved,
                k.entry.prompt.clone(),
            )
        };
        if !approved {
            return Err(ConnectError::NotApproved { prompt });
        }
        let driver = plugin.open(&device).await.map_err(ConnectError::Open)?;
        Ok(MachineHandle::spawn(driver, self.config))
    }
}

#[cfg(test)]
mod tests;
