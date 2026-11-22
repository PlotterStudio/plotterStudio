//! Level of detail: how finely to flatten, when to rebuild, when to draw a dot.

/// The flatten tolerance in screen pixels.
const TOLERANCE_PX: f64 = 0.25;
/// A cached mesh is rebuilt when the zoom changes by more than this factor either way.
const RETESSELLATE_FACTOR: f64 = 2.0;
/// The smallest zoom the tolerance maths accepts, so a degenerate zoom stays finite.
const ZOOM_FLOOR: f64 = 1e-9;

/// The flatten tolerance in millimetres at `zoom` pixels per millimetre.
#[must_use]
pub fn flatten_tolerance_mm(zoom: f64) -> f64 {
    TOLERANCE_PX / zoom.abs().max(ZOOM_FLOOR)
}

/// Whether a mesh built at `cached_zoom` must be rebuilt for `zoom`.
#[must_use]
pub fn needs_retessellation(cached_zoom: f64, zoom: f64) -> bool {
    let ratio = zoom / cached_zoom;
    ratio > RETESSELLATE_FACTOR || ratio < 1.0 / RETESSELLATE_FACTOR
}

/// The factor to scale a mesh built at `cached_zoom` by while showing it at `zoom`.
#[must_use]
pub fn cached_mesh_scale(cached_zoom: f64, zoom: f64) -> f64 {
    zoom / cached_zoom
}

/// Whether a path of this size (mm) is smaller than one pixel at `zoom` and collapses to a dot.
#[must_use]
pub fn is_dot(width_mm: f64, height_mm: f64, zoom: f64) -> bool {
    width_mm.max(height_mm) * zoom < 1.0
}

/// The centre of a bounding box given as `(x0, y0, x1, y1)`.
#[must_use]
pub fn dot_centre(x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64) {
    (x0.midpoint(x1), y0.midpoint(y1))
}

/// A length in screen pixels as millimetres at `zoom`.
#[must_use]
pub fn px_to_mm(px: f64, zoom: f64) -> f64 {
    px / zoom
}

#[cfg(test)]
mod tests;
