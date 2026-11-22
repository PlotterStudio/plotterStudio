//! The automation API: everything the window can do, as twelve parameterised tools.
//!
//! A host (a test, a script, later an MCP server) calls [`App::call_tool`] with a tool name and
//! JSON arguments and gets JSON back or an [`ApiError`] that says how to recover. The window
//! runs the same `AppState` code the tools do, so nothing here is a second implementation.
//!
//! Design rules:
//! - Objects, mats and rows are named by the ids earlier replies returned; nothing depends on
//!   what the window has selected. Replies that create things list the new ids under `created`.
//! - Every dialog is a parameter (paths, conflict choices).
//! - Moving real hardware needs a person to arm the API ([`App::arm_api`]); no tool arms it.
//! - [`coverage`] ties the command registry to the tools, so a new command cannot skip the API.

mod coverage;
mod doc;
mod edit;
mod job;
mod query;
mod shapes;
pub mod types;

use std::str::FromStr;

use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use ps_core::{MatId, ObjectId, OpRowId};

pub use coverage::{Coverage, coverage, covered_ids, model_coverage, model_covered};
use types::{
    Compile, Edit, Export, Import, Library, Machine, Mat, Object, Operation, Project, Query,
    Settings,
};

use crate::ui::App;

/// Why a tool call did not do what was asked, and what to try next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApiError {
    /// A stable machine-readable code such as `unknown_object`.
    pub code: &'static str,
    /// What went wrong.
    pub message: String,
    /// What to call or change to recover.
    pub hint: Option<String>,
}

impl ApiError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
        }
    }

    pub(crate) fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// The operation changed nothing.
    pub(crate) fn refused(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new("refused", message).hint(hint)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, " ({hint})")?;
        }
        Ok(())
    }
}

impl std::error::Error for ApiError {}

/// The result of a tool call.
pub type Reply = Result<Value, ApiError>;

/// One tool as a host lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolInfo {
    /// The tool's name.
    pub name: &'static str,
    /// What it is for, written for a model reading the tool list.
    pub description: &'static str,
    /// The JSON schema of its arguments.
    pub schema: Value,
}

fn schema_of<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).unwrap_or(Value::Null)
}

/// The twelve tools, in a fixed order.
#[must_use]
pub fn tool_catalog() -> Vec<ToolInfo> {
    let tool = |name, description, schema| ToolInfo {
        name,
        description,
        schema,
    };
    vec![
        tool(
            "project",
            "The project file: start a new one, open, save, describe.",
            schema_of::<Project>(),
        ),
        tool(
            "import",
            "Put things on the current mat: a drawing file, text, or a shape given by numbers. Replies list created object ids.",
            schema_of::<Import>(),
        ),
        tool(
            "object",
            "Act on objects by id: select, delete, duplicate, move, resize, rotate, mirror, align, distribute, arrays, node edits, start point, split.",
            schema_of::<Object>(),
        ),
        tool(
            "mat",
            "Mats: add, duplicate, remove, switch, set the material, and guides.",
            schema_of::<Mat>(),
        ),
        tool(
            "operation",
            "What is done to paths: assign a cut, draw, fill, score or emboss with a tool, then order, enable, tune and colour the rows. Also the job ordering and the machine profile.",
            schema_of::<Operation>(),
        ),
        tool(
            "edit",
            "Undo, redo, and whether either is available.",
            schema_of::<Edit>(),
        ),
        tool(
            "compile",
            "Turn the design into machine programs: start, wait, and read the result with its warnings.",
            schema_of::<Compile>(),
        ),
        tool(
            "export",
            "Get the compiled job out: G-code text, files in a folder, or a preview picture.",
            schema_of::<Export>(),
        ),
        tool(
            "machine",
            "The cutter: list, approve, connect, send a job, pause, resume, cancel, jog. Real hardware needs the user to arm the API first.",
            schema_of::<Machine>(),
        ),
        tool(
            "library",
            "Materials, tools and machine profiles: list, install a package, uninstall.",
            schema_of::<Library>(),
        ),
        tool(
            "query",
            "Read state: document, mats, objects, selection, rows, row parameters, tools, materials, machines, problems, bounds.",
            schema_of::<Query>(),
        ),
        tool(
            "settings",
            "Application preferences: read, change, calibrate the screen.",
            schema_of::<Settings>(),
        ),
    ]
}

/// Parses tool arguments into their typed form.
fn parse<T: DeserializeOwned>(args: Value) -> Result<T, ApiError> {
    serde_json::from_value(args).map_err(|e| {
        ApiError::new("bad_arguments", e.to_string()).hint(
            "the tool's schema is in the tool list; every call needs `op` (or `what` for query)",
        )
    })
}

fn object_id(text: &str) -> Result<ObjectId, ApiError> {
    ObjectId::from_str(text).map_err(|e| ApiError::new("bad_arguments", format!("{text:?}: {e}")))
}

fn mat_id(text: &str) -> Result<MatId, ApiError> {
    MatId::from_str(text).map_err(|e| ApiError::new("bad_arguments", format!("{text:?}: {e}")))
}

fn row_id(text: &str) -> Result<OpRowId, ApiError> {
    OpRowId::from_str(text).map_err(|e| ApiError::new("bad_arguments", format!("{text:?}: {e}")))
}

fn created(ids: &[String]) -> Value {
    json!({ "created": ids })
}

impl App {
    /// Runs one tool call.
    ///
    /// `now_ms` is the host's clock in Unix milliseconds; it stamps undo steps and the edit
    /// delay that triggers compiling.
    ///
    /// # Errors
    ///
    /// An [`ApiError`] when the tool or its arguments are wrong, an id does not exist, or the
    /// operation is refused. The project is unchanged after an error.
    pub fn call_tool(&mut self, tool: &str, args: Value, now_ms: u64) -> Reply {
        match tool {
            "project" => self.project_tool(parse(args)?, now_ms),
            "import" => self.import_tool(parse(args)?, now_ms),
            "object" => self.object_tool(parse(args)?, now_ms),
            "mat" => self.mat_tool(parse(args)?, now_ms),
            "operation" => self.operation_tool(parse(args)?, now_ms),
            "edit" => self.edit_tool(parse(args)?, now_ms),
            "compile" => self.compile_tool(parse(args)?),
            "export" => self.export_tool(parse(args)?, now_ms),
            "machine" => self.machine_tool(parse(args)?),
            "library" => self.library_tool(parse(args)?),
            "query" => self.query_tool(parse(args)?),
            "settings" => self.settings_tool(parse(args)?),
            other => {
                let names: Vec<_> = tool_catalog().into_iter().map(|t| t.name).collect();
                Err(
                    ApiError::new("unknown_tool", format!("no tool named {other:?}"))
                        .hint(format!("the tools are: {}", names.join(", "))),
                )
            }
        }
    }

    /// Lets tool calls move real hardware (or stops them). Only a person calls this, from the
    /// window or a command-line flag; no tool reaches it.
    pub fn arm_api(&mut self, armed: bool) {
        self.api_armed = armed;
    }

    /// Whether tool calls may move real hardware.
    #[must_use]
    pub fn api_armed(&self) -> bool {
        self.api_armed
    }

    /// How the automation server is doing.
    #[must_use]
    pub fn mcp_status(&self) -> &crate::mcp::McpStatus {
        &self.mcp_status
    }

    /// Records how the automation server is doing, for the settings window to show.
    pub fn set_mcp_status(&mut self, status: crate::mcp::McpStatus) {
        self.mcp_status = status;
    }
}

#[cfg(test)]
mod tests;
