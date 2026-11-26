//! Plotter Studio canvas rendering, the part that needs no GPU.
//!
//! * [`Viewport`]: pan, zoom about the cursor, fit, and true-scale.
//! * [`flatten_tolerance_mm`] and friends: the level-of-detail rules.
//! * [`style_of`], [`icon_name`], [`colour_of`]: line styles, icons and colours per operation kind,
//!   so colour is never the only cue.
//! * [`Mesh`]: fills and strokes tessellated with lyon, in object space.
//!
//! These are pure functions of their inputs, so they are tested without a window or a device;
//! the app uploads the meshes to the GPU.

mod dash;
mod gpu;
mod hit;
mod lod;
mod mesh;
mod orbit;
mod preview;
mod snap;
mod style;
mod toolpath;
mod viewport;
mod weeding;

pub use dash::{MAX_DASHES, dash, dash_for_zoom};
pub use gpu::{GpuContext, GpuError, Image, Scene, srgb_to_linear};
pub use hit::{
    BoxMode, PICK_RADIUS_PX, Pickable, distance_to_path, distance_to_segment, pick, pick_in_box,
    pick_innermost, pick_radius_mm, winding_number,
};
pub use lod::{
    cached_mesh_scale, dot_centre, flatten_tolerance_mm, is_dot, needs_retessellation, px_to_mm,
};
pub use mesh::{CapStyle, Mesh, dot_mesh, fill_mesh, stroke_mesh};
pub use orbit::{MAX_TILT_DEG, Orbit, orbit_view};
pub use preview::{
    BaseMaterial, Effect, EffectMaps, Light, MAX_PX_PER_MM, Quality, Texel, ToolLook,
    adaptive_scale, composite, scale_for_zoom,
};
pub use snap::{
    Candidate, SNAP_RADIUS_PX, SnapKind, bbox_anchors, edge_point, grid_point,
    polyline_intersections, segment_intersection, segment_midpoints, snap, snap_radius_mm,
};
pub use style::{
    colour_of, dash_pattern_mm, dash_pattern_px, icon_name, parallel_offsets_px, style_of,
    travel_style,
};
pub use toolpath::{
    Marker, MarkerKind, PLAYBACK_SPEEDS, STEPS_PER_SECOND, Segment, SegmentKind, markers,
    playback_step, segments, step_at_fraction, tool_position,
};
pub use viewport::{MAX_ZOOM, MIN_ZOOM, Viewport};
pub use weeding::{Pieces, Weeding, weeded};
