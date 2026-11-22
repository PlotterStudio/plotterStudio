//! The MCP wire contract, checked against the protocol's published message shapes and a real
//! `App`: what a client sends, and what it must be able to read back.

use ps_library::Settings;
use serde_json::{Value, json};

use super::*;

fn app() -> App {
    App::new(Settings::default(), "en")
}

fn send(app: &mut App, message: &Value) -> Value {
    let line = handle_message(app, &message.to_string(), 0).expect("a reply");
    serde_json::from_str(&line).expect("the reply is JSON")
}

fn call(app: &mut App, name: &str, arguments: &Value) -> Value {
    send(
        app,
        &json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call",
                "params": {"name": name, "arguments": arguments}}),
    )
}

/// The JSON a tool reply carries in its text content.
fn payload(reply: &Value) -> Value {
    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    serde_json::from_str(text).expect("the text is JSON")
}

#[test]
fn initialize_agrees_a_known_version_and_offers_tools() {
    let reply = send(
        &mut app(),
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "2025-03-26", "capabilities": {},
                           "clientInfo": {"name": "t", "version": "0"}}}),
    );
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["result"]["protocolVersion"], "2025-03-26");
    assert!(reply["result"]["capabilities"]["tools"].is_object());
    assert_eq!(reply["result"]["serverInfo"]["name"], "plotter-studio");
}

#[test]
fn initialize_falls_back_to_the_latest_version_for_an_unknown_one() {
    let reply = send(
        &mut app(),
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {"protocolVersion": "1999-01-01"}}),
    );
    assert_eq!(reply["result"]["protocolVersion"], LATEST_PROTOCOL);
}

#[test]
fn notifications_get_no_reply() {
    let none = handle_message(
        &mut app(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        0,
    );
    assert_eq!(none, None);
}

#[test]
fn tools_list_has_twelve_tools_with_object_schemas() {
    let reply = send(
        &mut app(),
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    );
    let tools = reply["result"]["tools"].as_array().expect("a tool array");
    assert_eq!(tools.len(), 12);
    for tool in tools {
        let name = tool["name"].as_str().expect("a name");
        assert!(
            !tool["description"].as_str().unwrap_or("").is_empty(),
            "{name}"
        );
        // MCP clients reject an input schema that is not a top-level object.
        assert_eq!(tool["inputSchema"]["type"], "object", "{name}");
        assert!(tool["inputSchema"].get("$schema").is_none(), "{name}");
    }
}

#[test]
fn a_tool_call_runs_against_the_app_and_returns_its_json() {
    let mut app = app();
    let reply = call(
        &mut app,
        "import",
        &json!({"op": "shape", "shape": "rectangle", "x": 10, "y": 5, "width": 40, "height": 20}),
    );
    assert_eq!(reply["result"]["isError"], false);
    let id = payload(&reply)["created"][0]
        .as_str()
        .expect("a created id")
        .to_owned();

    // The object is really in the document: a second call reads it back by that id.
    let bounds = call(
        &mut app,
        "query",
        &json!({"what": "bounds", "objects": [id]}),
    );
    let b = &payload(&bounds)["bounds"];
    assert!((b["x1"].as_f64().expect("x1") - 50.0).abs() < 1e-6);
    assert!((b["y1"].as_f64().expect("y1") - 25.0).abs() < 1e-6);
}

#[test]
fn a_refused_tool_call_is_an_error_result_with_a_code_the_model_can_read() {
    let reply = call(&mut app(), "no_such_tool", &json!({}));
    assert_eq!(reply["result"]["isError"], true);
    assert_eq!(payload(&reply)["code"], "unknown_tool");
}

#[test]
fn bad_arguments_are_an_error_result_not_a_protocol_error() {
    let reply = call(&mut app(), "object", &json!({"op": "teleport"}));
    assert!(reply.get("error").is_none());
    assert_eq!(reply["result"]["isError"], true);
    assert_eq!(payload(&reply)["code"], "bad_arguments");
}

#[test]
fn missing_arguments_are_treated_as_an_empty_object() {
    let reply = send(
        &mut app(),
        &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "edit"}}),
    );
    // `edit` needs an `op`, so the tool refuses; the point is the call reached it.
    assert_eq!(payload(&reply)["code"], "bad_arguments");
}

#[test]
fn a_call_without_a_name_is_invalid_params() {
    let reply = send(
        &mut app(),
        &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {}}),
    );
    assert_eq!(reply["error"]["code"], INVALID_PARAMS);
    assert_eq!(reply["id"], 4);
}

#[test]
fn unknown_methods_and_garbage_get_json_rpc_errors() {
    let mut app = app();
    let unknown = send(
        &mut app,
        &json!({"jsonrpc": "2.0", "id": "abc", "method": "resources/list"}),
    );
    assert_eq!(unknown["error"]["code"], METHOD_NOT_FOUND);
    assert_eq!(unknown["id"], "abc");

    let garbage: Value =
        serde_json::from_str(&handle_message(&mut app, "{nope", 0).expect("a reply"))
            .expect("JSON");
    assert_eq!(garbage["error"]["code"], PARSE_ERROR);
    assert_eq!(garbage["id"], Value::Null);
}

#[test]
fn ping_answers_with_an_empty_result() {
    let reply = send(
        &mut app(),
        &json!({"jsonrpc": "2.0", "id": 9, "method": "ping"}),
    );
    assert_eq!(reply["result"], json!({}));
}
