# CLAUDE.md

**Plotter Studio**: an open-source desktop application that prepares drawings for cutting
plotters and pen plotters (import SVG/DXF, assign tools and materials, compile to G-code, stream
to the machine) and a headless CLI (`pstudio`). Machines are supported through sandboxed
WebAssembly **device plugins**; support for any one vendor's machine lives in that plugin's own
repository, never in this one.

## Layout (Cargo workspace)

All crates are siblings under `crates/` and inherit metadata, dependency versions and lints
from the root `Cargo.toml` (`[workspace.package]`, `[workspace.dependencies]`, `[workspace.lints]`).

| Crate | Kind | Purpose |
|---|---|---|
| `crates/ps-app` | binary (`plotter-studio`) | The GUI application. |
| `crates/ps-cli` | binary (`pstudio`) | Headless export, validation, inspection and job running. |
| `crates/ps-core`, `ps-geometry`, `ps-model`, `ps-import`, `ps-compiler`, `ps-gcode`, `ps-job`, `ps-library`, `ps-nest`, `ps-render`, `ps-net` | libraries | Foundation types, geometry, document model, importers, compiler, G-code, job bundles, tool/material/profile libraries, nesting, rendering, network feeds. |
| `crates/ps-device` | library | Device host: loads plugins, streams jobs, permissions. |
| `crates/ps-device-sdk` | library | Helpers for plugin authors (also builds for `wasm32-wasip2`). |
| `crates/ps-fuzz`, `fuzz/` | libraries / cargo-fuzz | Fuzz entry points for the untrusted-file parsers. |
| `plugins/grbl` | WASM plugin | The GRBL reference plugin. |
| `plugins/rogue` | WASM plugin | A deliberately misbehaving plugin used only to test the sandbox. |
| `xtask` | binary | Repository chores (boundary checks, schemas, goldens, packaging). |

**No vendor-specific code, ids, names or fixtures belong in this repository.** Tests and docs use
the `generic.*` built-ins or an invented `acme` vendor. Machine support for a specific vendor is a
plugin (plus optional library JSON) in its own repository, built against the WIT contract in
`crates/ps-device/wit/ps-device.wit`.

## Commands

```sh
cargo build --workspace
cargo test-all          # = cargo test --workspace (unit + integration + doc tests)
cargo lint              # = clippy --workspace --all-targets -- -D warnings
cargo fmt --all         # format (a hook also formats each .rs file Claude edits)
cargo fmt-check         # CI's format check
cargo docs              # docs; CI uses RUSTDOCFLAGS="-D warnings"
cargo test -p ps-compiler <filter>   # narrow test run
```

Toolchain is pinned in `rust-toolchain.toml` (1.100.0); MSRV is `rust-version` in `Cargo.toml`.
Optional tools: `cargo deny check` (licences/advisories, `deny.toml`).

## Definition of done (run before saying a change is finished)

1. `cargo fmt-check` clean
2. `cargo lint` clean — **zero warnings**; pedantic clippy is on for the whole workspace
3. `cargo test-all` green, with new behaviour covered by tests
4. `RUSTDOCFLAGS="-D warnings" cargo docs` clean when public items changed

The `/rust-quality-gate` skill runs exactly this. Report failures honestly with their output;
never weaken a lint, delete a test, or add `#[ignore]` to get green.

## Hardware safety

- Real machines move. Never run a plugin or `pstudio run` against physical hardware unless the
  user asks for a hardware test and confirms they are at the machine.
- Device plugins must refuse firmware/EEPROM-style writes and must not provide escape hatches
  around such guards (see `docs/plugins.md`).

## Rust conventions (summary; details in `.claude/rules/`)

- **Edition 2024**, stable Rust only, `unsafe_code = "forbid"`.
- **Errors:** libraries use typed errors (`thiserror`, the crate's `Error` enum); applications
  may use `anyhow`. No `unwrap`/`expect` in library code paths that can fail at runtime — return
  `Result`. A panic is only for a genuine bug (e.g. a malformed compile-time constant), and a
  public function that can panic documents it under `# Panics`.
- **Lint overrides:** use `#[expect(lint, reason = "...")]`, never `#[allow]` (enforced by the
  `allow_attributes` lint). Prefer fixing the code over silencing the lint.
- **No lossy `as` casts:** use `From`/`TryFrom`, checked/saturating arithmetic, or an `#[expect]`
  with a reason when the conversion is the point (e.g. the f32 wire format).
- **Public types** implement `Debug` (hand-written when they hold secrets — never print keys).
- **Logging:** `tracing` with structured fields in the library; no `println!` outside binaries.
- **Docs:** new and changed public items get a doc comment whose first sentence stands alone;
  modules start with a `//!` overview. (`missing_docs` isn't enforced yet — some older items
  lack docs; fill them in when you touch them.)
- **Tests** assert ground truth (golden files, simulator behaviour), not a
  re-statement of the implementation. Async tests use `#[tokio::test(start_paused = true)]`.

## Dependencies

Prefer well-tested, widely used crates over hand-written code (the user's explicit preference).
Before writing a general-purpose component (parsing, geometry, protocols, CLI parsing, …) use
the `/evaluate-crate` skill: compare candidates on downloads, maintenance, API fit and licence,
then add the winner to `[workspace.dependencies]` and reference it with `.workspace = true`.
Only hand-write when nothing suitable exists, and say so. Current examples: path
simplification uses `simplify-polyline`, nesting uses `jagua-rs`, SVG parsing uses `usvg`.

## Git

- Default branch is `main`. Don't commit, push, or open PRs unless asked; branch off `main` first.
- Commit messages: imperative summary line, body explaining *why*.

## Documentation for extenders

`docs/` is the reference for people writing **device plugins** (`docs/plugins.md`) and
**tools, materials and machine profiles** (`docs/tools.md`, `materials.md`, `profiles.md`,
`libraries.md`). Update the matching page in the same change when you alter the WIT contract
(`crates/ps-device/wit/ps-device.wit`), the plugin manifest or package format
(`ps-device/src/package.rs`), the library model (`ps-library/src/model.rs`) or its validation
(`ps-library/src/schema.rs`), the compiler's parameter names (`ps-compiler/src/prepare.rs`), or
the G-code dialect (`ps-gcode/src/parse.rs`). The docs state which rules are *not* validated;
if you add a check, remove that caveat.

## Where things live for Claude

- `.claude/rules/*.md` — path-scoped rules (Rust style, testing).
- `.claude/skills/` — `/rust-quality-gate`, `/evaluate-crate`.
- `.claude/settings.json` — permissions (`--example`/`--bin` and bare `cargo run` always ask; `cargo run -p ps-*` and all build/test/lint commands are pre-approved), the
  format-on-edit hook, and the `rust-analyzer-lsp` plugin (live diagnostics after each edit;
  `rust-analyzer` comes with the pinned toolchain).

Reference guidelines: [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/),
[Microsoft Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/)
(IDs like `M-PANIC-ON-BUG` are cited in the rules), and the
[Clippy lint list](https://rust-lang.github.io/rust-clippy/).
