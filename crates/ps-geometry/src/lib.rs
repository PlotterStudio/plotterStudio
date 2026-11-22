//! Plotter Studio geometry engine.
//!
//! Geometry is kept as exact curves ([`Path`]: lines, true circular arcs, cubic Béziers) and
//! becomes polylines only at the last responsible moment. Edit-time operations
//! ([`PathBoolean`], [`offset_arcs`]) keep curves; compile-time operations ([`boolean`],
//! [`offset_shapes`], [`generate`]) work on the integer µm grid, where results are exact and
//! reproducible. Pure helpers live in [`arc`], [`cubic`], [`flatten`] and [`cubic_to_arcs`].
//!
//! # Determinism
//!
//! The compile path must give byte-identical G-code on every operating system.
//! * This crate's own code uses only `+ - * /`, `sqrt` and `libm`; a crate-local `clippy.toml`
//!   bans `HashMap`/`HashSet`, platform transcendental functions and kurbo's std-maths helpers,
//!   so a regression fails the lint gate.
//! * Integer-grid operations ([`boolean`], [`offset_shapes`], [`stroke_outline`], hatch
//!   clipping) are exact.
//! * **Not fully covered:** `i_overlay` (round joins, rotations) and `cavalier_contours` (arcs)
//!   call platform trigonometry internally. Their output is quantised to whole µm, and
//!   `tests/determinism.rs` holds a regression golden over the whole path that CI runs on every
//!   OS; that is a mitigation, not a proof.
//! * **Edit time only, outside the rule:** [`Sweep`] (`linesweeper`, which pulls kurbo with
//!   `std` maths) and the kurbo-based bounding boxes. Their results feed the editor, not G-code.

pub mod arc;
mod biarc;
mod boolean;
pub mod cubic;
mod fill;
mod flatten;
mod knife;
mod medial;
mod nodes;
mod offset;
mod path;
mod path_boolean;
mod refit;
mod region;
mod spatial;
mod thin;

pub use biarc::{cubic_to_arcs, path_to_arcs};
pub use boolean::{
    BoolOp, FillRule, Ring, Shape, boolean, clip_lines, normalize, ring_area2, shapes_area2,
};
pub use fill::{FillSpec, Pattern, generate, inset_region, spacing, stroke_passes};
pub use flatten::{flatten, flatten_to_grid};
pub use knife::{crossings, knife};
pub use kurbo::{Affine, Point, Rect, Vec2};
pub use medial::{Medial, MedialError, medial_axis};
pub use nodes::{
    SegmentHit, corner_node, delete_node, insert_node, move_handle, move_node, nearest_node,
    nearest_on_path, node_count, node_handles, node_position, segment_to_curve, segment_to_line,
    smooth_node,
};
pub use offset::{Cap, Join, offset_arcs, offset_shapes, stroke_outline};
pub use path::{Path, Seg};
pub use path_boolean::{Auto, BooleanError, Fallback, Grid, PathBoolean, RefitGrid, Sweep};
pub use refit::{CORNER_ANGLE, refit_polyline, refit_ring};
pub use region::Region;
pub use spatial::SpatialIndex;
pub use thin::{FEATURE_FRACTION, Fill, MIN_STROKE_TIPS, SPUR_RATIO, ThinParts, fill, thin_parts};
