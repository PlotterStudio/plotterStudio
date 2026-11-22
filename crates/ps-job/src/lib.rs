//! Plotter Studio job IR and `.psjob` bundle format.
//!
//! Shared by the G-code writer (`ps-gcode`) and the device host (`ps-device`). The IR
//! ([`Job`], [`MatJob`], [`Step`]) is what the compiler produces; the manifest and bundle
//! reader describe and safely open the exported `.psjob` file.

mod bundle;
mod ir;
mod manifest;

pub use bundle::{Bundle, BundleError};
pub use ir::{
    Bounds, Estimate, HolderId, Job, MatJob, PauseMsg, PenVariant, SrcRef, Step, ToolEntry,
};
pub use manifest::{
    Manifest, ManifestError, MatEntry, PauseRow, SCHEMA_VERSION, gcode_path, is_bundle_file,
    preview_path, schema, sha256_hex,
};
