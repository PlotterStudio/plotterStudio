//! App settings.
//!
//! A `settings.toml` in the platform config directory: units, locale and decimal separator, theme,
//! shortcuts, default machine, autosave interval, network opt-ins (both off by default), GPU
//! quality, and recent files. Changes take effect live and never touch project files. A missing
//! file means defaults, unknown keys from a newer version are ignored, and a damaged file falls
//! back to defaults with a reason the app can show.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

const MAX_RECENT: usize = 10;
const MIN_AUTOSAVE_S: u32 = 5;
const MAX_AUTOSAVE_S: u32 = 3_600;

/// Units shown in the UI (the document stores millimetres whatever this says).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayUnits {
    /// Millimetres.
    #[default]
    Millimetres,
    /// Inches.
    Inches,
}

/// The colour theme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Follow the system.
    #[default]
    System,
    /// Light.
    Light,
    /// Dark.
    Dark,
}

/// How much GPU work the preview may do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuQuality {
    /// Cheapest preview.
    Low,
    /// A balance.
    Medium,
    /// Full quality.
    #[default]
    High,
}

/// The decimal separator used in number fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecimalSeparator {
    /// Follow the locale.
    #[default]
    Auto,
    /// A full stop.
    Dot,
    /// A comma.
    Comma,
}

/// Languages whose conventional decimal separator is a comma.
const COMMA_LANGUAGES: [&str; 19] = [
    "de", "fr", "es", "it", "pt", "nl", "ru", "pl", "sv", "da", "nb", "no", "fi", "cs", "tr", "uk",
    "hu", "ro", "el",
];

impl DecimalSeparator {
    /// The separator to use for `locale` (a language tag such as `de-DE`, or `system`).
    #[must_use]
    pub fn resolve(self, locale: &str) -> char {
        match self {
            Self::Dot => '.',
            Self::Comma => ',',
            Self::Auto => {
                let language = locale.split(['-', '_']).next().unwrap_or("");
                if COMMA_LANGUAGES.contains(&language.to_ascii_lowercase().as_str()) {
                    ','
                } else {
                    '.'
                }
            }
        }
    }
}

/// The internet features; each is off until the user opts in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Network {
    /// Check for app updates.
    pub update_check: bool,
    /// Fetch the signed material index.
    pub material_index: bool,
}

/// The port the automation server listens on unless the user picks another.
pub const DEFAULT_MCP_PORT: u16 = 8765;

/// The lowest port the user may choose: below it, listening needs special privileges on some
/// systems and the numbers belong to well-known services.
const MIN_MCP_PORT: u16 = 1024;

/// Driving the app from outside: the automation server that lets an MCP client (an AI assistant,
/// a script) use the app's tools over HTTP. Off until the user turns it on, and on this computer
/// only until they allow the network; every request needs the access token.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Automation {
    /// Whether the server is running.
    pub mcp_enabled: bool,
    /// The port it listens on.
    pub mcp_port: u16,
    /// Whether other computers on the network may connect, not just this one.
    pub mcp_allow_network: bool,
    /// The secret a client sends as `Authorization: Bearer <token>`. Empty until the server is
    /// first turned on.
    pub mcp_token: String,
}

impl Default for Automation {
    fn default() -> Self {
        Self {
            mcp_enabled: false,
            mcp_port: DEFAULT_MCP_PORT,
            mcp_allow_network: false,
            mcp_token: String::new(),
        }
    }
}

// By hand so the token is never printed, whatever logs or panic reports hold a `Settings`.
impl std::fmt::Debug for Automation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Automation")
            .field("mcp_enabled", &self.mcp_enabled)
            .field("mcp_port", &self.mcp_port)
            .field("mcp_allow_network", &self.mcp_allow_network)
            .field(
                "mcp_token",
                &if self.mcp_token.is_empty() {
                    "<none>"
                } else {
                    "<hidden>"
                },
            )
            .finish()
    }
}

/// All app settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Display units.
    pub units: DisplayUnits,
    /// A language tag, or `system`.
    pub locale: String,
    /// The decimal separator.
    pub decimal_separator: DecimalSeparator,
    /// The colour theme.
    pub theme: Theme,
    /// Shortcut overrides by command id.
    pub shortcuts: BTreeMap<String, String>,
    /// The machine profile id chosen by default.
    pub default_machine: Option<String>,
    /// Seconds between autosaves.
    pub autosave_interval_s: u32,
    /// Keep the previous version of a saved project as `.bak`.
    pub keep_backup: bool,
    /// Internet opt-ins.
    pub network: Network,
    /// The automation (MCP) server.
    pub automation: Automation,
    /// Preview quality.
    pub gpu_quality: GpuQuality,
    /// Recently opened files, newest first.
    pub recent_files: Vec<String>,
    /// Whether the first-run notice (what is and is not sent over the internet) was shown.
    pub first_run_seen: bool,
    /// A factor for displays that misreport their size, found by measuring a 100 mm ruler on the
    /// screen: 100% zoom shows true size when it is right.
    pub screen_calibration: f64,
}

/// The smallest accepted screen calibration.
pub const MIN_SCREEN_CALIBRATION: f64 = 0.5;
/// The largest accepted screen calibration.
pub const MAX_SCREEN_CALIBRATION: f64 = 2.0;

/// The calibration that makes a bar drawn at `current` calibration, which measured
/// `measured_mm` on the screen where 100 mm was meant, true size. `None` for a measurement that
/// is not a length or that would give a calibration outside the accepted range.
#[must_use]
pub fn calibration_from_measurement(current: f64, measured_mm: f64) -> Option<f64> {
    if !(measured_mm.is_finite() && measured_mm > 0.0) {
        return None;
    }
    let next = current * 100.0 / measured_mm;
    (MIN_SCREEN_CALIBRATION..=MAX_SCREEN_CALIBRATION)
        .contains(&next)
        .then_some(next)
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            units: DisplayUnits::default(),
            locale: "system".to_owned(),
            decimal_separator: DecimalSeparator::default(),
            theme: Theme::default(),
            shortcuts: BTreeMap::new(),
            default_machine: None,
            autosave_interval_s: 30,
            keep_backup: true,
            network: Network::default(),
            automation: Automation::default(),
            gpu_quality: GpuQuality::default(),
            recent_files: Vec::new(),
            first_run_seen: false,
            screen_calibration: 1.0,
        }
    }
}

/// Why settings could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    /// The file is not valid settings.
    #[error("settings.toml is not valid: {0}")]
    Parse(String),
    /// A file operation failed.
    #[error("settings file error: {0}")]
    Io(String),
}

impl Settings {
    /// Parses settings from TOML. Missing keys take defaults, unknown keys are ignored, and values
    /// are kept within sensible bounds.
    ///
    /// # Errors
    ///
    /// [`SettingsError::Parse`] for broken TOML or a value of the wrong type.
    pub fn from_toml(text: &str) -> Result<Self, SettingsError> {
        let mut s: Self = toml::from_str(text).map_err(|e| SettingsError::Parse(e.to_string()))?;
        s.autosave_interval_s = s.autosave_interval_s.clamp(MIN_AUTOSAVE_S, MAX_AUTOSAVE_S);
        s.recent_files.truncate(MAX_RECENT);
        if s.automation.mcp_port < MIN_MCP_PORT {
            s.automation.mcp_port = DEFAULT_MCP_PORT;
        }
        s.screen_calibration = if s.screen_calibration.is_finite() {
            s.screen_calibration
                .clamp(MIN_SCREEN_CALIBRATION, MAX_SCREEN_CALIBRATION)
        } else {
            1.0
        };
        Ok(s)
    }

    /// Serialises to TOML.
    ///
    /// # Errors
    ///
    /// [`SettingsError::Parse`] if serialisation fails.
    pub fn to_toml(&self) -> Result<String, SettingsError> {
        toml::to_string_pretty(self).map_err(|e| SettingsError::Parse(e.to_string()))
    }

    /// Reads the file; a missing file gives the defaults.
    ///
    /// # Errors
    ///
    /// [`SettingsError`] when the file exists but cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self, SettingsError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(SettingsError::Io(e.to_string())),
        }
    }

    /// Like [`Settings::load`], but a damaged file gives the defaults and the reason.
    #[must_use]
    pub fn load_or_default(path: &Path) -> (Self, Option<SettingsError>) {
        match Self::load(path) {
            Ok(s) => (s, None),
            Err(e) => (Self::default(), Some(e)),
        }
    }

    /// Writes the file atomically, creating the config folder if needed.
    ///
    /// # Errors
    ///
    /// [`SettingsError`] when the file cannot be written.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        use std::io::Write;
        let io = |e: std::io::Error| SettingsError::Io(e.to_string());
        let text = self.to_toml()?;
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(dir).map_err(io)?;
        let mut temp = NamedTempFile::new_in(dir).map_err(io)?;
        temp.write_all(text.as_bytes()).map_err(io)?;
        temp.as_file().sync_all().map_err(io)?;
        temp.persist(path).map_err(|e| io(e.error))?;
        Ok(())
    }

    /// Records a file as the most recently opened.
    pub fn push_recent(&mut self, path: &str) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_owned());
        self.recent_files.truncate(MAX_RECENT);
    }

    /// Removes a file from the recent list.
    pub fn forget_recent(&mut self, path: &str) {
        self.recent_files.retain(|p| p != path);
    }
}

#[cfg(test)]
mod tests;
