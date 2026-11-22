---
paths:
  - "**/tests/**"
  - "**/*.rs"
---

# Testing

- Tests assert **ground truth**, not the implementation restated (M-TAUTOLOGICAL-TESTS):
  golden files, hand-computed geometry, observed hardware behaviour.
- Unit tests live beside the code in `#[cfg(test)] mod tests`; cross-module flows go in
  `crates/<crate>/tests/`. Public examples double as doc tests where they help.
- Async tests: `#[tokio::test(start_paused = true)]` so timeouts and sleeps run instantly and
  deterministically. Never use real wall-clock sleeps in tests.
- Hardware is never touched by `cargo test`. Flows are tested against simulated machines;
  make the simulator reproduce the real behaviour first, then test the code against it.
- A bug fix starts with a test that fails for the bug (regression test), then the fix.
- Avoid timing races: drive the simulator deterministically (e.g. `drop_vectors`, `load_mat()`
  from an event handler) rather than "sleep and hope".
- Float comparisons use a tolerance; compare in f64 (`f64::from(f32)`), not by casting down.
- Keep tests fast (whole suite in seconds). Name tests after the behaviour they pin down.
