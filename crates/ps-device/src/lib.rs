//! Plotter Studio device host.
//!
//! One interface to every machine: drivers (native or sandboxed plugins) translate the fixed
//! `ps-gcode/1` dialect into a machine's protocol and report its state; the host owns transports,
//! flow control, faults, permissions and the status snapshot the UI reads.

mod bus;
mod driver;
mod grbl;
mod handle;
mod host;
mod link;
mod package;
mod permissions;
mod runner;
mod sim;
mod simulator;
mod tls;
mod transcript;
mod transport;
mod types;
mod wasm;
mod wasm_plugin;
mod wireless;
pub mod wit;

pub use bus::{Buses, MAX_LISTEN, ScopedBuses, SystemBuses};
pub use driver::{Driver, DriverEvent};
pub use grbl::{GrblDriver, identify as identify_grbl, translate as translate_grbl};
pub use handle::{HandleConfig, HandleEvent, MachineHandle, RunRequest};
pub use host::{
    ConnectError, DeclaredHost, DeviceEntry, DeviceFilter, DeviceHost, DeviceId, DiscoveredDevice,
    Plugin, PluginManifest,
};
pub use link::{
    EndpointInfo, HttpTransport, SerialPortInfo, SerialTransport, TcpTransport, UdpTransport,
    UsbInfo, UsbTransport, WebSocketTransport, describe_serial_port, is_local_address,
    list_serial_ports, list_usb_devices, pick_bulk_endpoints,
};
pub use package::{
    CheckReport, MAX_PLUGIN_BYTES, PackageError, ReplayHandle, ReplayTransport, SimulatedSerial,
    Unpacked, check_against_recording, check_in_simulation, manifest_from_json, pack, unpack,
};
pub use permissions::{PermissionError, PermissionStore, approval_prompt};
pub use ps_device_sdk::{AckFlow, LineBuffer, numbered_line, sum_checksum, xor_checksum};
pub use runner::{Control, JobOutcome, RunConfig, RunEvent, run_job};
pub use sim::{SimConfig, SimulatedMachine, TimelineEntry};
pub use simulator::{SIMULATOR_ID, SIMULATOR_PLUGIN, SimulatorPlugin};
pub use tls::{
    HostGate, InternetAccess, InternetOpener, PinDecision, PinnedHttps, PinningTransports,
    PublicHttpsOpener, decide, fingerprint,
};
pub use transcript::{Direction, Entry, Transcript, TranscriptError};
pub use transport::{MockHandle, MockTransport, Transport};
pub use types::{
    Alert, AlertLevel, Capabilities, ConnectionState, DeviceError, DeviceInfo, GcodeLine,
    HolderStatus, JobHeader, JobProgress, MachineState, MachineStatus, PowerState, RunRefusal,
    STALE_AFTER, Status, TransportKind, UserAction, UserActionKind, check_can_run,
};
pub use wasm::{BlockingTransport, HttpRequest, HttpResponse, WasmDriver, WasmHost, WasmLimits};
pub use wasm_plugin::{SystemTransports, TransportFactory, WasmPlugin};
pub use wireless::{
    BleInfo, BleTransport, browse_mdns, device_from_service, instance_name, parse_uuid, scan_ble,
};
