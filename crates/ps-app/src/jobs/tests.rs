//! Ground truth for background work: the UI thread owns the document; long work runs on
//! a pool with a snapshot, a cancellation token and a generation number. Results come back on a
//! channel and apply only if their generation is still current; stale results are dropped. A
//! panicking job reports an error instead of taking the app down. Live preview compiles are
//! debounced (60 ms after the last edit).

use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use super::*;

const WAIT: Duration = Duration::from_secs(10);

/// A job that waits until the test lets it go.
fn gated(gate: Receiver<()>, value: u32) -> impl FnOnce(&JobContext) -> u32 + Send + 'static {
    move |_| {
        let _ = gate.recv_timeout(WAIT);
        value
    }
}

#[test]
fn a_finished_job_delivers_its_result_when_its_generation_is_current() {
    let m: JobManager<u32> = JobManager::new(2);
    let g = m.generation();
    m.submit(g, |_| 7);
    match m.recv(WAIT) {
        Some(Outcome::Done { generation, value }) => {
            assert_eq!(generation, g);
            assert_eq!(value, 7);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_result_from_an_older_generation_is_dropped() {
    let m: JobManager<u32> = JobManager::new(2);
    let (open, gate) = mpsc::channel();
    let old = m.generation();
    m.submit(old, gated(gate, 1));
    let new = m.next_generation();
    assert!(new > old);
    m.submit(new, |_| 2);
    open.send(()).unwrap();

    // Only the current generation's result comes out, however the two finished.
    let first = m.recv(WAIT).expect("a result");
    assert_eq!(
        first,
        Outcome::Done {
            generation: new,
            value: 2
        }
    );
    assert_eq!(m.recv(Duration::from_millis(200)), None);
    assert_eq!(m.stale_dropped(), 1);
}

#[test]
fn starting_a_new_generation_cancels_older_jobs() {
    let m: JobManager<bool> = JobManager::new(2);
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let g = m.generation();
    m.submit(g, move |ctx| {
        started_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(WAIT);
        ctx.is_cancelled()
    });
    started_rx.recv_timeout(WAIT).unwrap();
    m.next_generation();
    release_tx.send(()).unwrap();
    // The job saw that it was cancelled, but its (stale) result is not delivered.
    assert_eq!(m.recv(Duration::from_millis(200)), None);
    assert_eq!(m.stale_dropped(), 1);
}

#[test]
fn a_job_can_be_cancelled_explicitly() {
    let m: JobManager<bool> = JobManager::new(2);
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let g = m.generation();
    let id = m.submit(g, move |ctx| {
        started_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(WAIT);
        ctx.is_cancelled()
    });
    started_rx.recv_timeout(WAIT).unwrap();
    m.cancel(id);
    release_tx.send(()).unwrap();
    match m.recv(WAIT) {
        Some(Outcome::Cancelled { generation }) => assert_eq!(generation, g),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_job_that_notices_cancellation_and_stops_early_is_reported_cancelled() {
    let m: JobManager<u32> = JobManager::new(1);
    let g = m.generation();
    let id = m.submit(g, |ctx| {
        // Stands in for a long loop that checks the token.
        while !ctx.is_cancelled() {
            std::thread::yield_now();
        }
        0
    });
    m.cancel(id);
    assert!(matches!(m.recv(WAIT), Some(Outcome::Cancelled { .. })));
}

#[test]
fn a_panicking_job_reports_an_error_and_the_pool_keeps_working() {
    let m: JobManager<u32> = JobManager::new(1);
    let g = m.generation();
    m.submit(g, |_| panic!("the compiler found a bug"));
    match m.recv(WAIT) {
        Some(Outcome::Panicked { message, .. }) => {
            assert!(message.contains("the compiler found a bug"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    m.submit(g, |_| 5);
    assert!(matches!(m.recv(WAIT), Some(Outcome::Done { value: 5, .. })));
}

#[test]
fn the_number_of_running_jobs_is_visible_for_the_status_bar() {
    let m: JobManager<u32> = JobManager::new(2);
    assert_eq!(m.active(), 0);
    let (open, gate) = mpsc::channel();
    let g = m.generation();
    m.submit(g, gated(gate, 1));
    assert_eq!(m.active(), 1);
    open.send(()).unwrap();
    m.recv(WAIT).unwrap();
    // The job has delivered; it is no longer active once its result is out.
    assert_eq!(m.active(), 0);
}

#[test]
fn progress_reported_by_a_job_can_be_read_by_the_ui() {
    let m: JobManager<u32> = JobManager::new(1);
    let (reported_tx, reported_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let g = m.generation();
    let id = m.submit(g, move |ctx| {
        ctx.set_progress(0.5);
        reported_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(WAIT);
        1
    });
    reported_rx.recv_timeout(WAIT).unwrap();
    assert_eq!(m.progress(id), Some(0.5));
    release_tx.send(()).unwrap();
    m.recv(WAIT).unwrap();
    assert_eq!(m.progress(id), None, "finished jobs have no progress");
}

#[test]
fn progress_is_clamped_to_zero_and_one() {
    let m: JobManager<u32> = JobManager::new(1);
    let (reported_tx, reported_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let g = m.generation();
    let id = m.submit(g, move |ctx| {
        ctx.set_progress(7.0);
        reported_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(WAIT);
        1
    });
    reported_rx.recv_timeout(WAIT).unwrap();
    assert_eq!(m.progress(id), Some(1.0));
    release_tx.send(()).unwrap();
    m.recv(WAIT).unwrap();
}

#[test]
fn generations_only_ever_increase() {
    let m: JobManager<u32> = JobManager::new(1);
    let a = m.generation();
    let b = m.next_generation();
    let c = m.next_generation();
    assert!(a < b && b < c);
    assert_eq!(m.generation(), c);
}

#[test]
fn try_recv_does_not_block_and_returns_what_is_ready() {
    let m: JobManager<u32> = JobManager::new(1);
    assert_eq!(m.try_recv(), None);
    let g = m.generation();
    m.submit(g, |_| 9);
    let got = m.recv(WAIT).unwrap();
    assert_eq!(
        got,
        Outcome::Done {
            generation: g,
            value: 9
        }
    );
}

// ---- debounce ----

#[test]
fn a_debounce_fires_sixty_milliseconds_after_the_last_touch() {
    let mut d = Debounce::new(60);
    assert!(!d.is_due(1_000), "nothing pending");
    d.touch(1_000);
    assert!(!d.is_due(1_059));
    assert!(d.is_due(1_060));
}

#[test]
fn touching_again_restarts_the_wait() {
    let mut d = Debounce::new(60);
    d.touch(0);
    d.touch(50);
    assert!(!d.is_due(100));
    assert!(d.is_due(110));
}

#[test]
fn taking_a_due_debounce_fires_once() {
    let mut d = Debounce::new(60);
    d.touch(0);
    assert!(!d.take_if_due(30));
    assert!(d.take_if_due(60));
    assert!(!d.take_if_due(1_000), "it fired already");
    assert!(!d.is_pending());
}

#[test]
fn the_time_until_due_lets_the_ui_schedule_its_next_frame() {
    let mut d = Debounce::new(60);
    assert_eq!(d.remaining_ms(0), None);
    d.touch(100);
    assert_eq!(d.remaining_ms(120), Some(40));
    assert_eq!(d.remaining_ms(200), Some(0));
}

#[test]
fn the_default_compile_debounce_is_sixty_milliseconds() {
    assert_eq!(COMPILE_DEBOUNCE_MS, 60);
}
