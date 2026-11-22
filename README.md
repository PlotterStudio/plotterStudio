# Plotter Studio

An open-source desktop application for cutting plotters and pen plotters. Import a drawing,
assign tools and materials, and send it to your machine: no cloud account, no vendor software
in the loop.

- **Import** SVG and DXF drawings (PDF and AI are accepted too), with colour- and layer-based
  [mapping rules](docs/mapping-rules.md) that turn a stroke colour into "cut", "score" or "draw".
- **Plan** the job: blade-offset (drag-knife) compensation, overcut, path ordering and linking,
  stroke simplification, auto-nesting on the mat, and time estimates from the machine's measured
  speeds and acceleration.
- **Describe your gear** with plain JSON [tools](docs/tools.md), [materials](docs/materials.md)
  and [machine profiles](docs/profiles.md), validated against published
  [JSON Schemas](schemas/). Share them as packages.
- **Compile deterministically** to a documented G-code dialect (`ps-gcode/1`) and a `.psjob`
  bundle. The same input always yields byte-identical output; golden tests enforce it.
- **Drive machines through sandboxed plugins.** Each machine is a WebAssembly
  [device plugin](docs/plugins.md): it translates already-parsed steps into the machine's own
  protocol, runs in a resource-limited sandbox, and can only touch the buses and hosts you
  approve. A GRBL plugin ships in this repository.
- **Headless too:** the `pstudio` command line uses the same code as the app.
- **Scriptable:** turn on the MCP server in the settings and an AI assistant or script can use the
  app's whole feature set over HTTP ([docs/automation.md](docs/automation.md)). It is off by
  default, needs an access token, and moving real hardware this way needs a separate switch only
  a person can flip.

> **Status:** early (0.1). The compiler, importers and GUI are tested extensively; always do the
> first cut of a new machine, tool or material on scrap, close to the e-stop.

## Install

Release builds for Windows, macOS and Linux are attached to each
[GitHub release](../../releases) once published. To build from source, see below.

## Build from source

You need the Rust toolchain pinned in [`rust-toolchain.toml`](rust-toolchain.toml)
([rustup](https://rustup.rs) installs it automatically) and the WebAssembly target used to build
the plugins:

```sh
rustup target add wasm32-wasip2
```

On Linux also install `libudev-dev libdbus-1-dev pkg-config libxkbcommon-dev libwayland-dev libx11-dev
libxcursor-dev libxrandr-dev libxi-dev libvulkan-dev`.

```sh
cargo run --release -p ps-app               # the application
cargo run --release -p ps-app -- --simulate # with a built-in simulated machine to connect to
cargo run --release -p ps-cli -- --help     # the `pstudio` command line
```

## Command line

```sh
pstudio export design.svg --machine generic.cutter-small --rules rules.json --format gcode
pstudio validate design.svg --machine generic.cutter-small --rules rules.json
pstudio inspect job.psjob
pstudio devices
pstudio run job.psjob --device <id from `pstudio devices`>
pstudio device test my-plugin.psdevice
```

`pstudio <command> --help` lists the options.

## Machines and plugins

| Plugin | Machines |
|---|---|
| [`plugins/grbl`](plugins/grbl) | GRBL-based pen plotters (reference implementation) |

Support for further machines lives in **separate repositories**, one per plugin, built against
the WIT contract in [`crates/ps-device/wit/ps-device.wit`](crates/ps-device/wit/ps-device.wit).
Install a plugin by copying its `.psdevice` file into the app's plugins folder (on Windows
`%APPDATA%\PlotterStudio\data\plugins\`) and pressing refresh. To write your own, start with
[docs/plugins.md](docs/plugins.md).

## Repository layout

| Path | What |
|---|---|
| `crates/ps-app`, `crates/ps-cli` | The GUI (`plotter-studio`) and the CLI (`pstudio`) |
| `crates/ps-core`, `ps-geometry`, `ps-model`, `ps-import`, `ps-compiler`, `ps-gcode`, `ps-job` | Document model, geometry, importers, compiler, G-code and job bundles |
| `crates/ps-library` | Tool, material and machine-profile libraries, packages, storage |
| `crates/ps-device`, `crates/ps-device-sdk` | Plugin host, and helpers for plugin authors |
| `crates/ps-nest`, `ps-render`, `ps-net` | Nesting, canvas rendering, opt-in network feeds |
| `plugins/` | `grbl` (reference plugin) and `rogue` (sandbox test) |
| `docs/` | Reference for plugin, tool, material and profile authors, and the automation (MCP) guide |
| `schemas/` | Published JSON Schemas for library files |
| `fuzz/` | cargo-fuzz targets for every untrusted-file parser |
| `xtask/` | Repository chores: `cargo xtask <check-boundaries \| check-schemas \| goldens \| licenses \| package>` |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security issues: [SECURITY.md](SECURITY.md).

## Licence

MIT, see [LICENSE](LICENSE). Third-party licences are listed in
[THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md). Product and company names that appear in
plugin repositories belong to their owners; this project is not affiliated with or endorsed by
any machine vendor.
