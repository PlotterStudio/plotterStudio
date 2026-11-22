---
name: evaluate-crate
description: Evaluates and selects an existing Rust crate before writing a general-purpose component by hand (parsing, geometry, serialization, CLI args, GUI, async utilities, crypto, etc.), then adds it to the workspace correctly. Use whenever new functionality might already exist as a crate, or the user asks which crate to use.
argument-hint: "[what is needed]"
---

# Evaluate a crate before hand-writing code

The user strongly prefers well-tested, widely used crates over custom implementations.

## 1. Find candidates
- `cargo search <keywords> --limit 15` with several phrasings.
- Metadata: `curl -s -A "crate-research" https://crates.io/api/v1/crates/<name>` (downloads,
  recent downloads, `updated_at`, `max_version`). Don't send anything about the user or project.

## 2. Compare (keep the top 2–3)
| Criterion | What good looks like |
|---|---|
| Adoption | high total and recent downloads; used by known crates |
| Maintenance | release within ~12 months; issues answered |
| API fit | does what we need through a **public** API (read the source in `~/.cargo/registry/src`) |
| Correctness | own test suite; handles our edge cases (probe them in a scratch crate) |
| Licence | allowed by `deny.toml` (MIT/Apache-2.0/BSD/ISC/Zlib/MPL-2.0/Unicode-3.0) |
| Weight | few heavy transitive deps; `default-features = false` possible |
| MSRV/edition | builds on the pinned toolchain; compatible with existing deps |

Probe in the session scratchpad, not in the repo: a throwaway crate that exercises the candidate
on real inputs from this project (e.g. tricky G-code lines).

## 3. Decide and report
Give the user a short comparison table and a recommendation with the deciding reason. If nothing
fits, say so explicitly and why, then hand-write the smallest component, with tests.

## 4. Add it properly
- Version once in root `Cargo.toml` `[workspace.dependencies]` (minimal features).
- In the crate: `name.workspace = true` (add `features = [...]` there if crate-specific).
- Library crates: never depend on app-only crates (`anyhow`, GUI); don't leak the dependency's
  types through the public API.
- Run `/rust-quality-gate`; run `cargo deny check` if available.
