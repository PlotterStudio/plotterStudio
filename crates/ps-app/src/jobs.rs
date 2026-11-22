//! Background work.
//!
//! The UI thread owns the document. Long work runs on a [`JobManager`] pool: a job gets a
//! cancellation token and the generation it was started for, and returns its result over a channel.
//! The UI applies a result only if its generation is still current; stale results are dropped.
//! A panicking job reports an error instead of taking the app down.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How long after the last edit a live-preview compile starts.
pub const COMPILE_DEBOUNCE_MS: u64 = 60;

/// A started job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct JobId(u64);

/// How a job ended, for a generation that is still current.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome<T> {
    /// The job produced a value.
    Done {
        /// The generation it ran for.
        generation: u64,
        /// What it produced.
        value: T,
    },
    /// The job was cancelled.
    Cancelled {
        /// The generation it ran for.
        generation: u64,
    },
    /// The job panicked.
    Panicked {
        /// The generation it ran for.
        generation: u64,
        /// The panic message.
        message: String,
    },
}

impl<T> Outcome<T> {
    fn generation(&self) -> u64 {
        match self {
            Self::Done { generation, .. }
            | Self::Cancelled { generation }
            | Self::Panicked { generation, .. } => *generation,
        }
    }
}

/// What a running job can see: whether to stop, and where to report progress.
#[derive(Debug)]
pub struct JobContext {
    generation: u64,
    current: Arc<AtomicU64>,
    cancelled: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
}

impl JobContext {
    /// Whether the job should stop: it was cancelled, or a newer generation has started.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
            || self.current.load(Ordering::Acquire) != self.generation
    }

    /// Reports progress from 0 to 1.
    pub fn set_progress(&self, fraction: f32) {
        self.progress
            .store(fraction.clamp(0.0, 1.0).to_bits(), Ordering::Release);
    }
}

struct Entry {
    cancelled: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
}

/// A pool for background jobs that return `T`.
pub struct JobManager<T> {
    pool: rayon::ThreadPool,
    generation: Arc<AtomicU64>,
    next_id: AtomicU64,
    running: Arc<Mutex<BTreeMap<JobId, Entry>>>,
    active: Arc<AtomicUsize>,
    stale: AtomicUsize,
    results: Sender<Outcome<T>>,
    inbox: Mutex<Receiver<Outcome<T>>>,
}

impl<T> std::fmt::Debug for JobManager<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobManager")
            .field("generation", &self.generation.load(Ordering::Acquire))
            .field("active", &self.active.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "the job panicked".to_owned())
}

impl<T: Send + 'static> JobManager<T> {
    /// A pool with `threads` workers (at least one).
    ///
    /// # Panics
    ///
    /// Only if the operating system refuses to start threads at all.
    #[must_use]
    pub fn new(threads: usize) -> Self {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads.max(1))
            .thread_name(|i| format!("ps-job-{i}"))
            .build()
            .expect("the operating system can start worker threads");
        let (results, inbox) = mpsc::channel();
        Self {
            pool,
            generation: Arc::new(AtomicU64::new(1)),
            next_id: AtomicU64::new(1),
            running: Arc::new(Mutex::new(BTreeMap::new())),
            active: Arc::new(AtomicUsize::new(0)),
            stale: AtomicUsize::new(0),
            results,
            inbox: Mutex::new(inbox),
        }
    }

    /// The current generation.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Starts a new generation, which makes every older job's result stale and asks them to stop.
    pub fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Runs `job` for `generation` and returns its id.
    pub fn submit<F>(&self, generation: u64, job: F) -> JobId
    where
        F: FnOnce(&JobContext) -> T + Send + 'static,
    {
        let id = JobId(self.next_id.fetch_add(1, Ordering::AcqRel));
        let cancelled = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0));
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                id,
                Entry {
                    cancelled: Arc::clone(&cancelled),
                    progress: Arc::clone(&progress),
                },
            );
        self.active.fetch_add(1, Ordering::AcqRel);

        let context = JobContext {
            generation,
            current: Arc::clone(&self.generation),
            cancelled: Arc::clone(&cancelled),
            progress,
        };
        let running = Arc::clone(&self.running);
        let active = Arc::clone(&self.active);
        let results = self.results.clone();
        self.pool.spawn(move || {
            let outcome = match catch_unwind(AssertUnwindSafe(|| job(&context))) {
                Ok(value) if cancelled.load(Ordering::Acquire) => {
                    drop(value);
                    Outcome::Cancelled { generation }
                }
                Ok(value) => Outcome::Done { generation, value },
                Err(payload) => Outcome::Panicked {
                    generation,
                    message: panic_message(payload.as_ref()),
                },
            };
            running
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            active.fetch_sub(1, Ordering::AcqRel);
            // The receiver outlives the manager's pool use; a closed channel means shutdown.
            let _ = results.send(outcome);
        });
        id
    }

    /// Asks a job to stop; its outcome is [`Outcome::Cancelled`].
    pub fn cancel(&self, id: JobId) {
        if let Some(entry) = self
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
        {
            entry.cancelled.store(true, Ordering::Release);
        }
    }

    /// A running job's progress from 0 to 1; `None` once it has finished.
    #[must_use]
    pub fn progress(&self, id: JobId) -> Option<f32> {
        self.running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .map(|e| f32::from_bits(e.progress.load(Ordering::Acquire)))
    }

    /// How many jobs are running.
    #[must_use]
    pub fn active(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    /// How many results were dropped because their generation was no longer current.
    #[must_use]
    pub fn stale_dropped(&self) -> usize {
        self.stale.load(Ordering::Acquire)
    }

    fn accept(&self, outcome: Outcome<T>) -> Option<Outcome<T>> {
        if outcome.generation() == self.generation() {
            Some(outcome)
        } else {
            self.stale.fetch_add(1, Ordering::AcqRel);
            None
        }
    }

    /// The next current result if one is ready, without waiting.
    pub fn try_recv(&self) -> Option<Outcome<T>> {
        let inbox = self.inbox.lock().unwrap_or_else(PoisonError::into_inner);
        while let Ok(outcome) = inbox.try_recv() {
            if let Some(current) = self.accept(outcome) {
                return Some(current);
            }
        }
        None
    }

    /// Waits up to `timeout` for the next current result.
    pub fn recv(&self, timeout: Duration) -> Option<Outcome<T>> {
        let inbox = self.inbox.lock().unwrap_or_else(PoisonError::into_inner);
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let outcome = inbox.recv_timeout(left).ok()?;
            if let Some(current) = self.accept(outcome) {
                return Some(current);
            }
        }
    }
}

/// Waits for edits to settle: fires once, a fixed time after the last [`Debounce::touch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Debounce {
    delay_ms: u64,
    last_touch: Option<u64>,
}

impl Debounce {
    /// A debounce of `delay_ms` milliseconds.
    #[must_use]
    pub fn new(delay_ms: u64) -> Self {
        Self {
            delay_ms,
            last_touch: None,
        }
    }

    /// Records an edit at `now_ms`, restarting the wait.
    pub fn touch(&mut self, now_ms: u64) {
        self.last_touch = Some(now_ms);
    }

    /// Whether an edit is waiting to fire.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.last_touch.is_some()
    }

    /// Whether the wait is over at `now_ms`.
    #[must_use]
    pub fn is_due(&self, now_ms: u64) -> bool {
        self.last_touch
            .is_some_and(|t| now_ms >= t.saturating_add(self.delay_ms))
    }

    /// Fires if due, once.
    pub fn take_if_due(&mut self, now_ms: u64) -> bool {
        let due = self.is_due(now_ms);
        if due {
            self.last_touch = None;
        }
        due
    }

    /// Milliseconds until it fires, so the UI can schedule its next frame; `None` if idle.
    #[must_use]
    pub fn remaining_ms(&self, now_ms: u64) -> Option<u64> {
        self.last_touch
            .map(|t| t.saturating_add(self.delay_ms).saturating_sub(now_ms))
    }
}

#[cfg(test)]
mod tests;
