//! Ground truth for the GRBL reference driver. GRBL accepts `ps-gcode/1` lines almost
//! as they are, answers each with `ok` or `error:N`, has a 127-byte receive buffer (so flow
//! control counts characters), and takes realtime commands as single bytes: `?` status, `!` hold,
//! `~` resume, Ctrl-X (0x18) soft reset. These are GRBL 1.1's documented behaviours.

use std::fmt::Write as _;
use std::time::Duration;

use ps_gcode::{Command, parse_program};
use tokio::sync::mpsc;

use super::*;
use crate::runner::{Control, JobOutcome, RunConfig, RunEvent, run_job};
use crate::transport::{MockHandle, MockTransport};
use crate::types::{DeviceError, GcodeLine, JobHeader, MachineState};

fn p(x: i64, y: i64) -> ps_core::PtUm {
    ps_core::PtUm::new(x, y)
}

fn lines(text: &str) -> Vec<GcodeLine> {
    parse_program(text)
        .unwrap()
        .into_iter()
        .map(|l| GcodeLine {
            number: l.number,
            text: l.text,
            command: l.command,
        })
        .collect()
}

fn forty_moves() -> String {
    let mut text = String::new();
    for i in 0..40 {
        let _ = writeln!(text, "G0 X{i}.000 Y1.000");
    }
    text
}

fn driver() -> (GrblDriver<MockTransport>, MockHandle) {
    let (transport, handle) = MockTransport::new();
    (GrblDriver::new(transport, "1.1h"), handle)
}

// ---- translation ----

#[test]
fn commands_translate_to_grbl_lines() {
    let cases: [(Command, Option<&str>); 13] = [
        (
            Command::Rapid {
                to: p(12_500, 20_000),
            },
            Some("G0 X12.500 Y20.000"),
        ),
        (
            Command::Linear {
                to: p(62_500, 20_000),
                a_mdeg: None,
                feed_mm_min: 6_000,
            },
            Some("G1 X62.500 Y20.000 F6000"),
        ),
        (
            Command::Arc {
                to: p(0, 10_000),
                centre_offset: p(-10_000, 0),
                ccw: true,
                feed_mm_min: 3_000,
            },
            Some("G3 X0.000 Y10.000 I-10.000 J0.000 F3000"),
        ),
        (
            Command::Arc {
                to: p(10_000, 0),
                centre_offset: p(0, -10_000),
                ccw: false,
                feed_mm_min: 3_000,
            },
            Some("G2 X10.000 Y0.000 I0.000 J-10.000 F3000"),
        ),
        (Command::Dwell { ms: 80 }, Some("G4 P0.080")),
        (Command::Dwell { ms: 1_500 }, Some("G4 P1.500")),
        (Command::ToolDown { force: 150 }, Some("M3 S150")),
        (Command::ToolUp, Some("M5")),
        (Command::Units { millimetres: true }, Some("G21")),
        (Command::Absolute, Some("G90")),
        (Command::Pause("Load pen".into()), Some("M0")),
        (Command::End, Some("M2")),
        (Command::ToolChange { slot: 2 }, None),
    ];
    for (command, expected) in cases {
        assert_eq!(translate(&command).as_deref(), expected, "{command:?}");
    }
}

#[test]
fn comments_and_heater_commands_have_no_grbl_equivalent() {
    assert_eq!(translate(&Command::Comment("x".into())), None);
    assert_eq!(
        translate(&Command::Temperature {
            celsius: 1,
            wait: true
        }),
        None
    );
}

#[test]
fn the_tangent_axis_is_dropped_because_grbl_has_none() {
    let c = Command::Linear {
        to: p(1_000, 0),
        a_mdeg: Some(45_000),
        feed_mm_min: 100,
    };
    assert_eq!(translate(&c).as_deref(), Some("G1 X1.000 Y0.000 F100"));
}

// ---- sending and flow control ----

#[tokio::test]
async fn lines_are_written_one_per_line_and_counted_as_accepted() {
    let (mut d, h) = driver();
    let batch = lines("G21\nG90\nT1 M6\nG0 X1.000 Y2.000\n");
    let accepted = d.send(&batch).await.unwrap();
    assert_eq!(
        accepted, 4,
        "the tool change has no GRBL form but is still accepted"
    );
    assert_eq!(h.written_text(), "G21\nG90\nG0 X1.000 Y2.000\n");
}

#[tokio::test]
async fn the_receive_buffer_limits_how_many_lines_are_accepted_until_ok_arrives() {
    let (mut d, h) = driver();
    let program = forty_moves();
    let batch = lines(&program);
    let first = d.send(&batch).await.unwrap();
    assert!(first > 0 && (first as usize) < batch.len(), "{first}");
    let written: usize = h.written_text().len();
    assert!(written <= 127, "{written} bytes in flight");

    // The machine acknowledges everything it has: the rest fits.
    h.script(&"ok\n".repeat(first as usize));
    d.wait_event(Duration::from_millis(1)).await;
    let second = d.send(&batch[first as usize..]).await.unwrap();
    assert!(second > 0);
}

#[tokio::test]
async fn an_error_reply_becomes_a_fault_with_grbls_code() {
    let (mut d, h) = driver();
    d.send(&lines("G0 X1.000 Y1.000\n")).await.unwrap();
    h.script("error:20\n");
    match d.wait_event(Duration::from_millis(1)).await {
        Some(DriverEvent::Fault(e)) => {
            assert_eq!(e.code, "grbl-error-20");
            assert!(
                e.message.contains("Unsupported or invalid g-code command"),
                "{}",
                e.message
            );
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn an_alarm_is_a_fault_too() {
    let (mut d, h) = driver();
    h.script("ALARM:1\n");
    match d.wait_event(Duration::from_millis(1)).await {
        Some(DriverEvent::Fault(e)) => assert_eq!(e.code, "grbl-alarm-1"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_pause_line_asks_the_user_once_grbl_has_acknowledged_it() {
    let (mut d, h) = driver();
    d.send(&lines("M0 (Load pen)\nG0 X1.000 Y1.000\n"))
        .await
        .unwrap();
    assert!(
        d.wait_event(Duration::from_millis(1)).await.is_none(),
        "nothing before the machine answers"
    );
    h.script("ok\n");
    match d.wait_event(Duration::from_millis(1)).await {
        Some(DriverEvent::UserActionRequired(a)) => assert_eq!(a.message, "Load pen"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_end_of_the_program_finishes_the_job_when_acknowledged() {
    let (mut d, h) = driver();
    d.send(&lines("M5\nM2\n")).await.unwrap();
    h.script("ok\nok\n");
    assert_eq!(
        d.wait_event(Duration::from_millis(1)).await,
        Some(DriverEvent::JobFinished)
    );
}

#[tokio::test]
async fn answering_the_user_resumes_the_machine_with_the_cycle_start_byte() {
    let (mut d, h) = driver();
    d.user_action_done(1).await.unwrap();
    assert_eq!(h.written(), b"~");
}

// ---- realtime commands ----

#[tokio::test]
async fn hold_resume_and_cancel_are_single_realtime_bytes() {
    let (mut d, h) = driver();
    d.pause().await.unwrap();
    d.resume().await.unwrap();
    assert_eq!(h.written(), b"!~");
    d.cancel().await.unwrap();
    assert_eq!(*h.written().last().unwrap(), 0x18, "Ctrl-X soft reset");
}

#[tokio::test]
async fn a_soft_reset_forgets_what_was_in_flight() {
    let (mut d, _h) = driver();
    let program = forty_moves();
    let batch = lines(&program);
    let first = d.send(&batch).await.unwrap();
    assert!((first as usize) < batch.len());
    d.cancel().await.unwrap();
    let again = d.send(&batch).await.unwrap();
    assert_eq!(again, first, "the whole buffer is free again");
}

#[tokio::test]
async fn jogging_sends_a_relative_jog_command() {
    let (mut d, h) = driver();
    d.jog(1.5, -2.0).await.unwrap();
    assert_eq!(h.written_text(), "$J=G91 G21 X1.500 Y-2.000 F1000\n");
}

// ---- status ----

#[tokio::test]
async fn status_asks_with_a_question_mark_and_reads_the_report() {
    let (mut d, h) = driver();
    h.script("<Idle|MPos:12.500,20.000,0.000|FS:0,0>\nok\n");
    let s = d.status().await.unwrap();
    assert_eq!(h.written(), b"?");
    assert_eq!(s.state, MachineState::Idle);
    assert_eq!(s.position_mm, Some((12.5, 20.0)));
}

#[tokio::test]
async fn grbl_states_map_to_machine_states() {
    for (report, expected) in [
        ("<Run|MPos:0,0,0>", MachineState::RunningJob),
        ("<Jog|MPos:0,0,0>", MachineState::RunningJob),
        ("<Hold:0|MPos:0,0,0>", MachineState::Paused),
        ("<Hold:1|MPos:0,0,0>", MachineState::Paused),
        ("<Door:0|MPos:0,0,0>", MachineState::Paused),
        ("<Home|MPos:0,0,0>", MachineState::Busy),
        ("<Idle|WPos:3.000,4.000,0.000>", MachineState::Idle),
    ] {
        let (mut d, h) = driver();
        h.script(&format!("{report}\n"));
        assert_eq!(d.status().await.unwrap().state, expected, "{report}");
    }
}

#[tokio::test]
async fn work_positions_are_read_like_machine_positions() {
    let (mut d, h) = driver();
    h.script("<Idle|WPos:3.000,4.000,0.000>\n");
    assert_eq!(d.status().await.unwrap().position_mm, Some((3.0, 4.0)));
}

#[tokio::test]
async fn an_alarm_state_is_a_fault() {
    let (mut d, h) = driver();
    h.script("<Alarm|MPos:0,0,0>\n");
    assert!(matches!(
        d.status().await.unwrap().state,
        MachineState::Fault(_)
    ));
}

#[tokio::test]
async fn no_answer_to_the_status_request_is_a_timeout_error() {
    let (mut d, _h) = driver();
    let e = d.status().await.unwrap_err();
    assert_eq!(e.code, "timeout");
}

// ---- identity ----

#[tokio::test]
async fn the_startup_banner_identifies_the_firmware() {
    let (mut transport, h) = MockTransport::new();
    h.script("\r\nGrbl 1.1h ['$' for help]\r\n");
    let info = identify(&mut transport).await.expect("a GRBL banner");
    assert_eq!(info.model, "GRBL 1.1h");
    assert_eq!(info.firmware.as_deref(), Some("1.1h"));
    assert_eq!(info.transport, crate::TransportKind::Serial);
    // It sent a soft reset to provoke the banner.
    assert_eq!(h.written(), vec![0x18]);
}

#[tokio::test]
async fn something_that_is_not_grbl_is_not_identified() {
    let (mut transport, h) = MockTransport::new();
    h.script("Marlin 2.1.2\n");
    assert!(identify(&mut transport).await.is_none());
}

#[tokio::test]
async fn the_driver_reports_what_it_can_do() {
    let (d, _h) = driver();
    let caps = d.capabilities();
    assert!(caps.pause && caps.resume && caps.cancel && caps.jog && caps.reports_position);
    assert!(!caps.reports_temperature);
    assert_eq!(d.info().model, "GRBL 1.1h");
}

// ---- end to end ----

/// A pretend GRBL: acknowledges every line, answers `?`, forgets everything on Ctrl-X.
struct Emulator {
    inbox: Vec<u8>,
    outbox: Vec<u8>,
}

#[async_trait::async_trait]
impl Transport for Emulator {
    fn kind(&self) -> crate::TransportKind {
        crate::TransportKind::Serial
    }
    async fn write(&mut self, data: &[u8]) -> Result<(), DeviceError> {
        for &b in data {
            match b {
                b'?' => self
                    .outbox
                    .extend_from_slice(b"<Run|MPos:0.000,0.000,0.000>\n"),
                0x18 => self.inbox.clear(),
                b'!' | b'~' => {}
                b'\n' => {
                    self.outbox.extend_from_slice(b"ok\n");
                    self.inbox.clear();
                }
                other => self.inbox.push(other),
            }
        }
        Ok(())
    }
    async fn read(&mut self, max: usize, _timeout: Duration) -> Result<Vec<u8>, DeviceError> {
        if self.outbox.is_empty() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let n = self.outbox.len().min(max);
        Ok(self.outbox.drain(..n).collect())
    }
}

#[tokio::test(start_paused = true)]
async fn a_whole_job_runs_through_the_runner_and_a_grbl_emulator() {
    let program = "\
; plotter-studio test
G21
G90
M0 (Load pen)
T1 M6
G0 X12.500 Y20.000
M3 S150
G4 P80
G1 X62.500 Y20.000 F6000
G1 X62.500 Y70.000
M5
G0 X0.000 Y0.000
M0 (Unload mat)
M2
";
    let mut driver = GrblDriver::new(
        Emulator {
            inbox: Vec::new(),
            outbox: Vec::new(),
        },
        "1.1h",
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut asked = Vec::new();
    let mut observer = |e: RunEvent| {
        if let RunEvent::UserActionRequired(a) = &e {
            let _ = tx.send(Control::Ack(a.id));
            asked.push(a.message.clone());
        }
    };
    let header = JobHeader {
        job_id: "j".into(),
        mat_index: 1,
        profile_id: "p".into(),
        tools: vec![],
    };
    let cfg = RunConfig::default();
    let outcome = run_job(&mut driver, program, header, &mut rx, &mut observer, &cfg)
        .await
        .unwrap();
    assert!(
        matches!(outcome, JobOutcome::Completed { .. }),
        "{outcome:?}"
    );
    assert_eq!(asked, vec!["Load pen", "Unload mat"]);
}
