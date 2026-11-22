//! Plotter Studio job compiler.
//!
//! The compiler is one pure function from a document snapshot, the libraries and a machine
//! profile to the job IR. It does no I/O, reads no clock and uses no global state, which is what
//! makes byte-identical output and golden-file testing possible.

mod cache;
mod calibrate;
mod colour;
mod compile;
mod drag;
mod estimate;
mod link;
mod motion;
mod order;
mod prepare;
mod safety;
mod schedule;
mod simplify;
mod tangential;
mod toolplan;

pub use cache::{CacheStats, CompileCache, DEFAULT_CAPACITY_BYTES, memo_key};
pub use calibrate::{
    blade_offset, glyph, holder_offset, pressure_ladder, test_cut, text_strokes, time_reference,
    tip_width_ladder,
};
pub use colour::lightness;
pub use compile::{CompileError, CompileInput, MatSelection, compile, compile_cached};
pub use drag::{Compensated, DragParams, DragWarning, compensate};
pub use estimate::{EstimateInput, estimate, utilisation_ppm};
pub use link::connect;
pub use motion::{
    Dir, MIN_JUNCTION_SPEED, MotionLimits, Move, Plan, junction_speed, plan, segment_time,
};
pub use order::{
    Forest, Item, Visit, containment, emission_order, improve, improve_sweeps, nearest_neighbour,
    point_in_ring, start_nearest, tour_length,
};
pub use prepare::{
    Note, PrepareInput, Prepared, Stroke, effective_start_geom, prepare, prepare_compound,
};
pub use safety::{
    Cutter, Drawn, MachineLimits, Rect, Violation, check_point, check_segment, clamp_force,
    release_edges, segments_cross,
};
pub use schedule::{Block, Cycle, Edge, EdgeKind, Schedule, Stage, schedule, stage_of};
pub use tangential::{
    heading_mdeg, split_at_corners, turn_mdeg, unwrap_near_mdeg, unwrapped_angles,
};
pub use toolplan::{Switch, ToolPlan, ToolUse, merge_adjacent, plan_tools};
