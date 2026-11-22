//! A reference driver for GRBL-style machines, the worked example for plugin authors.
//!
//! GRBL takes `ps-gcode/1` lines almost as written, answers each with `ok` or `error:N`, has a
//! 127-byte receive buffer (so flow control counts characters), and takes realtime commands as
//! single bytes: `?` status, `!` hold, `~` resume, Ctrl-X soft reset.

use std::collections::VecDeque;
use std::time::Duration;

use async_trait::async_trait;
use ps_gcode::Command;

use crate::driver::{Driver, DriverEvent};
use crate::transport::Transport;
use crate::types::{
    Capabilities, DeviceError, DeviceInfo, GcodeLine, JobHeader, MachineState, PowerState, Status,
    UserAction, UserActionKind,
};
use ps_device_sdk::{AckFlow, LineBuffer};

/// GRBL's serial receive buffer in bytes.
const RX_BUFFER: usize = 127;
/// The soft-reset realtime byte (Ctrl-X).
const SOFT_RESET: u8 = 0x18;
/// How long to wait for a reply to a status request.
const REPLY_WAIT: Duration = Duration::from_millis(500);
/// The feed used for jogs, in mm/min.
const JOG_FEED: u32 = 1_000;

/// Millimetres with three decimals from micrometres, as GRBL expects them.
fn mm3(um: i64) -> String {
    let sign = if um < 0 { "-" } else { "" };
    let a = um.unsigned_abs();
    format!("{sign}{}.{:03}", a / 1000, a % 1000)
}

/// The GRBL line for a parsed command; `None` when GRBL has no form for it (comments, tool
/// changes, heaters). The tangent axis is dropped because GRBL has none.
#[must_use]
pub fn translate(command: &Command) -> Option<String> {
    Some(match command {
        Command::Rapid { to } => format!("G0 X{} Y{}", mm3(to.x), mm3(to.y)),
        Command::Linear {
            to, feed_mm_min, ..
        } => format!("G1 X{} Y{} F{feed_mm_min}", mm3(to.x), mm3(to.y)),
        Command::Arc {
            to,
            centre_offset,
            ccw,
            feed_mm_min,
        } => format!(
            "{} X{} Y{} I{} J{} F{feed_mm_min}",
            if *ccw { "G3" } else { "G2" },
            mm3(to.x),
            mm3(to.y),
            mm3(centre_offset.x),
            mm3(centre_offset.y)
        ),
        Command::Dwell { ms } => format!("G4 P{}.{:03}", ms / 1000, ms % 1000),
        Command::ToolDown { force } => format!("M3 S{force}"),
        Command::ToolUp => "M5".to_owned(),
        Command::Units { millimetres } => if *millimetres { "G21" } else { "G20" }.to_owned(),
        Command::Absolute => "G90".to_owned(),
        Command::Pause(_) => "M0".to_owned(),
        Command::End => "M2".to_owned(),
        Command::Comment(_) | Command::ToolChange { .. } | Command::Temperature { .. } => {
            return None;
        }
    })
}

/// GRBL 1.1's `error:N` meanings (the common ones).
fn error_message(code: u32) -> &'static str {
    match code {
        1 => "G-code words consist of a letter and a value. Letter was not found",
        2 => "Numeric value format is not valid or missing an expected value",
        3 => "Grbl '$' system command was not recognized or supported",
        8 => "'$' command cannot be used unless Grbl is IDLE",
        9 => "G-code locked out during alarm or jog state",
        20 => "Unsupported or invalid g-code command found in block",
        22 => "Feed rate has not yet been set or is undefined",
        _ => "GRBL rejected the line",
    }
}

/// What a line in flight means when its `ok` arrives.
#[derive(Debug)]
enum Marker {
    Plain,
    Pause(String),
    End,
}

/// A driver for GRBL over any [`Transport`].
#[derive(Debug)]
pub struct GrblDriver<T> {
    transport: T,
    version: String,
    flow: AckFlow,
    in_flight: VecDeque<Marker>,
    lines: LineBuffer,
    events: VecDeque<DriverEvent>,
    next_action: u32,
}

impl<T: Transport> GrblDriver<T> {
    /// A driver for a machine reporting firmware `version`.
    #[must_use]
    pub fn new(transport: T, version: &str) -> Self {
        Self {
            transport,
            version: version.to_owned(),
            flow: AckFlow::new(RX_BUFFER),
            in_flight: VecDeque::new(),
            lines: LineBuffer::default(),
            events: VecDeque::new(),
            next_action: 1,
        }
    }

    /// Handles bytes from the machine: queues events and returns the last status report seen.
    fn process(&mut self, bytes: &[u8]) -> Option<Status> {
        let mut status = None;
        for line in self.lines.push(bytes) {
            if line.starts_with('<') {
                status = Some(parse_status(&line));
            } else if line == "ok" {
                self.acknowledged();
            } else if let Some(code) = line.strip_prefix("error:") {
                let n = code.trim().parse::<u32>().unwrap_or(0);
                let _ = self.flow.acked();
                let _ = self.in_flight.pop_front();
                self.events.push_back(DriverEvent::Fault(DeviceError::new(
                    format!("grbl-error-{n}"),
                    error_message(n),
                )));
            } else if let Some(code) = line.strip_prefix("ALARM:") {
                let n = code.trim().parse::<u32>().unwrap_or(0);
                self.events.push_back(DriverEvent::Fault(DeviceError::new(
                    format!("grbl-alarm-{n}"),
                    "GRBL raised an alarm and stopped",
                )));
            }
        }
        status
    }

    fn acknowledged(&mut self) {
        let _ = self.flow.acked();
        match self.in_flight.pop_front() {
            Some(Marker::Pause(message)) => {
                let id = self.next_action;
                self.next_action += 1;
                self.events
                    .push_back(DriverEvent::UserActionRequired(UserAction {
                        id,
                        kind: UserActionKind::Confirm,
                        message,
                    }));
            }
            Some(Marker::End) => self.events.push_back(DriverEvent::JobFinished),
            Some(Marker::Plain) | None => {}
        }
    }

    fn forget_in_flight(&mut self) {
        self.flow.reset();
        self.in_flight.clear();
    }
}

/// Reads a `<State|MPos:x,y,z|...>` report.
fn parse_status(report: &str) -> Status {
    let inner = report.trim_start_matches('<').trim_end_matches('>');
    let mut fields = inner.split('|');
    let state_word = fields.next().unwrap_or("");
    let name = state_word.split(':').next().unwrap_or("");
    let state = match name {
        "Idle" => MachineState::Idle,
        "Run" | "Jog" => MachineState::RunningJob,
        "Hold" | "Door" => MachineState::Paused,
        "Home" | "Check" | "Sleep" => MachineState::Busy,
        "Alarm" => MachineState::Fault(DeviceError::new("grbl-alarm", "GRBL is in alarm")),
        _ => MachineState::Unknown,
    };
    let mut status = Status::new(state);
    status.power = PowerState::On;
    for field in fields {
        if let Some(pos) = field
            .strip_prefix("MPos:")
            .or_else(|| field.strip_prefix("WPos:"))
        {
            let mut parts = pos.split(',').map(|p| p.trim().parse::<f64>());
            if let (Some(Ok(x)), Some(Ok(y))) = (parts.next(), parts.next()) {
                status.position_mm = Some((x, y));
            }
        }
    }
    status
}

/// Provokes GRBL's startup banner with a soft reset and reads what the machine is.
pub async fn identify(transport: &mut impl Transport) -> Option<DeviceInfo> {
    transport.write(&[SOFT_RESET]).await.ok()?;
    let mut lines = LineBuffer::default();
    loop {
        let bytes = transport.read(256, REPLY_WAIT).await.ok()?;
        if bytes.is_empty() {
            return None;
        }
        for line in lines.push(&bytes) {
            if let Some(rest) = line.strip_prefix("Grbl ") {
                let version = rest.split_whitespace().next()?.to_owned();
                return Some(DeviceInfo {
                    model: format!("GRBL {version}"),
                    serial_number: None,
                    firmware: Some(version),
                    profile_id: None,
                    transport: transport.kind(),
                });
            }
        }
    }
}

#[async_trait]
impl<T: Transport> Driver for GrblDriver<T> {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            model: format!("GRBL {}", self.version),
            serial_number: None,
            firmware: Some(self.version.clone()),
            profile_id: None,
            transport: self.transport.kind(),
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            pause: true,
            resume: true,
            cancel: true,
            jog: true,
            reports_position: true,
            max_inflight_lines: 32,
            ..Capabilities::default()
        }
    }

    async fn status(&mut self) -> Result<Status, DeviceError> {
        self.transport.write(b"?").await?;
        loop {
            let bytes = self.transport.read(256, REPLY_WAIT).await?;
            if bytes.is_empty() {
                return Err(DeviceError::new(
                    "timeout",
                    "GRBL did not answer the status request",
                ));
            }
            if let Some(status) = self.process(&bytes) {
                return Ok(status);
            }
        }
    }

    async fn wait_event(&mut self, timeout: Duration) -> Option<DriverEvent> {
        if self.events.is_empty() {
            let bytes = self.transport.read(256, timeout).await.ok()?;
            if !bytes.is_empty() {
                self.process(&bytes);
            }
        }
        self.events.pop_front()
    }

    async fn begin_job(&mut self, _header: JobHeader) -> Result<(), DeviceError> {
        self.forget_in_flight();
        Ok(())
    }

    async fn send(&mut self, lines: &[GcodeLine]) -> Result<u32, DeviceError> {
        let mut out = Vec::new();
        let mut accepted = 0_u32;
        for line in lines {
            if let Some(text) = translate(&line.command) {
                if !self.flow.can_send(text.len()) {
                    break;
                }
                self.flow.sent(text.len());
                self.in_flight.push_back(match &line.command {
                    Command::Pause(message) => Marker::Pause(message.clone()),
                    Command::End => Marker::End,
                    _ => Marker::Plain,
                });
                out.extend_from_slice(text.as_bytes());
                out.push(b'\n');
            }
            accepted += 1;
        }
        if !out.is_empty() {
            self.transport.write(&out).await?;
        }
        Ok(accepted)
    }

    async fn end_job(&mut self) -> Result<(), DeviceError> {
        Ok(())
    }

    async fn pause(&mut self) -> Result<(), DeviceError> {
        self.transport.write(b"!").await
    }

    async fn resume(&mut self) -> Result<(), DeviceError> {
        self.transport.write(b"~").await
    }

    async fn cancel(&mut self) -> Result<(), DeviceError> {
        self.transport.write(&[SOFT_RESET]).await?;
        self.forget_in_flight();
        Ok(())
    }

    async fn user_action_done(&mut self, _id: u32) -> Result<(), DeviceError> {
        self.transport.write(b"~").await
    }

    async fn jog(&mut self, x_mm: f64, y_mm: f64) -> Result<(), DeviceError> {
        let line = format!("$J=G91 G21 X{x_mm:.3} Y{y_mm:.3} F{JOG_FEED}\n");
        self.transport.write(line.as_bytes()).await
    }

    async fn disconnect(&mut self) {}
}

#[cfg(test)]
mod tests;
