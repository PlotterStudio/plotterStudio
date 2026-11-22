//! The app's log file: what the app and the plugin host report, kept where a bug report can reach
//! it. The window application has no console, so without this a failure leaves no trace.
//!
//! One file per day, `plotter-studio.<date>.log`, the newest [`KEEP_FILES`] kept. The level is
//! `info` unless the `PLOTTER_STUDIO_LOG` environment variable says otherwise (it takes the usual
//! `tracing` filter syntax, e.g. `debug` or `ps_device=trace,info`).

use std::path::Path;

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;

/// How many daily files are kept before the oldest is removed.
pub const KEEP_FILES: usize = 7;

/// The environment variable that sets the log level.
pub const LOG_ENV: &str = "PLOTTER_STUDIO_LOG";

/// The level used when [`LOG_ENV`] is not set or is not a valid filter.
const DEFAULT_FILTER: &str = "info";

/// Opens the log file in `dir` for writing on a background thread. Dropping the guard flushes it.
///
/// # Errors
/// A message when the folder or file cannot be created.
pub fn file_writer(dir: &Path) -> Result<(NonBlocking, WorkerGuard), String> {
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("plotter-studio")
        .filename_suffix("log")
        .max_log_files(KEEP_FILES)
        .build(dir)
        .map_err(|e| e.to_string())?;
    Ok(tracing_appender::non_blocking(appender))
}

/// A subscriber that writes plain text lines to `writer`.
pub fn subscriber(writer: NonBlocking) -> impl tracing::Subscriber + Send + Sync {
    let filter =
        EnvFilter::try_from_env(LOG_ENV).unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(filter)
        .finish()
}

/// Starts logging to a file in `dir` for the rest of the process. Keep the returned guard alive
/// until the program ends; `None` when the log could not be opened (the app runs without one).
#[must_use]
pub fn init(dir: &Path) -> Option<WorkerGuard> {
    let (writer, guard) = file_writer(dir).ok()?;
    tracing::subscriber::set_global_default(subscriber(writer)).ok()?;
    Some(guard)
}

#[cfg(test)]
mod tests;
