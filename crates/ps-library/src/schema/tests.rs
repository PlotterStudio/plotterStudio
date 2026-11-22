//! Ground truth for schema validation: published JSON Schemas generated from the Rust
//! types, every error reported at once with its JSON pointer, then semantic checks.

use std::collections::BTreeSet;

use serde_json::json;

use super::*;
use crate::testdata::{MATERIAL, PEN, PROFILE, TOOL};

fn value(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn pointers(issues: &[Issue]) -> Vec<&str> {
    issues.iter().map(|i| i.pointer.as_str()).collect()
}

// ---- published schemas ----

#[test]
fn each_kind_publishes_a_draft_2020_12_schema_with_its_id() {
    for kind in LibraryKind::ALL {
        let s = schema_json(kind);
        assert_eq!(s["$schema"], "https://json-schema.org/draft/2020-12/schema");
        assert_eq!(s["$id"], kind.schema_id().as_str());
    }
}

#[test]
fn the_schema_requires_the_fields_the_types_require() {
    let s = schema_json(LibraryKind::Tool);
    let required: Vec<&str> = s["required"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    for field in ["schema_version", "id", "version", "name", "family", "drive"] {
        assert!(required.contains(&field), "{field} must be required");
    }
}

// ---- structural validation ----

#[test]
fn the_sample_files_are_valid() {
    assert_eq!(validate(LibraryKind::Tool, &value(TOOL)).len(), 0);
    assert_eq!(validate(LibraryKind::Tool, &value(PEN)).len(), 0);
    assert_eq!(validate(LibraryKind::Material, &value(MATERIAL)).len(), 0);
    assert_eq!(validate(LibraryKind::Profile, &value(PROFILE)).len(), 0);
}

#[test]
fn a_missing_field_is_reported_at_the_object() {
    let mut tool = value(TOOL);
    tool.as_object_mut().unwrap().remove("id");
    let issues = validate(LibraryKind::Tool, &tool);
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert_eq!(issues[0].pointer, "");
    assert!(issues[0].message.contains("id"), "{}", issues[0].message);
}

#[test]
fn a_wrong_type_is_reported_at_its_pointer() {
    let mut tool = value(TOOL);
    tool["parameters"]["force"]["min"] = json!("fifty");
    let issues = validate(LibraryKind::Tool, &tool);
    assert_eq!(pointers(&issues), ["/parameters/force/min"], "{issues:?}");
}

#[test]
fn every_error_is_reported_at_once() {
    let mut tool = value(TOOL);
    let obj = tool.as_object_mut().unwrap();
    obj.remove("id");
    obj.remove("name");
    obj.insert("family".to_owned(), json!("hammer"));
    tool["parameters"]["speed"]["max"] = json!(true);
    let issues = validate(LibraryKind::Tool, &tool);
    assert!(issues.len() >= 4, "{issues:?}");
    let p = pointers(&issues);
    assert!(p.contains(&"/family"));
    assert!(p.contains(&"/parameters/speed/max"));
}

#[test]
fn a_bad_enum_value_is_reported_at_the_field() {
    let mut profile = value(PROFILE);
    profile["origin"] = json!("middle");
    let issues = validate(LibraryKind::Profile, &profile);
    assert_eq!(pointers(&issues), ["/origin"], "{issues:?}");
}

#[test]
fn issues_come_out_in_a_stable_order() {
    let mut tool = value(TOOL);
    tool["family"] = json!(1);
    tool["drive"] = json!(2);
    tool["name"] = json!(3);
    assert_eq!(
        validate(LibraryKind::Tool, &tool),
        validate(LibraryKind::Tool, &tool)
    );
    let p: Vec<String> = validate(LibraryKind::Tool, &tool)
        .into_iter()
        .map(|i| i.pointer)
        .collect();
    let mut sorted = p.clone();
    sorted.sort();
    assert_eq!(p, sorted);
}

#[test]
fn issues_are_grouped_by_file() {
    let a = Issue::new("/id", "bad");
    let b = Issue::new("/name", "worse");
    let grouped = group_by_file(vec![
        ("wood/material.json".to_owned(), a.clone()),
        ("library.json".to_owned(), b.clone()),
        ("wood/material.json".to_owned(), b.clone()),
    ]);
    assert_eq!(
        grouped.keys().map(String::as_str).collect::<Vec<_>>(),
        ["library.json", "wood/material.json"]
    );
    assert_eq!(grouped["wood/material.json"], vec![a, b.clone()]);
    assert_eq!(grouped["library.json"], vec![b]);
}

// ---- semantic checks: tools ----

fn tool(text: &str) -> Tool {
    serde_json::from_str(text).unwrap()
}

#[test]
fn the_sample_tools_pass_the_semantic_checks() {
    assert_eq!(check_tool(&tool(TOOL)), vec![]);
    assert_eq!(check_tool(&tool(PEN)), vec![]);
}

#[test]
fn a_default_outside_its_range_is_flagged() {
    let mut t = tool(TOOL);
    t.parameters.get_mut("force").unwrap().default = 5_000.0;
    assert_eq!(pointers(&check_tool(&t)), ["/parameters/force/default"]);
}

#[test]
fn an_inverted_range_is_flagged() {
    let mut t = tool(TOOL);
    let p = t.parameters.get_mut("speed").unwrap();
    p.min = 500.0;
    p.max = 5.0;
    assert!(pointers(&check_tool(&t)).contains(&"/parameters/speed/min"));
}

#[test]
fn an_integer_parameter_needs_whole_numbers() {
    let mut t = tool(TOOL);
    t.parameters.get_mut("passes").unwrap().default = 1.5;
    assert!(pointers(&check_tool(&t)).contains(&"/parameters/passes/default"));
}

#[test]
fn units_must_be_known() {
    let mut t = tool(TOOL);
    t.parameters.get_mut("force").unwrap().unit = Some("furlongs".to_owned());
    assert_eq!(pointers(&check_tool(&t)), ["/parameters/force/unit"]);
    for unit in ["gf", "mm/s", "mm", "°C", "ms", "deg", "%", "s"] {
        t.parameters.get_mut("force").unwrap().unit = Some(unit.to_owned());
        assert_eq!(check_tool(&t), vec![], "{unit}");
    }
}

#[test]
fn a_pen_needs_a_tip_width_and_pen_details() {
    let mut t = tool(PEN);
    t.geometry.tip_width_mm = None;
    assert_eq!(pointers(&check_tool(&t)), ["/geometry/tip_width_mm"]);
    let mut t = tool(PEN);
    t.pen = None;
    assert_eq!(pointers(&check_tool(&t)), ["/pen"]);
}

#[test]
fn a_pen_colour_must_be_a_hex_colour() {
    let mut t = tool(PEN);
    t.pen.as_mut().unwrap().colour.hex = "black".to_owned();
    assert_eq!(pointers(&check_tool(&t)), ["/pen/colour/hex"]);
}

#[test]
fn a_drag_blade_needs_its_offset() {
    let mut t = tool(TOOL);
    t.geometry.blade_offset_mm = None;
    assert_eq!(pointers(&check_tool(&t)), ["/geometry/blade_offset_mm"]);
}

#[test]
fn a_tool_needs_operations_and_holders() {
    let mut t = tool(TOOL);
    t.operations.clear();
    t.holders.clear();
    let p = pointers(&check_tool(&t))
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert!(
        p.contains(&"/operations".to_owned()) && p.contains(&"/holders".to_owned()),
        "{p:?}"
    );
}

#[test]
fn ids_must_be_usable_in_references() {
    let mut t = tool(TOOL);
    t.id = "has space".to_owned();
    assert!(pointers(&check_tool(&t)).contains(&"/id"));
}

#[test]
fn a_non_positive_dimension_is_flagged() {
    let mut t = tool(TOOL);
    t.geometry.min_feature_mm = Some(0.0);
    assert!(pointers(&check_tool(&t)).contains(&"/geometry/min_feature_mm"));
}

// ---- semantic checks: materials ----

fn material() -> Material {
    serde_json::from_str(MATERIAL).unwrap()
}

#[test]
fn the_sample_material_passes() {
    assert_eq!(check_material(&material(), &BTreeSet::new()), vec![]);
}

#[test]
fn referenced_textures_must_exist() {
    let mut m = material();
    m.textures = Some(Textures {
        base_color: Some("base.png".to_owned()),
        normal: Some("normal.png".to_owned()),
        roughness: None,
        height: None,
        metallic: None,
        scale_mm: 50.0,
    });
    let have: BTreeSet<String> = ["base.png".to_owned()].into();
    let issues = check_material(&m, &have);
    assert_eq!(pointers(&issues), ["/textures/normal"], "{issues:?}");
    let both: BTreeSet<String> = ["base.png".to_owned(), "normal.png".to_owned()].into();
    assert_eq!(check_material(&m, &both), vec![]);
}

#[test]
fn texture_scale_and_thickness_must_be_positive() {
    let mut m = material();
    m.thickness_mm = Some(-1.0);
    m.textures = Some(Textures {
        base_color: None,
        normal: None,
        roughness: None,
        height: None,
        metallic: None,
        scale_mm: 0.0,
    });
    let p = pointers(&check_material(&m, &BTreeSet::new()))
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert!(
        p.contains(&"/thickness_mm".to_owned()) && p.contains(&"/textures/scale_mm".to_owned()),
        "{p:?}"
    );
}

#[test]
fn a_preset_needs_values_and_sane_forces() {
    let mut m = material();
    m.presets[0].params.clear();
    assert_eq!(
        pointers(&check_material(&m, &BTreeSet::new())),
        ["/presets/0/params"]
    );
    let mut m = material();
    m.presets[0].params.insert("force".to_owned(), -3.0);
    assert_eq!(
        pointers(&check_material(&m, &BTreeSet::new())),
        ["/presets/0/params/force"]
    );
}

// ---- semantic checks: profiles ----

fn profile() -> MachineProfile {
    serde_json::from_str(PROFILE).unwrap()
}

#[test]
fn the_sample_profile_passes() {
    assert_eq!(check_profile(&profile()), vec![]);
}

#[test]
fn a_profile_needs_a_positive_work_area_and_holders() {
    let mut p = profile();
    p.work_area.width_mm = 0.0;
    p.holders.clear();
    let found = pointers(&check_profile(&p))
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert!(
        found.contains(&"/work_area/width_mm".to_owned()) && found.contains(&"/holders".to_owned()),
        "{found:?}"
    );
}

#[test]
fn the_force_range_must_be_ordered_and_scaled() {
    let mut p = profile();
    p.force.min_gf = 500.0;
    assert!(pointers(&check_profile(&p)).contains(&"/force/min_gf"));
    let mut p = profile();
    p.force.units_per_gf = 0.0;
    assert!(pointers(&check_profile(&p)).contains(&"/force/units_per_gf"));
}

#[test]
fn chords_need_a_tolerance_and_the_dialect_a_known_name() {
    let mut p = profile();
    p.gcode.chord_tolerance_mm = 0.0;
    assert!(pointers(&check_profile(&p)).contains(&"/gcode/chord_tolerance_mm"));
    let mut p = profile();
    p.gcode.dialect = "marlin".to_owned();
    assert!(pointers(&check_profile(&p)).contains(&"/gcode/dialect"));
}

#[test]
fn a_heated_profile_needs_a_heating_rate() {
    let mut p = profile();
    p.heated = true;
    assert!(pointers(&check_profile(&p)).contains(&"/heating_rate_c_s"));
    p.heating_rate_c_s = Some(2.0);
    assert_eq!(check_profile(&p), vec![]);
}

#[test]
fn mat_sizes_must_fit_the_physical_mat() {
    let mut p = profile();
    p.mat_sizes[1].height_mm = -1.0;
    assert!(pointers(&check_profile(&p)).contains(&"/mat_sizes/1/height_mm"));
}

// ---- loading whole entries ----

#[test]
fn loading_valid_text_gives_an_entry() {
    let e = Entry::from_json(LibraryKind::Tool, TOOL).unwrap();
    assert_eq!(e.kind(), LibraryKind::Tool);
    assert_eq!(e.id(), "generic.drag-blade");
    assert_eq!(e.version(), &Version::new(1, 0, 0));
    let e = Entry::from_json(LibraryKind::Material, MATERIAL).unwrap();
    assert_eq!(e.kind(), LibraryKind::Material);
    let e = Entry::from_json(LibraryKind::Profile, PROFILE).unwrap();
    assert_eq!(e.lib_ref().to_string(), "generic.cutter-small@1.0.0");
}

#[test]
fn loading_bad_json_is_one_issue() {
    let issues = Entry::from_json(LibraryKind::Tool, "{ nope").unwrap_err();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].pointer, "");
    assert!(
        issues[0].message.contains("not valid JSON"),
        "{}",
        issues[0].message
    );
}

#[test]
fn loading_reports_structural_errors_before_semantic_ones() {
    let mut tool = value(TOOL);
    tool.as_object_mut().unwrap().remove("name");
    tool["parameters"]["force"]["default"] = json!(99_999);
    let issues = Entry::from_json(LibraryKind::Tool, &tool.to_string()).unwrap_err();
    // The structural error stops deserialising, so only it is reported.
    assert_eq!(pointers(&issues), [""], "{issues:?}");
}

#[test]
fn loading_runs_the_semantic_checks_on_structurally_valid_files() {
    let mut tool = value(TOOL);
    tool["parameters"]["force"]["default"] = json!(99_999);
    let issues = Entry::from_json(LibraryKind::Tool, &tool.to_string()).unwrap_err();
    assert_eq!(pointers(&issues), ["/parameters/force/default"]);
}

#[test]
fn a_file_from_a_newer_app_is_refused_with_a_clear_message() {
    let mut tool = value(TOOL);
    tool["schema_version"] = json!(99);
    let issues = Entry::from_json(LibraryKind::Tool, &tool.to_string()).unwrap_err();
    assert_eq!(pointers(&issues), ["/schema_version"]);
    assert!(issues[0].message.contains("newer"), "{}", issues[0].message);
}
