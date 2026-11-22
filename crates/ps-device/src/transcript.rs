//! Session transcripts.
//!
//! An opt-in setting records the raw bytes in and out of a session to a local file for bug
//! reports, and the plugin test harness replays a recorded exchange. Nothing is uploaded. Internet
//! hosts a plugin contacts are recorded as well, so a report shows what left the machine.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

/// Which way bytes travelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// From the host to the machine.
    Out,
    /// From the machine to the host.
    In,
}

/// One recorded transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Which way it went.
    pub direction: Direction,
    /// Milliseconds since the session began.
    pub at_ms: u64,
    /// The raw bytes.
    pub bytes: Vec<u8>,
}

/// A recorded session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transcript {
    entries: Vec<Entry>,
    hosts: Vec<(u64, String)>,
}

/// Why a transcript could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranscriptError {
    /// The JSON is malformed.
    #[error("not a valid transcript: {0}")]
    Parse(String),
    /// A file operation failed.
    #[error("transcript file error: {0}")]
    Io(String),
}

#[derive(Serialize, Deserialize)]
struct Wire {
    entries: Vec<WireEntry>,
    #[serde(default)]
    hosts: Vec<WireHost>,
}

#[derive(Serialize, Deserialize)]
struct WireEntry {
    t_ms: u64,
    dir: Direction,
    hex: String,
}

#[derive(Serialize, Deserialize)]
struct WireHost {
    t_ms: u64,
    host: String,
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn from_hex(text: &str) -> Result<Vec<u8>, TranscriptError> {
    if !text.len().is_multiple_of(2) {
        return Err(TranscriptError::Parse(
            "hex data has an odd length".to_owned(),
        ));
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&text[i..i + 2], 16)
                .map_err(|_| TranscriptError::Parse(format!("{:?} is not hex", &text[i..i + 2])))
        })
        .collect()
}

impl Transcript {
    /// Records a transfer.
    pub fn record(&mut self, direction: Direction, at_ms: u64, bytes: &[u8]) {
        self.entries.push(Entry {
            direction,
            at_ms,
            bytes: bytes.to_vec(),
        });
    }

    /// Records that the plugin contacted an internet host.
    pub fn record_host(&mut self, at_ms: u64, host: &str) {
        self.hosts.push((at_ms, host.to_owned()));
    }

    /// The recorded transfers in order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The transfers that went one way.
    pub fn in_direction(&self, direction: Direction) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(move |e| e.direction == direction)
    }

    /// The internet hosts contacted, each once, sorted.
    #[must_use]
    pub fn hosts(&self) -> Vec<String> {
        self.hosts
            .iter()
            .map(|(_, h)| h.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Serialises to JSON with the bytes as hex.
    ///
    /// # Errors
    ///
    /// [`TranscriptError::Parse`] if serialisation fails.
    pub fn to_json(&self) -> Result<String, TranscriptError> {
        let wire = Wire {
            entries: self
                .entries
                .iter()
                .map(|e| WireEntry {
                    t_ms: e.at_ms,
                    dir: e.direction,
                    hex: to_hex(&e.bytes),
                })
                .collect(),
            hosts: self
                .hosts
                .iter()
                .map(|(t, h)| WireHost {
                    t_ms: *t,
                    host: h.clone(),
                })
                .collect(),
        };
        serde_json::to_string_pretty(&wire).map_err(|e| TranscriptError::Parse(e.to_string()))
    }

    /// Parses a transcript.
    ///
    /// # Errors
    ///
    /// [`TranscriptError::Parse`] for malformed JSON, a bad direction or bad hex.
    pub fn from_json(text: &str) -> Result<Self, TranscriptError> {
        let wire: Wire =
            serde_json::from_str(text).map_err(|e| TranscriptError::Parse(e.to_string()))?;
        let entries = wire
            .entries
            .into_iter()
            .map(|e| {
                Ok(Entry {
                    direction: e.dir,
                    at_ms: e.t_ms,
                    bytes: from_hex(&e.hex)?,
                })
            })
            .collect::<Result<Vec<_>, TranscriptError>>()?;
        Ok(Self {
            entries,
            hosts: wire.hosts.into_iter().map(|h| (h.t_ms, h.host)).collect(),
        })
    }

    /// Writes the transcript to a local file.
    ///
    /// # Errors
    ///
    /// [`TranscriptError`] when the file cannot be written.
    pub fn save(&self, path: &Path) -> Result<(), TranscriptError> {
        let io = |e: std::io::Error| TranscriptError::Io(e.to_string());
        let json = self.to_json()?;
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temp = NamedTempFile::new_in(dir).map_err(io)?;
        temp.write_all(json.as_bytes()).map_err(io)?;
        temp.persist(path).map_err(|e| io(e.error))?;
        Ok(())
    }

    /// Reads a transcript from a file.
    ///
    /// # Errors
    ///
    /// [`TranscriptError`] when the file cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self, TranscriptError> {
        let text = std::fs::read_to_string(path).map_err(|e| TranscriptError::Io(e.to_string()))?;
        Self::from_json(&text)
    }
}

#[cfg(test)]
mod tests;
