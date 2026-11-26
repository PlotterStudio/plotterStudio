//! The app's view of the device host.
//!
//! Wraps [`DeviceHost`] and the open [`MachineHandle`] for the UI thread: looking for machines
//! (which only the installed device plugins know how to do) and approval, one session at a time, a run with its prompts, progress and outcome. Nothing here
//! blocks: the handle runs the machine on its own thread and the controller drains its events
//! each frame with [`MachineController::poll`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use ps_device::{
    ConnectError, DeclaredHost, DeviceEntry, DeviceHost, DeviceId, HandleConfig, HandleEvent,
    JobHeader, JobOutcome, JobProgress, MachineHandle, MachineStatus, PermissionStore,
    PinningTransports, Plugin, RunRefusal, RunRequest, SimulatorPlugin, UserAction, WasmHost,
    WasmLimits, WasmPlugin,
};
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::TryRecvError;

use crate::export::job_id_of;
use crate::state::AppState;

/// How long discovery and connecting may take before the UI gives up waiting.
const BRIEF: Duration = Duration::from_secs(5);

/// The job header for a compiled mat (0-based index), or `None` when it is not compiled.
#[must_use]
pub fn job_header(state: &AppState, mat_index: usize) -> Option<JobHeader> {
    let compiled = state.fresh_compile()?;
    let mat = compiled.job.mats.get(mat_index)?;
    let profile = state.profile()?;
    Some(JobHeader {
        job_id: job_id_of(state)?,
        mat_index: u32::try_from(mat_index + 1).unwrap_or(u32::MAX),
        profile_id: profile.id.clone(),
        // Some machines (a cutting plotter, say) have no command to pick a holder: the plugin starts the job
        // differently for each, so it needs to know which tool is in which.
        tools: mat
            .tool_table
            .iter()
            .map(|t| (t.slot, t.tool.id().to_owned(), t.holder.as_str().to_owned()))
            .collect(),
    })
}

/// What loading a folder of plugins found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginLoad {
    /// The ids of the plugins now installed from the folder.
    pub loaded: Vec<String>,
    /// One message for each file that could not be used.
    pub problems: Vec<String>,
}

/// A plugin that is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    /// Its id.
    pub id: String,
    /// Its display name.
    pub name: String,
}

/// What installing one plugin package did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installation {
    /// The plugin's id.
    pub id: String,
    /// The plugin's display name.
    pub name: String,
    /// True when a plugin with this id was already running: the new file is in place but the app
    /// must be restarted to use it.
    pub restart_needed: bool,
}

/// Discovery, approval, one machine session and its run.
pub struct MachineController {
    host: Arc<DeviceHost>,
    simulate: bool,
    /// True while a scan started by [`Self::refresh_in_background`] is running.
    refreshing: Arc<AtomicBool>,
    wasm: Option<Arc<WasmHost>>,
    /// The installed plugins: id to display name.
    plugins: BTreeMap<String, String>,
    runtime: tokio::runtime::Runtime,
    handle: Option<MachineHandle>,
    events: Option<Receiver<HandleEvent>>,
    prompt: Option<UserAction>,
    outcome: Option<JobOutcome>,
    lost: bool,
    /// Whether the connected machine is the built-in simulator, which cannot move anything.
    simulator_connected: bool,
    /// Where approvals and certificate pins are kept, and what was last written there.
    permissions_file: Option<(PathBuf, PermissionStore)>,
}

impl std::fmt::Debug for MachineController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MachineController")
            .field("connected", &self.handle.is_some())
            .finish_non_exhaustive()
    }
}

impl MachineController {
    /// A controller; with `simulate` the built-in simulated machine is discoverable.
    ///
    /// # Panics
    ///
    /// Only if the operating system cannot start a small async runtime.
    #[must_use]
    pub fn new(simulate: bool) -> Self {
        let host = Arc::new(DeviceHost::new(
            PermissionStore::default(),
            HandleConfig::default(),
        ));
        if simulate {
            host.add_plugin(Arc::new(SimulatorPlugin::new()));
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("the operating system can start an async runtime");
        Self {
            host,
            simulate,
            refreshing: Arc::new(AtomicBool::new(false)),
            wasm: None,
            plugins: BTreeMap::new(),
            runtime,
            handle: None,
            events: None,
            prompt: None,
            outcome: None,
            lost: false,
            simulator_connected: false,
            permissions_file: None,
        }
    }

    /// Keeps approvals, allowed internet hosts and certificate pins in `file`: they are read now
    /// and written whenever they change, so the user is asked once, not every session.
    ///
    /// # Errors
    ///
    /// A message when the file exists but is damaged; the session then starts with nothing
    /// approved and the damaged file is replaced the next time something is approved.
    pub fn persist_permissions(&mut self, file: &Path) -> Result<(), String> {
        let loaded = PermissionStore::load(file);
        let store = loaded.clone().unwrap_or_default();
        {
            let shared = self.host.shared_permissions();
            *shared.lock().unwrap_or_else(PoisonError::into_inner) = store.clone();
        }
        // Plugins added before this call learn the hosts that were allowed.
        for declared in self.host.declared_hosts() {
            if store.is_host_allowed(&declared.plugin_id, &declared.host) {
                self.host.allow_host(&declared.plugin_id, &declared.host);
            }
        }
        self.permissions_file = Some((file.to_path_buf(), store));
        loaded.map(|_| ()).map_err(|e| e.to_string())
    }

    /// Writes the approvals if they changed since they were last saved.
    fn save_permissions(&mut self) {
        let Some((file, saved)) = self.permissions_file.as_mut() else {
            return;
        };
        let now = self.host.permissions();
        if now != *saved && now.save(file).is_ok() {
            *saved = now;
        }
    }

    /// Every internet host an installed plugin declares, with whether it is allowed.
    #[must_use]
    pub fn declared_hosts(&self) -> Vec<DeclaredHost> {
        self.host.declared_hosts()
    }

    /// Allows a plugin to reach a host it declared; false when it did not declare it.
    pub fn allow_host(&mut self, plugin_id: &str, host: &str) -> bool {
        let ok = self.host.allow_host(plugin_id, host);
        self.save_permissions();
        ok
    }

    /// Blocks a declared host again; false when the plugin did not declare it.
    pub fn revoke_host(&mut self, plugin_id: &str, host: &str) -> bool {
        let ok = self.host.revoke_host(plugin_id, host);
        self.save_permissions();
        ok
    }

    /// The internet hosts plugins are using now, as `(plugin name, host)`.
    #[must_use]
    pub fn internet_in_use(&self) -> Vec<(String, String)> {
        self.host.internet_in_use()
    }

    /// Installs every `*.psdevice` plugin in `dir`. A missing folder is not an error; a file that
    /// is not a valid plugin, or repeats an installed plugin's id, is reported and skipped.
    pub fn install_plugins(&mut self, dir: &Path) -> PluginLoad {
        let mut load = PluginLoad::default();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return load;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "psdevice"))
            .collect();
        files.sort();
        for file in files {
            let name = file
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            let bytes = match std::fs::read(&file) {
                Ok(b) => b,
                Err(e) => {
                    load.problems.push(format!("{name}: {e}"));
                    continue;
                }
            };
            match self.make_plugin(&bytes) {
                Ok(plugin) => {
                    let id = plugin.manifest().id.clone();
                    if self.plugins.contains_key(&id) {
                        load.problems.push(format!(
                            "{name}: a plugin with id {id} is already installed"
                        ));
                    } else {
                        self.add_plugin(plugin);
                        load.loaded.push(id);
                    }
                }
                Err(e) => load.problems.push(format!("{name}: {e}")),
            }
        }
        load
    }

    /// Installs one plugin package the user chose: it is loaded to prove it is a plugin, copied
    /// into `dir` so the next start finds it there, and made available now. Nothing is written
    /// when the package does not load.
    ///
    /// A plugin whose id is already running has its file replaced but keeps running as it was,
    /// because a running plugin cannot be swapped; `Installation::restart_needed` says so.
    ///
    /// # Errors
    ///
    /// A message when the bytes are not a plugin or the file cannot be written.
    pub fn install_plugin(&mut self, dir: &Path, bytes: &[u8]) -> Result<Installation, String> {
        let plugin = self.make_plugin(bytes)?;
        let id = plugin.manifest().id.clone();
        let name = plugin.manifest().name.clone();
        crate::plugin_install::store(dir, &id, bytes).map_err(|e| e.to_string())?;
        let restart_needed = self.plugins.contains_key(&id);
        if !restart_needed {
            self.add_plugin(plugin);
        }
        Ok(Installation {
            id,
            name,
            restart_needed,
        })
    }

    /// The plugins installed now, by id.
    #[must_use]
    pub fn installed_plugins(&self) -> Vec<InstalledPlugin> {
        self.plugins
            .iter()
            .map(|(id, name)| InstalledPlugin {
                id: id.clone(),
                name: name.clone(),
            })
            .collect()
    }

    /// Loads a package without registering it.
    fn make_plugin(&mut self, bytes: &[u8]) -> Result<WasmPlugin, String> {
        let host = self.wasm_host()?;
        let pins = self.host.shared_permissions();
        WasmPlugin::from_package_with(bytes, host, |manifest| {
            Arc::new(PinningTransports::new(pins, &manifest.id))
        })
        .map_err(|e| e.to_string())
    }

    fn add_plugin(&mut self, plugin: WasmPlugin) {
        let manifest = plugin.manifest();
        self.plugins
            .insert(manifest.id.clone(), manifest.name.clone());
        self.host.add_plugin(Arc::new(plugin));
    }

    fn wasm_host(&mut self) -> Result<Arc<WasmHost>, String> {
        if let Some(host) = &self.wasm {
            return Ok(Arc::clone(host));
        }
        let host = Arc::new(WasmHost::new(WasmLimits::default()).map_err(|e| e.to_string())?);
        self.wasm = Some(Arc::clone(&host));
        Ok(host)
    }

    /// What plugins found on the last look but could not confirm.
    #[must_use]
    pub fn unclaimed(&self) -> Vec<ps_device::DiscoveredDevice> {
        self.host.unclaimed()
    }

    /// How many device plugins can look for machines. With none, the app can find nothing: the
    /// knowledge of how to find a machine lives in the plugin for it.
    #[must_use]
    pub fn plugin_count(&self) -> usize {
        self.plugins.len() + usize::from(self.simulate)
    }

    /// True while a look started by [`Self::refresh_in_background`] is still running.
    #[must_use]
    pub fn is_refreshing(&self) -> bool {
        self.refreshing.load(Ordering::Acquire)
    }

    /// Asks every installed plugin to look for its machines, without waiting for the answer: talking
    /// to ports and radios can take seconds, and the window must not freeze meanwhile. The list updates when it is done.
    /// Does nothing while an earlier look is still running.
    pub fn refresh_in_background(&mut self) {
        if self.refreshing.swap(true, Ordering::AcqRel) {
            return;
        }
        let host = Arc::clone(&self.host);
        let flag = Arc::clone(&self.refreshing);
        let started = std::thread::Builder::new()
            .name("machine-scan".to_owned())
            .spawn(move || {
                if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    runtime.block_on(host.refresh());
                    // A port still being opened must not hold up the process when it exits.
                    runtime.shutdown_background();
                }
                flag.store(false, Ordering::Release);
            });
        if started.is_err() {
            self.refreshing.store(false, Ordering::Release);
        }
    }

    /// Looks for machines again.
    pub fn refresh(&mut self) {
        // The timeout future must be built inside the runtime, hence the async block.
        let _ = self
            .runtime
            .block_on(async { tokio::time::timeout(BRIEF, self.host.refresh()).await });
    }

    /// The machines found, with whether the user has approved each.
    #[must_use]
    pub fn devices(&self) -> Vec<DeviceEntry> {
        self.host.devices().borrow().clone()
    }

    /// Records the user's approval of a machine for its plugin.
    pub fn approve(&mut self, id: &DeviceId) {
        self.host.approve(id);
        self.save_permissions();
    }

    /// Withdraws an approval.
    pub fn revoke(&mut self, id: &DeviceId) {
        self.host.revoke(id);
        self.save_permissions();
    }

    /// A copy of the approvals, to save in settings.
    #[must_use]
    pub fn permissions(&self) -> PermissionStore {
        self.host.permissions()
    }

    /// Opens a session with an approved machine, replacing any current one.
    ///
    /// # Errors
    ///
    /// [`ConnectError`] when the machine is unknown, not approved, or cannot be opened.
    pub fn connect(&mut self, id: &DeviceId) -> Result<(), ConnectError> {
        self.disconnect();
        let simulator = self
            .devices()
            .iter()
            .any(|d| d.id == *id && d.plugin_id == ps_device::SIMULATOR_PLUGIN);
        let handle = match self
            .runtime
            .block_on(async { tokio::time::timeout(BRIEF, self.host.connect(id)).await })
        {
            Ok(result) => result?,
            Err(_) => {
                return Err(ConnectError::Open(ps_device::DeviceError::new(
                    "timeout",
                    "the machine did not answer in time",
                )));
            }
        };
        self.events = Some(handle.subscribe());
        self.handle = Some(handle);
        self.simulator_connected = simulator;
        self.lost = false;
        Ok(())
    }

    /// Closes the session.
    pub fn disconnect(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.disconnect();
        }
        self.events = None;
        self.prompt = None;
        self.outcome = None;
        self.lost = false;
        self.simulator_connected = false;
    }

    /// Whether the machine connected now is the built-in simulator, which cannot move anything.
    #[must_use]
    pub fn is_simulator_connected(&self) -> bool {
        self.simulator_connected
    }

    /// Drains the machine's events: prompts, the end of a run, a lost connection.
    pub fn poll(&mut self) {
        // A certificate pinned on a first connection is recorded on another thread.
        self.save_permissions();
        let Some(rx) = self.events.as_mut() else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(HandleEvent::UserActionRequired(action)) => self.prompt = Some(action),
                Ok(HandleEvent::JobFinished(outcome)) => {
                    self.prompt = None;
                    self.outcome = Some(outcome);
                }
                Ok(HandleEvent::Lost) => {
                    self.lost = true;
                    self.prompt = None;
                }
                Ok(HandleEvent::StatusChanged(_)) | Err(TryRecvError::Lagged(_)) => {}
                Err(TryRecvError::Empty | TryRecvError::Closed) => return,
            }
        }
    }

    /// The machine's status snapshot, if connected.
    #[must_use]
    pub fn status(&self) -> Option<Arc<MachineStatus>> {
        self.handle.as_ref().map(MachineHandle::status)
    }

    /// Whether the connected machine can be jogged. Some machines take no direct move
    /// commands outside a job, so it offers none.
    #[must_use]
    pub fn can_jog(&self) -> bool {
        self.handle.as_ref().is_some_and(|h| h.capabilities().jog)
    }

    /// The connected machine's model name.
    #[must_use]
    pub fn model(&self) -> Option<String> {
        self.handle.as_ref().map(|h| h.info().model.clone())
    }

    /// Whether a machine is connected and idle, so a job may be sent.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.status().is_some_and(|s| {
            s.connection == ps_device::ConnectionState::Connected
                && s.state == ps_device::MachineState::Idle
        })
    }

    /// Whether the connection was lost.
    #[must_use]
    pub fn is_lost(&self) -> bool {
        self.lost
    }

    /// Starts a run of one mat's G-code.
    ///
    /// # Errors
    ///
    /// [`RunRefusal`] when nothing is connected, the machine is busy, the job was compiled for
    /// another machine, or the program is not valid.
    pub fn send(&mut self, program: String, header: JobHeader) -> Result<(), RunRefusal> {
        let handle = self.handle.as_ref().ok_or(RunRefusal::NotConnected)?;
        self.outcome = None;
        self.prompt = None;
        handle.run(RunRequest { program, header })
    }

    /// The prompt waiting for the user, if any.
    #[must_use]
    pub fn prompt(&self) -> Option<&UserAction> {
        self.prompt.as_ref()
    }

    /// Tells the machine the user did what the prompt asked.
    pub fn answer_prompt(&mut self) {
        if let (Some(prompt), Some(handle)) = (self.prompt.take(), &self.handle) {
            handle.ack_user_action(prompt.id);
        }
    }

    /// Progress of the run in progress.
    #[must_use]
    pub fn progress(&self) -> Option<JobProgress> {
        self.status().and_then(|s| s.progress.clone())
    }

    /// How the last run ended.
    #[must_use]
    pub fn last_outcome(&self) -> Option<&JobOutcome> {
        self.outcome.as_ref()
    }

    /// Forgets the last outcome.
    pub fn clear_outcome(&mut self) {
        self.outcome = None;
    }

    /// Pauses the run.
    pub fn pause(&self) {
        if let Some(h) = &self.handle {
            h.pause();
        }
    }

    /// Resumes a paused run.
    pub fn resume(&self) {
        if let Some(h) = &self.handle {
            h.resume();
        }
    }

    /// Cancels the run.
    pub fn cancel(&mut self) {
        if let Some(h) = &self.handle {
            h.cancel();
        }
    }

    /// Moves the carriage by an offset in millimetres.
    pub fn jog(&self, x_mm: f64, y_mm: f64) {
        if let Some(h) = &self.handle {
            h.jog(x_mm, y_mm);
        }
    }
}

#[cfg(test)]
mod tests;
