//! The data the device host and its plugins exchange: what a device is, what it can
//! do, and what state it is in. These mirror the WIT `types` interface one for one.

use std::time::{Duration, Instant};

use ps_gcode::Command;

/// A snapshot older than this is shown as stale.
pub const STALE_AFTER: Duration = Duration::from_secs(2);

/// How the host reaches a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportKind {
    /// USB bulk, interrupt and control transfers.
    Usb,
    /// A serial port (CDC, FTDI, Bluetooth Classic SPP).
    Serial,
    /// Bluetooth Low Energy.
    BluetoothLe,
    /// A TCP socket.
    Tcp,
    /// A UDP socket.
    Udp,
    /// A WebSocket.
    WebSocket,
    /// HTTP or HTTPS.
    Http,
}

/// What a plugin says a device is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// The model name.
    pub model: String,
    /// The serial number, when the device reports one.
    pub serial_number: Option<String>,
    /// The firmware version, when reported.
    pub firmware: Option<String>,
    /// The machine profile id the device matches, when reported.
    pub profile_id: Option<String>,
    /// How it is connected.
    pub transport: TransportKind,
}

/// What a device can do; the UI enables its buttons from this.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "a capability list mirrors the WIT record one flag at a time"
)]
pub struct Capabilities {
    /// Can pause a running job.
    pub pause: bool,
    /// Can resume a paused job.
    pub resume: bool,
    /// Can cancel a job.
    pub cancel: bool,
    /// Can jog the carriage.
    pub jog: bool,
    /// Can home.
    pub home: bool,
    /// Can load a mat.
    pub load_mat: bool,
    /// Can unload a mat.
    pub unload_mat: bool,
    /// Detects the tool in each holder.
    pub tool_detection: bool,
    /// Reports the carriage position.
    pub reports_position: bool,
    /// Reports a temperature.
    pub reports_temperature: bool,
    /// Most lines the host may keep unaccepted at once.
    pub max_inflight_lines: u32,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            pause: false,
            resume: false,
            cancel: false,
            jog: false,
            home: false,
            load_mat: false,
            unload_mat: false,
            tool_detection: false,
            reports_position: false,
            reports_temperature: false,
            max_inflight_lines: 1,
        }
    }
}

/// Power as the machine reports it (read-only in v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    /// On.
    On,
    /// Standby.
    Standby,
    /// Off.
    Off,
    /// Not reported.
    Unknown,
}

/// The state of the host's transport to a device, owned by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// No session.
    Disconnected,
    /// Opening a session.
    Connecting,
    /// A session is up.
    Connected,
    /// The device stopped answering; the session is dropped.
    Lost,
}

/// What a user is being asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserActionKind {
    /// Load a tool into a holder.
    LoadTool,
    /// Place or remove an overlay sheet.
    Overlay,
    /// Unload the mat.
    UnloadMat,
    /// Just confirm.
    Confirm,
}

/// A prompt that needs the user before the job continues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserAction {
    /// Identifies the prompt for `user_action_done`.
    pub id: u32,
    /// What kind of prompt.
    pub kind: UserActionKind,
    /// The text to show.
    pub message: String,
}

/// A fault reported by a device, a plugin or the transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceError {
    /// A stable code.
    pub code: String,
    /// What happened.
    pub message: String,
}

impl DeviceError {
    /// A device error.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for DeviceError {}

/// What the machine is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineState {
    /// Ready for a job.
    Idle,
    /// Busy with something else (homing, loading).
    Busy,
    /// Running a job.
    RunningJob,
    /// A job is paused.
    Paused,
    /// Waiting for the user.
    WaitingForUser(UserAction),
    /// A fault.
    Fault(DeviceError),
    /// Updating its firmware.
    UpdatingFirmware,
    /// Not reported.
    Unknown,
}

/// How a job is going, from the host's cursor plus the plugin's detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobProgress {
    /// Lines the machine has accepted.
    pub lines_done: u64,
    /// Lines in the program.
    pub lines_total: u64,
    /// What the machine reports it is doing, if anything.
    pub current_op: Option<String>,
}

/// How serious an alert is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertLevel {
    /// Information.
    Info,
    /// A warning.
    Warning,
    /// An error.
    Error,
}

/// A message from the machine or its plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    /// How serious.
    pub level: AlertLevel,
    /// A stable code.
    pub code: String,
    /// The text.
    pub message: String,
}

/// A tool a holder reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HolderStatus {
    /// The holder id.
    pub holder: String,
    /// The tool detected there, when the machine can tell.
    pub tool: Option<String>,
}

/// What a plugin reports; the host adds the connection state.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    /// Power.
    pub power: PowerState,
    /// What the machine is doing.
    pub state: MachineState,
    /// The plugin's progress detail.
    pub progress: Option<JobProgress>,
    /// Whether a mat is loaded, if the machine can tell.
    pub mat_loaded: Option<bool>,
    /// Tools detected in each holder.
    pub holders: Vec<HolderStatus>,
    /// The carriage position in mm.
    pub position_mm: Option<(f64, f64)>,
    /// A temperature in °C.
    pub temperature_c: Option<f64>,
    /// Battery charge, for battery-powered machines.
    pub battery_percent: Option<u8>,
    /// Alerts raised.
    pub alerts: Vec<Alert>,
}

impl Status {
    /// A status with every optional field unreported.
    #[must_use]
    pub fn new(state: MachineState) -> Self {
        Self {
            power: PowerState::Unknown,
            state,
            progress: None,
            mat_loaded: None,
            holders: Vec::new(),
            position_mm: None,
            temperature_c: None,
            battery_percent: None,
            alerts: Vec::new(),
        }
    }
}

/// The snapshot the UI reads every frame: the plugin's status plus the host's connection state.
#[derive(Debug, Clone, PartialEq)]
pub struct MachineStatus {
    /// Owned by the host's transport layer.
    pub connection: ConnectionState,
    /// Power.
    pub power: PowerState,
    /// What the machine is doing.
    pub state: MachineState,
    /// Progress: the host's cursor plus the plugin's detail.
    pub progress: Option<JobProgress>,
    /// Whether a mat is loaded.
    pub mat_loaded: Option<bool>,
    /// Tools detected in each holder.
    pub holders: Vec<HolderStatus>,
    /// The carriage position in mm.
    pub position_mm: Option<(f64, f64)>,
    /// A temperature in °C.
    pub temperature_c: Option<f64>,
    /// Battery charge.
    pub battery_percent: Option<u8>,
    /// Alerts raised.
    pub alerts: Vec<Alert>,
    /// When this snapshot was taken.
    pub updated: Instant,
}

impl MachineStatus {
    /// A disconnected machine with nothing reported.
    #[must_use]
    pub fn initial(now: Instant) -> Self {
        Self::from_status(
            ConnectionState::Disconnected,
            Status::new(MachineState::Unknown),
            now,
        )
    }

    /// Combines a plugin status with the host's connection state.
    #[must_use]
    pub fn from_status(connection: ConnectionState, s: Status, now: Instant) -> Self {
        Self {
            connection,
            power: s.power,
            state: s.state,
            progress: s.progress,
            mat_loaded: s.mat_loaded,
            holders: s.holders,
            position_mm: s.position_mm,
            temperature_c: s.temperature_c,
            battery_percent: s.battery_percent,
            alerts: s.alerts,
            updated: now,
        }
    }

    /// True when the snapshot is older than [`STALE_AFTER`] at `now`.
    #[must_use]
    pub fn is_stale(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.updated) > STALE_AFTER
    }
}

/// Why a job may not start now.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunRefusal {
    /// There is no live session.
    #[error("the machine is not connected")]
    NotConnected,
    /// The machine is doing something else.
    #[error("the machine is not idle")]
    NotIdle,
    /// The program is not valid `ps-gcode/1`.
    #[error("the program cannot be run: {0}")]
    InvalidProgram(String),
    /// The job was compiled for a different machine profile.
    #[error(
        "this machine reports profile {machine_reports}, but the job was compiled for {job_compiled_for}; select the {machine_reports} profile and export again"
    )]
    ProfileMismatch {
        /// The profile id the machine reports.
        machine_reports: String,
        /// The profile id the job was compiled for.
        job_compiled_for: String,
    },
}

/// The preconditions for starting a job: connected, idle, and the reported profile matches.
///
/// # Errors
///
/// The first [`RunRefusal`] that applies.
pub fn check_can_run(
    status: &MachineStatus,
    info: &DeviceInfo,
    job_profile_id: &str,
) -> Result<(), RunRefusal> {
    if status.connection != ConnectionState::Connected {
        return Err(RunRefusal::NotConnected);
    }
    if status.state != MachineState::Idle {
        return Err(RunRefusal::NotIdle);
    }
    if let Some(reported) = &info.profile_id
        && reported != job_profile_id
    {
        return Err(RunRefusal::ProfileMismatch {
            machine_reports: reported.clone(),
            job_compiled_for: job_profile_id.to_owned(),
        });
    }
    Ok(())
}

/// The header handed to a plugin at the start of a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobHeader {
    /// The job id.
    pub job_id: String,
    /// The 1-based mat position.
    pub mat_index: u32,
    /// The profile id the job was compiled for.
    pub profile_id: String,
    /// The tools of the mat, by slot, as `(slot, tool id, holder id)`.
    pub tools: Vec<(u8, String, String)>,
}

/// One line of the program: the exact text plus the host's parsed form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcodeLine {
    /// The 1-based line number.
    pub number: u32,
    /// The line as written.
    pub text: String,
    /// What it means.
    pub command: Command,
}

#[cfg(test)]
mod tests;
