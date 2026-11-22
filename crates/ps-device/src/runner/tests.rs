//! Ground truth for running a job. Preconditions are checked by the caller. The host
//! streams G-code lines in batches with `send`; the plugin returns how many it accepted; the host
//! keeps at most `max-inflight-lines` unaccepted and retries the rest. The host's cursor is the
//! source of truth for progress. Pause lines become waiting-for-user events and the job continues
//! after the user acknowledges. A fault stops everything and lines are never resent
//! automatically. No status reply for 5 s marks the machine lost. Cancel is always honoured.
//! All tests run on paused time, so every duration is exact.

use std::time::Duration;

use std::fmt::Write as _;

use ps_gcode::ParseError;
use tokio::sync::mpsc;
use tokio::time::Instant;

use super::*;
use crate::sim::{SimConfig, SimulatedMachine};

fn program(moves: u32, with_pause: bool) -> String {
    let mut text = String::from("; test\nG21\nG90\n");
    if with_pause {
        text.push_str("M0 (Load pen)\n");
    }
    text.push_str("T1 M6\nG0 X1.000 Y1.000\nM3 S100\n");
    for i in 0..moves {
        let feed = if i == 0 { " F6000" } else { "" };
        let _ = writeln!(text, "G1 X{}.000 Y1.000{feed}", i + 2);
    }
    text.push_str("M5\nM0 (Unload mat)\nM2\n");
    text
}

fn header() -> JobHeader {
    JobHeader {
        job_id: "job-1".into(),
        mat_index: 1,
        profile_id: "generic.cutter-medium".into(),
        tools: vec![(1, "pen.x".into(), "generic.holder-a".into())],
    }
}

fn sim(config: SimConfig) -> SimulatedMachine {
    SimulatedMachine::new(config)
}

struct Run {
    outcome: Result<JobOutcome, ParseError>,
    events: Vec<RunEvent>,
}

/// Runs a program, acknowledging every user prompt straight away unless told otherwise.
async fn go(machine: &mut SimulatedMachine, text: &str, auto_ack: bool) -> Run {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut events = Vec::new();
    let mut observer = |e: RunEvent| {
        if auto_ack && let RunEvent::UserActionRequired(a) = &e {
            let _ = tx.send(Control::Ack(a.id));
        }
        events.push(e);
    };
    let outcome = run_job(
        machine,
        text,
        header(),
        &mut rx,
        &mut observer,
        &RunConfig::default(),
    )
    .await;
    Run { outcome, events }
}

fn completed(run: &Run) -> u64 {
    match &run.outcome {
        Ok(JobOutcome::Completed { lines }) => *lines,
        other => panic!("expected a completed job, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn a_program_runs_to_completion_and_every_line_is_sent_once_in_order() {
    let text = program(30, false);
    let total = text.lines().count() as u64;
    let mut m = sim(SimConfig::default());
    let run = go(&mut m, &text, true).await;
    assert_eq!(completed(&run), total);
    let expected: Vec<u32> = (1..=u32::try_from(total).unwrap()).collect();
    assert_eq!(m.received_lines(), expected.as_slice());
    assert!(m.ended());
}

#[tokio::test(start_paused = true)]
async fn the_machine_is_never_given_more_than_its_inflight_limit() {
    let mut m = sim(SimConfig {
        max_inflight: 4,
        line_ms: 20,
        ..SimConfig::default()
    });
    let run = go(&mut m, &program(40, false), true).await;
    completed(&run);
    assert!(m.max_queue() <= 4, "queue peaked at {}", m.max_queue());
    assert!(
        m.max_queue() >= 2,
        "the limit should actually be used: {}",
        m.max_queue()
    );
}

#[tokio::test(start_paused = true)]
async fn a_machine_that_accepts_one_line_at_a_time_still_finishes() {
    let mut m = sim(SimConfig {
        max_inflight: 1,
        line_ms: 30,
        ..SimConfig::default()
    });
    let run = go(&mut m, &program(10, false), true).await;
    completed(&run);
    assert_eq!(m.max_queue(), 1);
}

#[tokio::test(start_paused = true)]
async fn progress_follows_the_hosts_cursor_and_never_goes_backwards() {
    let text = program(25, false);
    let total = text.lines().count() as u64;
    let mut m = sim(SimConfig {
        max_inflight: 5,
        ..SimConfig::default()
    });
    let run = go(&mut m, &text, true).await;
    completed(&run);
    let done: Vec<u64> = run
        .events
        .iter()
        .filter_map(|e| {
            if let RunEvent::Progress(p) = e {
                Some(p.lines_done)
            } else {
                None
            }
        })
        .collect();
    assert!(done.windows(2).all(|w| w[0] <= w[1]), "{done:?}");
    assert_eq!(*done.last().unwrap(), total);
    assert!(done.iter().all(|d| *d <= total));
    let totals: Vec<u64> = run
        .events
        .iter()
        .filter_map(|e| {
            if let RunEvent::Progress(p) = e {
                Some(p.lines_total)
            } else {
                None
            }
        })
        .collect();
    assert!(totals.iter().all(|t| *t == total));
}

#[tokio::test(start_paused = true)]
async fn a_pause_line_asks_the_user_and_the_job_continues_after_the_answer() {
    let mut m = sim(SimConfig::default());
    let run = go(&mut m, &program(5, true), true).await;
    completed(&run);
    let asked: Vec<&str> = run
        .events
        .iter()
        .filter_map(|e| {
            if let RunEvent::UserActionRequired(a) = e {
                Some(a.message.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(asked, vec!["Load pen", "Unload mat"]);
    assert_eq!(m.acked().len(), 2, "each answer reached the machine");
}

#[tokio::test(start_paused = true)]
async fn without_an_answer_the_job_waits_for_the_user_indefinitely() {
    let mut m = sim(SimConfig::default());
    let waited = tokio::time::timeout(
        Duration::from_secs(120),
        go(&mut m, &program(5, true), false),
    )
    .await;
    assert!(
        waited.is_err(),
        "the job must not run past an unanswered pause"
    );
    assert_eq!(m.acked().len(), 0);
}

#[tokio::test(start_paused = true)]
async fn cancelling_stops_the_machine_and_reports_where_it_got_to() {
    let mut m = sim(SimConfig {
        max_inflight: 2,
        line_ms: 100,
        ..SimConfig::default()
    });
    let (tx, mut rx) = mpsc::unbounded_channel();
    let text = program(200, false);
    let canceller = async {
        tokio::time::sleep(Duration::from_millis(500)).await;
        tx.send(Control::Cancel).unwrap();
    };
    let mut events = Vec::new();
    let mut observer = |e: RunEvent| events.push(e);
    let cfg = RunConfig::default();
    let runner = run_job(&mut m, &text, header(), &mut rx, &mut observer, &cfg);
    let (outcome, ()) = tokio::join!(runner, canceller);
    match outcome.unwrap() {
        JobOutcome::Cancelled { lines_done } => {
            assert!(lines_done > 0 && lines_done < 200, "{lines_done}");
        }
        other => panic!("{other:?}"),
    }
    assert!(m.cancelled());
}

#[tokio::test(start_paused = true)]
async fn a_fault_stops_the_job_and_nothing_is_resent() {
    let mut m = sim(SimConfig {
        fail_send_after: Some(10),
        ..SimConfig::default()
    });
    let run = go(&mut m, &program(40, false), true).await;
    match run.outcome.unwrap() {
        JobOutcome::Faulted { error, lines_done } => {
            assert_eq!(error.code, "send-failed");
            assert_eq!(lines_done, 10);
        }
        other => panic!("{other:?}"),
    }
    let mut seen = m.received_lines().to_vec();
    let before = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), before, "no line was sent twice");
    assert!(!m.ended(), "the session is dropped, not wound up");
}

#[tokio::test(start_paused = true)]
async fn a_fault_the_machine_reports_itself_also_stops_the_job() {
    let mut m = sim(SimConfig {
        fault_after: Some(7),
        line_ms: 10,
        ..SimConfig::default()
    });
    let run = go(&mut m, &program(40, false), true).await;
    match run.outcome.unwrap() {
        JobOutcome::Faulted { error, .. } => assert_eq!(error.code, "machine-fault"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn a_machine_that_stops_answering_is_lost_after_five_seconds() {
    let mut m = sim(SimConfig {
        stop_answering_after: Some(5),
        line_ms: 10,
        ..SimConfig::default()
    });
    let start = Instant::now();
    let run = go(&mut m, &program(40, false), true).await;
    let elapsed = start.elapsed();
    assert!(matches!(run.outcome.unwrap(), JobOutcome::Lost { .. }));
    assert!(elapsed >= Duration::from_secs(5), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(6), "{elapsed:?}");
}

#[tokio::test(start_paused = true)]
async fn pause_and_resume_reach_the_machine() {
    let mut m = sim(SimConfig {
        line_ms: 50,
        ..SimConfig::default()
    });
    let (tx, mut rx) = mpsc::unbounded_channel();
    let text = program(30, false);
    let driver = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        tx.send(Control::Pause).unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
        tx.send(Control::Resume).unwrap();
    };
    let mut observer = |_e: RunEvent| {};
    let cfg = RunConfig::default();
    let runner = run_job(&mut m, &text, header(), &mut rx, &mut observer, &cfg);
    // The machine's own user prompts are answered by a second control stream, so only run to the end
    // of the program part that has no prompts: this program's only prompt is the final unload.
    let ack = async {
        tokio::time::sleep(Duration::from_secs(60)).await;
        tx.send(Control::Ack(1)).unwrap();
    };
    let (outcome, (), ()) = tokio::join!(runner, driver, ack);
    assert!(matches!(outcome.unwrap(), JobOutcome::Completed { .. }));
    assert_eq!((m.pause_count(), m.resume_count()), (1, 1));
}

#[tokio::test(start_paused = true)]
async fn an_empty_program_still_begins_and_ends_a_session() {
    let mut m = sim(SimConfig::default());
    let run = go(&mut m, "", true).await;
    assert_eq!(completed(&run), 0);
    assert!(m.begun().is_some() && m.ended());
}

#[tokio::test(start_paused = true)]
async fn a_program_that_does_not_parse_is_refused_before_the_machine_is_touched() {
    let mut m = sim(SimConfig::default());
    let run = go(&mut m, "G21\nG99 X1\n", true).await;
    let e = run.outcome.unwrap_err();
    assert_eq!(e.line, 2);
    assert!(m.begun().is_none());
}

#[tokio::test(start_paused = true)]
async fn the_job_header_reaches_the_machine() {
    let mut m = sim(SimConfig::default());
    go(&mut m, &program(2, false), true).await;
    assert_eq!(m.begun(), Some(&header()));
}
