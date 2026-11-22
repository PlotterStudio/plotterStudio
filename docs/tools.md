# Tools

A **tool** describes something that goes in a machine's holder: a cutting blade, a pen, a
scoring stylus, a foil tool. It says how the tool follows a path, which holders it fits, which
operations it performs, and which parameters (force, speed, passes…) can be set, with their
ranges. It does **not** say what values suit a given material; that is a
[material preset](materials.md#presets).

Rust type: `Tool` in `crates/ps-library/src/model.rs`. Schema: `schemas/tool/v1.json`.
Built-in examples: `crates/ps-library/builtin/tools/`.

See [Libraries overview](libraries.md) for ids, versions, storage and overlays.

## Minimal example (a pen)

```json
{
  "schema_version": 1,
  "id": "acme.gel-pen-0.7-blue",
  "version": "1.0.0",
  "license": "CC0-1.0",
  "name": "Gel pen, blue 0.7",
  "family": "pen",
  "drive": "fixed_point",
  "geometry": { "tip_width_mm": 0.7 },
  "operations": ["draw", "fill"],
  "holders": ["generic.holder-a", "grbl.pen-holder"],
  "parameters": {
    "force": { "type": "number", "unit": "gf", "min": 20, "max": 300, "default": 100 },
    "speed": { "type": "number", "unit": "mm/s", "min": 5, "max": 300, "default": 80 }
  },
  "render": { "effect": "ink" },
  "pen": { "colour": { "hex": "#1E4FD8", "name": "Blue" }, "tip_type": "gel" }
}
```

## Fields

### Required

| Field | Type | Notes |
|---|---|---|
| `schema_version` | integer | `1`. |
| `id`, `version`, `name`, `license` | | See [common fields](libraries.md#entries-ids-and-versions). |
| `family` | enum | What kind of tool it is. See below. |
| `drive` | enum | How it follows a path. See below. |
| `operations` | string[] | What it can do. **At least one.** |
| `holders` | string[] | Ids of holders it fits. **At least one.** |
| `parameters` | object | Tunable parameters, keyed by name. May be empty `{}`. |
| `render` | object | `{ "effect": "<name>" }`: how the preview draws it (built-ins use `cut` and `ink`). |

### Optional

| Field | Type | Notes |
|---|---|---|
| `geometry` | object | Physical dimensions; see below. Defaults to all-absent. |
| `pen` | object | Required when `family` is `pen`. |

### `family`

`blade`, `pen`, `foil`, `scoring`, `emboss`, `engrave`, `perforate`, `wheel`, `heated`, `other`.

The family matters beyond labelling: a machine's holders list which families they **accept**
(`accepts` in the [profile](profiles.md#holders)), so a tool fits a holder only if the tool
lists the holder's id *and* the holder accepts the tool's family. Note that a rotary cutting
wheel is `family: "blade"` in the built-ins, with `drive: "rolling"`; `wheel` is available
if a holder should be able to tell the two apart.

### `drive`

| Value | Meaning | Typical tool |
|---|---|---|
| `drag` | A swivel blade dragged behind the holder. Needs blade-offset compensation. | Cutting-plotter fine-point blade |
| `fixed_point` | A tool with no direction. | Pens, stylus |
| `tangential` | A driven blade that turns to follow the path. | Oscillating/drag-less knife |
| `rolling` | A rolling wheel. | Rotary wheel |
| `heated` | A heated tip. | Foil/heat tools |

The drive does not by itself gate machines, but the G-code checker does: a line with a tangent
angle (`A`) is refused unless the profile has `tangential_axis: true`, and a temperature command
(`M104`/`M109`) is refused unless it has `heated: true`. Pick a `drive` your target profiles
can honour.

### `geometry`

All values in millimetres, all optional, and **each one given must be greater than zero**.

| Field | Meaning |
|---|---|
| `blade_offset_mm` | Distance from the swivel axis to the blade tip. **Required for `drive: "drag"`.** A measured value in the user's [calibration](libraries.md#per-machine-calibration) overrides it. |
| `min_feature_mm` | Smallest feature the tool can make. |
| `tip_width_mm` | Width of a pen's line. **Required for pens** (used to space the lines of a fill). |
| `wheel_radius_mm` | Radius of a wheel tool. |

### `operations`

Free-form strings in the file, but the app's operations use a fixed set of kinds, and a
[preset](materials.md#presets) matches a tool *and* an operation kind exactly. The kinds are
`cut`, `kiss_cut`, `draw`, `fill`, `score`, `emboss`, `deboss`, `engrave`, `foil`,
`foil_fill`, `perforate`, `wavy_cut`. The built-ins use `cut`, `kiss_cut`, `draw`, `fill`,
`score`. Nothing checks `operations` against that set, so a typo is only noticed when a preset
fails to match.

### `parameters`

An object whose keys are parameter names and whose values describe the permitted range:

```json
"force": { "type": "number", "unit": "gf", "min": 50, "max": 4000, "default": 350 }
```

| Field | Notes |
|---|---|
| `type` | `number` or `integer`. For `integer`, `min`, `max` and `default` must all be whole numbers. |
| `unit` | Optional. One of `gf`, `mm/s`, `mm`, `°C`, `ms`, `deg`, `%`, `s`. Anything else is rejected. Omit for dimensionless values (passes, ratios). |
| `min`, `max` | `min` must not exceed `max`. |
| `default` | Used when nothing overrides it; must lie within `min..max`. |

**Parameter names** are not restricted by a fixed list, but the compiler (`ps-compiler`, in
`prepare.rs` and `compile.rs`) looks up these by name. Use the same names so materials, the UI
and the compiler all work with your tool. A parameter the compiler does not know is stored and
shown but has no effect.

| Name | Unit | Compiler behaviour |
|---|---|---|
| `force` | gf | Downward force in grams-force. Clamped to the machine's force range (and reported, never silently changed), then converted to machine units via the profile's [`force`](profiles.md#force). |
| `speed` | mm/s | Drawing speed; capped at the profile's `max_speed_mm_s`. |
| `passes` | – | Passes over each path. The compiler clamps to 1–100 regardless of your `max`. |
| `overcut` | mm | Extra length past the start of a **closed** path so it cuts through. Ignored on open paths. |
| `tab_count`, `tab_width_mm` | – / mm | Uncut bridges that hold a piece in the sheet. Gaps never take more than half the path. |
| `lead_in_mm`, `lead_out_mm` | mm | Run-up before and run-off after an **open** path. Ignored on closed paths. |
| `force_ramp` | 0–1 | With several passes, the first pass is lighter and each later one presses harder until the last is at full force; `0` (default) means every pass at full force. |
| `corner_threshold_deg` | deg | Drag blades. Turns at or above this angle swivel the blade on an arc; gentler turns are joined by a line. Default 10. |
| `align_mm` | mm | Drag blades. Length of an alignment stroke before the first contact, so the blade is pointing the right way when it enters the cut. Default 0 (none). |
| `corner_lift_deg` | deg | Tangential tools. Default 30. |
| `cut_mm`, `gap_mm` | mm | `perforate` operations: length of each cut and of each gap between cuts. Default 2 and 1. |
| `tip_width` | mm | Pen line width for fills. |

### `pen`

For `family: "pen"`; **required** then, and `geometry.tip_width_mm` must also be set.

| Field | Notes |
|---|---|
| `colour.hex` | `#RRGGBB` (exactly six hex digits, with the `#`). |
| `colour.name` | Display name. |
| `tip_type` | Optional, free text (`fibre`, `ballpoint`, `gel`…). |
| `owned` | Boolean, default `false`. The user sets it (via overlay) to put the pen in their *pen inventory*; you normally leave it out of shared files. |

## What validation checks

On top of the schema (types, required fields, enum values), the semantic stage reports:

- id contains characters other than letters, digits, `.`, `-`, `_`;
- empty `operations` or `holders`;
- a parameter with `min > max`, a `default` outside the range, a non-whole `integer`, or an
  unknown `unit`;
- non-positive geometry values;
- a pen without `tip_width_mm`, without a `pen` block, or with a malformed colour;
- a `drag` tool without `blade_offset_mm`.

**Not checked:** that `holders` ids exist in any profile, that `operations` match any material,
and that parameter names are meaningful. A tool whose holder id matches no profile is valid but
fits no machine.

## Checklist for a new tool

1. Pick the `family` and `drive` that match how the tool physically moves.
2. List every holder id it fits (look at the [profiles](profiles.md#holders) you care about).
3. Declare the parameters with realistic ranges. Prefer the standard names above.
4. For a blade, measure and set `blade_offset_mm`; for a pen, set `tip_width_mm` and `pen`.
5. `pstudio validate your-tool.json`.
6. Add presets for it in at least one [material](materials.md) so it can be used.
