# Plotter Studio documentation

Reference documentation for people who **extend** Plotter Studio: writing a machine plugin, or
authoring tools, materials and machine profiles.

| If you want to… | Read |
|---|---|
| Add support for a new machine | [Device plugins](plugins.md) |
| Understand how the three libraries fit together | [Libraries overview](libraries.md) |
| Describe a blade, pen, stylus or other tool | [Tools](tools.md) |
| Add cutting/drawing settings for a material, or share a material pack | [Materials](materials.md) |
| Describe a machine's size, holders, speeds and limits | [Machine profiles](profiles.md) |
| Map colours/layers of an imported drawing to cuts, pens and scoring | [Mapping rules](mapping-rules.md) |

## How the pieces relate

```text
 project ──references──▶ tool (id@version)        what a tool is and what it can be told to do
         ──references──▶ material (id@version)    per-tool settings (force, speed, passes …)
         ──references──▶ machine profile          what the machine can physically do

 compiler ── reads those three ──▶ ps-gcode/1 text ──▶ host ──▶ device plugin ──▶ the machine
```

- **Libraries** (tools, materials, profiles) are plain JSON files, validated against published
  JSON Schemas. They describe *what* to do and *with what*.
- **Device plugins** are WebAssembly components. They only translate already-parsed G-code
  steps into a machine's own protocol. They never parse G-code, never open ports themselves,
  and never touch the file system.
- The **machine profile** is the link between the two worlds: a plugin reports a
  `profile-id` for the machine it found, and that id names a profile in the library.

## Conventions used in these documents

- *Must*, *must not* and *should* mean what they do in RFC 2119.
- File names such as `crates/ps-library/src/schema.rs` are relative to the repository root.
- Every claim about validation is checked by code in `crates/ps-library/src/schema.rs`; where a
  rule is **not** enforced (so a mistake passes silently), the document says so explicitly.

## Where the authoritative definitions live

| Thing | Source of truth |
|---|---|
| Tool / material / profile structure | Rust types in `crates/ps-library/src/model.rs` |
| Published JSON Schemas | `schemas/{tool,material,profile}/v1.json`, **generated** from those types |
| Plugin contract | `crates/ps-device/wit/ps-device.wit` |
| G-code dialect plugins receive | `crates/ps-gcode/src/parse.rs` |
| Plugin package format | `crates/ps-device/src/package.rs` |

Do not edit the JSON Schemas by hand: `cargo xtask schemas` regenerates them and
`cargo xtask check-schemas` (run in CI) fails if they drift from the Rust types.
