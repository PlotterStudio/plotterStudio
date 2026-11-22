# Machine profiles

A **machine profile** describes one machine model to the compiler and the G-code checker: how
big its work area is, where its coordinates start, which holders it has, how hard and fast it
can go, and which G-code features it supports. Nothing about *how to talk to* the machine lives
here; that is a [device plugin](plugins.md).

Rust type: `MachineProfile` in `crates/ps-library/src/model.rs`. Schema:
`schemas/profile/v1.json`. Built-in examples: `crates/ps-library/builtin/profiles/`
(`generic.cutter-small`, `generic.cutter-medium`, `generic.cutter-large`, `generic.grbl-pen-plotter`).

See [Libraries overview](libraries.md) for ids, versions, storage and overlays.

## How a profile is linked to a plugin

A plugin may report `profile-id` in its `device-info` when it identifies a machine (see
[Plugins](plugins.md#device-info)). That string is the `id` of a profile here, and when it is
reported the host will refuse to run a job that was compiled for any other profile. A material's
per-machine overrides (`machines` in a [preset](materials.md#presets)) and the user's
[calibration](libraries.md#per-machine-calibration) are keyed by the same id, so choose it once
and keep it stable. If a plugin supports several models, ship one profile per model.

## Example (trimmed)

```json
{
  "schema_version": 1,
  "id": "acme.pen-plotter-a4",
  "version": "1.0.0",
  "license": "CC0-1.0",
  "name": "Acme A4 pen plotter",
  "work_area": { "width_mm": 297, "height_mm": 210 },
  "mat_sizes": [{ "name": "A4", "width_mm": 297, "height_mm": 210 }],
  "origin": "bottom_left",
  "keep_outs": [],
  "holders": [{ "id": "acme.pen", "label": "Pen holder", "accepts": ["pen"] }],
  "force": { "min_gf": 0, "max_gf": 300, "units_per_gf": 1.0 },
  "max_speed_mm_s": 200,
  "max_accel_mm_s2": 1000,
  "junction_deviation_mm": 0.05,
  "tool_up_ms": 100,
  "tool_down_ms": 100,
  "arcs": true,
  "tangential_axis": false,
  "heated": false,
  "gcode": { "dialect": "ps-gcode/1", "arcs": true, "chord_tolerance_mm": 0.01 }
}
```

## Fields

All fields below are required unless marked optional.

### Geometry

| Field | Notes |
|---|---|
| `work_area` | `{ width_mm, height_mm }`, the cuttable/drawable area. Both must be greater than zero. Every move the compiler produces is checked to lie inside it. |
| `mat_sizes` | Named mat/sheet sizes the machine takes (`name`, `width_mm`, `height_mm`, each dimension > 0). Offered as choices for the mat in the UI; the compiler and G-code checker do not consult this list. May be empty. |
| `origin` | The corner coordinates are measured from: `top_left` (the convention of most cutting plotters), `top_right`, `bottom_left` or `bottom_right`. The compiler flips axes so G-code coordinates are in this machine frame. |
| `keep_outs` | Rectangles `{ x_mm, y_mm, width_mm, height_mm }` the tools must never enter (a clamp area, a docking zone). `x_mm`/`y_mm` is the rectangle's minimum corner. Checked in machine coordinates, both by the compiler and by the G-code checker. Use `[]` for none. |
| `roll` *(optional)* | `{ width_mm, max_length_mm }` for roll-fed (matless) machines. |

### Holders

```json
"holders": [
  { "id": "generic.holder-a", "label": "Clamp A", "accepts": ["pen", "scoring"] },
  { "id": "generic.holder-b", "label": "Clamp B", "accepts": ["blade", "foil"] }
]
```

At least one is required. `accepts` lists the [tool families](tools.md#family) the holder
takes. A [tool](tools.md) fits a holder only if the tool's `holders` array contains the holder's
`id` **and** the holder `accepts` the tool's family. The holder `id`s here are therefore the
shared vocabulary between profiles and tools: if you add a holder, add its id to the `holders`
of every tool that fits it.

Holder ids are also the keys of the calibrated pen-to-blade offsets
(`holder_offsets_mm` in the user's calibration).

### Force

```json
"force": { "min_gf": 0, "max_gf": 400, "units_per_gf": 1.0 }
```

Libraries work in **grams-force**. `min_gf`/`max_gf` is the range the machine can apply
(`min_gf` must not exceed `max_gf`); the compiler clamps every requested force into it and
warns. `units_per_gf` (must be greater than zero) converts gram-force into whatever number the
machine's own protocol expects; it is applied when the force is written as the `S` word of
`M3 S<force>`.

### Speed and motion

| Field | Unit | Notes |
|---|---|---|
| `max_speed_mm_s` | mm/s | Top speed while drawing or cutting. A tool's `speed` is capped at this. |
| `max_travel_speed_mm_s` *(opt.)* | mm/s | Top speed of moves with the tool up. Defaults to `max_speed_mm_s`. Set it for a machine that travels at a different speed than it draws (some hobby cutters do). |
| `max_accel_mm_s2` | mm/s² | Acceleration at the machine's top speed. Used for time estimates and look-ahead. |
| `accel_by_speed` *(opt.)* | `[[mm/s, mm/s²], …]` | For machines whose acceleration rises with speed: points in **ascending speed**. Empty means `max_accel_mm_s2` at every speed. |
| `vector_ms_by_speed` *(opt.)* | `[[mm/s, ms], …]` | Time the machine spends per vector it draws, for machines whose time depends on how many vectors they are fed, not only distance. Ascending speed. Empty means no such cost. |
| `fill_turn_step_deg` *(opt.)* | degrees | When set, the hairpins joining the rows of a pen fill are drawn as semicircles whose chords turn by at most this angle, for machines that slow at sharp corners. |
| `junction_deviation_mm` | mm | Junction deviation for the look-ahead planner (how fast corners may be taken). Typical: `0.05`. |
| `tool_up_ms`, `tool_down_ms` | ms | Time to raise/lower the tool. Used in estimates. |

The last four optional fields exist to make **time estimates and fill quality** match a real
machine. Measure them on hardware before filling them in; leave them out if unknown. They are
documented in the notes of the plugin that was measured against.

### Capabilities

| Field | Notes |
|---|---|
| `arcs` | The machine can follow `G2`/`G3` arcs. |
| `tangential_axis` | The machine has a tangential (A) axis. If `false`, the G-code checker refuses any `G1` line that carries an `A` word. |
| `heated` | The machine supports heated tools. If `false`, the checker refuses `M104`/`M109`. |
| `heating_rate_c_s` | Heating rate in °C per second. **Required** (and must be present) when `heated` is `true`. |

### G-code settings

```json
"gcode": { "dialect": "ps-gcode/1", "arcs": true, "chord_tolerance_mm": 0.01 }
```

| Field | Notes |
|---|---|
| `dialect` | Must start with `ps-gcode/` (currently only `ps-gcode/1` exists; see the [dialect](plugins.md#the-g-code-dialect)). |
| `arcs` | Whether the compiler *emits* `G2`/`G3`. Set `false` to have every arc turned into short lines. |
| `chord_tolerance_mm` | How far (mm) a line approximation may deviate from the true curve when curves become lines. Must be greater than zero. |

`arcs` (top level) states what the machine can do; `gcode.arcs` is what the output uses. Make
`gcode.arcs` `false` if the plugin's machine cannot do arcs, and the compiler will turn every
arc into short lines before the plugin ever sees it. A plugin should still refuse (with a
`device-error`) an arc it cannot execute rather than guess. (The arc-to-lines routine the
compiler uses is `linearise` in the `ps-gcode` crate; it is a host-side crate, not part of
`ps-device-sdk`, and plugins are not expected to depend on it.)

## What validation checks

Schema: types, required fields, enum values. Then:

- `work_area` and every `mat_sizes` dimension greater than zero;
- at least one holder;
- `force.min_gf` not above `force.max_gf`; `force.units_per_gf` greater than zero;
- `gcode.chord_tolerance_mm` greater than zero; `gcode.dialect` starting `ps-gcode/`;
- `heated` machines have `heating_rate_c_s`.

**Not checked:** that `max_speed_mm_s`, accelerations or times are positive, that
`accel_by_speed` / `vector_ms_by_speed` are sorted, and that holder ids match any tool. A
profile with `max_speed_mm_s: 0` validates; take care.

## Checklist for a new machine

1. Measure or look up the work area and origin corner; add any keep-outs.
2. List the holders and which tool families each accepts; update tool files to list the new
   holder ids.
3. Set the force range and `units_per_gf` from the machine's protocol.
4. Set speeds/accelerations conservatively; refine from measured runs.
5. Choose an `id` and have your [plugin](plugins.md) return it as `profile-id`.
6. `pstudio validate your-profile.json`, then test a small job in the simulator before real
   hardware.
