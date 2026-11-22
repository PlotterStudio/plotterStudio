//! Byte transports under native drivers: what a driver needs from a serial port, a socket
//! or a USB endpoint, and a scripted fake for tests.
//!
//! Real transports (serial, USB, BLE, sockets) implement [`Transport`] in the host; drivers never
//! open ports themselves, so the host can scope, log and cut a connection.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;

use crate::types::{DeviceError, TransportKind};

/// A bidirectional byte link to a machine.
#[async_trait]
pub trait Transport: Send {
    /// How the link is carried.
    fn kind(&self) -> TransportKind;

    /// Writes all of `data`.
    async fn write(&mut self, data: &[u8]) -> Result<(), DeviceError>;

    /// Reads up to `max` bytes, waiting at most `timeout`; an empty result means nothing arrived.
    async fn read(&mut self, max: usize, timeout: Duration) -> Result<Vec<u8>, DeviceError>;
}

#[derive(Debug, Default)]
struct MockState {
    written: Vec<u8>,
    script: VecDeque<u8>,
}

/// A fake serial link: records everything written and plays back scripted replies.
#[derive(Debug)]
pub struct MockTransport {
    state: Arc<Mutex<MockState>>,
}

/// The test's side of a [`MockTransport`].
#[derive(Debug, Clone)]
pub struct MockHandle {
    state: Arc<Mutex<MockState>>,
}

impl MockTransport {
    /// A transport and the handle that scripts it.
    #[must_use]
    pub fn new() -> (Self, MockHandle) {
        let state = Arc::new(Mutex::new(MockState::default()));
        (
            Self {
                state: Arc::clone(&state),
            },
            MockHandle { state },
        )
    }
}

impl MockHandle {
    /// Queues bytes for the driver to read.
    pub fn script(&self, text: &str) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .script
            .extend(text.bytes());
    }

    /// Everything written so far.
    #[must_use]
    pub fn written(&self) -> Vec<u8> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .written
            .clone()
    }

    /// Everything written so far, as text.
    #[must_use]
    pub fn written_text(&self) -> String {
        String::from_utf8_lossy(&self.written()).into_owned()
    }
}

#[async_trait]
impl Transport for MockTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Serial
    }

    async fn write(&mut self, data: &[u8]) -> Result<(), DeviceError> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .written
            .extend_from_slice(data);
        Ok(())
    }

    async fn read(&mut self, max: usize, _timeout: Duration) -> Result<Vec<u8>, DeviceError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let n = state.script.len().min(max);
        Ok(state.script.drain(..n).collect())
    }
}
