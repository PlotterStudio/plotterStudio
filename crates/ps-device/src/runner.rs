//! Running a job.
//!
//! The host parses the program once, hands a plugin `begin-job`, then streams lines in batches
//! with `send`. The plugin returns how many it accepted; the host offers the rest again and
//! keeps at most `max-inflight-lines` outstanding. The host's cursor is the source of truth for
//! progress. A pause line becomes a user prompt and the job continues after the answer.
//!
//! A fault stops the job and the host never resends lines by itself, because repeating a move
//! could cut twice; the user decides how to continue. A device that gives no reply for five
//! seconds is marked lost. Cancel is always honoured.

use std::collections::VecDeque;
use std::future::Future;
use std::time::Duration;

use ps_gcode::{ParseError, parse_program};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::driver::{Driver, DriverEvent};
use crate::types::{DeviceError, GcodeLine, JobHeader, JobProgress, UserAction};

/// Timings the runner uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunConfig {
    /// How long to wait for an event once every line is sent.
    pub poll: Duration,
    /// How long to wait for an event while lines remain to be sent.
    pub accept_wait: Duration,
    /// A device that does not reply to an event poll within this time is lost.
    pub status_timeout: Duration,
    /// A driver call that takes longer than this is treated as lost.
    pub call_timeout: Duration,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(50),
            accept_wait: Duration::from_millis(10),
            status_timeout: Duration::from_secs(5),
            call_timeout: Duration::from_secs(2),
        }
    }
}

/// What the user or the UI asks of a running job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Pause the job.
    Pause,
    /// Resume a paused job.
    Resume,
    /// Cancel the job.
    Cancel,
    /// The user answered a prompt.
    Ack(u32),
}

/// What the runner reports as it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent {
    /// The host cursor moved.
    Progress(JobProgress),
    /// The user is needed before the job can continue.
    UserActionRequired(UserAction),
    /// The job was paused at the user's request.
    Paused,
    /// The job was resumed.
    Resumed,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOutcome {
    /// The machine finished the whole program.
    Completed {
        /// Lines in the program.
        lines: u64,
    },
    /// The user cancelled.
    Cancelled {
        /// Lines the machine had accepted.
        lines_done: u64,
    },
    /// The machine or the link faulted; nothing was resent.
    Faulted {
        /// What went wrong.
        error: DeviceError,
        /// Lines the machine had accepted.
        lines_done: u64,
    },
    /// The device stopped answering.
    Lost {
        /// Lines the machine had accepted.
        lines_done: u64,
    },
}

/// A driver call that did not come back in time.
struct TimedOut;

async fn guard<T>(limit: Duration, call: impl Future<Output = T>) -> Result<T, TimedOut> {
    tokio::time::timeout(limit, call)
        .await
        .map_err(|_| TimedOut)
}

fn lines_of(text: &str) -> Result<Vec<GcodeLine>, ParseError> {
    Ok(parse_program(text)?
        .into_iter()
        .map(|l| GcodeLine {
            number: l.number,
            text: l.text,
            command: l.command,
        })
        .collect())
}

/// Runs `program` on `driver` until it completes, is cancelled, faults, or the device is lost.
///
/// # Errors
///
/// [`ParseError`] when the program is not valid `ps-gcode/1`; the machine is not touched.
#[expect(
    clippy::too_many_lines,
    reason = "the run loop reads best as one sequence over shared state"
)]
pub async fn run_job(
    driver: &mut dyn Driver,
    program: &str,
    header: JobHeader,
    controls: &mut UnboundedReceiver<Control>,
    observer: &mut dyn FnMut(RunEvent),
    cfg: &RunConfig,
) -> Result<JobOutcome, ParseError> {
    let lines = lines_of(program)?;
    let total = lines.len();
    let total_u64 = u64::try_from(total).unwrap_or(u64::MAX);
    let max_inflight =
        usize::try_from(driver.capabilities().max_inflight_lines.max(1)).unwrap_or(1);

    match guard(cfg.call_timeout, driver.begin_job(header)).await {
        Err(TimedOut) => return Ok(JobOutcome::Lost { lines_done: 0 }),
        Ok(Err(error)) => {
            return Ok(JobOutcome::Faulted {
                error,
                lines_done: 0,
            });
        }
        Ok(Ok(())) => {}
    }

    let mut cursor = 0_usize;
    let mut paused = false;
    let mut finished = total == 0;
    let mut pending: Option<UserAction> = None;
    let mut acks: VecDeque<u32> = VecDeque::new();
    let done = |cursor: usize| u64::try_from(cursor).unwrap_or(u64::MAX);

    loop {
        // What the user asked for.
        while let Ok(control) = controls.try_recv() {
            match control {
                Control::Cancel => {
                    let _ = guard(cfg.call_timeout, driver.cancel()).await;
                    return Ok(JobOutcome::Cancelled {
                        lines_done: done(cursor),
                    });
                }
                Control::Pause if !paused => {
                    let _ = guard(cfg.call_timeout, driver.pause()).await;
                    paused = true;
                    observer(RunEvent::Paused);
                }
                Control::Resume if paused => {
                    let _ = guard(cfg.call_timeout, driver.resume()).await;
                    paused = false;
                    observer(RunEvent::Resumed);
                }
                Control::Ack(id) => acks.push_back(id),
                Control::Pause | Control::Resume => {}
            }
        }

        // An answered prompt goes on to the machine.
        if let Some(action) = &pending
            && let Some(at) = acks.iter().position(|id| *id == action.id)
        {
            acks.remove(at);
            let id = action.id;
            pending = None;
            match guard(cfg.call_timeout, driver.user_action_done(id)).await {
                Err(TimedOut) => {
                    return Ok(JobOutcome::Lost {
                        lines_done: done(cursor),
                    });
                }
                Ok(Err(error)) => {
                    return Ok(JobOutcome::Faulted {
                        error,
                        lines_done: done(cursor),
                    });
                }
                Ok(Ok(())) => {}
            }
        }

        // Offer the next batch, unless paused or waiting for the user.
        if !paused && pending.is_none() && cursor < total {
            let end = (cursor + max_inflight).min(total);
            match guard(cfg.call_timeout, driver.send(&lines[cursor..end])).await {
                Err(TimedOut) => {
                    return Ok(JobOutcome::Lost {
                        lines_done: done(cursor),
                    });
                }
                Ok(Err(error)) => {
                    return Ok(JobOutcome::Faulted {
                        error,
                        lines_done: done(cursor),
                    });
                }
                Ok(Ok(accepted)) => {
                    cursor += usize::try_from(accepted).unwrap_or(0).min(end - cursor);
                    observer(RunEvent::Progress(JobProgress {
                        lines_done: done(cursor),
                        lines_total: total_u64,
                        current_op: None,
                    }));
                }
            }
        }

        if cursor == total && finished {
            break;
        }

        // Listen to the machine; this also paces the loop.
        let wait = if cursor < total && !paused {
            cfg.accept_wait
        } else {
            cfg.poll
        };
        match guard(cfg.status_timeout, driver.wait_event(wait)).await {
            Err(TimedOut) => {
                return Ok(JobOutcome::Lost {
                    lines_done: done(cursor),
                });
            }
            Ok(Some(DriverEvent::UserActionRequired(action))) => {
                observer(RunEvent::UserActionRequired(action.clone()));
                pending = Some(action);
            }
            Ok(Some(DriverEvent::JobFinished)) => finished = true,
            Ok(Some(DriverEvent::Fault(error))) => {
                return Ok(JobOutcome::Faulted {
                    error,
                    lines_done: done(cursor),
                });
            }
            Ok(Some(DriverEvent::StatusChanged(_)) | None) => {}
        }
    }

    let _ = guard(cfg.call_timeout, driver.end_job()).await;
    let _ = guard(cfg.call_timeout, driver.status()).await;
    Ok(JobOutcome::Completed { lines: total_u64 })
}

#[cfg(test)]
mod tests;
