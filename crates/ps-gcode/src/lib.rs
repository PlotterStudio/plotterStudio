//! Plotter Studio G-code output.
//!
//! The writer is a thin, deterministic serialiser of the job IR: it makes no geometric decisions.

mod bundle;
mod check;
mod linearise;
mod number;
mod parse;
mod preview;
mod write;

pub use bundle::{BundleInput, BundleMat, BundleWriteError, export_gcode, write_bundle};
pub use check::{ValidationError, Violation, ViolationKind, check, ensure_valid};
pub use linearise::linearise;
pub use number::{mdeg, pause_text, um, um_trimmed};
pub use parse::{
    Command, ParseError, ParseState, ParsedLine, end_line, parse_line, parse_program, pause_lines,
};
pub use preview::svg_preview;
pub use write::{Gcode, LineInfo, WriteContext, write_mat};
