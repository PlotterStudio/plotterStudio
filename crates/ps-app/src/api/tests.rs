//! Ground truth for the automation API. Expected numbers are worked out by hand from the request
//! (a 40 x 20 rectangle at (10, 5) spans exactly x 10..50, y 5..25), never read back from the
//! code under test.

use ps_library::Settings;
use serde_json::{Value, json};

use super::*;
use crate::commands::Registry;

const BLADE: &str = "generic.drag-blade@1.0.0";

fn app() -> App {
    App::new(Settings::default(), "en")
}

fn call(app: &mut App, tool: &str, args: Value) -> Value {
    app.call_tool(tool, args, 0)
        .unwrap_or_else(|e| panic!("{tool} failed: {e}"))
}

fn fail(app: &mut App, tool: &str, args: Value) -> ApiError {
    app.call_tool(tool, args, 0)
        .expect_err("the call should have been refused")
}

fn rect(app: &mut App, x: f64, y: f64, w: f64, h: f64) -> String {
    let reply = call(
        app,
        "import",
        json!({"op": "shape", "shape": "rectangle", "x": x, "y": y, "width": w, "height": h}),
    );
    reply["created"][0]
        .as_str()
        .expect("an object id")
        .to_owned()
}

/// The bounds `[x0, y0, x1, y1]` of one object, from `query`.
fn bounds(app: &mut App, id: &str) -> [f64; 4] {
    let reply = call(app, "query", json!({"what": "bounds", "objects": [id]}));
    let b = &reply["bounds"];
    ["x0", "y0", "x1", "y1"].map(|k| b[k].as_f64().expect("a number"))
}

fn near(got: [f64; 4], want: [f64; 4]) {
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < 1e-6, "got {got:?}, want {want:?}");
    }
}

// ---- the surface ----

#[test]
fn there_are_exactly_twelve_tools() {
    let names: Vec<_> = tool_catalog().into_iter().map(|t| t.name).collect();
    assert_eq!(
        names,
        [
            "project",
            "import",
            "object",
            "mat",
            "operation",
            "edit",
            "compile",
            "export",
            "machine",
            "library",
            "query",
            "settings"
        ]
    );
}

#[test]
fn every_tool_publishes_a_schema_that_names_its_operations() {
    for tool in tool_catalog() {
        assert!(
            !tool.description.is_empty(),
            "{} has no description",
            tool.name
        );
        let text = tool.schema.to_string();
        assert!(
            text.contains("\"op\"") || text.contains("\"what\""),
            "{}",
            tool.name
        );
    }
}

#[test]
fn every_registry_command_is_covered_by_an_operation_or_marked_ui_only() {
    let registry = Registry::new(&std::collections::BTreeMap::new());
    for command in registry.all() {
        assert!(
            coverage(command.id).is_some(),
            "command {} has no API coverage: map it to an operation or mark it UI-only",
            command.id
        );
    }
}

#[test]
fn coverage_names_only_commands_that_exist() {
    let registry = Registry::new(&std::collections::BTreeMap::new());
    for id in covered_ids() {
        assert!(
            registry.get(id).is_some(),
            "coverage lists unknown command {id}"
        );
    }
}

#[test]
fn every_operation_coverage_names_is_in_its_tools_schema() {
    let catalog = tool_catalog();
    for id in covered_ids() {
        let Some(Coverage::Op { tool, op }) = coverage(id) else {
            continue;
        };
        let schema = catalog
            .iter()
            .find(|t| t.name == tool)
            .unwrap_or_else(|| panic!("{id}: no tool {tool}"))
            .schema
            .to_string();
        assert!(
            schema.contains(&format!("\"{op}\"")),
            "{id}: {tool} has no op {op}"
        );
    }
}

// ---- errors an AI can recover from ----

#[test]
fn an_unknown_tool_is_refused_and_lists_the_tools() {
    let e = fail(&mut app(), "paint", json!({}));
    assert_eq!(e.code, "unknown_tool");
    assert!(e.hint.as_deref().unwrap_or_default().contains("object"));
}

#[test]
fn malformed_arguments_say_what_was_wrong() {
    let e = fail(&mut app(), "object", json!({"op": "rotate", "objects": []}));
    assert_eq!(e.code, "bad_arguments");
    assert!(e.message.contains("degrees"), "{}", e.message);
}

#[test]
fn an_unknown_op_is_a_bad_argument() {
    let e = fail(&mut app(), "edit", json!({"op": "explode"}));
    assert_eq!(e.code, "bad_arguments");
}

#[test]
fn a_missing_object_is_reported_with_how_to_find_ids() {
    let mut a = app();
    let e = fail(
        &mut a,
        "object",
        json!({"op": "delete", "objects": ["00000000-0000-7000-8000-000000000000"]}),
    );
    assert_eq!(e.code, "unknown_object");
    assert!(e.hint.as_deref().unwrap_or_default().contains("query"));
}

#[test]
fn a_malformed_id_is_a_bad_argument() {
    let e = fail(
        &mut app(),
        "object",
        json!({"op": "delete", "objects": ["nope"]}),
    );
    assert_eq!(e.code, "bad_arguments");
}

// ---- drawing and arranging ----

#[test]
fn a_rectangle_lands_exactly_where_asked() {
    let mut a = app();
    let id = rect(&mut a, 10.0, 5.0, 40.0, 20.0);
    near(bounds(&mut a, &id), [10.0, 5.0, 50.0, 25.0]);
}

#[test]
fn an_ellipse_fills_its_box() {
    let mut a = app();
    let reply = call(
        &mut a,
        "import",
        json!({"op": "shape", "shape": "ellipse", "x": 0.0, "y": 0.0, "width": 30.0, "height": 10.0}),
    );
    let id = reply["created"][0].as_str().unwrap().to_owned();
    near(bounds(&mut a, &id), [0.0, 0.0, 30.0, 10.0]);
}

#[test]
fn a_closed_path_has_the_bounds_of_its_points() {
    let mut a = app();
    let reply = call(
        &mut a,
        "import",
        json!({"op": "shape", "shape": "path", "closed": true,
               "points": [{"x": 5.0, "y": 5.0}, {"x": 25.0, "y": 5.0}, {"x": 15.0, "y": 30.0}]}),
    );
    let id = reply["created"][0].as_str().unwrap().to_owned();
    near(bounds(&mut a, &id), [5.0, 5.0, 25.0, 30.0]);
}

#[test]
fn a_path_with_one_point_is_refused() {
    let e = fail(
        &mut app(),
        "import",
        json!({"op": "shape", "shape": "path", "points": [{"x": 1.0, "y": 1.0}]}),
    );
    assert_eq!(e.code, "invalid_shape");
}

#[test]
fn move_shifts_by_a_distance() {
    let mut a = app();
    let id = rect(&mut a, 10.0, 5.0, 40.0, 20.0);
    call(
        &mut a,
        "object",
        json!({"op": "move", "objects": [id], "dx": 7.0, "dy": -2.0}),
    );
    near(bounds(&mut a, &id), [17.0, 3.0, 57.0, 23.0]);
}

#[test]
fn move_to_puts_the_anchor_on_the_point() {
    let mut a = app();
    let id = rect(&mut a, 10.0, 5.0, 40.0, 20.0);
    call(
        &mut a,
        "object",
        json!({"op": "move_to", "objects": [id], "anchor": "centre", "x": 100.0, "y": 100.0}),
    );
    near(bounds(&mut a, &id), [80.0, 90.0, 120.0, 110.0]);
}

#[test]
fn resize_keeps_the_anchor_fixed() {
    let mut a = app();
    let id = rect(&mut a, 10.0, 5.0, 40.0, 20.0);
    call(
        &mut a,
        "object",
        json!({"op": "resize", "objects": [id], "anchor": "top_left", "width": 80.0, "height": 10.0}),
    );
    near(bounds(&mut a, &id), [10.0, 5.0, 90.0, 15.0]);
}

#[test]
fn rotating_a_rectangle_a_quarter_turn_swaps_its_extent_about_its_centre() {
    let mut a = app();
    let id = rect(&mut a, 0.0, 0.0, 40.0, 20.0);
    call(
        &mut a,
        "object",
        json!({"op": "rotate", "objects": [id], "degrees": 90.0}),
    );
    // The centre (20, 10) stays; the box becomes 20 wide and 40 tall.
    near(bounds(&mut a, &id), [10.0, -10.0, 30.0, 30.0]);
}

#[test]
fn mirror_horizontal_keeps_a_symmetric_box_in_place() {
    let mut a = app();
    let id = rect(&mut a, 10.0, 5.0, 40.0, 20.0);
    call(
        &mut a,
        "object",
        json!({"op": "mirror", "objects": [id], "axis": "horizontal"}),
    );
    near(bounds(&mut a, &id), [10.0, 5.0, 50.0, 25.0]);
}

#[test]
fn align_left_moves_every_object_to_the_leftmost_edge() {
    let mut a = app();
    let first = rect(&mut a, 10.0, 0.0, 10.0, 10.0);
    let second = rect(&mut a, 60.0, 30.0, 10.0, 10.0);
    call(
        &mut a,
        "object",
        json!({"op": "align", "objects": [first, second], "to": "left"}),
    );
    near(bounds(&mut a, &second), [10.0, 30.0, 20.0, 40.0]);
}

#[test]
fn a_grid_array_adds_the_other_copies_and_names_them() {
    let mut a = app();
    let id = rect(&mut a, 0.0, 0.0, 10.0, 10.0);
    let reply = call(
        &mut a,
        "object",
        json!({"op": "grid_array", "objects": [id], "columns": 3, "rows": 2, "dx": 20.0, "dy": 30.0}),
    );
    assert_eq!(reply["created"].as_array().unwrap().len(), 5);
    let mats = call(&mut a, "query", json!({"what": "objects"}));
    assert_eq!(mats["objects"].as_array().unwrap().len(), 6);
}

#[test]
fn duplicate_returns_the_copy() {
    let mut a = app();
    let id = rect(&mut a, 0.0, 0.0, 10.0, 10.0);
    let reply = call(
        &mut a,
        "object",
        json!({"op": "duplicate", "objects": [id]}),
    );
    let copy = reply["created"][0].as_str().unwrap();
    assert_ne!(copy, id);
}

#[test]
fn delete_removes_the_object_and_undo_brings_it_back() {
    let mut a = app();
    let id = rect(&mut a, 0.0, 0.0, 10.0, 10.0);
    call(&mut a, "object", json!({"op": "delete", "objects": [id]}));
    let after = call(&mut a, "query", json!({"what": "objects"}));
    assert_eq!(after["objects"].as_array().unwrap().len(), 0);
    call(&mut a, "edit", json!({"op": "undo"}));
    let back = call(&mut a, "query", json!({"what": "objects"}));
    assert_eq!(back["objects"].as_array().unwrap().len(), 1);
    let history = call(&mut a, "edit", json!({"op": "history"}));
    assert_eq!(history["can_redo"], true);
}

#[test]
fn operations_never_depend_on_the_window_selection() {
    let mut a = app();
    let first = rect(&mut a, 0.0, 0.0, 10.0, 10.0);
    let second = rect(&mut a, 50.0, 0.0, 10.0, 10.0);
    call(
        &mut a,
        "object",
        json!({"op": "select", "objects": [second]}),
    );
    call(
        &mut a,
        "object",
        json!({"op": "move", "objects": [first], "dx": 5.0, "dy": 0.0}),
    );
    near(bounds(&mut a, &first), [5.0, 0.0, 15.0, 10.0]);
    near(bounds(&mut a, &second), [50.0, 0.0, 60.0, 10.0]);
}

// ---- mats ----

#[test]
fn adding_a_mat_returns_its_id_and_makes_it_current() {
    let mut a = app();
    let reply = call(&mut a, "mat", json!({"op": "add"}));
    let id = reply["created"][0].as_str().unwrap().to_owned();
    let mats = call(&mut a, "query", json!({"what": "mats"}));
    let list = mats["mats"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    let current: Vec<_> = list.iter().filter(|m| m["current"] == true).collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0]["id"], id);
}

#[test]
fn a_mat_can_be_addressed_without_being_current() {
    let mut a = app();
    let first = call(&mut a, "query", json!({"what": "mats"}))["mats"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    call(&mut a, "mat", json!({"op": "add"}));
    call(&mut a, "mat", json!({"op": "remove", "mat": first}));
    let mats = call(&mut a, "query", json!({"what": "mats"}));
    assert_eq!(mats["mats"].as_array().unwrap().len(), 1);
}

#[test]
fn guides_are_added_and_removed_by_position() {
    let mut a = app();
    call(
        &mut a,
        "mat",
        json!({"op": "add_guide", "axis": "vertical", "at_mm": 25.0}),
    );
    assert_eq!(a.state().current_mat().guides.len(), 1);
    call(
        &mut a,
        "mat",
        json!({"op": "remove_guide", "axis": "vertical", "at_mm": 25.4, "tolerance_mm": 1.0}),
    );
    assert_eq!(a.state().current_mat().guides.len(), 0);
}

// ---- operations ----

#[test]
fn assigning_an_operation_creates_a_row_that_query_lists() {
    let mut a = app();
    let id = rect(&mut a, 0.0, 0.0, 10.0, 10.0);
    let reply = call(
        &mut a,
        "operation",
        json!({"op": "assign", "objects": [id], "kind": "cut", "tool": BLADE}),
    );
    let row = reply["row"].as_str().expect("the row id").to_owned();
    let rows = call(&mut a, "query", json!({"what": "rows"}));
    assert_eq!(rows["rows"].as_array().unwrap().len(), 1);
    assert_eq!(rows["rows"][0]["id"], row);
    assert_eq!(rows["rows"][0]["kind"], "cut");
}

#[test]
fn a_tool_that_does_not_exist_is_refused() {
    let mut a = app();
    let id = rect(&mut a, 0.0, 0.0, 10.0, 10.0);
    let e = fail(
        &mut a,
        "operation",
        json!({"op": "assign", "objects": [id], "kind": "cut", "tool": "nothing@1.0.0"}),
    );
    assert_eq!(e.code, "refused");
    assert!(e.hint.as_deref().unwrap_or_default().contains("tools"));
}

#[test]
fn a_row_can_be_disabled_and_removed() {
    let mut a = app();
    let id = rect(&mut a, 0.0, 0.0, 10.0, 10.0);
    let row = call(
        &mut a,
        "operation",
        json!({"op": "assign", "objects": [id], "kind": "cut", "tool": BLADE}),
    )["row"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        &mut a,
        "operation",
        json!({"op": "enable", "row": row, "enabled": false}),
    );
    let rows = call(&mut a, "query", json!({"what": "rows"}));
    assert_eq!(rows["rows"][0]["enabled"], false);
    call(&mut a, "operation", json!({"op": "remove", "row": row}));
    let rows = call(&mut a, "query", json!({"what": "rows"}));
    assert_eq!(rows["rows"].as_array().unwrap().len(), 0);
}

#[test]
fn a_row_parameter_can_be_set_and_reset() {
    let mut a = app();
    let id = rect(&mut a, 0.0, 0.0, 10.0, 10.0);
    let row = call(
        &mut a,
        "operation",
        json!({"op": "assign", "objects": [id], "kind": "cut", "tool": BLADE}),
    )["row"]
        .as_str()
        .unwrap()
        .to_owned();
    let force = |a: &mut App| {
        let params = call(a, "query", json!({"what": "row_params", "row": row}));
        let force = params["params"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "force")
            .expect("the blade has a force parameter")
            .clone();
        (force["value"].as_f64().unwrap(), force["modified"] == true)
    };
    // The built-in blade's default force is 350 gf (its tool definition).
    assert_eq!(force(&mut a), (350.0, false));
    call(
        &mut a,
        "operation",
        json!({"op": "set_param", "row": row, "name": "force", "value": 200.0}),
    );
    assert_eq!(force(&mut a), (200.0, true));
    call(
        &mut a,
        "operation",
        json!({"op": "reset_param", "row": row, "name": "force"}),
    );
    assert_eq!(force(&mut a), (350.0, false));
}

#[test]
fn a_parameter_outside_its_range_is_refused() {
    let mut a = app();
    let id = rect(&mut a, 0.0, 0.0, 10.0, 10.0);
    let row = call(
        &mut a,
        "operation",
        json!({"op": "assign", "objects": [id], "kind": "cut", "tool": BLADE}),
    )["row"]
        .as_str()
        .unwrap()
        .to_owned();
    // The blade's force range starts at 50 gf.
    let e = fail(
        &mut a,
        "operation",
        json!({"op": "set_param", "row": row, "name": "force", "value": 1.0}),
    );
    assert_eq!(e.code, "refused");
}

#[test]
fn the_job_settings_are_changed_together_or_alone() {
    let mut a = app();
    call(
        &mut a,
        "operation",
        json!({"op": "set_job", "ordering": "strict_order"}),
    );
    assert_eq!(a.state().ordering(), ps_model::OrderingMode::StrictOrder);
    assert!(!a.state().pen_check());
    call(
        &mut a,
        "operation",
        json!({"op": "set_job", "pen_check": true}),
    );
    assert!(a.state().pen_check());
    assert_eq!(a.state().ordering(), ps_model::OrderingMode::StrictOrder);
}

// ---- compile and export ----

#[test]
fn a_compiled_design_exports_gcode() {
    let mut a = app();
    let id = rect(&mut a, 20.0, 20.0, 30.0, 30.0);
    call(
        &mut a,
        "operation",
        json!({"op": "assign", "objects": [id], "kind": "cut", "tool": BLADE}),
    );
    call(&mut a, "compile", json!({"op": "start"}));
    let done = call(
        &mut a,
        "compile",
        json!({"op": "wait", "timeout_ms": 30_000}),
    );
    assert_eq!(done["finished"], true);
    let result = call(&mut a, "compile", json!({"op": "result"}));
    assert!(result["mats"].as_u64().unwrap() >= 1);
    let gcode = call(&mut a, "export", json!({"op": "gcode"}));
    assert!(gcode["gcode"].as_str().unwrap().contains("G1"));
}

#[test]
fn exporting_before_anything_is_compiled_explains_what_to_do() {
    let e = fail(&mut app(), "export", json!({"op": "gcode"}));
    assert_eq!(e.code, "not_compiled");
    assert!(e.hint.as_deref().unwrap_or_default().contains("compile"));
}

// ---- projects ----

#[test]
fn a_project_round_trips_through_save_and_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.pstudio").display().to_string();
    let mut a = app();
    rect(&mut a, 1.0, 2.0, 3.0, 4.0);
    call(&mut a, "project", json!({"op": "save", "path": path}));
    let mut b = app();
    call(&mut b, "project", json!({"op": "open", "path": path}));
    let objects = call(&mut b, "query", json!({"what": "objects"}));
    assert_eq!(objects["objects"].as_array().unwrap().len(), 1);
}

#[test]
fn saving_a_project_that_has_no_file_needs_a_path() {
    let e = fail(&mut app(), "project", json!({"op": "save"}));
    assert_eq!(e.code, "needs_path");
}

#[test]
fn a_new_project_is_empty() {
    let mut a = app();
    rect(&mut a, 0.0, 0.0, 5.0, 5.0);
    call(&mut a, "project", json!({"op": "new"}));
    let objects = call(&mut a, "query", json!({"what": "objects"}));
    assert_eq!(objects["objects"].as_array().unwrap().len(), 0);
}

#[test]
fn importing_a_missing_file_says_so() {
    let e = fail(
        &mut app(),
        "import",
        json!({"op": "file", "path": "no/such/file.svg"}),
    );
    assert_eq!(e.code, "import_failed");
}

// ---- settings and library ----

#[test]
fn settings_are_merged_not_replaced() {
    let mut a = app();
    let before = call(&mut a, "settings", json!({"op": "get"}));
    call(
        &mut a,
        "settings",
        json!({"op": "set", "values": {"first_run_seen": true}}),
    );
    let after = call(&mut a, "settings", json!({"op": "get"}));
    assert_eq!(after["first_run_seen"], true);
    assert_eq!(after["locale"], before["locale"]);
}

#[test]
fn settings_with_an_unknown_field_are_refused() {
    let e = fail(
        &mut app(),
        "settings",
        json!({"op": "set", "values": {"no_such_setting": 1}}),
    );
    assert_eq!(e.code, "bad_arguments");
}

#[test]
fn the_library_lists_builtin_tools() {
    let mut a = app();
    let reply = call(&mut a, "library", json!({"op": "list", "kind": "tool"}));
    assert_ne!(reply["entries"].as_array().unwrap().len(), 0);
}

#[test]
fn query_tools_lists_the_blade() {
    let mut a = app();
    let reply = call(&mut a, "query", json!({"what": "tools", "kind": "cut"}));
    let refs: Vec<_> = reply["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["reference"].as_str())
        .collect();
    assert!(refs.contains(&BLADE), "{refs:?}");
}

// ---- the machine ----

#[test]
fn a_job_is_never_sent_to_real_hardware_unless_a_person_armed_the_api() {
    let mut a = app();
    let e = fail(&mut a, "machine", json!({"op": "send"}));
    assert_eq!(e.code, "not_armed");
    assert!(
        e.hint
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains("user")
    );
}

#[test]
fn arming_is_not_reachable_through_any_tool() {
    let mut a = app();
    for args in [
        json!({"op": "arm"}),
        json!({"op": "send", "armed": true}),
        json!({"op": "send", "arm": true}),
    ] {
        let _ = a.call_tool("machine", args, 0);
        assert!(!a.api_armed(), "a tool call armed the API");
    }
}

#[test]
fn the_simulator_needs_no_arming_once_connected() {
    let mut a = App::with_simulator(Settings::default(), "en");
    let devices = call(&mut a, "machine", json!({"op": "list"}));
    let device = devices["devices"][0]["id"].as_str().unwrap().to_owned();
    call(
        &mut a,
        "machine",
        json!({"op": "approve", "device": device}),
    );
    call(
        &mut a,
        "machine",
        json!({"op": "connect", "device": device}),
    );
    // Nothing is compiled, so the refusal is about the job, not about arming.
    let e = fail(&mut a, "machine", json!({"op": "send"}));
    assert_eq!(e.code, "not_compiled");
}

#[test]
fn a_compiled_job_runs_on_the_simulator() {
    let mut a = App::with_simulator(Settings::default(), "en");
    let id = rect(&mut a, 20.0, 20.0, 30.0, 30.0);
    call(
        &mut a,
        "operation",
        json!({"op": "assign", "objects": [id], "kind": "cut", "tool": BLADE}),
    );
    call(&mut a, "compile", json!({"op": "wait"}));
    let devices = call(&mut a, "machine", json!({"op": "list"}));
    let device = devices["devices"][0]["id"].as_str().unwrap().to_owned();
    call(
        &mut a,
        "machine",
        json!({"op": "approve", "device": device}),
    );
    call(
        &mut a,
        "machine",
        json!({"op": "connect", "device": device}),
    );
    let sent = call(&mut a, "machine", json!({"op": "send"}));
    assert_eq!(sent["started"], true);
}

#[test]
fn jogging_is_refused_unless_a_person_armed_the_api() {
    let mut a = app();
    assert_eq!(
        fail(
            &mut a,
            "machine",
            json!({"op": "jog", "x_mm": 1.0, "y_mm": 0.0})
        )
        .code,
        "not_armed"
    );
}

#[test]
fn machine_list_shows_the_simulator() {
    let mut a = App::with_simulator(Settings::default(), "en");
    let reply = call(&mut a, "machine", json!({"op": "list"}));
    assert_ne!(reply["devices"].as_array().unwrap().len(), 0);
}
// ---- the model's own commands ----

#[test]
fn every_document_command_of_the_model_has_an_operation() {
    let source = include_str!("../../../ps-model/src/command.rs");
    let found: Vec<&str> = source
        .lines()
        .filter_map(|l| l.trim_end().strip_prefix("impl Command for "))
        .map(|l| l.trim_end_matches(" {"))
        .collect();
    assert!(found.len() >= 20, "the scan found only {found:?}");
    for name in &found {
        assert!(
            model_coverage(name).is_some(),
            "ps-model command {name} has no API operation: map it in coverage.rs"
        );
    }
    for name in model_covered() {
        assert!(
            found.contains(&name),
            "coverage lists unknown model command {name}"
        );
    }
}

#[test]
fn every_operation_the_model_table_names_is_in_its_tools_schema() {
    let catalog = tool_catalog();
    for name in model_covered() {
        let Some(Coverage::Op { tool, op }) = model_coverage(name) else {
            continue;
        };
        let schema = catalog
            .iter()
            .find(|t| t.name == tool)
            .unwrap()
            .schema
            .to_string();
        assert!(
            schema.contains(&format!("\"{op}\"")),
            "{name}: {tool} has no op {op}"
        );
    }
}

// ---- the paths the first tests did not reach ----

fn path(app: &mut App, points: &[(f64, f64)], closed: bool) -> String {
    let points: Vec<Value> = points
        .iter()
        .map(|(x, y)| json!({"x": x, "y": y}))
        .collect();
    let reply = call(
        app,
        "import",
        json!({"op": "shape", "shape": "path", "points": points, "closed": closed}),
    );
    reply["created"][0].as_str().unwrap().to_owned()
}

#[test]
fn a_drawing_file_imports_and_reports_the_new_objects() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("square.svg");
    std::fs::write(
        &file,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><rect x="10" y="10" width="20" height="20" fill="none" stroke="black"/></svg>"#,
    )
    .unwrap();
    let mut a = app();
    let reply = call(
        &mut a,
        "import",
        json!({"op": "file", "path": file.display().to_string()}),
    );
    let ids = reply["created"].as_array().unwrap();
    assert_ne!(ids.len(), 0);
    let id = ids[0].as_str().unwrap().to_owned();
    let b = bounds(&mut a, &id);
    assert!(
        (b[2] - b[0] - 20.0).abs() < 1e-6 && (b[3] - b[1] - 20.0).abs() < 1e-6,
        "{b:?}"
    );
}

#[test]
fn deleting_the_middle_node_of_a_polyline_straightens_it() {
    let mut a = app();
    let id = path(&mut a, &[(0.0, 0.0), (10.0, 10.0), (20.0, 0.0)], false);
    call(
        &mut a,
        "object",
        json!({"op": "node", "object": id, "node": 1, "action": "delete"}),
    );
    near(bounds(&mut a, &id), [0.0, 0.0, 20.0, 0.0]);
}

#[test]
fn a_node_that_does_not_exist_is_refused() {
    let mut a = app();
    let id = path(&mut a, &[(0.0, 0.0), (10.0, 10.0)], false);
    let e = fail(
        &mut a,
        "object",
        json!({"op": "node", "object": id, "path": 5, "node": 0, "action": "delete"}),
    );
    assert_eq!(e.code, "unknown_node");
}

#[test]
fn a_cutting_line_splits_a_rectangle_into_two_paths() {
    let mut a = app();
    let id = rect(&mut a, 0.0, 0.0, 20.0, 10.0);
    call(
        &mut a,
        "object",
        json!({"op": "split", "objects": [id],
               "from": {"x": 10.0, "y": -5.0}, "to": {"x": 10.0, "y": 15.0}}),
    );
    let objects = call(&mut a, "query", json!({"what": "objects"}));
    let paths: usize = objects["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["paths"].as_array().unwrap().len())
        .sum();
    assert_eq!(paths, 2);
}

#[test]
fn a_line_that_misses_splits_nothing() {
    let mut a = app();
    let id = rect(&mut a, 0.0, 0.0, 20.0, 10.0);
    let e = fail(
        &mut a,
        "object",
        json!({"op": "split", "objects": [id],
               "from": {"x": 50.0, "y": 0.0}, "to": {"x": 50.0, "y": 10.0}}),
    );
    assert_eq!(e.code, "refused");
}

#[test]
fn a_start_point_can_be_chosen_on_a_closed_path() {
    let mut a = app();
    let id = rect(&mut a, 0.0, 0.0, 20.0, 10.0);
    call(
        &mut a,
        "object",
        json!({"op": "set_start_point", "object": id, "near": {"x": 20.0, "y": 10.0}}),
    );
}

#[test]
fn a_circular_array_makes_count_minus_one_copies() {
    let mut a = app();
    let id = rect(&mut a, 40.0, 0.0, 10.0, 10.0);
    let reply = call(
        &mut a,
        "object",
        json!({"op": "circular_array", "objects": [id], "count": 4, "total_degrees": 360.0}),
    );
    assert_eq!(reply["created"].as_array().unwrap().len(), 3);
}

#[test]
fn distribute_spaces_three_objects_evenly() {
    let mut a = app();
    let first = rect(&mut a, 0.0, 0.0, 10.0, 10.0);
    let middle = rect(&mut a, 15.0, 0.0, 10.0, 10.0);
    let last = rect(&mut a, 100.0, 0.0, 10.0, 10.0);
    call(
        &mut a,
        "object",
        json!({"op": "distribute", "objects": [first, middle, last], "axis": "horizontal"}),
    );
    // Ten wide boxes from 0 to 110 leave 90 spare; two equal gaps of 45 put the middle at 55 - 5.
    near(bounds(&mut a, &middle), [50.0, 0.0, 60.0, 10.0]);
}

#[test]
fn a_fill_row_takes_its_pattern_from_set_fill() {
    let mut a = app();
    let id = rect(&mut a, 0.0, 0.0, 20.0, 20.0);
    let row = call(
        &mut a,
        "operation",
        json!({"op": "assign", "objects": [id], "kind": "fill",
               "tool": "pen.fineliner-black-0.4@1.0.0"}),
    )["row"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        &mut a,
        "operation",
        json!({"op": "set_fill", "row": row,
               "fill": {"pattern": "concentric", "angle_deg": 30.0, "overlap": 0.25}}),
    );
    let fill = a
        .state()
        .row_fill(row.parse().unwrap())
        .expect("a fill row has a fill");
    assert_eq!(fill.pattern, ps_geometry::Pattern::Concentric);
    assert!((fill.angle_deg - 30.0).abs() < 1e-9 && (fill.overlap - 0.25).abs() < 1e-9);
}

#[test]
fn the_mirror_flag_and_the_rules_round_trip() {
    let mut a = app();
    call(&mut a, "mat", json!({"op": "set_mirror", "mirror": true}));
    assert!(a.state().current_mat().mirror);
    let rules = call(&mut a, "query", json!({"what": "rules"}));
    call(
        &mut a,
        "operation",
        json!({"op": "set_rules", "rules": rules["rules"]}),
    );
}

#[test]
fn rules_that_do_not_parse_are_a_bad_argument() {
    let e = fail(
        &mut app(),
        "operation",
        json!({"op": "set_rules", "rules": [{"nope": 1}]}),
    );
    assert_eq!(e.code, "bad_arguments");
}

#[test]
fn text_lands_with_its_top_left_corner_where_asked() {
    let mut a = app();
    let reply = call(
        &mut a,
        "import",
        json!({"op": "text", "text": "Hi", "size_mm": 10.0, "at": {"x": 30.0, "y": 40.0}}),
    );
    let id = reply["created"][0].as_str().unwrap().to_owned();
    let b = bounds(&mut a, &id);
    assert!(
        (b[0] - 30.0).abs() < 1e-6 && (b[1] - 40.0).abs() < 1e-6,
        "{b:?}"
    );
    assert!(b[2] > b[0] && b[3] > b[1]);
}

#[test]
fn blank_text_is_refused() {
    let e = fail(
        &mut app(),
        "import",
        json!({"op": "text", "text": "  ", "size_mm": 10.0, "at": {"x": 0.0, "y": 0.0}}),
    );
    assert_eq!(e.code, "invalid_text");
}

#[test]
fn installing_without_a_library_folder_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("p.psmaterials");
    std::fs::write(&file, b"x").unwrap();
    let e = fail(
        &mut app(),
        "library",
        json!({"op": "install", "path": file.display().to_string()}),
    );
    assert_eq!(e.code, "no_library_store");
}

fn compiled(app: &mut App) {
    let id = rect(app, 20.0, 20.0, 30.0, 30.0);
    call(
        app,
        "operation",
        json!({"op": "assign", "objects": [id], "kind": "cut", "tool": BLADE}),
    );
    let done = call(app, "compile", json!({"op": "wait"}));
    assert_eq!(done["finished"], true);
}

#[test]
fn a_compiled_job_exports_files_into_a_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = app();
    compiled(&mut a);
    let reply = call(
        &mut a,
        "export",
        json!({"op": "files", "folder": dir.path().display().to_string()}),
    );
    let files = reply["files"].as_array().unwrap();
    assert_ne!(files.len(), 0);
    for f in files {
        assert!(std::path::Path::new(f.as_str().unwrap()).is_file());
    }
}

#[test]
fn a_compiled_job_renders_a_png_preview() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("p.png").display().to_string();
    let mut a = app();
    compiled(&mut a);
    let reply = call(
        &mut a,
        "export",
        json!({"op": "preview", "path": file, "px_per_mm": 1.0}),
    );
    assert!(reply["width"].as_u64().unwrap() > 0);
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn the_compile_result_lists_time_and_problems() {
    let mut a = app();
    compiled(&mut a);
    let r = call(&mut a, "compile", json!({"op": "result"}));
    assert_eq!(r["current"], true);
    assert!(r["machine_time_ms"].as_u64().unwrap() > 0);
    assert!(r["problems"].is_array());
}

#[test]
fn editing_after_a_compile_makes_the_result_stale() {
    let mut a = app();
    compiled(&mut a);
    rect(&mut a, 0.0, 0.0, 5.0, 5.0);
    let status = call(&mut a, "compile", json!({"op": "status"}));
    assert_eq!(status["current"], false);
}
