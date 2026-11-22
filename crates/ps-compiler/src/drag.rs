//! Drag-blade (swivel) compensation.
//!
//! A drag blade trails its holder by the blade offset `d`. For the tip to follow the artwork the
//! holder must follow the tip path displaced forward along the tangent by `d`. On a line that is
//! the line shifted forward; on an arc it is an arc about the same centre of radius
//! `sqrt(r² + d²)`, rotated forward by `atan(d / r)`. At a corner the tip must stay put while the
//! blade swings round, so the holder moves on an arc of radius `d` about the corner point.
//! Corners gentler than the threshold are joined by a short line instead.
//!
//! Closed paths start in the middle of their longest straight segment (when they have one) and
//! overcut past the start; an alignment stroke before the first contact orients the blade.
//! Cubic segments are first fitted with arcs, so compensation works on lines and true arcs and
//! G2/G3-capable profiles keep arcs.

use ps_geometry::arc::{angle_of, dist, length, point_at};
use ps_geometry::{Path, Point, Seg, Vec2, path_to_arcs};

/// Tolerance when fitting cubics with arcs, in mm.
const FIT_TOL: f64 = 0.002;
/// Edges within this relative length of each other tie, and the first in path order wins, so
/// float noise from a transform cannot change which edge is chosen.
const TIE_REL: f64 = 1e-6;
/// Turns smaller than this (radians) are treated as no turn at all.
const NO_TURN: f64 = 1e-9;

/// Settings for drag compensation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragParams {
    /// Distance from the swivel axis to the blade tip, in mm.
    pub offset_mm: f64,
    /// Turns at or above this many degrees swivel on an arc; gentler turns are joined by a line.
    pub corner_threshold_deg: f64,
    /// How far a closed path continues past its start, in mm.
    pub overcut_mm: f64,
    /// Length of the alignment stroke before the first contact, in mm (zero for none).
    pub align_mm: f64,
    /// Keeps a closed path's own start instead of moving it to the longest straight edge.
    pub keep_start: bool,
}

/// Something worth telling the user about.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragWarning {
    /// An arc is tighter than the blade offset, so the blade cannot follow it exactly.
    MinRadius {
        /// Index of the segment in the path as processed (after any rotation and overcut).
        segment: usize,
        /// The arc's radius in mm.
        radius_mm: f64,
    },
}

/// The holder path and its warnings.
#[derive(Debug, Clone, PartialEq)]
pub struct Compensated {
    /// The path the holder follows: lines and arcs only.
    pub path: Path,
    /// Warnings raised.
    pub warnings: Vec<DragWarning>,
}

fn unit(v: Vec2) -> Option<Vec2> {
    let l = length(v);
    (l > 0.0).then(|| v / l)
}

fn tangent_start(from: Point, seg: &Seg) -> Option<Vec2> {
    match *seg {
        Seg::Line { to } => unit(to - from),
        Seg::Arc { center, ccw, .. } => arc_tangent(from, center, ccw),
        Seg::Cubic { .. } => None,
    }
}

fn tangent_end(seg: &Seg, from: Point) -> Option<Vec2> {
    match *seg {
        Seg::Line { to } => unit(to - from),
        Seg::Arc { to, center, ccw } => arc_tangent(to, center, ccw),
        Seg::Cubic { .. } => None,
    }
}

fn arc_tangent(at: Point, center: Point, ccw: bool) -> Option<Vec2> {
    let u = unit(at - center)?;
    Some(if ccw {
        Vec2::new(-u.y, u.x)
    } else {
        Vec2::new(u.y, -u.x)
    })
}

/// Segments with their start points, dropping zero-length ones.
fn live_segments(path: &Path) -> Vec<(Point, Seg)> {
    path.segments()
        .filter(|(from, seg)| match **seg {
            Seg::Line { to } => dist(*from, to) > 1e-12,
            Seg::Arc { to, center, .. } => {
                dist(*from, center) > 1e-12 && dist(*from, to) + dist(*from, center) > 0.0
            }
            Seg::Cubic { .. } => false,
        })
        .map(|(from, seg)| (from, *seg))
        .collect()
}

/// Rotates a closed path to start in the middle of its longest straight segment, if it has one.
pub(crate) fn start_on_longest_line(path: &Path) -> Path {
    let mut walked = 0.0;
    let mut best: Option<(f64, f64)> = None; // (segment length, distance of its middle)
    for (from, seg) in path.segments() {
        let len = seg.length(from);
        if matches!(seg, Seg::Line { .. }) && best.is_none_or(|(b, _)| len > b * (1.0 + TIE_REL)) {
            best = Some((len, walked + len / 2.0));
        }
        walked += len;
    }
    let Some((_, middle)) = best else {
        return path.clone();
    };
    let pieces = path.split_at_lengths(&[middle]);
    let [first, second] = pieces.as_slice() else {
        return path.clone();
    };
    let mut rotated = Path::new(second.start);
    rotated.segs.extend(second.segs.iter().copied());
    rotated.segs.extend(first.segs.iter().copied());
    rotated.closed = path.closed;
    rotated
}

/// Computes the path the holder must follow so that the blade tip follows `path`.
#[must_use]
pub fn compensate(path: &Path, params: &DragParams) -> Compensated {
    let d = params.offset_mm;
    if d <= 0.0 {
        return Compensated {
            path: path.clone(),
            warnings: Vec::new(),
        };
    }

    let mut work = path_to_arcs(path, FIT_TOL);
    if work.closed {
        if !params.keep_start {
            work = start_on_longest_line(&work);
        }
        if params.overcut_mm > 0.0 {
            let overcut = work.split_at_lengths(&[params.overcut_mm]);
            if let [lead, _] = overcut.as_slice() {
                work.segs.extend(lead.segs.iter().copied());
            }
        }
        work.closed = false;
    }

    let segs = live_segments(&work);
    let mut warnings = Vec::new();
    let Some(&(first_from, first_seg)) = segs.first() else {
        return Compensated {
            path: Path::new(work.start),
            warnings,
        };
    };
    let threshold = params.corner_threshold_deg.to_radians();

    // Holder start and end of each segment: P + d·T at the tip's own start and end.
    let t0 = tangent_start(first_from, &first_seg).unwrap_or(Vec2::new(1.0, 0.0));
    let h0 = first_from + t0 * d;
    let mut out = if params.align_mm > 0.0 {
        Path::new(h0 - t0 * params.align_mm).line_to(h0)
    } else {
        Path::new(h0)
    };

    let mut prev: Option<(Point, Vec2)> = None; // (tip point, tangent) at the previous end
    for (i, (from, seg)) in segs.iter().enumerate() {
        let (Some(ts), Some(te)) = (tangent_start(*from, seg), tangent_end(seg, *from)) else {
            continue;
        };
        if let Some((corner, te_prev)) = prev {
            let turn = libm::atan2(te_prev.cross(ts), te_prev.dot(ts));
            if turn.abs() >= NO_TURN {
                let to = corner + ts * d;
                out = if turn.abs() >= threshold {
                    out.arc_to(to, corner, turn > 0.0)
                } else {
                    out.line_to(to)
                };
            }
        }
        match *seg {
            Seg::Line { to } => out = out.line_to(to + te * d),
            Seg::Arc { to, center, ccw } => {
                let r = dist(*from, center);
                if r < d {
                    warnings.push(DragWarning::MinRadius {
                        segment: i,
                        radius_mm: r,
                    });
                }
                let big = libm::hypot(r, d);
                let shift = libm::atan2(d, r) * if ccw { 1.0 } else { -1.0 };
                let end = point_at(center, big, angle_of(to, center) + shift);
                out = out.arc_to(end, center, ccw);
            }
            Seg::Cubic { .. } => {}
        }
        let tip_end = match *seg {
            Seg::Line { to } | Seg::Arc { to, .. } | Seg::Cubic { to, .. } => to,
        };
        prev = Some((tip_end, te));
    }
    Compensated {
        path: out,
        warnings,
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "tests are the independent oracle and may use platform maths"
)]
mod tests;
