//! Ground truth for the library data model. The tool JSON below is the
//! example printed in the specification, character for character.

use super::*;

const SPEC_TOOL: &str = r#"{
  "schema_version": 1,
  "id": "generic.drag-blade",
  "version": "1.0.0",
  "name": "Fine-point blade (45°)",
  "family": "blade",
  "drive": "drag",
  "geometry": { "blade_offset_mm": 0.25, "min_feature_mm": 0.8 },
  "operations": ["cut", "kiss_cut"],
  "holders": ["generic.holder-b"],
  "parameters": {
    "force":  { "type": "number", "unit": "gf",   "min": 50, "max": 4000, "default": 350 },
    "speed":  { "type": "number", "unit": "mm/s", "min": 5,  "max": 400,  "default": 100 },
    "passes": { "type": "integer",                "min": 1,  "max": 30,   "default": 1 },
    "overcut":{ "type": "number", "unit": "mm",   "min": 0,  "max": 5,    "default": 1.0 }
  },
  "render": { "effect": "cut" },
  "license": "CC0-1.0"
}"#;

const PEN: &str = r##"{
  "schema_version": 1,
  "id": "pen.fineliner-black-0.4",
  "version": "1.0.0",
  "name": "Fine liner, black 0.4",
  "family": "pen",
  "drive": "fixed_point",
  "geometry": { "tip_width_mm": 0.4 },
  "operations": ["draw", "fill"],
  "holders": ["generic.holder-a"],
  "parameters": {
    "force": { "type": "number", "unit": "gf", "min": 20, "max": 300, "default": 120 }
  },
  "render": { "effect": "ink" },
  "license": "CC0-1.0",
  "pen": { "colour": { "hex": "#231F20", "name": "Black" }, "tip_type": "fibre" }
}"##;

const MATERIAL: &str = r#"{
  "schema_version": 1,
  "id": "community.cardstock-80lb",
  "version": "2.1.0",
  "name": "Cardstock 80 lb",
  "category": "paper",
  "brand": "Generic",
  "thickness_mm": 0.22,
  "grip": ["standard", "strong"],
  "presets": [
    {
      "tool": "generic.drag-blade",
      "operation": "cut",
      "params": { "force": 210, "speed": 100, "passes": 1 },
      "machines": { "generic.cutter-medium": { "force": 230 } }
    }
  ],
  "license": "CC0-1.0",
  "names": { "de": "Karton 80 lb" }
}"#;

const PROFILE: &str = r#"{
  "schema_version": 1,
  "id": "generic.cutter-small",
  "version": "1.0.0",
  "name": "Generic Cutter S",
  "work_area": { "width_mm": 292.0, "height_mm": 597.0 },
  "mat_sizes": [
    { "name": "12x12 in", "width_mm": 304.8, "height_mm": 304.8 },
    { "name": "12x24 in", "width_mm": 304.8, "height_mm": 609.6 }
  ],
  "origin": "top_left",
  "keep_outs": [],
  "holders": [
    { "id": "generic.holder-a", "label": "Clamp A", "accepts": ["pen", "scoring"] },
    { "id": "generic.holder-b", "label": "Clamp B", "accepts": ["blade", "foil"] }
  ],
  "force": { "min_gf": 0, "max_gf": 400, "units_per_gf": 1.0 },
  "max_speed_mm_s": 500.0,
  "max_accel_mm_s2": 4000.0,
  "junction_deviation_mm": 0.05,
  "tool_up_ms": 40,
  "tool_down_ms": 60,
  "arcs": true,
  "tangential_axis": false,
  "heated": false,
  "gcode": { "dialect": "ps-gcode/1", "arcs": true, "chord_tolerance_mm": 0.01 },
  "license": "CC0-1.0"
}"#;

#[test]
fn the_tdd_tool_example_parses() {
    let t: Tool = serde_json::from_str(SPEC_TOOL).unwrap();
    assert_eq!(t.id, "generic.drag-blade");
    assert_eq!(t.version, Version::new(1, 0, 0));
    assert_eq!(t.family, ToolFamily::Blade);
    assert_eq!(t.drive, DriveType::Drag);
    assert_eq!(t.geometry.blade_offset_mm, Some(0.25));
    assert_eq!(t.geometry.min_feature_mm, Some(0.8));
    assert_eq!(t.operations, ["cut", "kiss_cut"]);
    assert_eq!(t.holders, ["generic.holder-b"]);
    let force = &t.parameters["force"];
    assert_eq!(
        (
            force.kind,
            force.unit.as_deref(),
            force.min,
            force.max,
            force.default
        ),
        (ParamType::Number, Some("gf"), 50.0, 4_000.0, 350.0)
    );
    assert_eq!(t.parameters["passes"].kind, ParamType::Integer);
    assert_eq!(t.parameters["passes"].unit, None);
    assert_eq!(t.render.effect, "cut");
    assert_eq!(t.license, "CC0-1.0");
    assert!(t.pen.is_none());
}

#[test]
fn tools_round_trip_through_json() {
    let t: Tool = serde_json::from_str(SPEC_TOOL).unwrap();
    let again: Tool = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
    assert_eq!(again, t);
}

#[test]
fn a_tool_names_itself_as_a_library_reference() {
    let t: Tool = serde_json::from_str(SPEC_TOOL).unwrap();
    assert_eq!(t.lib_ref().to_string(), "generic.drag-blade@1.0.0");
}

#[test]
fn an_unknown_family_is_an_error() {
    let bad = SPEC_TOOL.replace("\"blade\"", "\"hammer\"");
    assert!(serde_json::from_str::<Tool>(&bad).is_err());
}

#[test]
fn pens_carry_colour_and_tip_width() {
    let t: Tool = serde_json::from_str(PEN).unwrap();
    assert_eq!(t.family, ToolFamily::Pen);
    assert_eq!(t.geometry.tip_width_mm, Some(0.4));
    let pen = t.pen.as_ref().unwrap();
    assert_eq!(pen.colour.hex, "#231F20");
    assert_eq!(pen.colour.name, "Black");
    assert_eq!(pen.tip_type.as_deref(), Some("fibre"));
}

#[test]
fn colours_parse_to_rgb() {
    let c = Colour {
        hex: "#231F20".to_owned(),
        name: "Black".to_owned(),
    };
    assert_eq!(c.rgb(), Some([0x23, 0x1F, 0x20]));
    let lower = Colour {
        hex: "#ff8a1f".to_owned(),
        name: String::new(),
    };
    assert_eq!(lower.rgb(), Some([0xff, 0x8a, 0x1f]));
    for bad in ["", "231F20", "#12345", "#GGGGGG", "#1234567"] {
        let c = Colour {
            hex: bad.to_owned(),
            name: String::new(),
        };
        assert_eq!(c.rgb(), None, "{bad:?}");
    }
}

#[test]
fn a_material_parses_with_presets_and_translations() {
    let m: Material = serde_json::from_str(MATERIAL).unwrap();
    assert_eq!(m.id, "community.cardstock-80lb");
    assert_eq!(m.category, "paper");
    assert_eq!(m.thickness_mm, Some(0.22));
    assert_eq!(m.grip, ["standard", "strong"]);
    assert_eq!(m.presets.len(), 1);
    assert_eq!(m.names["de"], "Karton 80 lb");
    assert_eq!(m.lib_ref().to_string(), "community.cardstock-80lb@2.1.0");
}

#[test]
fn a_preset_prefers_the_machine_specific_value() {
    let m: Material = serde_json::from_str(MATERIAL).unwrap();
    let p = &m.presets[0];
    // The generic preset, for a machine without an override.
    let generic = p.params_for("generic.cutter-large");
    assert_eq!(generic["force"], 210.0);
    assert_eq!(generic["speed"], 100.0);
    // The medium cutter's override replaces only the force.
    let explore = p.params_for("generic.cutter-medium");
    assert_eq!(explore["force"], 230.0);
    assert_eq!(explore["speed"], 100.0);
    assert_eq!(explore["passes"], 1.0);
}

#[test]
fn presets_say_whether_a_machine_override_applies() {
    let m: Material = serde_json::from_str(MATERIAL).unwrap();
    assert!(m.presets[0].has_machine_override("generic.cutter-medium"));
    assert!(!m.presets[0].has_machine_override("generic.cutter-large"));
}

#[test]
fn a_machine_profile_parses() {
    let p: MachineProfile = serde_json::from_str(PROFILE).unwrap();
    assert_eq!(p.name, "Generic Cutter S");
    assert_eq!(p.work_area.width_mm, 292.0);
    assert_eq!(p.mat_sizes.len(), 2);
    assert_eq!(p.origin, Origin::TopLeft);
    assert_eq!(p.holders.len(), 2);
    assert!(p.arcs && !p.tangential_axis && !p.heated);
    assert_eq!(p.gcode.dialect, "ps-gcode/1");
    assert_eq!(p.gcode.chord_tolerance_mm, 0.01);
    assert_eq!(p.force.max_gf, 400.0);
}

#[test]
fn a_profile_knows_which_holders_take_a_tool() {
    let p: MachineProfile = serde_json::from_str(PROFILE).unwrap();
    let blade: Tool = serde_json::from_str(SPEC_TOOL).unwrap();
    let pen: Tool = serde_json::from_str(PEN).unwrap();
    assert_eq!(
        p.holders_for(&blade)
            .map(|h| h.id.as_str())
            .collect::<Vec<_>>(),
        ["generic.holder-b"]
    );
    assert_eq!(
        p.holders_for(&pen)
            .map(|h| h.id.as_str())
            .collect::<Vec<_>>(),
        ["generic.holder-a"]
    );
}

#[test]
fn a_tool_a_machine_cannot_hold_has_no_holder() {
    let p: MachineProfile = serde_json::from_str(PROFILE).unwrap();
    let mut blade: Tool = serde_json::from_str(SPEC_TOOL).unwrap();
    blade.holders = vec!["acme.turret".to_owned()];
    assert_eq!(p.holders_for(&blade).count(), 0);
}

#[test]
fn force_converts_to_machine_units() {
    let mut p: MachineProfile = serde_json::from_str(PROFILE).unwrap();
    p.force.units_per_gf = 0.5;
    assert_eq!(p.force.to_machine_units(300.0), 150.0);
    assert_eq!(p.force.clamp_gf(900.0), 400.0);
    assert_eq!(p.force.clamp_gf(-5.0), 0.0);
}

#[test]
fn each_kind_has_a_stable_schema_id() {
    assert_eq!(
        LibraryKind::Tool.schema_id(),
        "https://plotter-studio.github.io/schemas/tool/v1.json"
    );
    assert_eq!(
        LibraryKind::Material.schema_id(),
        "https://plotter-studio.github.io/schemas/material/v1.json"
    );
    assert_eq!(
        LibraryKind::Profile.schema_id(),
        "https://plotter-studio.github.io/schemas/profile/v1.json"
    );
}

#[test]
fn kinds_name_their_storage_folders() {
    assert_eq!(LibraryKind::Tool.folder(), "tools");
    assert_eq!(LibraryKind::Material.folder(), "materials");
    assert_eq!(LibraryKind::Profile.folder(), "profiles");
}

#[test]
fn a_profile_without_a_travel_speed_travels_as_fast_as_it_draws() {
    let p: MachineProfile = serde_json::from_str(PROFILE).unwrap();
    assert_eq!(p.max_travel_speed_mm_s, None);
    assert!((p.travel_speed_mm_s() - 500.0).abs() < 1e-9);
}

#[test]
fn a_profile_can_give_its_travel_speed_apart_from_its_drawing_speed() {
    let text = PROFILE.replace(
        "\"max_speed_mm_s\": 500.0,",
        "\"max_speed_mm_s\": 100.0, \"max_travel_speed_mm_s\": 76.2,",
    );
    let p: MachineProfile = serde_json::from_str(&text).unwrap();
    assert_eq!(p.max_travel_speed_mm_s, Some(76.2));
    assert!((p.travel_speed_mm_s() - 76.2).abs() < 1e-9);
    // It survives a round trip, and an absent one is not written.
    let back: MachineProfile = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(back, p);
    let plain: MachineProfile = serde_json::from_str(PROFILE).unwrap();
    assert!(
        !serde_json::to_string(&plain)
            .unwrap()
            .contains("max_travel_speed_mm_s")
    );
}

// Measured on a hobby cutting plotter (hardware speed test, 2026-11-21, pen on paper): the carriage drew at 50.6 mm/s
// at 2.0 in/s and 101 mm/s at 4.0 in/s (the most its vendor software asks for), and travelled at 3.0 in/s (76.2 mm/s) on both axes at the normal setting.
#[test]
fn the_explore_air_2_profile_has_the_speeds_measured_on_the_machine() {
    let p: MachineProfile = serde_json::from_str(include_str!(
        "../../builtin/profiles/generic.cutter-small.json"
    ))
    .unwrap();
    assert!(
        (p.max_speed_mm_s - 101.6).abs() < 1e-9,
        "{}",
        p.max_speed_mm_s
    );
    assert!((p.travel_speed_mm_s() - 76.2).abs() < 1e-9);
}

#[test]
fn a_profile_can_say_how_its_acceleration_follows_its_speed() {
    let plain: MachineProfile = serde_json::from_str(PROFILE).unwrap();
    assert_eq!(plain.accel_by_speed.len(), 0);
    assert!(
        !serde_json::to_string(&plain)
            .unwrap()
            .contains("accel_by_speed")
    );
    let text = PROFILE.replace(
        "\"max_accel_mm_s2\": 4000.0,",
        "\"max_accel_mm_s2\": 300.0, \"accel_by_speed\": [[50.0, 100.0], [100.0, 300.0]],",
    );
    let p: MachineProfile = serde_json::from_str(&text).unwrap();
    assert_eq!(p.accel_by_speed, vec![(50.0, 100.0), (100.0, 300.0)]);
    let back: MachineProfile = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(back, p);
}

// Measured on the machine (harness `speed`, 2026-11-21, 100 mm line out and back at five drawing
// speeds), fitted with the planner of `ps-compiler`: the turnaround of a pass took 0.46 s at
// 25 mm/s and 0.30 s at 102 mm/s, i.e. 55, 115, 190, 255 and 335 mm/s^2 at 25.4, 50.8, 76.2, 88.9
// and 101.6 mm/s. With a junction deviation of 0.5 mm the planner then predicts 183 s for a fill
// of 9,254 moves that took about 180 s on the machine at 50.8 mm/s.
#[test]
fn the_explore_air_2_profile_has_the_acceleration_measured_on_the_machine() {
    let p: MachineProfile = serde_json::from_str(include_str!(
        "../../builtin/profiles/generic.cutter-small.json"
    ))
    .unwrap();
    assert_eq!(
        p.accel_by_speed,
        vec![
            (25.4, 55.0),
            (50.8, 115.0),
            (76.2, 190.0),
            (88.9, 255.0),
            (101.6, 335.0)
        ]
    );
    assert!((p.max_accel_mm_s2 - 335.0).abs() < 1e-9);
    // 0.5 mm fitted the long rows of the speed tests; with the cost per vector added, 0.05 mm
    // fits the fill as well (see `the_explore_air_2_profile_costs_each_vector_as_measured`).
    assert!((p.junction_deviation_mm - 0.05).abs() < 1e-9);
}

#[test]
fn a_profile_can_say_what_each_vector_costs_the_machine() {
    let plain: MachineProfile = serde_json::from_str(PROFILE).unwrap();
    assert_eq!(plain.vector_ms_by_speed.len(), 0);
    assert!(
        !serde_json::to_string(&plain)
            .unwrap()
            .contains("vector_ms_by_speed")
    );
    let text = PROFILE.replace(
        "\"max_accel_mm_s2\": 4000.0,",
        "\"max_accel_mm_s2\": 4000.0, \"vector_ms_by_speed\": [[50.0, 20.0], [100.0, 10.0]],",
    );
    let p: MachineProfile = serde_json::from_str(&text).unwrap();
    assert_eq!(p.vector_ms_by_speed, vec![(50.0, 20.0), (100.0, 10.0)]);
}

// Measured on the machine (harness `speed` on the pen fill, 2026-11-21): the fill took 321 s as
// 9,214 vectors and 220 s as 2,414, at Normal speed, and 221 s and 157 s at 4 in/s with Fast Mode's
// second Geometry float. Planner plus a cost per vector of 5.0 ms divided by that float (0.4 at
// 50.8 mm/s and below, 0.5 at 76.2, 0.55 at 88.9, 0.6 at 101.6) reproduces the four to within
// 15 %, and the two at the speeds jobs actually run at to within 2 %.
#[test]
fn the_explore_air_2_profile_costs_each_vector_as_measured() {
    let p: MachineProfile = serde_json::from_str(include_str!(
        "../../builtin/profiles/generic.cutter-small.json"
    ))
    .unwrap();
    assert_eq!(
        p.vector_ms_by_speed,
        vec![
            (25.4, 12.5),
            (50.8, 12.5),
            (76.2, 10.0),
            (88.9, 9.09),
            (101.6, 8.33)
        ]
    );
    assert!((p.junction_deviation_mm - 0.05).abs() < 1e-9);
}

#[test]
fn a_profile_can_ask_for_fill_turns_to_be_rounded() {
    let plain: MachineProfile = serde_json::from_str(PROFILE).unwrap();
    assert_eq!(plain.fill_turn_step_deg, None);
    assert!(
        !serde_json::to_string(&plain)
            .unwrap()
            .contains("fill_turn_step_deg")
    );
    let text = PROFILE.replace(
        "\"max_accel_mm_s2\": 4000.0,",
        "\"max_accel_mm_s2\": 4000.0, \"fill_turn_step_deg\": 15.0,",
    );
    let p: MachineProfile = serde_json::from_str(&text).unwrap();
    assert_eq!(p.fill_turn_step_deg, Some(15.0));
}

// Rounded turns cut a 30 mm zigzag row by 35 % on the machine (harness `speed`, 2026-11-21). On the
// real fill, rounding every turn gained 16 % but jerked on the short rows of the D and H, so the
// compiler rounds only turns between rows of 8 mm or more (`ps-compiler` `link.rs`).
#[test]
fn the_explore_air_2_rounds_long_row_fill_turns_in_fifteen_degree_steps() {
    let p: MachineProfile = serde_json::from_str(include_str!(
        "../../builtin/profiles/generic.cutter-small.json"
    ))
    .unwrap();
    assert_eq!(p.fill_turn_step_deg, Some(15.0));
}
