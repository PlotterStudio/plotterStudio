//! Plugin packages and plugin checks.
//!
//! A `.psdevice` file is a zip of `manifest.json` and `plugin.wasm`. Authors check a plugin without
//! hardware two ways: replaying a recorded session to it (does it recognise the device, and does it
//! write what the recording says it should?) and running it through a whole job against a
//! simulated serial machine.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::host::{DeviceFilter, PluginManifest};
use crate::transcript::{Direction, Transcript};
use crate::types::{JobHeader, TransportKind};
use crate::wasm::{BlockingTransport, WasmHost, WasmLimits};
use crate::wit;

/// The largest component a package may carry.
pub const MAX_PLUGIN_BYTES: usize = 16 * 1024 * 1024;
const MANIFEST: &str = "manifest.json";
const COMPONENT: &str = "plugin.wasm";

/// Why a package could not be made or read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackageError {
    /// The bytes are not a zip file.
    #[error("not a plugin package: {0}")]
    NotAPackage(String),
    /// A required file is absent.
    #[error("the package has no {0}")]
    Missing(String),
    /// The manifest is malformed.
    #[error("the manifest is invalid: {0}")]
    Manifest(String),
    /// The component is over the size limit.
    #[error("the plugin is larger than the {MAX_PLUGIN_BYTES} byte limit")]
    TooLarge,
}

/// A package's contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unpacked {
    /// What the plugin declares.
    pub manifest: PluginManifest,
    /// The plugin's own version.
    pub version: String,
    /// The component.
    pub wasm: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct WireFilter {
    transport: String,
    #[serde(default)]
    usb: Option<(u16, u16)>,
}

#[derive(Serialize, Deserialize)]
struct WireManifest {
    id: String,
    name: String,
    version: String,
    filters: Vec<WireFilter>,
    #[serde(default)]
    internet_hosts: Vec<String>,
}

const KINDS: [TransportKind; 7] = [
    TransportKind::Usb,
    TransportKind::Serial,
    TransportKind::BluetoothLe,
    TransportKind::Tcp,
    TransportKind::Udp,
    TransportKind::WebSocket,
    TransportKind::Http,
];

fn kind_from_name(name: &str) -> Option<TransportKind> {
    KINDS.into_iter().find(|k| wit::transport_kind(*k) == name)
}

/// Makes a `.psdevice` package.
///
/// # Errors
///
/// [`PackageError::TooLarge`] when the component is over [`MAX_PLUGIN_BYTES`].
pub fn pack(
    manifest: &PluginManifest,
    wasm: &[u8],
    version: &str,
) -> Result<Vec<u8>, PackageError> {
    if wasm.len() > MAX_PLUGIN_BYTES {
        return Err(PackageError::TooLarge);
    }
    let wire = WireManifest {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: version.to_owned(),
        filters: manifest
            .filters
            .iter()
            .map(|f| WireFilter {
                transport: wit::transport_kind(f.transport).to_owned(),
                usb: f.usb,
            })
            .collect(),
        internet_hosts: manifest.internet_hosts.clone(),
    };
    let json =
        serde_json::to_vec_pretty(&wire).map_err(|e| PackageError::Manifest(e.to_string()))?;
    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut cursor);
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in [(MANIFEST, json.as_slice()), (COMPONENT, wasm)] {
        zip.start_file(name, options)
            .and_then(|()| zip.write_all(bytes).map_err(zip::result::ZipError::Io))
            .map_err(|e| PackageError::NotAPackage(e.to_string()))?;
    }
    zip.finish()
        .map_err(|e| PackageError::NotAPackage(e.to_string()))?;
    Ok(cursor.into_inner())
}

fn read_member(
    zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    name: &str,
    limit: usize,
) -> Result<Vec<u8>, PackageError> {
    let file = zip
        .by_name(name)
        .map_err(|_| PackageError::Missing(name.to_owned()))?;
    let mut out = Vec::new();
    file.take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut out)
        .map_err(|e| PackageError::NotAPackage(e.to_string()))?;
    if out.len() > limit {
        return Err(PackageError::TooLarge);
    }
    Ok(out)
}

/// Reads a manifest file: the plugin manifest and the plugin's own version.
///
/// # Errors
///
/// [`PackageError::Manifest`] when the JSON is malformed or names an unknown transport.
pub fn manifest_from_json(text: &str) -> Result<(PluginManifest, String), PackageError> {
    let wire: WireManifest =
        serde_json::from_str(text).map_err(|e| PackageError::Manifest(e.to_string()))?;
    let mut filters = Vec::new();
    for f in wire.filters {
        let transport = kind_from_name(&f.transport).ok_or_else(|| {
            PackageError::Manifest(format!("unknown transport {:?}", f.transport))
        })?;
        filters.push(DeviceFilter {
            transport,
            usb: f.usb,
        });
    }
    Ok((
        PluginManifest {
            id: wire.id,
            name: wire.name,
            filters,
            internet_hosts: wire.internet_hosts,
        },
        wire.version,
    ))
}

/// Reads a `.psdevice` package.
///
/// # Errors
///
/// A [`PackageError`] when it is not a zip, lacks a file, has a bad manifest or is too large.
pub fn unpack(bytes: &[u8]) -> Result<Unpacked, PackageError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| PackageError::NotAPackage(e.to_string()))?;
    let manifest = read_member(&mut zip, MANIFEST, 1024 * 1024)?;
    let wasm = read_member(&mut zip, COMPONENT, MAX_PLUGIN_BYTES)?;
    let wire: WireManifest =
        serde_json::from_slice(&manifest).map_err(|e| PackageError::Manifest(e.to_string()))?;
    let mut filters = Vec::new();
    for f in wire.filters {
        let transport = kind_from_name(&f.transport).ok_or_else(|| {
            PackageError::Manifest(format!("unknown transport {:?}", f.transport))
        })?;
        filters.push(DeviceFilter {
            transport,
            usb: f.usb,
        });
    }
    Ok(Unpacked {
        manifest: PluginManifest {
            id: wire.id,
            name: wire.name,
            filters,
            internet_hosts: wire.internet_hosts,
        },
        version: wire.version,
        wasm,
    })
}

// ---- replaying a recorded session ----

#[derive(Debug)]
struct ReplayState {
    queue: VecDeque<(Direction, Vec<u8>)>,
    mismatch: Option<String>,
}

/// A transport that plays a recorded session back to a plugin.
#[derive(Debug)]
pub struct ReplayTransport {
    kind: TransportKind,
    state: Arc<Mutex<ReplayState>>,
}

/// The checker's view of a [`ReplayTransport`].
#[derive(Debug, Clone)]
pub struct ReplayHandle {
    state: Arc<Mutex<ReplayState>>,
}

impl ReplayTransport {
    /// A replay of `transcript` and the handle that reports on it.
    #[must_use]
    pub fn new(transcript: &Transcript, kind: TransportKind) -> (Self, ReplayHandle) {
        let queue = transcript
            .entries()
            .iter()
            .map(|e| (e.direction, e.bytes.clone()))
            .collect();
        let state = Arc::new(Mutex::new(ReplayState {
            queue,
            mismatch: None,
        }));
        (
            Self {
                kind,
                state: Arc::clone(&state),
            },
            ReplayHandle { state },
        )
    }
}

impl ReplayHandle {
    /// True once every recorded transfer has been played or matched.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .queue
            .is_empty()
    }

    /// What differed from the recording, if anything.
    #[must_use]
    pub fn mismatch(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .mismatch
            .clone()
    }
}

impl BlockingTransport for ReplayTransport {
    fn kind(&self) -> TransportKind {
        self.kind
    }

    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut rest = data;
        while !rest.is_empty() {
            let Some((Direction::Out, expected)) = state.queue.front_mut() else {
                let why =
                    format!("the plugin wrote {rest:02x?} where the recording has no more writes");
                state.mismatch = Some(why.clone());
                return Err(why);
            };
            let n = rest.len().min(expected.len());
            if rest[..n] != expected[..n] {
                let why = format!(
                    "the plugin wrote {:02x?} but the recording has {:02x?}",
                    &rest[..n],
                    &expected[..n]
                );
                state.mismatch = Some(why.clone());
                return Err(why);
            }
            expected.drain(..n);
            if expected.is_empty() {
                state.queue.pop_front();
            }
            rest = &rest[n..];
        }
        Ok(())
    }

    fn read(&mut self, max: usize, _timeout: Duration) -> Result<Vec<u8>, String> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let Some((Direction::In, bytes)) = state.queue.front_mut() else {
            return Ok(Vec::new());
        };
        let n = bytes.len().min(max);
        let out: Vec<u8> = bytes.drain(..n).collect();
        if bytes.is_empty() {
            state.queue.pop_front();
        }
        Ok(out)
    }
}

// ---- a simulated GRBL serial machine ----

/// A serial machine that answers like GRBL: a banner after a reset, a status for `?`, `ok` for
/// every line. It has no motion model: every line is done as soon as it is acknowledged, so it
/// always reports `Idle`.
#[derive(Debug, Clone, Default)]
pub struct SimulatedSerial {
    out: Arc<Mutex<Vec<u8>>>,
}

impl BlockingTransport for SimulatedSerial {
    fn kind(&self) -> TransportKind {
        TransportKind::Serial
    }

    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        for &b in data {
            match b {
                0x18 => out.extend_from_slice(b"\r\nGrbl 1.1h ['$' for help]\r\n"),
                b'?' => out.extend_from_slice(b"<Idle|MPos:0.000,0.000,0.000>\n"),
                b'\n' => out.extend_from_slice(b"ok\n"),
                _ => {}
            }
        }
        Ok(())
    }

    fn read(&mut self, max: usize, _timeout: Duration) -> Result<Vec<u8>, String> {
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        let n = out.len().min(max);
        Ok(out.drain(..n).collect())
    }
}

// ---- checking a plugin ----

/// What a plugin check found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckReport {
    /// The model the plugin identified, when it did.
    pub identified: Option<String>,
    /// How a write differed from the recording.
    pub mismatch: Option<String>,
    /// Anything else that went wrong.
    pub problems: Vec<String>,
    /// Lines the plugin was sent in a simulated job.
    pub lines_sent: usize,
}

impl CheckReport {
    /// True when nothing went wrong.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.mismatch.is_none() && self.problems.is_empty()
    }
}

fn failed(message: &dyn std::fmt::Display) -> CheckReport {
    CheckReport {
        problems: vec![message.to_string()],
        ..CheckReport::default()
    }
}

/// Replays a recorded session to a plugin: it must identify the device and write what was recorded.
pub async fn check_against_recording(wasm: &[u8], recording: &Transcript) -> CheckReport {
    let host = match WasmHost::new(WasmLimits::default()) {
        Ok(h) => h,
        Err(e) => return failed(&e),
    };
    let (replay, handle) = ReplayTransport::new(recording, TransportKind::Serial);
    let mut report = CheckReport::default();
    match host.identify(wasm, Box::new(replay)).await {
        Some(info) => report.identified = Some(info.model),
        None => report
            .problems
            .push("the plugin did not recognise the recorded device".to_owned()),
    }
    report.mismatch = handle.mismatch();
    report
}

const SIMULATED_JOB: &str = "G21\nG90\nM0 (Load pen)\nT1 M6\nG0 X12.500 Y20.000\nM3 S150\nG4 P80\nG1 X62.500 Y20.000 F6000\nM5\nM0 (Unload mat)\nM2\n";

/// Runs a plugin through a whole job against a simulated serial machine.
pub async fn check_in_simulation(wasm: &[u8]) -> CheckReport {
    use crate::runner::{Control, JobOutcome, RunConfig, RunEvent, run_job};

    let host = match WasmHost::new(WasmLimits::default()) {
        Ok(h) => h,
        Err(e) => return failed(&e),
    };
    let mut driver = match host.open(wasm, Box::new(SimulatedSerial::default())).await {
        Ok(d) => d,
        Err(e) => return failed(&e),
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut observer = |event: RunEvent| {
        if let RunEvent::UserActionRequired(action) = &event {
            let _ = tx.send(Control::Ack(action.id));
        }
    };
    let header = JobHeader {
        job_id: "check".into(),
        mat_index: 1,
        profile_id: "check".into(),
        tools: Vec::new(),
    };
    let mut report = CheckReport::default();
    match run_job(
        &mut driver,
        SIMULATED_JOB,
        header,
        &mut rx,
        &mut observer,
        &RunConfig::default(),
    )
    .await
    {
        Ok(JobOutcome::Completed { .. }) => report.lines_sent = SIMULATED_JOB.lines().count(),
        Ok(other) => report
            .problems
            .push(format!("the job did not complete: {other:?}")),
        Err(e) => report.problems.push(e.to_string()),
    }
    report
}

#[cfg(test)]
mod tests;
