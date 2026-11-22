//! Node editing on a path's own geometry.
//!
//! Nodes are the vertices of a path: an open path with `n` segments has `n + 1` nodes, a closed
//! one has `n` (its closing point is its first node, not another). Every function returns a new
//! path and `None` when the edit is not possible, so an editor can offer only what works.
//! Converting between lines and curves and between smooth and corner nodes follows the usual
//! vector-editor rules.

use kurbo::{CubicBez, ParamCurveNearest, Point, Vec2};

use crate::arc;
use crate::path::{Path, Seg};

/// How a handle is sized when a line becomes a curve or a node is smoothed: a third of the way.
const HANDLE_FRACTION: f64 = 1.0 / 3.0;

/// The segment and parameter nearest to a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentHit {
    /// The segment index.
    pub segment: usize,
    /// The parameter along it, from 0 to 1.
    pub t: f64,
    /// The distance from the point to the path.
    pub distance: f64,
}

/// How many nodes a path has.
#[must_use]
pub fn node_count(path: &Path) -> usize {
    if path.closed {
        path.segs.len().max(1)
    } else {
        path.segs.len() + 1
    }
}

/// Where node `index` is.
#[must_use]
pub fn node_position(path: &Path, index: usize) -> Option<Point> {
    if index >= node_count(path) {
        return None;
    }
    if index == 0 {
        Some(path.start)
    } else {
        path.segs.get(index - 1).map(Seg::to)
    }
}

/// The node a segment ends at.
fn end_node(path: &Path, segment: usize) -> usize {
    if path.closed {
        (segment + 1) % node_count(path)
    } else {
        segment + 1
    }
}

fn lerp(a: Point, b: Point, t: f64) -> Point {
    a.lerp(b, t)
}

fn with_end(seg: Seg, to: Point, delta: Vec2) -> Seg {
    match seg {
        Seg::Line { .. } | Seg::Arc { .. } => Seg::Line { to },
        Seg::Cubic { c1, c2, .. } => Seg::Cubic {
            c1,
            c2: c2 + delta,
            to,
        },
    }
}

fn with_start_moved(seg: Seg, delta: Vec2) -> Seg {
    match seg {
        Seg::Line { to } | Seg::Arc { to, .. } => Seg::Line { to },
        Seg::Cubic { c1, c2, to } => Seg::Cubic {
            c1: c1 + delta,
            c2,
            to,
        },
    }
}

/// Moves a node. Curve handles attached to it move by the same offset; an arc next to it becomes
/// a line, since an arc cannot follow a moved end.
#[must_use]
pub fn move_node(path: &Path, index: usize, to: Point) -> Option<Path> {
    let from = node_position(path, index)?;
    let delta = to - from;
    let mut out = path.clone();
    if index == 0 {
        out.start = to;
    }
    for k in 0..path.segs.len() {
        let mut seg = path.segs[k];
        if end_node(path, k) == index {
            seg = with_end(seg, to, delta);
        }
        if k == index {
            // The segment k starts at node k.
            seg = with_start_moved(seg, delta);
        }
        out.segs[k] = seg;
    }
    Some(out)
}

/// Splits segment `segment` at parameter `t` (strictly between 0 and 1), adding a node there.
#[must_use]
pub fn insert_node(path: &Path, segment: usize, t: f64) -> Option<Path> {
    if !(t.is_finite() && t > 0.0 && t < 1.0) || segment >= path.segs.len() {
        return None;
    }
    let from = node_position(path, segment)?;
    let (first, second) = match path.segs[segment] {
        Seg::Line { to } => {
            let mid = lerp(from, to, t);
            (Seg::Line { to: mid }, Seg::Line { to })
        }
        Seg::Cubic { c1, c2, to } => {
            let q0 = lerp(from, c1, t);
            let q1 = lerp(c1, c2, t);
            let q2 = lerp(c2, to, t);
            let r0 = lerp(q0, q1, t);
            let r1 = lerp(q1, q2, t);
            let s = lerp(r0, r1, t);
            (
                Seg::Cubic {
                    c1: q0,
                    c2: r0,
                    to: s,
                },
                Seg::Cubic { c1: r1, c2: q2, to },
            )
        }
        Seg::Arc { to, center, ccw } => {
            let radius = arc::dist(from, center);
            let total = arc::sweep(from, to, center, ccw);
            let mid = arc::point_at(center, radius, arc::angle_of(from, center) + total * t);
            (
                Seg::Arc {
                    to: mid,
                    center,
                    ccw,
                },
                Seg::Arc { to, center, ccw },
            )
        }
    };
    let mut out = path.clone();
    out.segs[segment] = first;
    out.segs.insert(segment + 1, second);
    Some(out)
}

/// Removes a node, joining its neighbours with a line. An open path keeps at least two nodes and
/// a closed one at least three.
#[must_use]
pub fn delete_node(path: &Path, index: usize) -> Option<Path> {
    let count = node_count(path);
    let minimum = if path.closed { 3 } else { 2 };
    if index >= count || count <= minimum {
        return None;
    }
    let mut out = path.clone();
    let n = path.segs.len();
    if !path.closed {
        if index == 0 {
            out.start = path.segs[0].to();
            out.segs.remove(0);
        } else if index == n {
            out.segs.pop();
        } else {
            let next = path.segs[index].to();
            out.segs[index - 1] = Seg::Line { to: next };
            out.segs.remove(index);
        }
    } else if index == 0 {
        // The new start is the old second node; the path closes with a line back to it.
        let new_start = path.segs[0].to();
        out.start = new_start;
        out.segs.remove(0);
        if let Some(last) = out.segs.last_mut() {
            *last = Seg::Line { to: new_start };
        }
    } else {
        let next = path.segs[index].to();
        out.segs[index - 1] = Seg::Line { to: next };
        out.segs.remove(index);
    }
    Some(out)
}

/// The segment that arrives at node `index` and the one that leaves it, where the path has them.
fn around(path: &Path, index: usize) -> (Option<usize>, Option<usize>) {
    let n = path.segs.len();
    if n == 0 || index >= node_count(path) {
        return (None, None);
    }
    if path.closed {
        ((Some((index + n - 1) % n)), Some(index % n))
    } else {
        (index.checked_sub(1), (index < n).then_some(index))
    }
}

/// The curve handles of node `index`: where the arriving curve's handle is, then where the
/// leaving curve's is. A side whose segment is a line or an arc has none.
#[must_use]
pub fn node_handles(path: &Path, index: usize) -> (Option<Point>, Option<Point>) {
    let (before, after) = around(path, index);
    let incoming = before.and_then(|k| match path.segs[k] {
        Seg::Cubic { c2, .. } => Some(c2),
        _ => None,
    });
    let outgoing = after.and_then(|k| match path.segs[k] {
        Seg::Cubic { c1, .. } => Some(c1),
        _ => None,
    });
    (incoming, outgoing)
}

/// Drags a curve handle of node `index` to `to`: the leaving one when `outgoing`, else the arriving
/// one. With `smooth`, the handle on the other side swings to stay in line with it, keeping its own
/// length, so the curve has no corner at the node. `None` when that side has no curve handle.
#[must_use]
pub fn move_handle(
    path: &Path,
    index: usize,
    outgoing: bool,
    to: Point,
    smooth: bool,
) -> Option<Path> {
    let here = node_position(path, index)?;
    let (before, after) = around(path, index);
    let (this, other) = if outgoing {
        (after?, before)
    } else {
        (before?, after)
    };
    let mut out = path.clone();
    match &mut out.segs[this] {
        Seg::Cubic { c1, c2, .. } => {
            if outgoing {
                *c1 = to;
            } else {
                *c2 = to;
            }
        }
        _ => return None,
    }
    if smooth
        && let Some(other) = other
        && let Some(direction) = unit(here - to)
        && let Seg::Cubic { c1, c2, .. } = &mut out.segs[other]
    {
        // The far side's own handle: its leaving handle when we drag the arriving one.
        let handle = if outgoing { c2 } else { c1 };
        let length = arc::length(*handle - here);
        *handle = here + direction * length;
    }
    Some(out)
}

/// Turns a segment into a curve: a line becomes a straight cubic with handles a third of the way,
/// an arc becomes the cubics that follow it, and a curve is left as it is.
#[must_use]
pub fn segment_to_curve(path: &Path, segment: usize) -> Option<Path> {
    let seg = *path.segs.get(segment)?;
    let from = node_position(path, segment)?;
    let mut out = path.clone();
    match seg {
        Seg::Cubic { .. } => {}
        Seg::Line { to } => {
            let d = to - from;
            out.segs[segment] = Seg::Cubic {
                c1: from + d * HANDLE_FRACTION,
                c2: from + d * (2.0 * HANDLE_FRACTION),
                to,
            };
        }
        Seg::Arc { to, center, ccw } => {
            let pieces: Vec<Seg> = arc::to_cubics(from, to, center, ccw)
                .into_iter()
                .map(|[c1, c2, end]| Seg::Cubic { c1, c2, to: end })
                .collect();
            out.segs.splice(segment..=segment, pieces);
        }
    }
    Some(out)
}

/// Turns a segment into a straight line between its end points.
#[must_use]
pub fn segment_to_line(path: &Path, segment: usize) -> Option<Path> {
    let seg = *path.segs.get(segment)?;
    let mut out = path.clone();
    out.segs[segment] = Seg::Line { to: seg.to() };
    Some(out)
}

fn unit(v: Vec2) -> Option<Vec2> {
    let len = arc::length(v);
    (len > f64::EPSILON).then(|| v / len)
}

/// Makes a node smooth: the handles on both sides lie on the line through the neighbouring nodes,
/// each a third of the way to its neighbour. End nodes of an open path cannot be smoothed.
#[must_use]
pub fn smooth_node(path: &Path, index: usize) -> Option<Path> {
    let count = node_count(path);
    if index >= count || (!path.closed && (index == 0 || index == count - 1)) {
        return None;
    }
    let n = path.segs.len();
    let incoming = (index + n - 1) % n;
    let outgoing = index % n;
    let here = node_position(path, index)?;
    let before = node_position(path, (index + count - 1) % count)?;
    let after = node_position(path, (index + 1) % count)?;
    let tangent = unit(after - before)?;
    // Arcs and lines on either side become cubics first.
    let mut out = path.clone();
    for k in [incoming, outgoing] {
        if matches!(out.segs[k], Seg::Arc { .. }) {
            out = segment_to_line(&out, k)?;
        }
        out = segment_to_curve(&out, k)?;
    }
    let a = arc::length(here - before) * HANDLE_FRACTION;
    let b = arc::length(after - here) * HANDLE_FRACTION;
    if let Seg::Cubic { c1, to, .. } = out.segs[incoming] {
        out.segs[incoming] = Seg::Cubic {
            c1,
            c2: here - tangent * a,
            to,
        };
    }
    if let Seg::Cubic { c2, to, .. } = out.segs[outgoing] {
        out.segs[outgoing] = Seg::Cubic {
            c1: here + tangent * b,
            c2,
            to,
        };
    }
    Some(out)
}

/// Makes a node a corner: the curve handles at it are pulled onto the node.
#[must_use]
pub fn corner_node(path: &Path, index: usize) -> Option<Path> {
    let count = node_count(path);
    if index >= count {
        return None;
    }
    let here = node_position(path, index)?;
    let n = path.segs.len();
    let mut out = path.clone();
    if n == 0 {
        return Some(out);
    }
    if (path.closed || index > 0)
        && let Seg::Cubic { c1, to, .. } = out.segs[(index + n - 1) % n]
    {
        out.segs[(index + n - 1) % n] = Seg::Cubic { c1, c2: here, to };
    }
    if (path.closed || index < n)
        && let Seg::Cubic { c2, to, .. } = out.segs[index % n]
    {
        out.segs[index % n] = Seg::Cubic { c1: here, c2, to };
    }
    Some(out)
}

fn nearest_on_segment(from: Point, seg: Seg, p: Point, accuracy: f64) -> (f64, f64) {
    match seg {
        Seg::Line { to } => {
            let d = to - from;
            let len2 = d.hypot2();
            let t = if len2 <= f64::EPSILON {
                0.0
            } else {
                ((p - from).dot(d) / len2).clamp(0.0, 1.0)
            };
            (t, arc::length(p - lerp(from, to, t)))
        }
        Seg::Cubic { c1, c2, to } => {
            let n = CubicBez::new(from, c1, c2, to).nearest(p, accuracy);
            (n.t, n.distance_sq.sqrt())
        }
        Seg::Arc { to, center, ccw } => {
            let radius = arc::dist(from, center);
            let total = arc::sweep(from, to, center, ccw);
            let a0 = arc::angle_of(from, center);
            let angle = arc::angle_of(p, center);
            // The signed angle from the start towards the point, in the arc's direction.
            let mut delta = angle - a0;
            while delta > std::f64::consts::PI {
                delta -= std::f64::consts::TAU;
            }
            while delta < -std::f64::consts::PI {
                delta += std::f64::consts::TAU;
            }
            if total != 0.0 && delta / total >= 0.0 && (delta / total) <= 1.0 {
                let t = delta / total;
                (t, (arc::dist(p, center) - radius).abs())
            } else {
                let (d0, d1) = (arc::dist(p, from), arc::dist(p, to));
                if d0 <= d1 { (0.0, d0) } else { (1.0, d1) }
            }
        }
    }
}

/// The segment and parameter nearest to `p`; `accuracy` bounds the error for curves.
#[must_use]
pub fn nearest_on_path(path: &Path, p: Point, accuracy: f64) -> Option<SegmentHit> {
    let mut best: Option<SegmentHit> = None;
    for (segment, (from, seg)) in path.segments().enumerate() {
        let (t, distance) = nearest_on_segment(from, *seg, p, accuracy);
        if best.is_none_or(|b| distance < b.distance) {
            best = Some(SegmentHit {
                segment,
                t,
                distance,
            });
        }
    }
    best
}

/// The nearest node to `p` within `radius`.
#[must_use]
pub fn nearest_node(path: &Path, p: Point, radius: f64) -> Option<usize> {
    (0..node_count(path))
        .filter_map(|i| node_position(path, i).map(|q| (i, arc::length(q - p))))
        .filter(|(_, d)| *d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests;
