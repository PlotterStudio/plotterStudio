//! Sample library files shared by tests: the specification's own tool example plus a pen, a material
//! and a machine profile.

pub(crate) const TOOL: &str = r#"{
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

pub(crate) const PEN: &str = r##"{
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

pub(crate) const MATERIAL: &str = r#"{
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

pub(crate) const PROFILE: &str = r#"{
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
