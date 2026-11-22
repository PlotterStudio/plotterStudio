//! Names in the published WIT contract (`wit/ps-device.wit`) for the host's Rust types.
//!
//! The functions are exhaustive matches, so adding a Rust case fails to compile until the contract
//! has a name for it; `tests/wit.rs` then checks that the WIT file really declares each name.

use ps_gcode::Command;

use crate::types::{AlertLevel, ConnectionState, MachineState, PowerState, TransportKind};

/// The WIT `connection-state` case for a connection state.
#[must_use]
pub fn connection_state(s: ConnectionState) -> &'static str {
    match s {
        ConnectionState::Disconnected => "disconnected",
        ConnectionState::Connecting => "connecting",
        ConnectionState::Connected => "connected",
        ConnectionState::Lost => "lost",
    }
}

/// The WIT `power-state` case.
#[must_use]
pub fn power_state(s: PowerState) -> &'static str {
    match s {
        PowerState::On => "on",
        PowerState::Standby => "standby",
        PowerState::Off => "off",
        PowerState::Unknown => "unknown",
    }
}

/// The WIT `transport-kind` case.
#[must_use]
pub fn transport_kind(t: TransportKind) -> &'static str {
    match t {
        TransportKind::Usb => "usb",
        TransportKind::Serial => "serial",
        TransportKind::BluetoothLe => "ble",
        TransportKind::Tcp => "tcp",
        TransportKind::Udp => "udp",
        TransportKind::WebSocket => "websocket",
        TransportKind::Http => "http",
    }
}

/// The WIT `alert-level` case.
#[must_use]
pub fn alert_level(l: AlertLevel) -> &'static str {
    match l {
        AlertLevel::Info => "info",
        AlertLevel::Warning => "warning",
        AlertLevel::Error => "error",
    }
}

/// The WIT `machine-state` case.
#[must_use]
pub fn machine_state_case(s: &MachineState) -> &'static str {
    match s {
        MachineState::Idle => "idle",
        MachineState::Busy => "busy",
        MachineState::RunningJob => "running-job",
        MachineState::Paused => "paused",
        MachineState::WaitingForUser(_) => "waiting-for-user",
        MachineState::Fault(_) => "fault",
        MachineState::UpdatingFirmware => "updating-firmware",
        MachineState::Unknown => "unknown",
    }
}

/// The WIT `step` case for a parsed command.
#[must_use]
pub fn step_case(c: &Command) -> &'static str {
    match c {
        Command::Comment(_) => "comment",
        Command::Units { .. } => "units",
        Command::Absolute => "absolute",
        Command::Rapid { .. } => "travel",
        Command::Linear { .. } => "line",
        Command::Arc { .. } => "arc",
        Command::Dwell { .. } => "dwell",
        Command::Pause(_) => "pause",
        Command::ToolChange { .. } => "tool-change",
        Command::ToolDown { .. } => "tool-down",
        Command::ToolUp => "tool-up",
        Command::Temperature { .. } => "temperature",
        Command::End => "end",
    }
}
