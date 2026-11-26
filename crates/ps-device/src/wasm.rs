//! The WebAssembly plugin host: loads a device plugin built as a component for
//! the `plotter:device` contract and runs it as a [`Driver`].
//!
//! The sandbox grants a plugin nothing of its own: no files, environment, arguments or sockets.
//! Its only way out is the host's `transport`, `discovery` and `host` interfaces. Each call has a deadline
//! (epoch interruption) and the plugin's memory is capped, so a plugin that spins or hoards is
//! stopped and reported rather than freezing or exhausting the app. Plugins are plain blocking
//! code, so every call runs on a blocking thread.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use wasmtime::component::{Component, HasSelf, Linker, Resource, ResourceAny, ResourceTable};
use wasmtime::{Config, Engine, Store, Trap};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::bus::ScopedBuses;
use crate::driver::{Driver, DriverEvent};
use crate::host::{DeviceId, DiscoveredDevice};
use crate::tls::InternetAccess;
use crate::types::{
    Alert, AlertLevel, Capabilities, DeviceError, DeviceInfo, GcodeLine, HolderStatus, JobHeader,
    JobProgress, MachineState, PowerState, Status, TransportKind, UserAction, UserActionKind,
};

mod bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "device-plugin",
        with: { "plotter:device/transport.connection": super::Conn },
    });
}

use bindings::DevicePlugin;
use bindings::exports::plotter::device::driver as guest;
use bindings::plotter::device::{
    discovery as wit_discovery, host as wit_host, transport as wit_transport, types as wit,
};

/// How often the epoch advances; a call's deadline is counted in these ticks.
const TICK: Duration = Duration::from_millis(10);
/// The most keys a plugin's key-value store holds.
const KV_KEYS: usize = 64;
/// The largest value a plugin may store.
const KV_VALUE_BYTES: usize = 64 * 1024;
/// The longest a plugin may ask the host to sleep in one call.
const MAX_SLEEP_MS: u32 = 1_000;

/// The sandbox limits for plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WasmLimits {
    /// How long one call into a plugin may run.
    pub call_timeout: Duration,
    /// How long a plugin's `discover` may run. It is longer than an ordinary call because it waits
    /// on the host's bus listings (a radio scan, a network browse), which count against it.
    pub discover_timeout: Duration,
    /// The most memory a plugin may grow to.
    pub memory_bytes: usize,
}

impl Default for WasmLimits {
    fn default() -> Self {
        Self {
            call_timeout: Duration::from_secs(2),
            discover_timeout: Duration::from_secs(15),
            memory_bytes: 64 * 1024 * 1024,
        }
    }
}

/// A byte link a plugin reads and writes through the host's `transport` interface.
///
/// Plugins block, so this is blocking too; the host calls it from the plugin's thread.
pub trait BlockingTransport: Send {
    /// How the link is carried.
    fn kind(&self) -> TransportKind;
    /// Writes all of `data`.
    ///
    /// # Errors
    ///
    /// A message when the write fails.
    fn write(&mut self, data: &[u8]) -> Result<(), String>;
    /// Reads up to `max` bytes, waiting at most `timeout`; empty means nothing arrived.
    ///
    /// # Errors
    ///
    /// A message when the read fails.
    fn read(&mut self, max: usize, timeout: Duration) -> Result<Vec<u8>, String>;

    /// Writes to a Bluetooth LE characteristic. Links that are not Bluetooth LE refuse.
    ///
    /// # Errors
    ///
    /// A message when the link is not Bluetooth LE or the write fails.
    fn ble_write(&mut self, _characteristic: &str, _data: &[u8]) -> Result<(), String> {
        Err("this link is not Bluetooth LE".to_owned())
    }

    /// Waits for a notification from a Bluetooth LE characteristic; empty at the timeout.
    ///
    /// # Errors
    ///
    /// A message when the link is not Bluetooth LE or the wait fails.
    fn ble_notify(&mut self, _characteristic: &str, _timeout: Duration) -> Result<Vec<u8>, String> {
        Err("this link is not Bluetooth LE".to_owned())
    }

    /// Sends one datagram to the machine.
    ///
    /// # Errors
    ///
    /// A message when the link is not UDP or the send fails.
    fn send_datagram(&mut self, _data: &[u8]) -> Result<(), String> {
        Err("this link is not UDP".to_owned())
    }

    /// Waits for one datagram from the machine; empty at the timeout.
    ///
    /// # Errors
    ///
    /// A message when the link is not UDP or the receive fails.
    fn recv_datagram(&mut self, _timeout: Duration) -> Result<Vec<u8>, String> {
        Err("this link is not UDP".to_owned())
    }

    /// Makes an HTTP request to the machine.
    ///
    /// # Errors
    ///
    /// A message when the link is not HTTP, the path leaves the machine, or the request fails.
    fn http(&mut self, _request: HttpRequest) -> Result<HttpResponse, String> {
        Err("this link is not HTTP".to_owned())
    }
}

/// An HTTP request from a plugin; the path is relative to the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// The method, such as `GET`.
    pub method: String,
    /// The path and query, starting with `/`.
    pub path: String,
    /// The headers.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
}

/// The machine's answer to an [`HttpRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// The status code.
    pub status: u16,
    /// The headers.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
}

/// The host side of the `connection` resource.
#[expect(unreachable_pub, reason = "the generated bindings re-export it")]
pub struct Conn(Box<dyn BlockingTransport>);

impl std::fmt::Debug for Conn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conn").finish_non_exhaustive()
    }
}

/// Counts what a plugin asks for and refuses what is over the cap.
struct Limiter {
    memory_bytes: usize,
    denied: bool,
}

impl wasmtime::ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.memory_bytes {
            self.denied = true;
            return Ok(false);
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= 100_000)
    }
}

struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    limiter: Limiter,
    kv: BTreeMap<String, Vec<u8>>,
    internet: Option<InternetAccess>,
    /// Set only while a plugin is discovering.
    discovery: Option<Discovering>,
}

/// What a plugin may look at while it discovers, and what the host has shown it so far.
struct Discovering {
    buses: ScopedBuses,
    shown: Vec<DiscoveredDevice>,
}

impl State {
    /// Lists one bus for the plugin and remembers what was shown, so `discover` can only name
    /// devices the host really offered.
    fn show(
        &mut self,
        list: impl FnOnce(&ScopedBuses) -> Vec<DiscoveredDevice>,
    ) -> Vec<wit_discovery::Candidate> {
        let Some(discovering) = self.discovery.as_mut() else {
            return Vec::new();
        };
        let found = list(&discovering.buses);
        for device in &found {
            if !discovering.shown.iter().any(|s| s.id == device.id) {
                discovering.shown.push(device.clone());
            }
        }
        found.iter().map(to_candidate).collect()
    }
}

fn to_candidate(device: &DiscoveredDevice) -> wit_discovery::Candidate {
    wit_discovery::Candidate {
        id: device.id.0.clone(),
        name: device.name.clone(),
        transport: to_wit_kind(device.transport),
        usb: device.usb,
        address: device.address.clone(),
        serial: device.serial.clone(),
    }
}

impl wit_discovery::Host for State {
    fn list_serial_ports(&mut self) -> Vec<wit_discovery::Candidate> {
        self.show(ScopedBuses::serial_ports)
    }

    fn list_usb(&mut self) -> Vec<wit_discovery::Candidate> {
        self.show(ScopedBuses::usb_devices)
    }

    fn scan_ble(&mut self, listen_ms: u32) -> Vec<wit_discovery::Candidate> {
        self.show(|buses| buses.ble_devices(listen_ms))
    }

    fn browse_mdns(
        &mut self,
        service_type: String,
        listen_ms: u32,
    ) -> Vec<wit_discovery::Candidate> {
        self.show(|buses| buses.mdns_services(&service_type, listen_ms))
    }
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

fn io_error(message: impl Into<String>) -> wit_transport::IoError {
    wit_transport::IoError {
        kind: "io".to_owned(),
        message: message.into(),
    }
}

impl wit::Host for State {}

impl wit_transport::Host for State {
    fn open_declared_host(
        &mut self,
        name: String,
        kind: wit::TransportKind,
    ) -> Result<Resource<Conn>, wit_transport::IoError> {
        let access = self
            .internet
            .as_ref()
            .ok_or_else(|| io_error("no internet hosts are declared for this plugin"))?;
        let link = access.open(&name, from_wit_kind(kind)).map_err(io_error)?;
        self.table
            .push(Conn(link))
            .map_err(|e| io_error(e.to_string()))
    }
}

impl wit_transport::HostConnection for State {
    fn kind(&mut self, this: Resource<Conn>) -> wit::TransportKind {
        self.table
            .get(&this)
            .map_or(wit::TransportKind::Serial, |c| to_wit_kind(c.0.kind()))
    }

    fn write(&mut self, this: Resource<Conn>, data: Vec<u8>) -> Result<(), wit_transport::IoError> {
        let conn = self
            .table
            .get_mut(&this)
            .map_err(|e| io_error(e.to_string()))?;
        conn.0.write(&data).map_err(io_error)
    }

    fn read(
        &mut self,
        this: Resource<Conn>,
        max_bytes: u32,
        timeout_ms: u32,
    ) -> Result<Vec<u8>, wit_transport::IoError> {
        let conn = self
            .table
            .get_mut(&this)
            .map_err(|e| io_error(e.to_string()))?;
        let max = usize::try_from(max_bytes).unwrap_or(usize::MAX);
        conn.0
            .read(max, Duration::from_millis(u64::from(timeout_ms)))
            .map_err(io_error)
    }

    fn ble_write(
        &mut self,
        this: Resource<Conn>,
        characteristic: String,
        data: Vec<u8>,
    ) -> Result<(), wit_transport::IoError> {
        let conn = self
            .table
            .get_mut(&this)
            .map_err(|e| io_error(e.to_string()))?;
        conn.0.ble_write(&characteristic, &data).map_err(io_error)
    }

    fn ble_notify(
        &mut self,
        this: Resource<Conn>,
        characteristic: String,
        timeout_ms: u32,
    ) -> Result<Vec<u8>, wit_transport::IoError> {
        let conn = self
            .table
            .get_mut(&this)
            .map_err(|e| io_error(e.to_string()))?;
        conn.0
            .ble_notify(
                &characteristic,
                Duration::from_millis(u64::from(timeout_ms)),
            )
            .map_err(io_error)
    }

    fn send_datagram(
        &mut self,
        this: Resource<Conn>,
        data: Vec<u8>,
    ) -> Result<(), wit_transport::IoError> {
        let conn = self
            .table
            .get_mut(&this)
            .map_err(|e| io_error(e.to_string()))?;
        conn.0.send_datagram(&data).map_err(io_error)
    }

    fn recv_datagram(
        &mut self,
        this: Resource<Conn>,
        timeout_ms: u32,
    ) -> Result<Vec<u8>, wit_transport::IoError> {
        let conn = self
            .table
            .get_mut(&this)
            .map_err(|e| io_error(e.to_string()))?;
        conn.0
            .recv_datagram(Duration::from_millis(u64::from(timeout_ms)))
            .map_err(io_error)
    }

    fn http(
        &mut self,
        this: Resource<Conn>,
        req: wit_transport::HttpRequest,
    ) -> Result<wit_transport::HttpResponse, wit_transport::IoError> {
        let conn = self
            .table
            .get_mut(&this)
            .map_err(|e| io_error(e.to_string()))?;
        let response = conn
            .0
            .http(HttpRequest {
                method: req.method,
                path: req.path,
                headers: req.headers,
                body: req.body,
            })
            .map_err(io_error)?;
        Ok(wit_transport::HttpResponse {
            status: response.status,
            headers: response.headers,
            body: response.body,
        })
    }

    fn drop(&mut self, rep: Resource<Conn>) -> wasmtime::Result<()> {
        self.table.delete(rep)?;
        Ok(())
    }
}

impl wit_host::Host for State {
    fn log(&mut self, level: u8, message: String) {
        match level {
            0 | 1 => tracing::error!(plugin = true, "{message}"),
            2 => tracing::warn!(plugin = true, "{message}"),
            3 => tracing::info!(plugin = true, "{message}"),
            _ => tracing::debug!(plugin = true, "{message}"),
        }
    }

    fn now_ms(&mut self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }

    fn sleep(&mut self, ms: u32) {
        std::thread::sleep(Duration::from_millis(u64::from(ms.min(MAX_SLEEP_MS))));
    }

    fn kv_get(&mut self, key: String) -> Option<Vec<u8>> {
        self.kv.get(&key).cloned()
    }

    fn kv_set(&mut self, key: String, value: Vec<u8>) {
        if value.len() <= KV_VALUE_BYTES && (self.kv.len() < KV_KEYS || self.kv.contains_key(&key))
        {
            self.kv.insert(key, value);
        }
    }
}

// ---- conversions between the host's types and the contract's ----

fn to_wit_kind(k: TransportKind) -> wit::TransportKind {
    match k {
        TransportKind::Usb => wit::TransportKind::Usb,
        TransportKind::Serial => wit::TransportKind::Serial,
        TransportKind::BluetoothLe => wit::TransportKind::Ble,
        TransportKind::Tcp => wit::TransportKind::Tcp,
        TransportKind::Udp => wit::TransportKind::Udp,
        TransportKind::WebSocket => wit::TransportKind::Websocket,
        TransportKind::Http => wit::TransportKind::Http,
    }
}

fn from_wit_kind(k: wit::TransportKind) -> TransportKind {
    match k {
        wit::TransportKind::Usb => TransportKind::Usb,
        wit::TransportKind::Serial => TransportKind::Serial,
        wit::TransportKind::Ble => TransportKind::BluetoothLe,
        wit::TransportKind::Tcp => TransportKind::Tcp,
        wit::TransportKind::Udp => TransportKind::Udp,
        wit::TransportKind::Websocket => TransportKind::WebSocket,
        wit::TransportKind::Http => TransportKind::Http,
    }
}

fn from_wit_error(e: wit::DeviceError) -> DeviceError {
    DeviceError::new(e.code, e.message)
}

fn from_wit_info(i: wit::DeviceInfo) -> DeviceInfo {
    DeviceInfo {
        model: i.model,
        serial_number: i.serial_number,
        firmware: i.firmware,
        profile_id: i.profile_id,
        transport: from_wit_kind(i.transport),
    }
}

fn from_wit_action(a: wit::UserAction) -> UserAction {
    UserAction {
        id: a.id,
        kind: UserActionKind::Confirm,
        message: a.message,
    }
}

fn from_wit_state(s: wit::MachineState) -> MachineState {
    match s {
        wit::MachineState::Idle => MachineState::Idle,
        wit::MachineState::Busy => MachineState::Busy,
        wit::MachineState::RunningJob => MachineState::RunningJob,
        wit::MachineState::Paused => MachineState::Paused,
        wit::MachineState::WaitingForUser(a) => MachineState::WaitingForUser(from_wit_action(a)),
        wit::MachineState::Fault(e) => MachineState::Fault(from_wit_error(e)),
        wit::MachineState::UpdatingFirmware => MachineState::UpdatingFirmware,
        wit::MachineState::Unknown => MachineState::Unknown,
    }
}

fn from_wit_status(s: wit::Status) -> Status {
    Status {
        power: match s.power {
            wit::PowerState::Off => PowerState::Off,
            wit::PowerState::Standby => PowerState::Standby,
            wit::PowerState::On => PowerState::On,
            wit::PowerState::Unknown => PowerState::Unknown,
        },
        state: from_wit_state(s.state),
        progress: s.progress.map(|p| JobProgress {
            lines_done: p.lines_done,
            lines_total: p.lines_total,
            current_op: p.current_op,
        }),
        mat_loaded: s.mat_loaded,
        holders: s
            .holders
            .into_iter()
            .map(|h| HolderStatus {
                holder: h.holder,
                tool: h.tool_detected,
            })
            .collect(),
        position_mm: s.position_mm,
        temperature_c: s.temperature_c.map(f64::from),
        battery_percent: s.battery_percent,
        alerts: s
            .alerts
            .into_iter()
            .map(|a| Alert {
                level: match a.level {
                    wit::AlertLevel::Info => AlertLevel::Info,
                    wit::AlertLevel::Warning => AlertLevel::Warning,
                    wit::AlertLevel::Error => AlertLevel::Error,
                },
                code: a.code,
                message: a.message,
            })
            .collect(),
    }
}

fn from_wit_event(e: wit::Event) -> DriverEvent {
    match e {
        wit::Event::StatusChanged(s) => DriverEvent::StatusChanged(from_wit_status(s)),
        wit::Event::UserActionRequired(a) => DriverEvent::UserActionRequired(from_wit_action(a)),
        wit::Event::JobFinished => DriverEvent::JobFinished,
        wit::Event::Fault(e) => DriverEvent::Fault(from_wit_error(e)),
    }
}

fn pt(p: ps_core::PtUm) -> wit::PointUm {
    wit::PointUm { x: p.x, y: p.y }
}

fn to_wit_step(c: &ps_gcode::Command) -> wit::Step {
    use ps_gcode::Command as C;
    match c {
        C::Comment(s) => wit::Step::Comment(s.clone()),
        C::Units { millimetres } => wit::Step::Units(*millimetres),
        C::Absolute => wit::Step::Absolute,
        C::Rapid { to } => wit::Step::Travel(pt(*to)),
        C::Linear {
            to,
            a_mdeg,
            feed_mm_min,
        } => wit::Step::Line(wit::LineStep {
            to: pt(*to),
            a_mdeg: *a_mdeg,
            feed_mm_min: *feed_mm_min,
        }),
        C::Arc {
            to,
            centre_offset,
            ccw,
            feed_mm_min,
        } => wit::Step::Arc(wit::ArcStep {
            to: pt(*to),
            centre_offset: pt(*centre_offset),
            ccw: *ccw,
            feed_mm_min: *feed_mm_min,
        }),
        C::Dwell { ms } => wit::Step::Dwell(*ms),
        C::Pause(s) => wit::Step::Pause(s.clone()),
        C::ToolChange { slot } => wit::Step::ToolChange(*slot),
        C::ToolDown { force } => wit::Step::ToolDown(*force),
        C::ToolUp => wit::Step::ToolUp,
        C::Temperature { celsius, wait } => wit::Step::Temperature(wit::TemperatureStep {
            celsius: *celsius,
            wait: *wait,
        }),
        C::End => wit::Step::End,
    }
}

// ---- running plugins ----

/// Loads WebAssembly device plugins under the sandbox limits.
pub struct WasmHost {
    engine: Engine,
    limits: WasmLimits,
    stop: Arc<AtomicBool>,
}

impl std::fmt::Debug for WasmHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmHost")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl Drop for WasmHost {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn plugin_error(code: &str, message: impl std::fmt::Display) -> DeviceError {
    DeviceError::new(code, message.to_string())
}

impl WasmHost {
    /// A host with its engine and the thread that advances the epoch for deadlines.
    ///
    /// # Errors
    ///
    /// When the engine cannot be created on this machine.
    pub fn new(limits: WasmLimits) -> Result<Self, DeviceError> {
        let mut config = Config::new();
        config.epoch_interruption(true);
        let engine =
            Engine::new(&config).map_err(|e| plugin_error("plugin-engine", format!("{e:#}")))?;
        let stop = Arc::new(AtomicBool::new(false));
        let ticker = engine.clone();
        let flag = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("ps-wasm-epoch".to_owned())
            .spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(TICK);
                    ticker.increment_epoch();
                }
            })
            .map_err(|e| plugin_error("plugin-engine", e))?;
        Ok(Self {
            engine,
            limits,
            stop,
        })
    }

    fn ticks_for(limit: Duration) -> u64 {
        let ticks = limit.as_millis() / TICK.as_millis().max(1);
        u64::try_from(ticks).unwrap_or(u64::MAX).max(1)
    }

    fn deadline_ticks(&self) -> u64 {
        Self::ticks_for(self.limits.call_timeout)
    }

    /// A plugin with a connection to a device, for `identify` and for a session.
    fn instantiate(
        &self,
        bytes: &[u8],
        transport: Box<dyn BlockingTransport>,
        internet: Option<InternetAccess>,
    ) -> Result<(Store<State>, DevicePlugin, Resource<Conn>), DeviceError> {
        let (mut store, plugin) = self.build(bytes, internet, None)?;
        let conn = store
            .data_mut()
            .table
            .push(Conn(transport))
            .map_err(|e| plugin_error("plugin-engine", e))?;
        Ok((store, plugin, conn))
    }

    /// A plugin with no connection yet; `buses` is set only for discovery.
    fn build(
        &self,
        bytes: &[u8],
        internet: Option<InternetAccess>,
        buses: Option<ScopedBuses>,
    ) -> Result<(Store<State>, DevicePlugin), DeviceError> {
        let component = Component::new(&self.engine, bytes)
            .map_err(|e| plugin_error("plugin-invalid", format!("{e:#}")))?;
        let mut linker = Linker::<State>::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
            .map_err(|e| plugin_error("plugin-engine", format!("{e:#}")))?;
        DevicePlugin::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
            .map_err(|e| plugin_error("plugin-invalid", format!("{e:#}")))?;
        let state = State {
            // Nothing is preopened, inherited or opened: the plugin has no ambient authority.
            wasi: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
            limiter: Limiter {
                memory_bytes: self.limits.memory_bytes,
                denied: false,
            },
            kv: BTreeMap::new(),
            internet,
            discovery: buses.map(|buses| Discovering {
                buses,
                shown: Vec::new(),
            }),
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limiter);
        store.set_epoch_deadline(self.deadline_ticks());
        let plugin = DevicePlugin::instantiate(&mut store, &component, &linker)
            .map_err(|e| classify(&store, &e))?;
        Ok((store, plugin))
    }

    /// Asks a plugin which of the devices on the buses `buses` allows are its candidates.
    ///
    /// The plugin looks at the buses through the host and names the ids it wants offered; only ids
    /// the host showed it in this call are returned, as the host's own records.
    ///
    /// # Errors
    ///
    /// A `plugin-*` [`DeviceError`] when the bytes are not a valid component, the plugin traps,
    /// runs past [`WasmLimits::discover_timeout`], or wants more memory than allowed.
    pub async fn discover(
        &self,
        bytes: &[u8],
        buses: ScopedBuses,
    ) -> Result<Vec<DiscoveredDevice>, DeviceError> {
        let (mut store, plugin) = self.build(bytes, None, Some(buses))?;
        let ticks = Self::ticks_for(self.limits.discover_timeout);
        tokio::task::spawn_blocking(move || {
            store.set_epoch_deadline(ticks);
            let named = plugin
                .plotter_device_driver()
                .call_discover(&mut store)
                .map_err(|e| classify(&store, &e))?;
            let shown = store
                .data_mut()
                .discovery
                .take()
                .map(|d| d.shown)
                .unwrap_or_default();
            let mut offered: Vec<DiscoveredDevice> = Vec::new();
            for id in named {
                let id = DeviceId(id);
                if offered.iter().any(|d| d.id == id) {
                    continue;
                }
                if let Some(device) = shown.iter().find(|d| d.id == id) {
                    offered.push(device.clone());
                }
            }
            Ok(offered)
        })
        .await
        .map_err(|e| plugin_error("plugin-engine", e))?
    }

    /// Asks a plugin whether `transport` is a device it drives.
    pub async fn identify(
        &self,
        bytes: &[u8],
        transport: Box<dyn BlockingTransport>,
    ) -> Option<DeviceInfo> {
        let (mut store, plugin, conn) = self.instantiate(bytes, transport, None).ok()?;
        let ticks = self.deadline_ticks();
        let outcome = tokio::task::spawn_blocking(move || {
            store.set_epoch_deadline(ticks);
            plugin
                .plotter_device_driver()
                .call_identify(&mut store, conn)
                .ok()
                .flatten()
                .map(from_wit_info)
        })
        .await;
        outcome.ok().flatten()
    }

    /// Opens a session with a plugin over `transport`.
    ///
    /// # Errors
    ///
    /// A `plugin-*` [`DeviceError`] when the bytes are not a valid component, the plugin traps,
    /// runs too long, or wants more memory than allowed.
    pub async fn open(
        &self,
        bytes: &[u8],
        transport: Box<dyn BlockingTransport>,
    ) -> Result<WasmDriver, DeviceError> {
        self.open_with(bytes, transport, None).await
    }

    /// Opens a session that may also reach the internet hosts `internet` approves.
    ///
    /// # Errors
    ///
    /// The same as [`open`](Self::open).
    pub async fn open_with(
        &self,
        bytes: &[u8],
        transport: Box<dyn BlockingTransport>,
        internet: Option<InternetAccess>,
    ) -> Result<WasmDriver, DeviceError> {
        let (store, plugin, conn) = self.instantiate(bytes, transport, internet)?;
        let ticks = self.deadline_ticks();
        let opened = tokio::task::spawn_blocking(move || {
            let mut inner = Inner {
                store,
                plugin,
                session: None,
                info: None,
                capabilities: None,
                ticks,
                broken: false,
            };
            let session = inner.call(|s, plugin| {
                plugin
                    .plotter_device_driver()
                    .session()
                    .call_constructor(&mut *s, conn)
            })?;
            inner.session = Some(session);
            let info = inner.session_call(|s, g, h| g.call_info(s, h))?;
            let caps = inner.session_call(|s, g, h| g.call_capabilities(s, h))?;
            inner.info = Some(from_wit_info(info));
            inner.capabilities = Some(Capabilities {
                pause: caps.pause,
                resume: caps.resume,
                cancel: caps.cancel,
                jog: caps.jog,
                home: caps.home,
                load_mat: caps.load_mat,
                unload_mat: caps.unload_mat,
                tool_detection: caps.tool_detection,
                reports_position: caps.reports_position,
                reports_temperature: caps.reports_temperature,
                max_inflight_lines: caps.max_inflight_lines,
            });
            Ok::<Inner, DeviceError>(inner)
        })
        .await
        .map_err(|e| plugin_error("plugin-engine", e))??;
        let info = opened.info.clone().unwrap_or_else(|| DeviceInfo {
            model: String::new(),
            serial_number: None,
            firmware: None,
            profile_id: None,
            transport: TransportKind::Serial,
        });
        let capabilities = opened.capabilities.clone().unwrap_or_default();
        Ok(WasmDriver {
            inner: Arc::new(Mutex::new(opened)),
            info,
            capabilities,
        })
    }
}

/// Turns a wasmtime failure into a plugin error, recognising the two limits.
fn classify(store: &Store<State>, error: &wasmtime::Error) -> DeviceError {
    if error.downcast_ref::<Trap>() == Some(&Trap::Interrupt) {
        return plugin_error("plugin-timeout", "the plugin did not return in time");
    }
    if store.data().limiter.denied {
        return plugin_error(
            "plugin-memory",
            "the plugin asked for more memory than allowed",
        );
    }
    plugin_error("plugin-trap", format!("{error:#}"))
}

struct Inner {
    store: Store<State>,
    plugin: DevicePlugin,
    session: Option<ResourceAny>,
    info: Option<DeviceInfo>,
    capabilities: Option<Capabilities>,
    ticks: u64,
    broken: bool,
}

impl Inner {
    fn call<T>(
        &mut self,
        f: impl FnOnce(&mut Store<State>, &DevicePlugin) -> wasmtime::Result<T>,
    ) -> Result<T, DeviceError> {
        if self.broken {
            return Err(plugin_error("plugin-broken", "the plugin stopped earlier"));
        }
        self.store.set_epoch_deadline(self.ticks);
        match f(&mut self.store, &self.plugin) {
            Ok(v) => Ok(v),
            Err(e) => {
                self.broken = true;
                Err(classify(&self.store, &e))
            }
        }
    }

    fn session_call<T>(
        &mut self,
        f: impl FnOnce(&mut Store<State>, &guest::GuestSession<'_>, ResourceAny) -> wasmtime::Result<T>,
    ) -> Result<T, DeviceError> {
        let Some(session) = self.session else {
            return Err(plugin_error("plugin-broken", "no session is open"));
        };
        self.call(|store, plugin| f(store, &plugin.plotter_device_driver().session(), session))
    }
}

/// An open session with a WebAssembly plugin.
pub struct WasmDriver {
    inner: Arc<Mutex<Inner>>,
    info: DeviceInfo,
    capabilities: Capabilities,
}

impl std::fmt::Debug for WasmDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmDriver")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl WasmDriver {
    async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Inner) -> Result<T, DeviceError> + Send + 'static,
    ) -> Result<T, DeviceError> {
        let inner = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || {
            let mut guard = inner.lock().unwrap_or_else(PoisonError::into_inner);
            f(&mut guard)
        })
        .await
        .map_err(|e| plugin_error("plugin-engine", e))?
    }

    async fn unit(
        &self,
        f: impl FnOnce(
            &mut Store<State>,
            &guest::GuestSession<'_>,
            ResourceAny,
        ) -> wasmtime::Result<Result<(), wit::DeviceError>>
        + Send
        + 'static,
    ) -> Result<(), DeviceError> {
        self.run(move |i| i.session_call(f)?.map_err(from_wit_error))
            .await
    }
}

#[async_trait]
impl Driver for WasmDriver {
    fn info(&self) -> DeviceInfo {
        self.info.clone()
    }

    fn capabilities(&self) -> Capabilities {
        self.capabilities.clone()
    }

    async fn status(&mut self) -> Result<Status, DeviceError> {
        self.run(|i| {
            i.session_call(|s, g, h| g.call_status(s, h))
                .map(from_wit_status)
        })
        .await
    }

    async fn wait_event(&mut self, timeout: Duration) -> Option<DriverEvent> {
        let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        self.run(move |i| i.session_call(|s, g, h| g.call_wait_event(s, h, ms)))
            .await
            .ok()
            .flatten()
            .map(from_wit_event)
    }

    async fn begin_job(&mut self, header: JobHeader) -> Result<(), DeviceError> {
        let header = wit::JobHeader {
            job_id: header.job_id,
            mat_index: header.mat_index,
            profile_id: header.profile_id,
            tools: header
                .tools
                .into_iter()
                .map(|(slot, tool, holder)| wit::ToolEntry { slot, tool, holder })
                .collect(),
        };
        self.unit(move |s, g, h| g.call_begin_job(s, h, &header))
            .await
    }

    async fn send(&mut self, lines: &[GcodeLine]) -> Result<u32, DeviceError> {
        let lines: Vec<wit::GcodeLine> = lines
            .iter()
            .map(|l| wit::GcodeLine {
                number: l.number,
                text: l.text.clone(),
                step: to_wit_step(&l.command),
            })
            .collect();
        self.run(move |i| {
            i.session_call(|s, g, h| g.call_send(s, h, &lines))?
                .map_err(from_wit_error)
        })
        .await
    }

    async fn end_job(&mut self) -> Result<(), DeviceError> {
        self.unit(|s, g, h| g.call_end_job(s, h)).await
    }

    async fn pause(&mut self) -> Result<(), DeviceError> {
        self.unit(|s, g, h| g.call_pause(s, h)).await
    }

    async fn resume(&mut self) -> Result<(), DeviceError> {
        self.unit(|s, g, h| g.call_resume(s, h)).await
    }

    async fn cancel(&mut self) -> Result<(), DeviceError> {
        self.unit(|s, g, h| g.call_cancel(s, h)).await
    }

    async fn user_action_done(&mut self, id: u32) -> Result<(), DeviceError> {
        self.unit(move |s, g, h| g.call_user_action_done(s, h, id))
            .await
    }

    async fn jog(&mut self, across_mm: f64, down_mm: f64) -> Result<(), DeviceError> {
        self.unit(move |s, g, h| g.call_jog(s, h, across_mm, down_mm))
            .await
    }

    async fn disconnect(&mut self) {
        let _ = self
            .run(|i| i.session_call(|s, g, h| g.call_disconnect(s, h)))
            .await;
    }
}

#[cfg(test)]
mod tests;

/// Builds the plugins for tests elsewhere in the crate.
#[cfg(test)]
pub(crate) mod tests_support {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::OnceLock;

    /// A fixed set of buses for plugin discovery tests; reading the serial list takes `delay`, as a
    /// slow driver would.
    #[derive(Default)]
    pub(crate) struct FakeBuses {
        pub(crate) serial: Vec<crate::host::DiscoveredDevice>,
        pub(crate) usb: Vec<crate::host::DiscoveredDevice>,
        pub(crate) delay: std::time::Duration,
    }

    impl crate::bus::Buses for FakeBuses {
        fn serial_ports(&self) -> Vec<crate::host::DiscoveredDevice> {
            std::thread::sleep(self.delay);
            self.serial.clone()
        }
        fn usb_devices(&self) -> Vec<crate::host::DiscoveredDevice> {
            self.usb.clone()
        }
        fn ble_devices(&self, _: std::time::Duration) -> Vec<crate::host::DiscoveredDevice> {
            Vec::new()
        }
        fn mdns_services(
            &self,
            _: &str,
            _: std::time::Duration,
        ) -> Vec<crate::host::DiscoveredDevice> {
            Vec::new()
        }
    }

    /// A serial port as the system lists it.
    pub(crate) fn port(
        name: &str,
        usb: Option<(u16, u16)>,
        product: Option<&str>,
    ) -> crate::host::DiscoveredDevice {
        crate::host::DiscoveredDevice {
            id: crate::host::DeviceId(format!("serial:{name}")),
            name: product.unwrap_or(name).to_owned(),
            transport: crate::types::TransportKind::Serial,
            usb,
            address: Some(name.to_owned()),
            serial: None,
        }
    }

    /// A manifest filter that allows serial ports.
    pub(crate) fn serial_filters() -> Vec<crate::host::DeviceFilter> {
        vec![crate::host::DeviceFilter {
            transport: crate::types::TransportKind::Serial,
            usb: None,
        }]
    }

    /// The GRBL plugin as a component, built for `wasm32-wasip2` on first use.
    pub(crate) fn grbl() -> Vec<u8> {
        plugin("grbl-plugin")
    }

    /// The named test plugin (`grbl-plugin` or `rogue-plugin`) as a component.
    ///
    /// Both are built and read once for the whole test binary: cargo replaces an output file even
    /// when it is up to date, so a test that builds while another reads can find no file.
    pub(crate) fn plugin(package: &str) -> Vec<u8> {
        static BUILT: OnceLock<BTreeMap<&str, Vec<u8>>> = OnceLock::new();
        BUILT.get_or_init(|| {
            let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
                || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"),
                PathBuf::from,
            );
            ["grbl-plugin", "rogue-plugin"]
                .into_iter()
                .map(|package| {
                    let status = Command::new(env!("CARGO"))
                        .args(["build", "-p", package])
                        .args(["--target", "wasm32-wasip2", "--release"])
                        .status()
                        .expect("cargo runs");
                    assert!(status.success(), "building {package} for wasm32-wasip2");
                    let file = format!("{}.wasm", package.replace('-', "_"));
                    let wasm = std::fs::read(target.join("wasm32-wasip2/release").join(file))
                        .expect("the built plugin");
                    (package, wasm)
                })
                .collect()
        })[package]
            .clone()
    }
}
