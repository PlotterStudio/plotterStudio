//! The machine handle.
//!
//! Each connected machine gets its own device thread with a single-threaded async runtime, so a
//! slow plugin never blocks the UI or another machine. The UI holds a cheap-to-clone
//! [`MachineHandle`]: [`MachineHandle::status`] returns the latest snapshot without locks or
//! waiting, commands go to the device thread through a channel, and state changes, prompts and
//! results come back as broadcast events.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use ps_gcode::parse_program;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::driver::Driver;
use crate::runner::{Control, JobOutcome, RunConfig, RunEvent, run_job};
use crate::types::{
    Capabilities, ConnectionState, DeviceError, DeviceInfo, JobHeader, JobProgress, MachineState,
    MachineStatus, RunRefusal, Status, UserAction, check_can_run,
};

/// Timings of the device thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleConfig {
    /// How often an idle machine's status is read.
    pub status_poll: Duration,
    /// No reply to a status read within this time marks the machine lost.
    pub status_timeout: Duration,
    /// Timings of a running job.
    pub run: RunConfig,
}

impl Default for HandleConfig {
    fn default() -> Self {
        Self {
            status_poll: Duration::from_millis(500),
            status_timeout: Duration::from_secs(5),
            run: RunConfig::default(),
        }
    }
}

/// A job to run: the program text and its header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    /// The mat's G-code.
    pub program: String,
    /// The header handed to the plugin.
    pub header: JobHeader,
}

/// What the device thread tells the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum HandleEvent {
    /// The status snapshot changed.
    StatusChanged(Arc<MachineStatus>),
    /// The user is needed before the job can continue.
    UserActionRequired(UserAction),
    /// A job ended.
    JobFinished(JobOutcome),
    /// The device stopped answering and the session was dropped.
    Lost,
}

enum Cmd {
    Run(RunRequest),
    Pause,
    Resume,
    Cancel,
    Ack(u32),
    Jog(f64, f64),
    Disconnect,
}

struct Shared {
    status: ArcSwap<MachineStatus>,
    events: broadcast::Sender<HandleEvent>,
    running: AtomicBool,
}

impl Shared {
    fn publish(&self, mut next: MachineStatus) {
        let previous = self.status.load_full();
        next.updated = Instant::now();
        let changed = {
            let mut same_time = next.clone();
            same_time.updated = previous.updated;
            same_time != *previous
        };
        let snapshot = Arc::new(next);
        self.status.store(Arc::clone(&snapshot));
        if changed {
            let _ = self.events.send(HandleEvent::StatusChanged(snapshot));
        }
    }

    /// Replaces parts of the current snapshot and publishes the result.
    fn update(&self, edit: impl FnOnce(&mut MachineStatus)) {
        let mut next = (*self.status.load_full()).clone();
        edit(&mut next);
        self.publish(next);
    }

    fn publish_driver_status(&self, connection: ConnectionState, status: Status) {
        self.publish(MachineStatus::from_status(
            connection,
            status,
            Instant::now(),
        ));
    }
}

struct Inner {
    info: DeviceInfo,
    capabilities: Capabilities,
    shared: Arc<Shared>,
    commands: UnboundedSender<Cmd>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.commands.send(Cmd::Disconnect);
        let thread = self
            .thread
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

/// One machine, as the UI and the CLI see it.
#[derive(Clone)]
pub struct MachineHandle {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for MachineHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MachineHandle")
            .field("model", &self.inner.info.model)
            .field("state", &self.inner.shared.status.load().state)
            .finish_non_exhaustive()
    }
}

impl MachineHandle {
    /// Starts the device thread for a driver and returns its handle.
    #[must_use]
    pub fn spawn(driver: Box<dyn Driver>, config: HandleConfig) -> Self {
        let info = driver.info();
        let capabilities = driver.capabilities();
        let (events, _) = broadcast::channel(256);
        let shared = Arc::new(Shared {
            status: ArcSwap::from_pointee(MachineStatus::initial(Instant::now())),
            events,
            running: AtomicBool::new(false),
        });
        let (commands, receiver) = unbounded_channel();
        let thread_shared = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name(format!("ps-device {}", info.model))
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build();
                if let Ok(runtime) = runtime {
                    runtime.block_on(device_main(driver, receiver, thread_shared, config));
                }
            })
            .ok();
        Self {
            inner: Arc::new(Inner {
                info,
                capabilities,
                shared,
                commands,
                thread: Mutex::new(thread),
            }),
        }
    }

    /// What the device is.
    #[must_use]
    pub fn info(&self) -> &DeviceInfo {
        &self.inner.info
    }

    /// What the device can do.
    #[must_use]
    pub fn capabilities(&self) -> &Capabilities {
        &self.inner.capabilities
    }

    /// The latest status snapshot; never blocks.
    #[must_use]
    pub fn status(&self) -> Arc<MachineStatus> {
        self.inner.shared.status.load_full()
    }

    /// Subscribes to state changes, prompts and results.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<HandleEvent> {
        self.inner.shared.events.subscribe()
    }

    /// Starts a job.
    ///
    /// # Errors
    ///
    /// [`RunRefusal`] when the machine is not connected and idle, was compiled for another
    /// profile, or the program is not valid.
    pub fn run(&self, request: RunRequest) -> Result<(), RunRefusal> {
        let shared = &self.inner.shared;
        check_can_run(
            &shared.status.load(),
            &self.inner.info,
            &request.header.profile_id,
        )?;
        parse_program(&request.program).map_err(|e| RunRefusal::InvalidProgram(e.to_string()))?;
        if shared
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(RunRefusal::NotIdle);
        }
        if self.inner.commands.send(Cmd::Run(request)).is_err() {
            shared.running.store(false, Ordering::Release);
            return Err(RunRefusal::NotConnected);
        }
        Ok(())
    }

    /// Pauses the running job.
    pub fn pause(&self) {
        let _ = self.inner.commands.send(Cmd::Pause);
    }

    /// Resumes a paused job.
    pub fn resume(&self) {
        let _ = self.inner.commands.send(Cmd::Resume);
    }

    /// Cancels the running job.
    pub fn cancel(&self) {
        let _ = self.inner.commands.send(Cmd::Cancel);
    }

    /// Tells the job the user answered a prompt.
    pub fn ack_user_action(&self, id: u32) {
        let _ = self.inner.commands.send(Cmd::Ack(id));
    }

    /// Moves the carriage by an offset in mm.
    pub fn jog(&self, x_mm: f64, y_mm: f64) {
        let _ = self.inner.commands.send(Cmd::Jog(x_mm, y_mm));
    }

    /// Closes the session; the handle stays valid but reports a disconnected machine.
    pub fn disconnect(&self) {
        let _ = self.inner.commands.send(Cmd::Disconnect);
    }
}

// ---- the device thread ----

async fn device_main(
    mut driver: Box<dyn Driver>,
    mut commands: UnboundedReceiver<Cmd>,
    shared: Arc<Shared>,
    config: HandleConfig,
) {
    shared.update(|s| s.connection = ConnectionState::Connecting);
    let mut lost = !read_status(&mut *driver, &shared, &config).await;

    while !lost {
        tokio::select! {
            cmd = commands.recv() => match cmd {
                None | Some(Cmd::Disconnect) => break,
                Some(Cmd::Run(request)) => {
                    lost = run_one(&mut *driver, request, &mut commands, &shared, &config).await == Ended::Lost;
                    shared.running.store(false, Ordering::Release);
                }
                Some(Cmd::Jog(dx, dy)) => {
                    let _ = tokio::time::timeout(config.status_timeout, driver.jog(dx, dy)).await;
                }
                Some(Cmd::Pause | Cmd::Resume | Cmd::Cancel | Cmd::Ack(_)) => {}
            },
            () = tokio::time::sleep(config.status_poll) => {
                lost = !read_status(&mut *driver, &shared, &config).await;
            }
        }
    }

    if lost {
        let _ = tokio::time::timeout(config.status_timeout, driver.disconnect()).await;
        return;
    }
    driver.disconnect().await;
    shared.update(|s| {
        s.connection = ConnectionState::Disconnected;
        s.state = MachineState::Unknown;
        s.progress = None;
    });
    shared.running.store(false, Ordering::Release);
}

/// Reads the device's status; false when it did not answer in time (the machine is lost).
async fn read_status(driver: &mut dyn Driver, shared: &Shared, config: &HandleConfig) -> bool {
    match tokio::time::timeout(config.status_timeout, driver.status()).await {
        Err(_) => {
            mark_lost(shared);
            false
        }
        Ok(Ok(status)) => {
            shared.publish_driver_status(ConnectionState::Connected, status);
            true
        }
        Ok(Err(error)) => {
            shared.update(|s| {
                s.connection = ConnectionState::Connected;
                s.state = MachineState::Fault(error);
            });
            true
        }
    }
}

fn mark_lost(shared: &Shared) {
    shared.update(|s| {
        s.connection = ConnectionState::Lost;
        s.state = MachineState::Unknown;
    });
    let _ = shared.events.send(HandleEvent::Lost);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    Fine,
    Lost,
}

async fn run_one(
    driver: &mut dyn Driver,
    request: RunRequest,
    commands: &mut UnboundedReceiver<Cmd>,
    shared: &Arc<Shared>,
    config: &HandleConfig,
) -> Ended {
    let (control, mut controls) = unbounded_channel();
    let observer_shared = Arc::clone(shared);
    let mut observer = move |event: RunEvent| match event {
        RunEvent::Progress(progress) => observer_shared.update(|s| {
            s.state = MachineState::RunningJob;
            s.progress = Some(progress);
        }),
        RunEvent::UserActionRequired(action) => {
            let _ = observer_shared
                .events
                .send(HandleEvent::UserActionRequired(action.clone()));
            observer_shared.update(|s| s.state = MachineState::WaitingForUser(action));
        }
        RunEvent::Paused => observer_shared.update(|s| s.state = MachineState::Paused),
        RunEvent::Resumed => observer_shared.update(|s| s.state = MachineState::RunningJob),
    };
    shared.update(|s| {
        s.state = MachineState::RunningJob;
        s.progress = Some(JobProgress {
            lines_done: 0,
            lines_total: 0,
            current_op: None,
        });
    });

    let job = run_job(
        driver,
        &request.program,
        request.header,
        &mut controls,
        &mut observer,
        &config.run,
    );
    tokio::pin!(job);
    let mut disconnect = false;
    let outcome = loop {
        tokio::select! {
            result = &mut job => break result,
            cmd = commands.recv() => match cmd {
                Some(Cmd::Pause) => { let _ = control.send(Control::Pause); }
                Some(Cmd::Resume) => { let _ = control.send(Control::Resume); }
                Some(Cmd::Cancel) => { let _ = control.send(Control::Cancel); }
                Some(Cmd::Ack(id)) => { let _ = control.send(Control::Ack(id)); }
                None | Some(Cmd::Disconnect) => {
                    disconnect = true;
                    let _ = control.send(Control::Cancel);
                }
                Some(Cmd::Run(_) | Cmd::Jog(..)) => {}
            }
        }
    };

    let outcome = outcome.unwrap_or_else(|e| JobOutcome::Faulted {
        error: DeviceError::new("bad-program", e.to_string()),
        lines_done: 0,
    });
    let lost = matches!(outcome, JobOutcome::Lost { .. });
    match &outcome {
        JobOutcome::Completed { .. } | JobOutcome::Cancelled { .. } => shared.update(|s| {
            s.state = MachineState::Idle;
            s.progress = None;
        }),
        JobOutcome::Faulted { error, .. } => {
            let error = error.clone();
            shared.update(|s| s.state = MachineState::Fault(error));
        }
        JobOutcome::Lost { .. } => mark_lost(shared),
    }
    let _ = shared.events.send(HandleEvent::JobFinished(outcome));
    if disconnect {
        return Ended::Lost;
    }
    if lost { Ended::Lost } else { Ended::Fine }
}

#[cfg(test)]
mod tests;
