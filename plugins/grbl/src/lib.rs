//! The GRBL reference device plugin, a WebAssembly component.
//!
//! It translates the host's parsed steps into GRBL lines, counts characters against GRBL's
//! 127-byte receive buffer, and reads `ok`, `error:N` and status reports. It never parses G-code:
//! every line arrives already parsed, with the exact text beside it.

use std::cell::RefCell;
use std::collections::VecDeque;

use ps_device_sdk::{AckFlow, LineBuffer};

wit_bindgen::generate!({
    path: "../../crates/ps-device/wit",
    world: "device-plugin",
});

use exports::plotter::device::driver::{Guest, GuestSession};
use plotter::device::transport::Connection;
use plotter::device::types::{
    Capabilities, DeviceError, DeviceInfo, Event, GcodeLine, JobHeader, MachineState, PowerState,
    Status, Step, UserAction,
};

/// GRBL's serial receive buffer in bytes.
const RX_BUFFER: usize = 127;
/// The soft-reset realtime byte (Ctrl-X).
const SOFT_RESET: u8 = 0x18;
/// How long to wait for a reply to a request, in milliseconds.
const REPLY_MS: u32 = 500;

struct Component;

fn millimetres(um: i64) -> String {
    let sign = if um < 0 { "-" } else { "" };
    let a = um.unsigned_abs();
    format!("{sign}{}.{:03}", a / 1000, a % 1000)
}

/// The GRBL line for a step; `None` when GRBL has no form for it.
fn translate(step: &Step) -> Option<String> {
    Some(match step {
        Step::Travel(to) => format!("G0 X{} Y{}", millimetres(to.x), millimetres(to.y)),
        Step::Line(l) => format!(
            "G1 X{} Y{} F{}",
            millimetres(l.to.x),
            millimetres(l.to.y),
            l.feed_mm_min
        ),
        Step::Arc(a) => format!(
            "{} X{} Y{} I{} J{} F{}",
            if a.ccw { "G3" } else { "G2" },
            millimetres(a.to.x),
            millimetres(a.to.y),
            millimetres(a.centre_offset.x),
            millimetres(a.centre_offset.y),
            a.feed_mm_min
        ),
        Step::Dwell(ms) => format!("G4 P{}.{:03}", ms / 1000, ms % 1000),
        Step::ToolDown(force) => format!("M3 S{force}"),
        Step::ToolUp => "M5".to_owned(),
        Step::Units(mm) => if *mm { "G21" } else { "G20" }.to_owned(),
        Step::Absolute => "G90".to_owned(),
        Step::Pause(_) => "M0".to_owned(),
        Step::End => "M2".to_owned(),
        Step::Comment(_) | Step::ToolChange(_) | Step::Temperature(_) => return None,
    })
}

fn error(code: &str, message: &str) -> DeviceError {
    DeviceError {
        code: code.to_owned(),
        message: message.to_owned(),
        recoverable: false,
    }
}

fn grbl_error_message(code: u32) -> &'static str {
    match code {
        20 => "Unsupported or invalid g-code command found in block",
        22 => "Feed rate has not yet been set or is undefined",
        _ => "GRBL rejected the line",
    }
}

enum Marker {
    Plain,
    Pause(String),
    End,
}

struct State {
    conn: Connection,
    flow: AckFlow,
    in_flight: VecDeque<Marker>,
    lines: LineBuffer,
    events: VecDeque<Event>,
    next_action: u32,
    version: String,
    /// The last line of the job was accepted; the job is finished once GRBL reports `Idle`.
    finishing: bool,
}

struct GrblSession {
    state: RefCell<State>,
}

fn idle_status(state: MachineState) -> Status {
    Status {
        power: PowerState::On,
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

fn parse_status(report: &str) -> Status {
    let inner = report.trim_start_matches('<').trim_end_matches('>');
    let mut fields = inner.split('|');
    let name = fields.next().unwrap_or("").split(':').next().unwrap_or("");
    let state = match name {
        "Idle" => MachineState::Idle,
        "Run" | "Jog" => MachineState::RunningJob,
        "Hold" | "Door" => MachineState::Paused,
        "Home" | "Check" | "Sleep" => MachineState::Busy,
        "Alarm" => MachineState::Fault(error("grbl-alarm", "GRBL is in alarm")),
        _ => MachineState::Unknown,
    };
    let mut status = idle_status(state);
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

impl State {
    fn process(&mut self, bytes: &[u8]) -> Option<Status> {
        let mut status = None;
        for line in self.lines.push(bytes) {
            if line.starts_with('<') {
                let report = parse_status(&line);
                if self.finishing && matches!(report.state, MachineState::Idle) {
                    self.finishing = false;
                    self.events.push_back(Event::JobFinished);
                }
                status = Some(report);
            } else if line == "ok" {
                self.acknowledged();
            } else if let Some(code) = line.strip_prefix("error:") {
                let n = code.trim().parse::<u32>().unwrap_or(0);
                let _ = self.flow.acked();
                let _ = self.in_flight.pop_front();
                self.finishing = false;
                self.events.push_back(Event::Fault(error(
                    &format!("grbl-error-{n}"),
                    grbl_error_message(n),
                )));
            } else if let Some(code) = line.strip_prefix("ALARM:") {
                let n = code.trim().parse::<u32>().unwrap_or(0);
                // GRBL flushes its buffer on an alarm and answers none of the queued lines, so
                // nothing is in flight any more.
                self.flow.reset();
                self.in_flight.clear();
                self.finishing = false;
                self.events.push_back(Event::Fault(error(
                    &format!("grbl-alarm-{n}"),
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
                    .push_back(Event::UserActionRequired(UserAction { id, message }));
            }
            // `ok` only says the planner took the line; its moves may still be running.
            Some(Marker::End) => self.finishing = true,
            Some(Marker::Plain) | None => {}
        }
    }

    fn write(&self, bytes: &[u8]) -> Result<(), DeviceError> {
        self.conn.write(bytes).map_err(|e| error("io", &e.message))
    }
}

impl Guest for Component {
    type Session = GrblSession;

    /// Every serial port is a candidate: a self-made GRBL board is only a USB-serial adapter, with
    /// nothing in its ids that says what is behind it, so `identify` asks it. USB devices are left
    /// alone; nothing GRBL runs on shows up as one.
    fn discover() -> Vec<String> {
        plotter::device::discovery::list_serial_ports()
            .into_iter()
            .map(|c| c.id)
            .collect()
    }

    fn identify(conn: &Connection) -> Option<DeviceInfo> {
        conn.write(&[SOFT_RESET]).ok()?;
        let mut lines = LineBuffer::default();
        loop {
            let bytes = conn.read(256, REPLY_MS).ok()?;
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
                        transport: conn.kind(),
                    });
                }
            }
        }
    }
}

impl GuestSession for GrblSession {
    fn new(conn: Connection) -> Self {
        Self {
            state: RefCell::new(State {
                conn,
                flow: AckFlow::new(RX_BUFFER),
                in_flight: VecDeque::new(),
                lines: LineBuffer::default(),
                events: VecDeque::new(),
                next_action: 1,
                version: "1.1".to_owned(),
                finishing: false,
            }),
        }
    }

    fn info(&self) -> DeviceInfo {
        let s = self.state.borrow();
        DeviceInfo {
            model: format!("GRBL {}", s.version),
            serial_number: None,
            firmware: Some(s.version.clone()),
            profile_id: None,
            transport: s.conn.kind(),
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            pause: true,
            resume: true,
            cancel: true,
            jog: true,
            home: false,
            load_mat: false,
            unload_mat: false,
            tool_detection: false,
            reports_position: true,
            reports_temperature: false,
            max_inflight_lines: 32,
        }
    }

    fn status(&self) -> Status {
        let mut s = self.state.borrow_mut();
        if let Err(e) = s.write(b"?") {
            return idle_status(MachineState::Fault(e));
        }
        loop {
            let bytes = match s.conn.read(256, REPLY_MS) {
                Ok(b) => b,
                Err(e) => return idle_status(MachineState::Fault(error("io", &e.message))),
            };
            if bytes.is_empty() {
                return idle_status(MachineState::Fault(error(
                    "timeout",
                    "GRBL did not answer the status request",
                )));
            }
            if let Some(status) = s.process(&bytes) {
                return status;
            }
        }
    }

    fn wait_event(&self, timeout_ms: u32) -> Option<Event> {
        let mut s = self.state.borrow_mut();
        if s.events.is_empty() {
            if s.finishing
                && let Err(e) = s.write(b"?")
            {
                return Some(Event::Fault(e));
            }
            match s.conn.read(256, timeout_ms) {
                Ok(bytes) if !bytes.is_empty() => {
                    s.process(&bytes);
                }
                Ok(_) => {}
                Err(e) => return Some(Event::Fault(error("io", &e.message))),
            }
        }
        s.events.pop_front()
    }

    fn begin_job(&self, _header: JobHeader) -> Result<(), DeviceError> {
        let mut s = self.state.borrow_mut();
        s.flow.reset();
        s.in_flight.clear();
        s.finishing = false;
        Ok(())
    }

    fn send(&self, lines: Vec<GcodeLine>) -> Result<u32, DeviceError> {
        let mut s = self.state.borrow_mut();
        let mut out = Vec::new();
        let mut accepted = 0_u32;
        for line in &lines {
            if let Some(text) = translate(&line.step) {
                if !s.flow.can_send(text.len()) {
                    break;
                }
                s.flow.sent(text.len());
                s.in_flight.push_back(match &line.step {
                    Step::Pause(message) => Marker::Pause(message.clone()),
                    Step::End => Marker::End,
                    _ => Marker::Plain,
                });
                out.extend_from_slice(text.as_bytes());
                out.push(b'\n');
            }
            accepted += 1;
        }
        if !out.is_empty() {
            s.write(&out)?;
        }
        Ok(accepted)
    }

    fn end_job(&self) -> Result<(), DeviceError> {
        Ok(())
    }

    fn pause(&self) -> Result<(), DeviceError> {
        self.state.borrow().write(b"!")
    }

    fn resume(&self) -> Result<(), DeviceError> {
        self.state.borrow().write(b"~")
    }

    fn cancel(&self) -> Result<(), DeviceError> {
        let mut s = self.state.borrow_mut();
        s.write(&[SOFT_RESET])?;
        s.flow.reset();
        s.in_flight.clear();
        s.finishing = false;
        Ok(())
    }

    fn user_action_done(&self, _id: u32) -> Result<(), DeviceError> {
        self.state.borrow().write(b"~")
    }

    fn jog(&self, dx_mm: f64, dy_mm: f64) -> Result<(), DeviceError> {
        let mut s = self.state.borrow_mut();
        // GRBL answers a jog like any other line, so one that is not tracked would be taken for
        // the acknowledgement of the next job line; and jogging under a running job is never
        // wanted.
        if !s.in_flight.is_empty() || s.finishing {
            return Err(DeviceError {
                code: "busy".to_owned(),
                message: "the machine is running a job".to_owned(),
                recoverable: true,
            });
        }
        let line = format!("$J=G91 G21 X{dx_mm:.3} Y{dy_mm:.3} F1000");
        if !s.flow.can_send(line.len()) {
            return Err(error("busy", "GRBL's receive buffer is full"));
        }
        s.write(format!("{line}\n").as_bytes())?;
        s.flow.sent(line.len());
        s.in_flight.push_back(Marker::Plain);
        Ok(())
    }

    fn disconnect(&self) {}
}

export!(Component);
