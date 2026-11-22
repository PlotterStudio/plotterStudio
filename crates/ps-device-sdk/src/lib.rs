//! The SDK for device-plugin authors: line buffering, checksums, and
//! acknowledgement-based flow control, so no driver re-implements them. It has no dependencies, so
//! it builds for `wasm32-wasip2` as well as for the host. (Arc linearisation lives in `ps-gcode`.)

use std::collections::VecDeque;

/// The default most bytes kept for one unfinished line.
const DEFAULT_LINE_LIMIT: usize = 4_096;

/// Splits a byte stream into lines. Machines answer in whole lines, but transports deliver
/// arbitrary chunks; this holds the partial line until its newline arrives.
#[derive(Debug, Clone)]
pub struct LineBuffer {
    pending: Vec<u8>,
    limit: usize,
}

impl Default for LineBuffer {
    fn default() -> Self {
        Self::with_limit(DEFAULT_LINE_LIMIT)
    }
}

impl LineBuffer {
    /// A buffer that keeps at most `limit` bytes of an unfinished line.
    #[must_use]
    pub fn with_limit(limit: usize) -> Self {
        Self {
            pending: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Feeds bytes and returns the lines they completed, without line endings; empty lines are
    /// dropped and invalid UTF-8 is replaced.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                let text = String::from_utf8_lossy(&self.pending).trim().to_owned();
                self.pending.clear();
                if !text.is_empty() {
                    lines.push(text);
                }
            } else if self.pending.len() < self.limit {
                self.pending.push(byte);
            }
        }
        lines
    }

    /// The unfinished line so far.
    #[must_use]
    pub fn pending(&self) -> String {
        String::from_utf8_lossy(&self.pending).into_owned()
    }
}

/// The XOR of every byte (Marlin's line checksum).
#[must_use]
pub fn xor_checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |acc, b| acc ^ b)
}

/// The sum of every byte, modulo 256.
#[must_use]
pub fn sum_checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0_u8, |acc, b| acc.wrapping_add(*b))
}

/// A line with a line number and a trailing XOR checksum: `N3 G1 X1*45`.
#[must_use]
pub fn numbered_line(number: u32, line: &str) -> String {
    let body = format!("N{number} {line}");
    let sum = xor_checksum(body.as_bytes());
    format!("{body}*{sum}")
}

/// Character-counting flow control for machines that acknowledge each line (GRBL's `ok`): the
/// machine has a fixed receive buffer, every sent line takes its length plus a newline, and each
/// acknowledgement frees the oldest line.
#[derive(Debug, Clone)]
pub struct AckFlow {
    capacity: usize,
    sent: VecDeque<usize>,
    in_flight: usize,
}

impl AckFlow {
    /// A flow for a receive buffer of `capacity` bytes.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            sent: VecDeque::new(),
            in_flight: 0,
        }
    }

    /// Whether a line of `len` characters fits now.
    #[must_use]
    pub fn can_send(&self, len: usize) -> bool {
        self.in_flight + len < self.capacity
    }

    /// Records a line of `len` characters as sent.
    pub fn sent(&mut self, len: usize) {
        self.sent.push_back(len + 1);
        self.in_flight += len + 1;
    }

    /// Records an acknowledgement; returns the bytes it freed, or `None` when nothing was in
    /// flight.
    pub fn acked(&mut self) -> Option<usize> {
        let freed = self.sent.pop_front()?;
        self.in_flight -= freed;
        Some(freed)
    }

    /// Bytes currently unacknowledged.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.in_flight
    }

    /// Forgets everything in flight (after a reset).
    pub fn reset(&mut self) {
        self.sent.clear();
        self.in_flight = 0;
    }
}

#[cfg(test)]
mod tests;
