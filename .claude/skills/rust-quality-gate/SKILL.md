---
name: rust-quality-gate
description: Runs this workspace's full Rust quality gate (rustfmt check, clippy pedantic with -D warnings, all tests, rustdoc with -D warnings) and reports results. Use before declaring any code change done, before a commit or PR, or when the user asks to check, verify, lint or test the code.
allowed-tools: Bash(cargo fmt *), Bash(cargo clippy *), Bash(cargo test *), Bash(cargo doc *), Bash(cargo lint*), Bash(cargo fmt-check*), Bash(cargo test-all*), Bash(cargo docs*), Bash(cargo deny *)
---

# Rust quality gate

Run every step, in order, from the repository root, even if an earlier one fails, so the report
is complete. Do not touch hardware (no harness, no examples against a COM port).

1. **Format**: `cargo fmt --all --check`
   - On failure: run `cargo fmt --all`, then re-check. Formatting fixes are always safe.
2. **Lint**: `cargo clippy --workspace --all-targets -- -D warnings`
   - Fix the code. Only if a lint is genuinely wrong for a site, add
     `#[expect(clippy::<lint>, reason = "<why>")]` on the narrowest item. Never `#[allow]`,
     never edit `[workspace.lints]` to silence one site.
3. **Tests**: `cargo test --workspace`
   - A failing test is information. Find the cause; don't delete, `#[ignore]`, or loosen the
     assertion unless the test itself is wrong, and then say so explicitly.
4. **Docs** (when public items changed): `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`
   - In PowerShell: `$env:RUSTDOCFLAGS="-D warnings"; cargo doc --workspace --no-deps`
5. **Supply chain** (only if `cargo deny --version` works; skip silently otherwise):
   `cargo deny check`

## Also check by reading the diff

- New public items have `///` docs; modules touched have `//!` docs.
- No new `unwrap()`/`expect()` on runtime-fallible paths in library code; no `println!` in the library.
- No lossy `as` casts; no secrets in `Debug`/logs.
- New behaviour has tests; bug fixes have a regression test.
- Protocol changes followed the `/protocol-change` checklist.

## Report

A short table, one row per step: pass/fail, plus the first relevant error lines for failures.
State plainly what was fixed and what remains. Never claim "all green" unless every step ran
and passed in this session.
