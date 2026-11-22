//! Checks on the repository's CI and release files: the three workflows, the release
//! profile, the packager configuration and the changelog. They are tests rather than code because
//! the files are the product; `cargo test -p xtask` fails when one drifts from the design.

#[cfg(test)]
mod tests;
