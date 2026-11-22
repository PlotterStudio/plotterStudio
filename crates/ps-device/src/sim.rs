//! A simulated machine: runs a job against a model of a plotter and records what
//! happened, so the host's flow control, prompts and fault handling can be tested without
//! hardware and a plugin can be checked with `--simulate`.
//!
//! The machine consumes accepted lines as time passes (each move costs `line_ms`, a dwell costs
//! its duration, setup lines are free), stops at `M0` until the user answers, and can be told to
//! refuse lines, report a fault, or stop answering at all.

use std::collections::VecDeque;
use std::time::Duration;

use async_trait::async_trait;
use ps_gcode::Command;
use tokio::time::Instant;

use crate::driver::{Driver, DriverEvent};
use crate::types::{
    Capabilities, DeviceError, DeviceInfo, GcodeLine, JobHeader, JobProgress, MachineState,
    PowerState, Status, TransportKind, UserAction, UserActionKind,
};

/// How the simulated machine behaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimConfig {
    /// The profile id it reports.
    pub profile_id: Option<String>,
    /// How many lines it buffers.
    pub max_inflight: u32,
    /// Milliseconds a move takes.
    pub line_ms: u64,
    /// Refuse `send` with an error after accepting this many lines.
    pub fail_send_after: Option<u32>,
    /// Stop answering `status` and `wait_event` after processing this many lines.
    pub stop_answering_after: Option<u32>,
    /// Report a machine fault after processing this many lines.
    pub fault_after: Option<u32>,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            profile_id: Some("generic.cutter-medium".to_owned()),
            max_inflight: 8,
            line_ms: 10,
            fail_send_after: None,
            stop_answering_after: None,
            fault_after: None,
        }
    }
}

/// One entry of the recorded timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineEntry {
    /// Milliseconds since the job began.
    pub at_ms: u64,
    /// The state it entered.
    pub state: MachineState,
    /// Lines processed so far.
    pub lines_processed: u64,
}

/// The simulated machine.
#[derive(Debug)]
pub struct SimulatedMachine {
    cfg: SimConfig,
    queue: VecDeque<GcodeLine>,
    received: Vec<u32>,
    processed: u64,
    accepted: u32,
    carry_ms: u64,
    last_tick: Option<Instant>,
    started: Option<Instant>,
    state: MachineState,
    events: VecDeque<DriverEvent>,
    begun: Option<JobHeader>,
    ended: bool,
    cancelled: bool,
    acked: Vec<u32>,
    pause_count: u32,
    resume_count: u32,
    max_queue: u32,
    next_action: u32,
    stalled: bool,
    timeline: Vec<TimelineEntry>,
}

impl SimulatedMachine {
    /// A machine that has not been given a job.
    #[must_use]
    pub fn new(cfg: SimConfig) -> Self {
        Self {
            cfg,
            queue: VecDeque::new(),
            received: Vec::new(),
            processed: 0,
            accepted: 0,
            carry_ms: 0,
            last_tick: None,
            started: None,
            state: MachineState::Idle,
            events: VecDeque::new(),
            begun: None,
            ended: false,
            cancelled: false,
            acked: Vec::new(),
            pause_count: 0,
            resume_count: 0,
            max_queue: 0,
            next_action: 1,
            stalled: false,
            timeline: Vec::new(),
        }
    }

    /// Line numbers in the order they were accepted.
    #[must_use]
    pub fn received_lines(&self) -> &[u32] {
        &self.received
    }

    /// The most lines it ever held unprocessed.
    #[must_use]
    pub fn max_queue(&self) -> u32 {
        self.max_queue
    }

    /// Whether the job was ended normally.
    #[must_use]
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// Whether the job was cancelled.
    #[must_use]
    pub fn cancelled(&self) -> bool {
        self.cancelled
    }

    /// The ids of the prompts the user answered.
    #[must_use]
    pub fn acked(&self) -> &[u32] {
        &self.acked
    }

    /// The header the job began with.
    #[must_use]
    pub fn begun(&self) -> Option<&JobHeader> {
        self.begun.as_ref()
    }

    /// How many times it was paused.
    #[must_use]
    pub fn pause_count(&self) -> u32 {
        self.pause_count
    }

    /// How many times it was resumed.
    #[must_use]
    pub fn resume_count(&self) -> u32 {
        self.resume_count
    }

    /// The recorded state timeline.
    #[must_use]
    pub fn timeline(&self) -> &[TimelineEntry] {
        &self.timeline
    }

    fn enter(&mut self, state: MachineState) {
        let at_ms = self.started.map_or(0, |s| {
            u64::try_from(s.elapsed().as_millis()).unwrap_or(u64::MAX)
        });
        self.state = state.clone();
        self.timeline.push(TimelineEntry {
            at_ms,
            state,
            lines_processed: self.processed,
        });
    }

    fn cost_ms(&self, command: &Command) -> u64 {
        match command {
            Command::Rapid { .. } | Command::Linear { .. } | Command::Arc { .. } => {
                self.cfg.line_ms
            }
            Command::Dwell { ms } => u64::from(*ms),
            _ => 0,
        }
    }

    /// Lets the machine consume what the elapsed time allows.
    fn advance(&mut self) {
        let now = Instant::now();
        let elapsed = self.last_tick.map_or(0, |t| {
            u64::try_from(now.duration_since(t).as_millis()).unwrap_or(0)
        });
        self.last_tick = Some(now);
        if self.state != MachineState::RunningJob || self.stalled {
            return;
        }
        self.carry_ms += elapsed;
        while let Some(front) = self.queue.front() {
            let cost = self.cost_ms(&front.command);
            if self.carry_ms < cost {
                break;
            }
            self.carry_ms -= cost;
            let Some(line) = self.queue.pop_front() else {
                break;
            };
            self.processed += 1;
            match line.command {
                Command::Pause(message) => {
                    let action = UserAction {
                        id: self.next_action,
                        kind: UserActionKind::Confirm,
                        message,
                    };
                    self.next_action += 1;
                    self.events
                        .push_back(DriverEvent::UserActionRequired(action.clone()));
                    self.enter(MachineState::WaitingForUser(action));
                    self.carry_ms = 0;
                    return;
                }
                Command::End => {
                    self.events.push_back(DriverEvent::JobFinished);
                    return;
                }
                _ => {}
            }
            let processed = u32::try_from(self.processed).unwrap_or(u32::MAX);
            if self.cfg.fault_after.is_some_and(|n| processed >= n) {
                let error = DeviceError::new("machine-fault", "the simulated machine faulted");
                self.events.push_back(DriverEvent::Fault(error.clone()));
                self.enter(MachineState::Fault(error));
                return;
            }
            if self
                .cfg
                .stop_answering_after
                .is_some_and(|n| processed >= n)
            {
                self.stalled = true;
                return;
            }
        }
    }

    fn current_status(&self) -> Status {
        let mut status = Status::new(self.state.clone());
        status.power = PowerState::On;
        status.progress = Some(JobProgress {
            lines_done: self.processed,
            lines_total: u64::from(self.accepted),
            current_op: None,
        });
        status
    }
}

#[async_trait]
impl Driver for SimulatedMachine {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            model: "Simulated plotter".to_owned(),
            serial_number: Some("SIM-0001".to_owned()),
            firmware: Some("sim 1.0".to_owned()),
            profile_id: self.cfg.profile_id.clone(),
            transport: TransportKind::Serial,
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            pause: true,
            resume: true,
            cancel: true,
            jog: true,
            max_inflight_lines: self.cfg.max_inflight,
            ..Capabilities::default()
        }
    }

    async fn status(&mut self) -> Result<Status, DeviceError> {
        if self.stalled {
            std::future::pending::<()>().await;
        }
        self.advance();
        Ok(self.current_status())
    }

    async fn wait_event(&mut self, timeout: Duration) -> Option<DriverEvent> {
        if self.stalled {
            std::future::pending::<()>().await;
        }
        self.advance();
        if let Some(event) = self.events.pop_front() {
            return Some(event);
        }
        if !timeout.is_zero() {
            tokio::time::sleep(timeout).await;
            self.advance();
        }
        self.events.pop_front()
    }

    async fn begin_job(&mut self, header: JobHeader) -> Result<(), DeviceError> {
        self.begun = Some(header);
        self.started = Some(Instant::now());
        self.last_tick = Some(Instant::now());
        self.enter(MachineState::RunningJob);
        Ok(())
    }

    async fn send(&mut self, lines: &[GcodeLine]) -> Result<u32, DeviceError> {
        self.advance();
        if self.cfg.fail_send_after.is_some_and(|n| self.accepted >= n) {
            return Err(DeviceError::new(
                "send-failed",
                "the simulated link dropped",
            ));
        }
        let capacity = usize::try_from(self.cfg.max_inflight)
            .unwrap_or(usize::MAX)
            .saturating_sub(self.queue.len());
        let mut take = lines.len().min(capacity);
        if let Some(limit) = self.cfg.fail_send_after {
            take = take.min(usize::try_from(limit - self.accepted).unwrap_or(usize::MAX));
        }
        for line in &lines[..take] {
            self.queue.push_back(line.clone());
            self.received.push(line.number);
        }
        let taken = u32::try_from(take).unwrap_or(u32::MAX);
        self.accepted += taken;
        self.max_queue = self
            .max_queue
            .max(u32::try_from(self.queue.len()).unwrap_or(u32::MAX));
        Ok(taken)
    }

    async fn end_job(&mut self) -> Result<(), DeviceError> {
        self.ended = true;
        self.enter(MachineState::Idle);
        Ok(())
    }

    async fn pause(&mut self) -> Result<(), DeviceError> {
        self.advance();
        self.pause_count += 1;
        self.enter(MachineState::Paused);
        Ok(())
    }

    async fn resume(&mut self) -> Result<(), DeviceError> {
        self.resume_count += 1;
        self.last_tick = Some(Instant::now());
        self.enter(MachineState::RunningJob);
        Ok(())
    }

    async fn cancel(&mut self) -> Result<(), DeviceError> {
        self.cancelled = true;
        self.queue.clear();
        self.enter(MachineState::Idle);
        Ok(())
    }

    async fn user_action_done(&mut self, id: u32) -> Result<(), DeviceError> {
        if let MachineState::WaitingForUser(action) = &self.state
            && action.id == id
        {
            self.acked.push(id);
            self.last_tick = Some(Instant::now());
            self.enter(MachineState::RunningJob);
        }
        Ok(())
    }

    async fn jog(&mut self, _dx_mm: f64, _dy_mm: f64) -> Result<(), DeviceError> {
        Ok(())
    }

    async fn disconnect(&mut self) {
        self.enter(MachineState::Unknown);
    }
}
