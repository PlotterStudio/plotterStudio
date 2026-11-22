//! Ground truth for the machine handle: one cheap-to-clone handle per machine;
//! panels read a status snapshot every frame without locks or blocking; commands go to the
//! machine's own device thread through a channel; each machine has its own single-threaded async
//! runtime, so a slow plugin never blocks the UI or another machine. Real threads and real
//! (short) time are used here, with generous timeouts so the tests are not flaky.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use std::fmt::Write as _;

use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::TryRecvError;

use super::*;
use crate::sim::{SimConfig, SimulatedMachine};
use crate::types::{DeviceError, GcodeLine, TransportKind};

fn fast() -> SimConfig {
    SimConfig {
        line_ms: 1,
        max_inflight: 8,
        ..SimConfig::default()
    }
}

fn config() -> HandleConfig {
    HandleConfig {
        status_poll: Duration::from_millis(20),
        status_timeout: Duration::from_millis(300),
        run: RunConfig {
            poll: Duration::from_millis(5),
            accept_wait: Duration::from_millis(2),
            status_timeout: Duration::from_millis(300),
            call_timeout: Duration::from_millis(300),
        },
    }
}

fn spawn(sim: SimConfig) -> MachineHandle {
    MachineHandle::spawn(Box::new(SimulatedMachine::new(sim)), config())
}

fn program(moves: u32, prompt: bool) -> String {
    let mut text = String::from("G21\nG90\n");
    if prompt {
        text.push_str("M0 (Load pen)\n");
    }
    text.push_str("T1 M6\nG0 X1.000 Y1.000\nM3 S100\n");
    for i in 0..moves {
        let _ = writeln!(text, "G1 X{}.000 Y1.000 F6000", i + 2);
    }
    text.push_str("M5\nM2\n");
    text
}

fn request(moves: u32, prompt: bool) -> RunRequest {
    RunRequest {
        program: program(moves, prompt),
        header: JobHeader {
            job_id: "j".into(),
            mat_index: 1,
            profile_id: "generic.cutter-medium".into(),
            tools: Vec::new(),
        },
    }
}

fn wait_for(
    rx: &mut Receiver<HandleEvent>,
    mut want: impl FnMut(&HandleEvent) -> bool,
) -> Option<HandleEvent> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match rx.try_recv() {
            Ok(e) if want(&e) => return Some(e),
            Ok(_) | Err(TryRecvError::Lagged(_)) => {}
            Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(2)),
            Err(TryRecvError::Closed) => return None,
        }
    }
    None
}

fn wait_until(mut ok: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    false
}

#[test]
fn a_new_handle_connects_and_reports_idle() {
    let h = spawn(fast());
    assert!(wait_until(|| {
        let s = h.status();
        s.connection == ConnectionState::Connected && s.state == MachineState::Idle
    }));
}

#[test]
fn the_handle_passes_on_what_the_device_is_and_can_do() {
    let h = spawn(fast());
    assert_eq!(h.info().model, "Simulated plotter");
    assert!(h.capabilities().cancel && h.capabilities().pause);
    assert_eq!(h.capabilities().max_inflight_lines, 8);
}

#[test]
fn running_a_job_asks_the_user_and_finishes_after_the_answer() {
    let h = spawn(fast());
    let mut rx = h.subscribe();
    assert!(wait_until(|| h.status().state == MachineState::Idle));
    h.run(request(20, true)).unwrap();

    let asked =
        wait_for(&mut rx, |e| matches!(e, HandleEvent::UserActionRequired(_))).expect("a prompt");
    let HandleEvent::UserActionRequired(action) = asked else {
        unreachable!()
    };
    assert_eq!(action.message, "Load pen");
    h.ack_user_action(action.id);

    let done =
        wait_for(&mut rx, |e| matches!(e, HandleEvent::JobFinished(_))).expect("the job finishes");
    assert!(
        matches!(done, HandleEvent::JobFinished(JobOutcome::Completed { .. })),
        "{done:?}"
    );
    assert!(wait_until(|| h.status().state == MachineState::Idle));
}

#[test]
fn a_second_job_is_refused_while_one_runs() {
    let h = spawn(SimConfig {
        line_ms: 20,
        ..fast()
    });
    assert!(wait_until(|| h.status().state == MachineState::Idle));
    h.run(request(50, false)).unwrap();
    assert_eq!(h.run(request(5, false)), Err(RunRefusal::NotIdle));
    h.cancel();
}

#[test]
fn a_job_for_another_profile_is_refused_and_nothing_starts() {
    let h = spawn(fast());
    assert!(wait_until(|| h.status().state == MachineState::Idle));
    let mut req = request(5, false);
    req.header.profile_id = "generic.cutter-large".into();
    match h.run(req) {
        Err(RunRefusal::ProfileMismatch {
            machine_reports,
            job_compiled_for,
        }) => {
            assert_eq!(machine_reports, "generic.cutter-medium");
            assert_eq!(job_compiled_for, "generic.cutter-large");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(h.status().state, MachineState::Idle);
}

#[test]
fn cancelling_ends_the_job_as_cancelled() {
    let h = spawn(SimConfig {
        line_ms: 30,
        ..fast()
    });
    let mut rx = h.subscribe();
    assert!(wait_until(|| h.status().state == MachineState::Idle));
    h.run(request(200, false)).unwrap();
    assert!(wait_until(|| h
        .status()
        .progress
        .as_ref()
        .is_some_and(|p| p.lines_done > 3)));
    h.cancel();
    let done = wait_for(&mut rx, |e| matches!(e, HandleEvent::JobFinished(_))).unwrap();
    assert!(
        matches!(done, HandleEvent::JobFinished(JobOutcome::Cancelled { .. })),
        "{done:?}"
    );
}

#[test]
fn progress_in_the_snapshot_follows_the_hosts_cursor() {
    let h = spawn(SimConfig {
        line_ms: 5,
        ..fast()
    });
    assert!(wait_until(|| h.status().state == MachineState::Idle));
    // Subscribe once the machine has settled, so earlier idle events are not mistaken for the job.
    let mut rx = h.subscribe();
    h.run(request(30, false)).unwrap();
    let mut seen: Vec<u64> = Vec::new();
    while let Some(HandleEvent::StatusChanged(s)) = wait_for(&mut rx, |e| {
        matches!(
            e,
            HandleEvent::StatusChanged(_) | HandleEvent::JobFinished(_)
        )
    }) {
        if let Some(p) = &s.progress {
            seen.push(p.lines_done);
        }
        if s.state == MachineState::Idle && !seen.is_empty() {
            break;
        }
    }
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
    assert!(seen.iter().any(|d| *d > 0));
}

#[test]
fn a_machine_fault_ends_the_job_and_shows_in_the_status() {
    let h = spawn(SimConfig {
        fault_after: Some(5),
        ..fast()
    });
    let mut rx = h.subscribe();
    assert!(wait_until(|| h.status().state == MachineState::Idle));
    h.run(request(30, false)).unwrap();
    let done = wait_for(&mut rx, |e| matches!(e, HandleEvent::JobFinished(_))).unwrap();
    assert!(
        matches!(done, HandleEvent::JobFinished(JobOutcome::Faulted { .. })),
        "{done:?}"
    );
    assert!(wait_until(|| matches!(
        h.status().state,
        MachineState::Fault(_)
    )));
}

#[test]
fn a_device_that_stops_answering_during_a_job_is_lost() {
    let h = spawn(SimConfig {
        stop_answering_after: Some(3),
        line_ms: 2,
        ..fast()
    });
    let mut rx = h.subscribe();
    assert!(wait_until(|| h.status().state == MachineState::Idle));
    h.run(request(30, false)).unwrap();
    assert!(wait_for(&mut rx, |e| matches!(e, HandleEvent::Lost)).is_some());
    assert!(wait_until(|| h.status().connection == ConnectionState::Lost));
}

/// A device whose status never comes back.
struct Hanging;

#[async_trait]
impl Driver for Hanging {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            model: "Hung".into(),
            serial_number: None,
            firmware: None,
            profile_id: None,
            transport: TransportKind::Usb,
        }
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    async fn status(&mut self) -> Result<Status, DeviceError> {
        std::future::pending().await
    }
    async fn wait_event(&mut self, _: Duration) -> Option<crate::driver::DriverEvent> {
        None
    }
    async fn begin_job(&mut self, _: JobHeader) -> Result<(), DeviceError> {
        Ok(())
    }
    async fn send(&mut self, _: &[GcodeLine]) -> Result<u32, DeviceError> {
        Ok(0)
    }
    async fn end_job(&mut self) -> Result<(), DeviceError> {
        Ok(())
    }
    async fn pause(&mut self) -> Result<(), DeviceError> {
        Ok(())
    }
    async fn resume(&mut self) -> Result<(), DeviceError> {
        Ok(())
    }
    async fn cancel(&mut self) -> Result<(), DeviceError> {
        Ok(())
    }
    async fn user_action_done(&mut self, _: u32) -> Result<(), DeviceError> {
        Ok(())
    }
    async fn jog(&mut self, _: f64, _: f64) -> Result<(), DeviceError> {
        Ok(())
    }
    async fn disconnect(&mut self) {}
}

#[test]
fn an_idle_device_that_never_replies_is_marked_lost() {
    let h = MachineHandle::spawn(Box::new(Hanging), config());
    let mut rx = h.subscribe();
    assert!(wait_for(&mut rx, |e| matches!(e, HandleEvent::Lost)).is_some());
    assert_eq!(h.status().connection, ConnectionState::Lost);
    // A lost machine refuses jobs.
    assert_eq!(h.run(request(2, false)), Err(RunRefusal::NotConnected));
}

#[test]
fn the_status_snapshot_is_cheap_and_never_blocks_while_a_job_runs() {
    let h = spawn(SimConfig {
        line_ms: 20,
        ..fast()
    });
    assert!(wait_until(|| h.status().state == MachineState::Idle));
    h.run(request(100, false)).unwrap();
    let start = Instant::now();
    for _ in 0..5_000 {
        let _ = h.status();
    }
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "{:?}",
        start.elapsed()
    );
    h.cancel();
}

#[test]
fn clones_share_one_machine() {
    let h = spawn(SimConfig {
        line_ms: 20,
        ..fast()
    });
    let c = h.clone();
    assert!(wait_until(|| c.status().state == MachineState::Idle));
    // A job started through one handle is visible through the other.
    h.run(request(50, false)).unwrap();
    assert!(wait_until(|| c.status().state == MachineState::RunningJob));
    assert_eq!(c.run(request(1, false)), Err(RunRefusal::NotIdle));
    c.cancel();
    assert!(wait_until(|| h.status().state == MachineState::Idle));
}

#[test]
fn disconnecting_shuts_the_thread_down_and_reports_it() {
    let h = spawn(fast());
    assert!(wait_until(|| h.status().state == MachineState::Idle));
    h.disconnect();
    assert!(wait_until(
        || h.status().connection == ConnectionState::Disconnected
    ));
    assert_eq!(h.run(request(1, false)), Err(RunRefusal::NotConnected));
    drop(h); // joins the thread; a hang here would fail the test run
}

#[test]
fn dropping_the_last_handle_stops_the_device_thread() {
    let h = spawn(fast());
    assert!(wait_until(|| h.status().state == MachineState::Idle));
    let c = h.clone();
    drop(h);
    assert_eq!(c.status().connection, ConnectionState::Connected);
    drop(c);
}
