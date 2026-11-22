//! Device permissions.
//!
//! The host owns all hardware access, and the first connection to a device asks the user. The
//! answer is stored per plugin and device serial number and can be revoked. A plugin may also
//! declare named internet hosts (for machines reached through a vendor cloud); each is blocked
//! until the user allows it. The store is plain data with an atomic JSON file.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::TransportKind;

/// What the user has approved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionStore {
    #[serde(default)]
    devices: BTreeSet<(String, String)>,
    #[serde(default)]
    hosts: BTreeSet<(String, String)>,
    /// Pinned certificate fingerprints by `plugin|serial` (self-signed machines).
    #[serde(default)]
    pins: BTreeMap<String, String>,
}

/// Why the store could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PermissionError {
    /// The file is not a valid permission store.
    #[error("the permissions file is not valid: {0}")]
    Parse(String),
    /// A file operation failed.
    #[error("permissions file error: {0}")]
    Io(String),
}

impl PermissionStore {
    /// Records that a plugin may use a device.
    pub fn allow_device(&mut self, plugin: &str, serial: &str) {
        self.devices.insert((plugin.to_owned(), serial.to_owned()));
    }

    /// Withdraws a device approval.
    pub fn revoke_device(&mut self, plugin: &str, serial: &str) {
        self.devices.remove(&(plugin.to_owned(), serial.to_owned()));
    }

    /// Whether a plugin may use a device.
    #[must_use]
    pub fn is_device_allowed(&self, plugin: &str, serial: &str) -> bool {
        self.devices
            .contains(&(plugin.to_owned(), serial.to_owned()))
    }

    /// Records that a plugin may reach an internet host.
    pub fn allow_host(&mut self, plugin: &str, host: &str) {
        self.hosts.insert((plugin.to_owned(), host.to_owned()));
    }

    /// Withdraws a host approval.
    pub fn revoke_host(&mut self, plugin: &str, host: &str) {
        self.hosts.remove(&(plugin.to_owned(), host.to_owned()));
    }

    /// Whether a plugin may reach an internet host.
    #[must_use]
    pub fn is_host_allowed(&self, plugin: &str, host: &str) -> bool {
        self.hosts.contains(&(plugin.to_owned(), host.to_owned()))
    }

    fn pin_key(plugin: &str, serial: &str) -> String {
        format!("{plugin}|{serial}")
    }

    /// The certificate fingerprint pinned for a plugin's device, if any.
    #[must_use]
    pub fn pin(&self, plugin: &str, serial: &str) -> Option<&str> {
        self.pins
            .get(&Self::pin_key(plugin, serial))
            .map(String::as_str)
    }

    /// Pins a device's certificate fingerprint (the first approved connection).
    pub fn set_pin(&mut self, plugin: &str, serial: &str, fingerprint: &str) {
        self.pins
            .insert(Self::pin_key(plugin, serial), fingerprint.to_owned());
    }

    /// Forgets a device's pin, so the next approved connection pins afresh.
    pub fn clear_pin(&mut self, plugin: &str, serial: &str) {
        self.pins.remove(&Self::pin_key(plugin, serial));
    }

    /// Every approved `(plugin, serial)`, sorted.
    #[must_use]
    pub fn devices(&self) -> Vec<(String, String)> {
        self.devices.iter().cloned().collect()
    }

    /// Every approved `(plugin, host)`, sorted.
    #[must_use]
    pub fn hosts(&self) -> Vec<(String, String)> {
        self.hosts.iter().cloned().collect()
    }

    /// Reads the store; a missing file is an empty store.
    ///
    /// # Errors
    ///
    /// [`PermissionError`] when the file exists but cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self, PermissionError> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                serde_json::from_str(&text).map_err(|e| PermissionError::Parse(e.to_string()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(PermissionError::Io(e.to_string())),
        }
    }

    /// Writes the store atomically, creating the folder if needed.
    ///
    /// # Errors
    ///
    /// [`PermissionError`] when the file cannot be written.
    pub fn save(&self, path: &Path) -> Result<(), PermissionError> {
        let io = |e: std::io::Error| PermissionError::Io(e.to_string());
        let json =
            serde_json::to_vec_pretty(self).map_err(|e| PermissionError::Parse(e.to_string()))?;
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(dir).map_err(io)?;
        let mut temp = NamedTempFile::new_in(dir).map_err(io)?;
        temp.write_all(&json).map_err(io)?;
        temp.as_file().sync_all().map_err(io)?;
        temp.persist(path).map_err(|e| io(e.error))?;
        Ok(())
    }
}

/// The question shown the first time a plugin is asked to use a device.
#[must_use]
pub fn approval_prompt(plugin_name: &str, model: &str, transport: TransportKind) -> String {
    let on = match transport {
        TransportKind::Usb => "USB",
        TransportKind::Serial => "a serial port",
        TransportKind::BluetoothLe => "Bluetooth",
        TransportKind::Tcp
        | TransportKind::Udp
        | TransportKind::WebSocket
        | TransportKind::Http => "the network",
    };
    format!("Allow {plugin_name} to use {model} on {on}?")
}

#[cfg(test)]
mod tests;
