//! A Model Context Protocol server over the automation API.
//!
//! [`handle_message`] takes one JSON-RPC message (one line of the stdio transport) and returns
//! the reply line, if any. It is synchronous and owns no I/O, so the window can run it on its own
//! thread of control between frames and tests can drive it directly. [`spawn_stdio`] is the
//! transport: it reads lines from stdin on a background thread and hands them to the host
//! through a channel, because the app state may only be touched from the window's thread.
//!
//! Hand-written rather than built on `rmcp`: the server side of the protocol needed here is
//! three methods (`initialize`, `tools/list`, `tools/call`) over newline-delimited JSON, and
//! the SDK's API is large and still moving.

use std::io::{BufRead, Write};
use std::sync::mpsc::{Receiver, Sender, channel};

use serde_json::{Map, Value, json};

use crate::api::tool_catalog;
use crate::ui::App;

/// The protocol revision this server speaks when the client asks for one we do not know.
const LATEST_PROTOCOL: &str = "2025-06-18";
/// Revisions the server accepts as asked; the tool surface is the same in all of them.
const KNOWN_PROTOCOLS: [&str; 3] = ["2024-11-05", "2025-03-26", LATEST_PROTOCOL];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

fn error_reply(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn result_reply(id: &Value, result: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// A tool's argument schema in the shape MCP clients require: a JSON object at the top level.
fn input_schema(mut schema: Value) -> Value {
    if let Some(object) = schema.as_object_mut() {
        object.remove("$schema");
        object
            .entry("type")
            .or_insert_with(|| Value::String("object".to_owned()));
    }
    schema
}

fn tools_list() -> Value {
    let tools: Vec<Value> = tool_catalog()
        .into_iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": input_schema(t.schema),
            })
        })
        .collect();
    json!({ "tools": tools })
}

/// A tool result: the reply as pretty JSON text, flagged when it is an error.
fn tool_content(value: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

fn call_tool(app: &mut App, params: &Value, now_ms: u64) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("tools/call needs a string `name`")?;
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => Value::Object(Map::new()),
        Some(args) => args.clone(),
    };
    Ok(match app.call_tool(name, arguments, now_ms) {
        Ok(reply) => tool_content(&reply, false),
        Err(e) => tool_content(&serde_json::to_value(&e).unwrap_or_default(), true),
    })
}

fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = asked
        .filter(|v| KNOWN_PROTOCOLS.contains(v))
        .unwrap_or(LATEST_PROTOCOL);
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "plotter-studio", "version": env!("CARGO_PKG_VERSION")},
        "instructions": "Plotter Studio, driven through twelve tools. Start with query \
            {\"what\": \"document\"}. Objects, mats and rows are named by the ids earlier replies \
            returned. Errors carry a `code` and a `hint`; real hardware only moves when the \
            user has armed the API.",
    })
}

/// Handles one JSON-RPC message and returns the reply line, or `None` for a notification.
///
/// Tool failures are reported inside a successful reply with `isError: true` (the protocol's way
/// of letting the model read and recover); only malformed messages and unknown methods become
/// JSON-RPC errors.
#[must_use]
pub fn handle_message(app: &mut App, message: &str, now_ms: u64) -> Option<String> {
    let reply = respond(app, message, now_ms)?;
    Some(reply.to_string())
}

fn respond(app: &mut App, message: &str, now_ms: u64) -> Option<Value> {
    let Ok(request) = serde_json::from_str::<Value>(message) else {
        return Some(error_reply(&Value::Null, PARSE_ERROR, "invalid JSON"));
    };
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        // A response to something we never asked, or garbage.
        return request
            .get("id")
            .map(|id| error_reply(id, INVALID_REQUEST, "expected a request with a `method`"));
    };
    // No `id` means a notification: it never gets a reply.
    let id = request.get("id")?;
    let params = request.get("params").unwrap_or(&Value::Null);
    Some(match method {
        "initialize" => result_reply(id, &initialize(params)),
        "ping" => result_reply(id, &json!({})),
        "tools/list" => result_reply(id, &tools_list()),
        "tools/call" => match call_tool(app, params, now_ms) {
            Ok(result) => result_reply(id, &result),
            Err(message) => error_reply(id, INVALID_PARAMS, &message),
        },
        other => error_reply(id, METHOD_NOT_FOUND, &format!("unknown method {other:?}")),
    })
}

/// One line from the client and where to send the reply line.
#[derive(Debug)]
pub struct Request {
    /// The raw JSON-RPC message.
    pub line: String,
    reply: Sender<String>,
}

impl Request {
    /// Runs the request against the app and sends the reply, if there is one.
    pub fn serve(self, app: &mut App, now_ms: u64) {
        if let Some(reply) = handle_message(app, &self.line, now_ms) {
            // The writer thread is gone only when stdout closed; nothing to do about it.
            let _ = self.reply.send(reply);
        }
    }
}

/// Starts the stdio transport: requests arrive on the returned receiver, in order.
///
/// A reader thread forwards stdin lines; a writer thread prints replies to stdout. Both end
/// when stdin closes. Nothing else in the process may write to stdout while this runs.
#[must_use]
pub fn spawn_stdio() -> Receiver<Request> {
    let (request_tx, request_rx) = channel::<Request>();
    let (reply_tx, reply_rx) = channel::<String>();
    std::thread::spawn(move || {
        let mut out = std::io::stdout().lock();
        for reply in reply_rx {
            if writeln!(out, "{reply}").and_then(|()| out.flush()).is_err() {
                break;
            }
        }
    });
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            let request = Request {
                line,
                reply: reply_tx.clone(),
            };
            if request_tx.send(request).is_err() {
                break;
            }
        }
    });
    request_rx
}

#[cfg(test)]
mod tests;
