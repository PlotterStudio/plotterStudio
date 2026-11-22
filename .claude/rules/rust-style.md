---
paths:
  - "**/*.rs"
---

# Rust style and correctness (all crates)

Sources: Rust API Guidelines (C-*), Microsoft Pragmatic Rust Guidelines (M-*), Clippy.

## Errors and panics
- Fallible operations return `Result`. Library code uses the crate's `thiserror` `Error`;
  binaries may use `anyhow` with `.context(...)` (M-APP-ERROR).
- A panic means "this is a bug": malformed compile-time constant, broken internal invariant
  (M-PANIC-ON-BUG, M-PANIC-IS-STOP). Never panic on input from users, files, or the machine.
- `unwrap()` is fine in tests. Elsewhere prefer `?`, `let ... else`, `ok_or(...)`, or
  `expect("why this cannot fail")` for true invariants, plus a `# Panics` doc on public fns.
- Mutex locks: `lock().unwrap_or_else(PoisonError::into_inner)`; don't cascade panics.

## Types and conversions
- No lossy `as` casts. Use `From`/`TryFrom`, `u8::try_from(x)?`, `checked_*`/`saturating_*`.
  If a narrowing cast is the intent (e.g. f64 → f32 wire format), use
  `#[expect(clippy::cast_possible_truncation, reason = "...")]` on the smallest scope.
- Parse user input directly into the target type (`"300".parse::<u16>()`), never parse wide and cast.
- Prefer slices/patterns over indexing: `first_chunk::<N>()`, `split_first_chunk`, slice patterns.
- Public types derive or implement `Debug` (M-PUBLIC-DEBUG); hand-write it to hide secrets.
- Make "invalid states unrepresentable": enums over booleans/magic numbers; newtypes for units
  when mixing them is a real risk. Document magic values (M-DOCUMENTED-MAGIC).

## Lints
- The workspace enables `clippy::pedantic` plus selected restriction lints; CI uses `-D warnings`.
- Fix the code first. If an override is justified: `#[expect(clippy::x, reason = "...")]` on the
  narrowest item (M-LINT-OVERRIDE-EXPECT). `#[allow]` is itself a lint error here.
- Never change `[workspace.lints]` to silence a single site.

## API shape
- Builders: `#[must_use]` methods taking `mut self` and returning `Self`.
- Accept borrowed/generic inputs (`&str`, `&[T]`, `impl AsRef<Path>`); return owned data.
- Keep items reachable through one public path (M-SINGLE-ITEM-PATH); re-export at the crate root.
- Names are short and precise, free of weasel words like `Manager`/`Helper` (M-SHORT-NAMES,
  M-WEASEL-WORDS).

## Async (Tokio)
- Never block in async code (no `std::thread::sleep`, no blocking I/O); use `tokio::time`.
- Every wait that depends on hardware or a user has a timeout or a cancellation path
  (`CancellationToken`), and cancellation leaves the machine in a safe state.
- Don't hold a `std::sync::Mutex` guard across `.await`.

## Docs and logging
- `///` on every new or changed public item; first sentence is one line (M-FIRST-DOC-SENTENCE).
  `//!` module docs. (`missing_docs` isn't enforced yet; document older items as you touch them.)
- Library logs via `tracing` with fields (`tracing::debug!(op = ..., "msg")`); no `println!`.
- Comments explain *why*, not what. No commented-out code.
