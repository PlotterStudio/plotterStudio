//! The `compile`, `export` and `machine` tools: from design to machine.

use std::path::Path;
use std::time::{Duration, Instant};

use ps_device::DeviceId;
use serde_json::{Value, json};

use super::types::{Compile, Export, Machine};
use super::{ApiError, Reply};
use crate::clock::rfc3339_utc;
use crate::export::{ExportFormat, export_job, gcode_for_mat, job_id_of};
use crate::machine::job_header;
use crate::ui::App;

/// How long `compile` `wait` waits when no timeout is given.
const DEFAULT_WAIT_MS: u64 = 30_000;
/// How long `machine` `connect` waits for the session to become ready.
const CONNECT_WAIT: Duration = Duration::from_secs(3);
/// Pixels per millimetre of a preview picture when none is given.
const DEFAULT_PX_PER_MM: f64 = 4.0;

impl App {
    pub(super) fn compile_tool(&mut self, op: Compile) -> Reply {
        match op {
            Compile::Start => {
                if self.state().compile_running() {
                    return Ok(json!({"started": false, "running": true}));
                }
                if !self.state_mut().compile_now() {
                    return Err(ApiError::new(
                        "no_machine_profile",
                        "there is no machine profile to compile for",
                    )
                    .hint("call `operation` set_machine; `query` machines lists ids"));
                }
                Ok(json!({"started": true, "running": true}))
            }
            Compile::Status => Ok(json!({
                "running": self.state().compile_running(),
                "current": self.state().compile_is_current(),
                "error": self.state().compile_error(),
            })),
            Compile::Wait { timeout_ms } => {
                if !self.state().compile_running() && !self.state().compile_is_current() {
                    self.state_mut().compile_now();
                }
                let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_WAIT_MS));
                self.state_mut().wait_for_compile(timeout);
                let finished = !self.state().compile_running() && self.state().compile_is_current();
                Ok(json!({
                    "finished": finished,
                    "running": self.state().compile_running(),
                    "error": self.state().compile_error(),
                }))
            }
            Compile::Result => {
                let summary = self.state().compile_summary();
                let problems: Vec<Value> = self
                    .state()
                    .problems()
                    .iter()
                    .map(|d| self.diagnostic_json(d))
                    .collect();
                Ok(json!({
                    "current": self.state().compile_is_current(),
                    "error": self.state().compile_error(),
                    "mats": summary.map(|s| s.mats),
                    "machine_time_ms": summary.map(|s| s.machine_time_ms),
                    "tool_changes": summary.map(|s| s.tool_changes),
                    "problems": problems,
                }))
            }
        }
    }

    fn not_compiled() -> ApiError {
        ApiError::new("not_compiled", "there is no up-to-date compiled job").hint(
            "call `compile` start, then `compile` wait, and fix any errors in `compile` result",
        )
    }

    pub(super) fn export_tool(&mut self, op: Export, now_ms: u64) -> Reply {
        match op {
            Export::Gcode { mat } => gcode_for_mat(self.state(), mat)
                .map(|text| json!({"gcode": text}))
                .ok_or_else(Self::not_compiled),
            Export::Files { folder } => {
                if job_id_of(self.state()).is_none() || self.state().fresh_compile().is_none() {
                    return Err(Self::not_compiled());
                }
                let stem = self
                    .state()
                    .project_path()
                    .and_then(Path::file_stem)
                    .map_or_else(|| "job".to_owned(), |s| s.to_string_lossy().into_owned());
                let time = rfc3339_utc(now_ms / 1_000);
                let files = export_job(
                    self.state(),
                    Path::new(&folder),
                    &stem,
                    ExportFormat::Both,
                    &time,
                )
                .map_err(|e| ApiError::new("export_failed", e.to_string()))?;
                let files: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
                Ok(json!({"files": files}))
            }
            Export::Preview {
                path,
                px_per_mm,
                look: _,
            } => {
                let scale = px_per_mm.unwrap_or(DEFAULT_PX_PER_MM);
                if !(scale > 0.0 && scale.is_finite()) {
                    return Err(ApiError::new("bad_arguments", "px_per_mm must be positive"));
                }
                let preview = self
                    .state()
                    .preview(scale, usize::MAX)
                    .ok_or_else(Self::not_compiled)?;
                let image = &preview.image;
                image::save_buffer(
                    &path,
                    &image.rgba,
                    image.width,
                    image.height,
                    image::ColorType::Rgba8,
                )
                .map_err(|e| ApiError::new("export_failed", format!("{path}: {e}")))?;
                Ok(json!({"path": path, "width": image.width, "height": image.height}))
            }
        }
    }

    /// Whether a call may act on this device: the simulator always, real hardware only when a
    /// person armed the API.
    fn may_drive(&self, simulator: bool) -> Result<(), ApiError> {
        if simulator || self.api_armed {
            Ok(())
        } else {
            Err(ApiError::new(
                "not_armed",
                "this would move real hardware, and the API has not been armed",
            )
            .hint("ask the user to arm the API; only the user can, and no tool does it"))
        }
    }

    fn device(&self, text: &str) -> Result<(DeviceId, bool), ApiError> {
        let id = DeviceId(text.to_owned());
        self.machine
            .devices()
            .iter()
            .find(|d| d.id == id)
            .map(|d| (id, d.plugin_id == ps_device::SIMULATOR_PLUGIN))
            .ok_or_else(|| {
                ApiError::new("unknown_device", format!("no device {text:?}"))
                    .hint("call `machine` list")
            })
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one arm per operation of the tool; each arm is a few lines and splitting would only scatter the dispatch"
    )]
    pub(super) fn machine_tool(&mut self, op: Machine) -> Reply {
        match op {
            Machine::List => {
                let devices: Vec<Value> = self
                    .machine
                    .devices()
                    .into_iter()
                    .map(|d| {
                        json!({
                            "id": d.id.0,
                            "plugin": d.plugin_id,
                            "model": d.info.model,
                            "approved": d.approved,
                            "simulator": d.plugin_id == ps_device::SIMULATOR_PLUGIN,
                        })
                    })
                    .collect();
                Ok(json!({"devices": devices}))
            }
            Machine::Approve { device } => {
                let (id, simulator) = self.device(&device)?;
                self.may_drive(simulator)?;
                self.machine.approve(&id);
                Ok(json!({}))
            }
            Machine::Connect { device } => {
                let (id, _) = self.device(&device)?;
                self.machine.connect(&id).map_err(|e| match e {
                    ps_device::ConnectError::NotApproved { prompt } => {
                        ApiError::new("not_approved", prompt)
                            .hint("call `machine` approve for this device first")
                    }
                    other => ApiError::new("connect_failed", other.to_string()),
                })?;
                // The session reports ready a moment after it opens.
                let deadline = Instant::now() + CONNECT_WAIT;
                while !self.machine.is_ready() && Instant::now() < deadline {
                    self.machine.poll();
                    std::thread::sleep(Duration::from_millis(2));
                }
                Ok(json!({"ready": self.machine.is_ready()}))
            }
            Machine::Disconnect => {
                self.machine.disconnect();
                Ok(json!({}))
            }
            Machine::Status => {
                self.machine.poll();
                let status = self.machine.status();
                Ok(json!({
                    "connected": status.is_some(),
                    "ready": self.machine.is_ready(),
                    "lost": self.machine.is_lost(),
                    "simulator": self.machine.is_simulator_connected(),
                    "model": self.machine.model(),
                    "state": status.as_ref().map(|s| format!("{:?}", s.state)),
                    "progress": self.machine.progress().map(|p| json!({
                        "lines_done": p.lines_done,
                        "lines_total": p.lines_total,
                        "current_op": p.current_op,
                    })),
                    "prompt": self.machine.prompt().map(|p| json!({
                        "kind": format!("{:?}", p.kind),
                        "message": p.message,
                    })),
                    "last_outcome": self.machine.last_outcome().map(|o| format!("{o:?}")),
                    "armed": self.api_armed,
                }))
            }
            Machine::Send { mat } => {
                self.machine.poll();
                self.may_drive(self.machine.is_simulator_connected())?;
                if !self.machine.is_ready() {
                    return Err(
                        ApiError::new("no_machine", "no machine is connected and idle")
                            .hint("call `machine` list, approve, connect, then status"),
                    );
                }
                let (Some(program), Some(header)) = (
                    gcode_for_mat(self.state(), mat),
                    job_header(self.state(), mat),
                ) else {
                    return Err(Self::not_compiled());
                };
                self.machine
                    .send(program, header)
                    .map_err(|e| ApiError::refused(e.to_string(), "call `machine` status"))?;
                Ok(json!({"started": true}))
            }
            Machine::Pause => {
                self.machine.pause();
                Ok(json!({}))
            }
            Machine::Resume => {
                self.machine.resume();
                Ok(json!({}))
            }
            Machine::Cancel => {
                self.machine.cancel();
                Ok(json!({}))
            }
            Machine::Jog { x_mm, y_mm } => {
                self.may_drive(self.machine.is_simulator_connected())?;
                if !self.machine.can_jog() {
                    return Err(ApiError::refused(
                        "the machine cannot jog now",
                        "it must be connected and idle",
                    ));
                }
                self.machine.jog(x_mm, y_mm);
                Ok(json!({}))
            }
            Machine::AnswerPrompt => {
                self.machine.answer_prompt();
                Ok(json!({}))
            }
        }
    }
}
