//! Offsets and outlines.
//!
//! Two engines, chosen by what the result must keep:
//! * [`offset_arcs`] uses `cavalier_contours` on lines and true arcs, so round joins stay exact
//!   arcs (blade compensation, concentric fills, G2/G3 output).
//! * [`offset_shapes`] and [`stroke_outline`] use `i_overlay` on the µm grid, which handles
//!   miter and bevel joins, holes and self-intersections robustly.

use cavalier_contours::polyline::{PlineSource, PlineSourceMut, Polyline};
use i_overlay::mesh::outline::offset::OutlineOffset;
use i_overlay::mesh::stroke::offset::StrokeOffset;
use i_overlay::mesh::style::{LineCap, LineJoin, OutlineStyle, StrokeStyle};
use kurbo::Point;
use ps_core::PtUm;

use crate::arc;
use crate::biarc::path_to_arcs;
use crate::boolean::{Ring, Shape};
use crate::path::{Path, Seg};

/// Chord tolerance, in µm, used when approximating round joins and caps on the grid.
const GRID_ROUND_TOL_UM: f64 = 2.0;
/// Scale passed to the engine: coordinates are already integer µm, so 1 unit = 1 grid step.
const GRID_SCALE: f64 = 1.0;

/// How the corners of an offset are joined.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Join {
    /// Circular arcs around the corner.
    Round,
    /// A sharp corner, cut off (bevelled) when the miter length exceeds `limit` times the offset.
    Miter {
        /// Maximum ratio of miter length to offset distance (the SVG miter limit).
        limit: f64,
    },
    /// The corner is cut off with a straight line.
    Bevel,
}

/// How the ends of an open stroke are finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    /// Squared off at the end point.
    Butt,
    /// A half disc.
    Round,
    /// Squared off half a width beyond the end point.
    Square,
}

#[expect(
    clippy::cast_precision_loss,
    reason = "grid distances are far below 2^53 µm, where i64 -> f64 is exact"
)]
pub(crate) fn um_f64(v: i64) -> f64 {
    v as f64
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "engine output is integral and within i32 range; `as` saturates anyway"
)]
pub(crate) fn um_i64(v: f64) -> i64 {
    v.round() as i64
}

fn engine_join(join: Join, radius_um: f64) -> LineJoin<f64> {
    match join {
        Join::Round => LineJoin::Round(arc::max_step(radius_um.abs(), GRID_ROUND_TOL_UM)),
        Join::Miter { limit } => LineJoin::Miter(2.0 * libm::asin((1.0 / limit.max(1.0)).min(1.0))),
        Join::Bevel => LineJoin::Bevel,
    }
}

fn to_float(rings: &[Ring]) -> Vec<Vec<[f64; 2]>> {
    rings
        .iter()
        .map(|r| r.iter().map(|p| [um_f64(p.x), um_f64(p.y)]).collect())
        .collect()
}

fn from_float(shapes: Vec<Vec<Vec<[f64; 2]>>>) -> Vec<Shape> {
    shapes
        .into_iter()
        .map(|shape| {
            shape
                .into_iter()
                .map(|ring| {
                    ring.into_iter()
                        .map(|[x, y]| PtUm::new(um_i64(x), um_i64(y)))
                        .collect()
                })
                .collect()
        })
        .collect()
}

/// Offsets shapes by `distance_um` (positive grows, negative shrinks) on the µm grid.
///
/// Holes move the opposite way, so growing a shape shrinks its holes.
#[must_use]
pub fn offset_shapes(shapes: &[Shape], distance_um: i64, join: Join) -> Vec<Shape> {
    if distance_um == 0 {
        return shapes.to_vec();
    }
    let d = um_f64(distance_um);
    let style = OutlineStyle::new(d).line_join(engine_join(join, d));
    let input: Vec<Vec<Vec<[f64; 2]>>> = shapes.iter().map(|s| to_float(s)).collect();
    input
        .outline_fixed_scale(&style, GRID_SCALE)
        .map(from_float)
        .unwrap_or_default()
}

/// Builds the outline of a stroke of the given width along a polyline.
#[must_use]
pub fn stroke_outline(
    points: &[PtUm],
    width_um: i64,
    closed: bool,
    cap: Cap,
    join: Join,
) -> Vec<Shape> {
    let w = um_f64(width_um);
    let cap = match cap {
        Cap::Butt => LineCap::Butt,
        Cap::Square => LineCap::Square,
        Cap::Round => LineCap::Round(arc::max_step(w / 2.0, GRID_ROUND_TOL_UM)),
    };
    let style = StrokeStyle::new(w)
        .start_cap(cap.clone())
        .end_cap(cap)
        .line_join(engine_join(join, w / 2.0));
    let path: Vec<[f64; 2]> = points.iter().map(|p| [um_f64(p.x), um_f64(p.y)]).collect();
    path.stroke_fixed_scale(style, closed, GRID_SCALE)
        .map(from_float)
        .unwrap_or_default()
}

/// Offsets a path made of lines and arcs by `distance` mm, keeping arcs exact.
///
/// Cubic segments are first fitted with arcs within `tol` mm. For closed paths a positive
/// distance grows the shape whichever way it winds; for open paths a positive distance is to the
/// left of the direction of travel. Offsets that collapse the shape return nothing.
#[must_use]
pub fn offset_arcs(path: &Path, distance: f64, tol: f64) -> Vec<Path> {
    let fitted = path_to_arcs(path, tol);
    let pline = to_polyline(&fitted);
    let signed = if fitted.closed && fitted.signed_area() > 0.0 {
        -distance // counter-clockwise: the left side is the inside
    } else {
        distance
    };
    pline
        .parallel_offset(signed)
        .iter()
        .map(from_polyline)
        .collect()
}

fn to_polyline(path: &Path) -> Polyline<f64> {
    let mut pl = if path.closed {
        Polyline::new_closed()
    } else {
        Polyline::new()
    };
    for (from, seg) in path.segments() {
        let bulge = match *seg {
            Seg::Arc { to, center, ccw } => arc::bulge_from_arc(from, to, center, ccw),
            Seg::Line { .. } | Seg::Cubic { .. } => 0.0,
        };
        pl.add(from.x, from.y, bulge);
    }
    if !path.closed {
        let end = path.end();
        pl.add(end.x, end.y, 0.0);
    }
    pl
}

fn from_polyline(pl: &Polyline<f64>) -> Path {
    let n = pl.vertex_count();
    let first = pl.at(0);
    let mut out = Path::new(Point::new(first.x, first.y));
    let segments = if pl.is_closed() {
        n
    } else {
        n.saturating_sub(1)
    };
    for i in 0..segments {
        let a = pl.at(i);
        let b = pl.at((i + 1) % n);
        let (p0, p1) = (Point::new(a.x, a.y), Point::new(b.x, b.y));
        match arc::center_from_bulge(p0, p1, a.bulge) {
            Some(center) => out = out.arc_to(p1, center, a.bulge > 0.0),
            None => out = out.line_to(p1),
        }
    }
    out.closed = pl.is_closed();
    out
}

#[cfg(test)]
mod tests;
