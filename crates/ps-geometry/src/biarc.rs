//! Bézier → circular-arc fitting.
//!
//! Each cubic is approximated by a biarc (two tangent-continuous arcs, Bolton's equal-chord
//! construction). If the fit deviates more than the tolerance the cubic is split in half and
//! each half is fitted again, so the result is always within tolerance.

use kurbo::{CubicBez, Point, Vec2};

use crate::path::{Path, Seg};
use crate::{arc, cubic};

/// Deepest subdivision before a half is accepted as a straight line.
const MAX_DEPTH: u32 = 16;
/// Number of samples used to measure the deviation of a candidate fit.
const SAMPLES: u32 = 64;
/// Fraction of the tolerance a candidate may use; the rest covers samples between the checks.
const SAFETY: f64 = 0.8;
/// A sweep larger than this is subdivided instead of accepted.
const MAX_ARC_SWEEP: f64 = 1.5 * std::f64::consts::PI;
/// Tangent dot products closer to 1 than this count as parallel.
const PARALLEL_EPS: f64 = 1e-12;

/// Fits a cubic with arcs and lines, returning segments from `p0` to exactly `p3`.
#[must_use]
pub fn cubic_to_arcs(p0: Point, c1: Point, c2: Point, p3: Point, tol: f64) -> Vec<Seg> {
    let mut out = Vec::new();
    fit(CubicBez::new(p0, c1, c2, p3), tol, 0, &mut out);
    out
}

/// Replaces every cubic in a path by arcs, leaving lines and arcs untouched.
#[must_use]
pub fn path_to_arcs(path: &Path, tol: f64) -> Path {
    let mut segs = Vec::with_capacity(path.segs.len());
    for (from, seg) in path.segments() {
        match *seg {
            Seg::Cubic { c1, c2, to } => segs.extend(cubic_to_arcs(from, c1, c2, to, tol)),
            other => segs.push(other),
        }
    }
    Path {
        start: path.start,
        segs,
        closed: path.closed,
    }
}

fn fit(c: CubicBez, tol: f64, depth: u32, out: &mut Vec<Seg>) {
    if is_straight(&c, tol) || depth >= MAX_DEPTH {
        out.push(Seg::Line { to: c.p3 });
        return;
    }
    if let Some(pair) = biarc(&c)
        && deviation(&c, &pair) <= SAFETY * tol
    {
        out.extend(pair.iter().map(|piece| piece.seg));
        return;
    }
    let (a, b) = cubic::split_half(&c);
    fit(a, tol, depth + 1, out);
    fit(b, tol, depth + 1, out);
}

/// True when both control points lie within `tol` of the chord (so the whole curve does).
fn is_straight(c: &CubicBez, tol: f64) -> bool {
    let chord = c.p3 - c.p0;
    let len2 = chord.hypot2();
    let dist = |p: Point| {
        if len2 == 0.0 {
            arc::dist(p, c.p0)
        } else {
            (p - c.p0).cross(chord).abs() / len2.sqrt()
        }
    };
    dist(c.p1) <= tol && dist(c.p2) <= tol
}

/// One arc of a biarc, kept with its start so it can be measured.
#[derive(Clone, Copy)]
struct Piece {
    from: Point,
    seg: Seg,
}

fn unit(v: Vec2) -> Option<Vec2> {
    let l = arc::length(v);
    (l > 0.0).then(|| v / l)
}

/// Builds the biarc for a cubic, or `None` if the construction is degenerate.
fn biarc(curve: &CubicBez) -> Option<[Piece; 2]> {
    let chord = curve.p3 - curve.p0;
    let t0 = unit(curve.p1 - curve.p0)
        .or_else(|| unit(curve.p2 - curve.p0))
        .or_else(|| unit(chord))?;
    let t1 = unit(curve.p3 - curve.p2)
        .or_else(|| unit(curve.p3 - curve.p1))
        .or_else(|| unit(chord))?;
    let turn = 1.0 - t0.dot(t1);
    if turn < PARALLEL_EPS {
        return None;
    }
    // Equal-chord construction: the tangent points P0 + d·T0 and P3 − d·T1 are 2d apart.
    let along = chord.dot(t0 + t1);
    let reach = (-along + (along * along + 2.0 * turn * chord.hypot2()).sqrt()) / (2.0 * turn);
    if !reach.is_finite() || reach <= 0.0 {
        return None;
    }
    let q0 = curve.p0 + t0 * reach;
    let q1 = curve.p3 - t1 * reach;
    let join = q0.midpoint(q1);
    let first = tangent_arc(curve.p0, t0, join, true)?;
    let second = tangent_arc(join, t1, curve.p3, false)?;
    Some([first, second])
}

/// An arc (or line) from `a` to `b` that leaves `a` with tangent `t` (`at_start`) or arrives at
/// `b` with tangent `t` (not `at_start`).
fn tangent_arc(a: Point, t: Vec2, b: Point, at_start: bool) -> Option<Piece> {
    let (anchor, other) = if at_start { (a, b) } else { (b, a) };
    let normal = Vec2::new(-t.y, t.x);
    let chord = other - anchor;
    let proj = normal.dot(chord);
    if proj.abs() < 1e-12 * arc::length(chord).max(1.0) {
        return Some(Piece {
            from: a,
            seg: Seg::Line { to: b },
        });
    }
    let rho = chord.hypot2() / (2.0 * proj);
    let center = anchor + normal * rho;
    // Travelling along `t` with the centre on the left means counter-clockwise.
    let ccw = rho > 0.0;
    if arc::sweep(a, b, center, ccw).abs() > MAX_ARC_SWEEP {
        return None;
    }
    Some(Piece {
        from: a,
        seg: Seg::Arc { to: b, center, ccw },
    })
}

fn dist_to_piece(p: Point, piece: &Piece) -> f64 {
    match piece.seg {
        Seg::Line { to } => {
            let ab = to - piece.from;
            let t = ((p - piece.from).dot(ab) / ab.hypot2()).clamp(0.0, 1.0);
            arc::dist(p, piece.from.lerp(to, t))
        }
        Seg::Arc { to, center, ccw } => {
            let r = arc::dist(piece.from, center);
            let total = arc::sweep(piece.from, to, center, ccw).abs();
            let a0 = arc::angle_of(piece.from, center);
            let a = arc::angle_of(p, center);
            let along = if ccw { a - a0 } else { a0 - a }.rem_euclid(std::f64::consts::TAU);
            if along <= total {
                (arc::dist(p, center) - r).abs()
            } else {
                arc::dist(p, piece.from).min(arc::dist(p, to))
            }
        }
        Seg::Cubic { .. } => f64::INFINITY,
    }
}

/// Largest sampled distance from the cubic to the fitted pieces.
fn deviation(c: &CubicBez, pieces: &[Piece; 2]) -> f64 {
    (0..=SAMPLES)
        .map(|i| {
            let p = cubic::eval(c, f64::from(i) / f64::from(SAMPLES));
            pieces
                .iter()
                .map(|piece| dist_to_piece(p, piece))
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max)
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "tests are the independent oracle and may use platform maths"
)]
mod tests;
