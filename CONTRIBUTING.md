# Contributing

Thanks for helping. A few ground rules keep the project easy to maintain.

## Before you open a pull request

Run the same checks CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings   # zero warnings; pedantic lints are on
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo xtask check-boundaries && cargo xtask check-schemas
```

You need the `wasm32-wasip2` target (`rustup target add wasm32-wasip2`): some tests build the
plugins.

- New behaviour comes with tests that assert ground truth (hand-computed values, golden files,
  simulator behaviour), not a restatement of the implementation. A bug fix starts with a test
  that fails for the bug.
- Golden files are compared byte for byte. If a change to the output is intended, run
  `cargo xtask goldens --bless` and explain the diff in the pull request.
- No `unwrap`/`expect` on paths that can fail at runtime in library crates; use typed errors.
  Silence a lint only with `#[expect(lint, reason = "...")]`.
- Public items get doc comments. If you change the plugin contract, the library model or the
  G-code dialect, update the matching page in [`docs/`](docs/) in the same change.
- Prefer a well-maintained crate over hand-written code for general-purpose components.

## Machine support belongs in plugins

Support for a specific vendor's machine is a [device plugin](docs/plugins.md) in its own
repository, not a change to this one. This repository contains no vendor-specific code, ids or
fixtures: tests and documentation use the `generic.*` built-ins or an invented "acme" vendor.

## Hardware safety

Machines move and blades cut. Never run a change against real hardware without being at the
machine, and describe what you tested on in the pull request. Plugins must refuse firmware or
configuration writes; see the safety rules in [docs/plugins.md](docs/plugins.md).

## Licence

By contributing you agree that your contribution is licensed under the [MIT licence](LICENSE).
