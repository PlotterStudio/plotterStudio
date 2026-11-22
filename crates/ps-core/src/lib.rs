//! Plotter Studio foundation crate.
//!
//! Everything here is pure and headless: identifiers, physical-unit newtypes, the integer µm
//! grid, diagnostics and content hashing. It has no workspace dependencies, so every other
//! crate can build on it.

mod diagnostics;
mod grid;
mod hashing;
mod ids;
mod libref;
mod safezip;
mod units;

pub use diagnostics::{Diagnostic, Diagnostics, L10nKey, Location, Severity};
pub use grid::{
    GridError, PtUm, format_mdeg, format_um, mdeg_from_degrees, mm_from_um, um_from_mm,
};
pub use hashing::{ContentHash, ParseHashError, StableHasher, hash_bytes};
pub use ids::{
    AssetId, AttachSetId, GroupId, MatId, ObjectId, OpId, OpRowId, ParseIdError, ParseKeyError,
    PathKey,
};
pub use libref::{LibRef, LibRefError};
pub use safezip::{ZipError, ZipLimits, is_safe_path, read_archive};
pub use units::{Celsius, Degrees, GramsForce, Millis, Mm, MmPerSec, MmPerSec2, tolerance};
