# Device plugins

A **device plugin** teaches Plotter Studio to find, identify and drive one kind of machine. It is
a WebAssembly component, packaged as a `.psdevice` file, that runs in a sandbox inside the app.

The built-in examples in this repository:

| Folder | Machine | Notes |
|---|---|---|
| `plugins/grbl` | Any GRBL pen plotter | Short and readable: the best starting point. Uses `ps-device-sdk`. |
| `plugins/rogue` | none | Misbehaves on purpose, to test the sandbox. Not a template. |

Source of truth for everything below: the WIT contract `crates/ps-device/wit/ps-device.wit`,
and the host in `crates/ps-device/`.

## The model in one page

```text
 ┌──────────────── host (Plotter Studio) ────────────────┐       ┌─── plugin (WASM) ───┐
 │ parses G-code once      owns USB/serial/BLE/network   │       │ discover()          │
 │ streams lines in batches  asks the user for approval  │◀─────▶│ identify(conn)      │
 │ keeps progress, faults, permissions                   │ WIT   │ session: send(),    │
 │                                                       │       │   status(), pause…  │
 └───────────────────────────────────────────────────────┘       └─────────────────────┘
```

Design rules that shape everything:

1. **The host finds nothing by itself.** With no plugin installed no machine can be found.
   Each plugin looks for its own hardware in `discover`.
2. **The host owns all I/O.** A plugin never opens a port, socket or file. It is handed a
   `connection` and uses its `read`/`write` (etc.). It has no file system, no environment, no
   clock other than `host.now-ms`, and no network except through the host.
3. **Plugins never parse G-code.** Every `gcode-line` carries the exact text from the file
   *and* the host's parsed `step`, so two plugins cannot disagree about what a line means.
4. **Calls are plain blocking functions.** Each plugin runs on its own host thread, so waiting
   in `read` never stalls the app. No async runtime is needed in a plugin.
5. **Connection state is not the plugin's business.** The host owns the transports, so it
   knows when a cable is pulled even if the plugin is stuck.

## Lifecycle

1. **Refresh.** The user asks the host to look for machines. The host calls `discover` on every
   installed plugin (in parallel).
2. **Discover.** The plugin calls the `discovery` imports (`list-serial-ports`, `list-usb`,
   `scan-ble`, `browse-mdns`) and returns the **ids** of the candidates it wants to look at.
   It can narrow what it was shown but can never invent a device: an id it was not shown in
   this call is ignored.
3. **Filter.** The host drops anything outside the plugin's manifest `filters`.
4. **Identify.** For each remaining candidate the host opens a transport and calls
   `identify(conn)`. The plugin talks to the device just enough to confirm the model and
   returns `device-info`, or `none` if it is not a machine it drives.
5. **Approve.** The first time a plugin is used with a given device (by serial number) the user
   is asked, e.g. *"Allow Acme Cutter to use Acme Cutter 3000 on a serial port?"*
   The answer is stored per plugin and device and can be revoked.
6. **Connect.** The host constructs a `session` with the connection.
7. **Job.** `begin-job`, then repeated `send` batches, then `end-job`. The host runs the job
   loop; the plugin just executes lines and reports events.
8. **Disconnect.** `disconnect`, and the session is dropped.

## The contract (WIT)

Package `plotter:device@1.0.0`, world `device-plugin`: the plugin **imports** `transport`,
`discovery` and `host`, and **exports** `driver`.

### What you implement: `driver`

```wit
discover: func() -> list<string>;
identify: func(conn: borrow<connection>) -> option<device-info>;

resource session {
  constructor(conn: connection);
  info: func() -> device-info;
  capabilities: func() -> capabilities;
  status: func() -> status;
  wait-event: func(timeout-ms: u32) -> option<event>;
  begin-job: func(header: job-header) -> result<_, device-error>;
  send: func(lines: list<gcode-line>) -> result<u32, device-error>;
  end-job: func() -> result<_, device-error>;
  pause / resume / cancel: func() -> result<_, device-error>;
  user-action-done: func(id: u32) -> result<_, device-error>;
  jog: func(dx-mm: f64, dy-mm: f64) -> result<_, device-error>;
  disconnect: func();
}
```

| Call | What it must do |
|---|---|
| `discover` | List candidates via `discovery`; return the ids to consider. Must finish within the discovery limit (15 s). |
| `identify` | Probe the connection; return `device-info` only for a machine you drive. **Must be safe to run on a device that is not yours** (it may be someone's printer): send only harmless, read-only queries. |
| `new(conn)` | Establish the session (handshake, reset). Remember the transport kind. |
| `info`, `capabilities` | Static description. Do not do I/O if you can avoid it. |
| `status` | Current state; see below. Cheap and non-blocking in spirit. |
| `wait-event(timeout)` | Block up to `timeout-ms` for the next `event`; `none` on timeout. **Also your chance to do background work** (see [Timing](#timing-and-limits)). |
| `begin-job(header)` | Prepare for a job; `header` has `job-id`, `mat-index`, `profile-id` and the tool table (`slot`, `tool` id, `holder` id). |
| `send(lines)` | Accept lines **from the front of the batch** and return **how many you accepted**. Returning fewer is how you apply back-pressure; the host offers the rest again. |
| `end-job` | The host has sent everything; finish up. |
| `pause`/`resume`/`cancel` | Pause, continue, abort. **`cancel` is always honoured by the host** and must leave the machine in a safe state (tool up). |
| `user-action-done(id)` | The user confirmed a prompt you raised with `user-action-required`. |
| `jog(dx, dy)` | Move the carriage by an offset in mm (only if `capabilities.jog`). |
| `disconnect` | Release the machine. |

All fallible calls return a `device-error { code, message, recoverable }`. `code` is a short
stable string (e.g. `"grbl-alarm"`); `message` is shown to the user and should say what to do.

### Types you return

#### Device info

```wit
record device-info {
  model: string,
  serial-number: option<string>,
  firmware: option<string>,
  profile-id: option<string>,   // id of a machine profile in the library
  transport: transport-kind,
}
```

`profile-id` names the [machine profile](profiles.md) that describes this model, and it is a
**safety check**: when you report one, the host refuses to start a job that was compiled for a
different profile (`ProfileMismatch`: "machine reports X, job compiled for Y"). When you report
`none` no check is made, so the user could run a job compiled for the wrong machine. Report it
whenever the model is certain; leave it `none` for generic machines (the GRBL plugin does) where
any profile may apply. `serial-number` is what device approval is keyed on, so report a real
one when the hardware has one.

The host also refuses to start a job unless the machine is connected and `idle`.

#### Capabilities

Booleans for `pause`, `resume`, `cancel`, `jog`, `home`, `load-mat`, `unload-mat`,
`tool-detection`, `reports-position`, `reports-temperature`, and `max-inflight-lines: u32`. The
app offers controls according to these (for example, jogging is only offered when `jog` is
true), so do not claim what you do not implement.

`max-inflight-lines` is the **most lines the host offers in one `send` call** (minimum 1, so `0`
behaves as `1`). It is *not* a window of unfinished work: the host cannot see when a line
finishes, it only sees how many your `send` accepted. Real back-pressure is therefore your
`send` return value; see [Job flow](#job-flow-and-back-pressure). Pick a batch size your
machine's buffer can usually take whole.

#### Status

```wit
record status {
  power: power-state,                 // off | standby | on | unknown
  state: machine-state,
  progress: option<job-progress>,     // lines-done, lines-total, current-op
  mat-loaded: option<bool>,
  holders: list<holder-status>,       // which tool is detected in each holder
  position-mm: option<tuple<f64, f64>>,
  temperature-c: option<f32>,
  battery-percent: option<u8>,
  alerts: list<alert>,                // info | warning | error with code and message
}
```

Optional fields mean **"not reported"**, not "zero". `machine-state` is one of `idle`, `busy`,
`running-job`, `paused`, `waiting-for-user(user-action)`, `fault(device-error)`,
`updating-firmware`, `unknown`.

#### Events

Returned by `wait-event`:

| Event | When |
|---|---|
| `status-changed(status)` | Anything in the status changed. |
| `user-action-required(user-action)` | The machine needs a human (load a tool, load/unload the mat). `user-action` is `{ id, message }`; the host shows the message and later calls `user-action-done(id)`. |
| `job-finished` | The machine has completed the job. |
| `fault(device-error)` | The machine failed. The host stops the job and **never resends lines by itself** (repeating a move could cut twice); the user decides how to continue. |

### What the host gives you

#### `transport` — the connection

A `connection` resource, already open to the approved device:

| Method | Use |
|---|---|
| `kind()` | How it is carried (usb, serial, ble, tcp, udp, websocket, http). |
| `write(data)` / `read(max-bytes, timeout-ms)` | Byte stream (serial, USB, TCP). `read` returns whatever arrived within the timeout; an empty list means nothing arrived. |
| `ble-write(characteristic, data)` / `ble-notify(characteristic, timeout-ms)` | Bluetooth LE. |
| `send-datagram` / `recv-datagram(timeout-ms)` | UDP. |
| `http(req)` | HTTP request relative to the machine. |

Errors are `io-error { kind, message }`. Network connections are **scoped**: a TCP, UDP or
WebSocket connection goes only to the approved machine's address and declared ports; HTTP paths
are relative to that machine. `open-declared-host(name, kind)` reaches an internet host, but
**only** one named in your manifest's `internet_hosts` **and** allowed by the user.

#### `discovery` — what is on the buses

Only meaningful inside `discover`:

| Function | Returns |
|---|---|
| `list-serial-ports()` | Serial ports, with USB vendor/product ids and product string when known. |
| `list-usb()` | USB devices. |
| `scan-ble(listen-ms)` | BLE advertisers. |
| `browse-mdns(service-type, listen-ms)` | mDNS services; `service-type` is a plain name such as `_plotter._tcp.local.`, local network only. |

Each returns `candidate { id, name, transport, usb: option<(vid, pid)>, address, serial }`.
**A bus your manifest does not name returns an empty list and is not touched** (no radio is
powered on, no socket opened). Listen times are capped at 3 s.

#### `host` — utilities

| Function | Notes |
|---|---|
| `log(level, message)` | Level 0–1 error, 2 warn, 3 info, anything else debug. Goes to the app's log. Never log secrets. |
| `now-ms()` | Milliseconds since the Unix epoch. |
| `sleep(ms)` | Blocks; capped at 1 s per call. |
| `kv-get(key)` / `kv-set(key, value)` | A tiny key–value store: up to 64 keys, values up to 64 KiB (larger or extra writes are silently dropped). **Currently it is in memory and per instance, not per plugin:** the host creates a fresh, empty store for each `discover` call, each `identify` call and each connection (session), so a value set in `discover` is not visible in `identify`, and nothing survives the connection or an app restart. The WIT comment describes a per-plugin store kept in the library folder; that is not what the host does today, so do not rely on it. |

## The G-code dialect

Plugins receive `ps-gcode/1`, a small fixed dialect. The host guarantees that every line you
get is already valid (anything outside the dialect is refused before a job starts).
`gcode-line` is `{ number, text, step }` with `number` 1-based. **Coordinates are integer
micrometres, exactly as written** in the file (3-decimal millimetres, no floats), and **the feed
is modal in the file but resolved for you**: every `line` and `arc` step carries the feed in
force, in mm/min.

| G-code | `step` | Meaning |
|---|---|---|
| `; text` | `comment` | Comment (no action). |
| `G20` / `G21` | `units(bool)` | `true` = millimetres. |
| `G90` | `absolute` | Absolute coordinates (the only mode). |
| `T<n> M6` | `tool-change(slot)` | Select the tool in slot `n`; slots are defined by the job header's tool table. |
| `M0 (message)` | `pause(message)` | Stop and wait for the user. The host turns it into a prompt and continues after the answer. |
| `M104 S<c>` / `M109 S<c>` | `temperature{celsius, wait}` | Set a heater; `M109` waits. Only emitted for `heated` machines. |
| `G4 P<ms>` | `dwell(ms)` | Wait. |
| `M3 S<force>` | `tool-down(force)` | Lower the tool. `force` is in **machine units** (the profile's `units_per_gf` already applied). |
| `M5` | `tool-up` | Raise the tool. |
| `G0 X Y` | `travel(point)` | Rapid move, tool up. |
| `G1 X Y [A] F` | `line{to, a-mdeg, feed-mm-min}` | Drawn line; `A` is a tangent angle in milli-degrees, only for machines with a tangential axis. |
| `G2/G3 X Y I J F` | `arc{to, centre-offset, ccw, feed-mm-min}` | Drawn arc; `I`/`J` are the centre relative to the start. Only if the profile has `arcs`. |
| `M2` | `end` | End of program. |

Your translation of each step is the whole job of the plugin. For steps your machine has no
equivalent for, ignore them deliberately (GRBL ignores `comment`, `tool-change`,
`temperature`) or raise a `device-error`; never guess. If your machine cannot do arcs, set
`gcode.arcs: false` in its profile and the compiler will not send any.

## Job flow and back-pressure

The host parses the program once, calls `begin-job`, and streams lines with `send` in batches.
Each batch starts at the host's cursor and is at most `max-inflight-lines` long. Your `send`
returns how many lines you accepted from the **front** of the batch; the host advances its
cursor by exactly that many and offers the remainder on its next turn (about every 10 ms while
lines remain, interleaved with a `wait-event` poll). The cursor is the source of truth for
progress, so **reported progress means "accepted by the plugin", not "executed by the machine"**:
a plugin that accepts a whole batch into a deep buffer makes progress run ahead of the
machine. Return fewer than you were given (or `0`) when your buffer is full.

- A `pause` line becomes a user prompt; the job continues after the answer.
- A fault stops the job. Lines are never resent automatically.
- A device that gives no reply to an event poll for **5 s** is marked lost.
- A driver call that takes longer than **2 s** is treated as lost.
- **Cancel is always honoured.** Make it leave the tool raised.

For machines that acknowledge each line (like GRBL's `ok`) use the SDK below rather than
writing flow control yourself.

## Timing and limits

The host enforces these so a broken plugin cannot freeze the app:

| Limit | Value | What happens on breach |
|---|---|---|
| Any single call (`identify`, `send`, `status`, `wait-event`…) | **2 s** | Treated as lost; the call is interrupted. |
| `discover` | **15 s** (it waits on bus scans, which count) | Interrupted; the plugin finds nothing this refresh. |
| Plugin memory | **64 MiB** | The call fails. |
| `.psdevice` size (the component) | **16 MiB** | Package refused. |
| `host.sleep` | 1 s per call | Clamped. |
| Discovery listen time | 3 s | Clamped. |

The 2 s limit shapes the design of anything slow. If one logical step can take longer than 2 s
(a handshake, a long move), **do it in slices from `wait-event`**: start a bit of work, return
within your budget, and carry on on the next call. A plugin with a slow handshake can keep a
step-driven engine and let `wait-event` run steps for at most a few hundred milliseconds per call. No call may spin: epoch interruption cuts off
a plugin stuck in a loop.

The sandbox has **no ambient authority**: nothing is pre-opened, inherited or opened, so the
plugin cannot read files, environment variables or reach the network except through the
imports above. `plugins/rogue` exists to prove this (it tries to read host files, allocate
huge memory, hang, and invent devices) and the host tests check that each attempt fails.

## The manifest

`manifest.json` declares who you are and what you may touch:

```json
{
  "id": "plotter-studio.grbl",
  "name": "GRBL pen plotter",
  "version": "1.0.0",
  "filters": [
    { "transport": "serial" }
  ],
  "internet_hosts": []
}
```

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | Stable unique id, the key for permissions. Never change it after release, or users lose their approvals. |
| `name` | yes | Shown to the user (in the approval prompt, the device list). |
| `version` | yes | The plugin's own version. |
| `filters` | yes | The buses and devices the plugin may **see**. A device outside every filter is dropped from the host's list even if you return it from `discover`, and a bus not named is never listed to you. May be `[]` (then the plugin sees nothing). |
| `internet_hosts` | no | Internet host names the plugin may contact (for machines reached through a vendor cloud). Each is **blocked until the user allows it**; the host connects only to those, public HTTPS only. Defaults to none. |

A **filter** is `{ "transport": "<kind>" }`, optionally with `"usb": [vendorId, productId]`
(decimal numbers in JSON) to match only that USB device. Transport names:
`usb`, `serial`, `ble`, `tcp`, `udp`, `websocket`, `http`. An unknown name makes the manifest
invalid. For `usb` without `usb`, any USB device matches.

Be narrow. A user reads what a plugin may see; declaring only what you need is part of earning
trust. A plugin that declares only `serial` can pick its own port in `discover` by USB
vendor id or by product name, so it never opens other people's ports.

## The SDK (`ps-device-sdk`)

`crates/ps-device-sdk` has no dependencies, so it builds for `wasm32-wasip2` as well as the
host. It provides the parts every driver would otherwise rewrite:

| Item | Purpose |
|---|---|
| `LineBuffer` | Splits a byte stream into lines. Transports deliver arbitrary chunks; this holds the partial line until its newline arrives. Drops empty lines, replaces bad UTF-8, caps an unfinished line (4096 bytes by default). |
| `AckFlow` | Character-counting flow control for machines that acknowledge each line (GRBL `ok`): a fixed receive buffer, each sent line costs its length + 1 (newline), each acknowledgement frees the oldest. `can_send(len)`, `sent(len)`, `acked()`, `in_flight()`, `reset()`. |
| `xor_checksum`, `sum_checksum` | Marlin-style XOR and byte-sum checksums. |
| `numbered_line(n, line)` | `N3 G1 X1*45`: a numbered, checksummed line. |

Arc linearisation (turning arcs into short lines) lives in `ps-gcode` (`linearise`).

## Writing a plugin

### 1. Create the crate

```text
plugins/acme/
├── Cargo.toml
├── manifest.json
└── src/lib.rs
```

`Cargo.toml` (copy `plugins/grbl/Cargo.toml`): a `cdylib`, named `<folder>-plugin`, depending on
`wit-bindgen`, and on `ps-device-sdk` if you want it. The crate name matters: the packaging
tool builds `-p <folder>-plugin`. The generated bindings contain `unsafe`, so the plugin crate
opts out of the workspace's `forbid(unsafe_code)`; write none yourself.

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
wit-bindgen = "0.57"
ps-device-sdk.workspace = true

[lints.rust]
unsafe_code = "allow"
```

### 2. Generate bindings and implement `driver`

```rust
wit_bindgen::generate!({
    path: "../../crates/ps-device/wit",
    world: "device-plugin",
});

use exports::plotter::device::driver::{Guest, GuestSession};
use plotter::device::transport::Connection;
use plotter::device::types::{DeviceInfo, GcodeLine, JobHeader, Step /* … */};

struct Component;
impl Guest for Component {
    type Session = Session;
    fn discover() -> Vec<String> { /* list-serial-ports, filter, map to ids */ }
    fn identify(conn: &Connection) -> Option<DeviceInfo> { /* harmless probe */ }
}
struct Session { /* state in RefCell, as methods take &self */ }
impl GuestSession for Session { /* constructor and all calls */ }
export!(Component);
```

Session methods take `&self`, so keep mutable state in a `RefCell`. See `plugins/grbl/src/lib.rs`
for a complete, short driver: a `translate(step) -> Option<String>` function that maps each
`step` to a GRBL line, an `AckFlow` for the 127-byte buffer, and a `LineBuffer` for replies.

### 3. Write the manifest

See [The manifest](#the-manifest). Choose the narrowest `filters` that work.

### 4. Provide a machine profile

Your machine needs a [profile](profiles.md) so the compiler knows its work area, holders and
limits, and your `device-info.profile-id` must name it. Add it to
`crates/ps-library/builtin/profiles/` (and `builtin.rs`) for a built-in machine, or ship it as a
library entry for others. Add tools/materials if the machine has its own.

### 5. Build and package

Requires the target once: `rustup target add wasm32-wasip2`.

```sh
cargo xtask package          # builds every plugin that has a manifest.json and packs it
# -> target/packages/acme.psdevice
```

`xtask package` builds `-p <folder>-plugin --target wasm32-wasip2 --release --locked`, requires
`manifest.json` to have `id`, `name`, `version` and `filters`, and writes a zip with fixed
timestamps (reproducible builds).

A `.psdevice` file is a plain zip containing exactly:

```text
manifest.json    the manifest (limit 1 MiB)
plugin.wasm      the component (limit 16 MiB)
```

### 6. Test it without hardware

```sh
# Replay a recorded session: it must identify the device and write exactly what was recorded
cargo run -p ps-cli -- device test target/packages/acme.psdevice --transcript session.json

# Run a whole job against a simulated GRBL-style serial machine
cargo run -p ps-cli -- device test target/packages/acme.psdevice --simulate
```

You must give at least one of the two flags. Both can be combined; the command prints what it
found and exits non-zero on any failure.

- **`--transcript`** replays recorded bytes. The plugin must `identify` the device, and every
  byte it writes must match the recording in order (`the plugin wrote … but the recording has
  …` on a mismatch). Record one from a real session using the app's opt-in transcript setting
  (bytes in and out stay on your machine; nothing is uploaded). Format:

  ```json
  { "entries": [ { "t_ms": 0, "dir": "out", "hex": "0a0b" },
                 { "t_ms": 12, "dir": "in",  "hex": "6f6b0a" } ],
    "hosts": [] }
  ```

  `dir` is `out` (host to machine) or `in` (machine to host); `hex` is the raw bytes.
- **`--simulate`** runs a fixed little job (units, pause, tool change, a move, a drawn line)
  against a simulated serial machine that **answers like GRBL**: a banner after `Ctrl-X`, a
  status for `?`, `ok` for every line. It is therefore a real test only for GRBL-style
  plugins; a plugin for another protocol will fail it, and should be tested with a
  transcript instead, or against a protocol simulator of its own through the public
  `ps-device` API (`WasmDriver`, `MockTransport`, `run_job`).

Also add tests in `crates/ps-device` for non-trivial plugins, following `wasm/tests.rs`.

### 7. Install

Copy the `.psdevice` into the plugins folder under the data directory
(`ps_library::default_plugins_dir()`; on Windows `%APPDATA%\PlotterStudio\data\plugins\`). A
plugin built before the `discover` export existed will not load; rebuild with the current WIT.
Then press refresh in the app.

### Plugins in their own repository

A plugin does not have to live in this repository, and a vendor's plugin should not: it needs
only the WIT contract and (optionally) `ps-device-sdk`. A typical out-of-tree layout:

```text
my-plugin/
  wit/ps-device.wit        a copy of crates/ps-device/wit/ps-device.wit at the version you target
  plugin/Cargo.toml        a cdylib; wit_bindgen::generate!({ path: "../wit", world: "device-plugin" })
  plugin/manifest.json     id, name, version, filters, internet_hosts
  library/                 optional profile / tool / material JSON, installed through the app's
                           library import (plugins cannot bundle library entries)
```

Build with `cargo build --target wasm32-wasip2 --release`, pack `manifest.json` and the component
as `plugin.wasm` into a zip named `<id>.psdevice` (this repository's `cargo xtask package` shows
how), and check it with `pstudio device test`. To test against the real host runner, depend on
`ps-device` from an integration test, as `ps-device`'s own plugin tests do.

On Linux, a plugin for a USB device usually needs a udev rule so the logged-in user can open it;
ship that with the plugin, not with the application.

## Safety rules for plugin authors

- **Never move the machine in `identify`.** It runs against devices that may not be yours.
- **Never repeat a move on your own** after an error or timeout: a repeated cut cuts twice.
  Report a `fault` and let the user decide.
- **Honour `cancel` promptly** and leave the tool up.
- **Do not claim capabilities you do not implement.**
- **Refuse what you cannot do safely** (a step with no equivalent, a force above a hard limit)
  with a `device-error` rather than approximating.
- For protocols with **firmware or configuration writes** (EEPROM, flash), do not expose them
  at all. A plugin should refuse such opcodes in one central guard, and must
  never provide an escape hatch around that kind of guard.
- Do not log secrets (session keys, credentials).
- Keep the manifest narrow, and prefer no `internet_hosts`.

## Permissions model (what the user sees)

| Thing | Rule |
|---|---|
| Using a device | Asked the first time per *(plugin id, device serial)*; remembered; revocable. |
| Internet host | Declared in the manifest, **blocked until allowed** per *(plugin id, host)*; revocable. |
| Self-signed machines | The host pins the certificate fingerprint on the first approved connection, per *(plugin, serial)*; a later, different fingerprint stops the connection and warns the user. |
| Everything else | Not available. |

Approvals are stored by the host in a JSON file (`PermissionStore`), written atomically.

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| Plugin does not appear / will not load | Built before the `discover` export; wrong or missing `manifest.json` fields; component over 16 MiB; unknown transport name in `filters`. |
| Device never listed | Manifest `filters` do not include its transport, so the host never shows the bus or drops the device; `discover` returned an id it was not shown in that call; `identify` returned `none`. |
| Device listed but "not approved" | Expected the first time: the user must approve it. |
| Job dies with "lost" | A call exceeded 2 s, or no reply for 5 s. Slice slow work across `wait-event` calls and reply promptly. |
| `--simulate` fails for a non-GRBL plugin | Expected: the simulator is GRBL-shaped. Use `--transcript`. |
| Stored values vanish | `kv` is per call/connection and in memory only (see above); over 64 keys or 64 KiB, writes are dropped silently. |
