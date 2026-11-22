//! Flattening paths to polylines within a tolerance.
//!
//! Arcs are subdivided from the exact sagitta bound; cubics use the deterministic [`cubic::flatten`].

use kurbo::{CubicBez, Point};
use ps_core::{GridError, PtUm};

use crate::path::{Path, Seg};
use crate::{arc, cubic};

/// Flattens a path to polyline vertices, none of which deviates from the curve by more than
/// `tol` mm.
///
/// An open path keeps both end points; a closed path does not repeat its start at the end.
#[must_use]
pub fn flatten(path: &Path, tol: f64) -> Vec<Point> {
    let mut out = vec![path.start];
    for (from, seg) in path.segments() {
        match *seg {
            Seg::Line { to } => out.push(to),
            Seg::Arc { to, center, ccw } => flatten_arc(&mut out, from, to, center, ccw, tol),
            Seg::Cubic { c1, c2, to } => {
                out.extend(cubic::flatten(&CubicBez::new(from, c1, c2, to), tol));
            }
        }
    }
    if path.closed && out.len() > 1 && out.last() == Some(&path.start) {
        out.pop();
    }
    out
}

fn flatten_arc(out: &mut Vec<Point>, from: Point, to: Point, center: Point, ccw: bool, tol: f64) {
    let radius = arc::dist(from, center);
    let total = arc::sweep(from, to, center, ccw);
    let step = arc::max_step(radius, tol);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "ceil of a bounded positive ratio; arcs never need more than u32::MAX chords"
    )]
    let n = ((total.abs() / step).ceil().max(1.0)) as u32;
    let a0 = arc::angle_of(from, center);
    for i in 1..n {
        let angle = a0 + total * (f64::from(i) / f64::from(n));
        out.push(arc::point_at(center, radius, angle));
    }
    out.push(to);
}

/// Flattens a path and snaps the vertices onto the µm grid, removing consecutive duplicates.
///
/// # Errors
/// [`GridError`] if a coordinate is not finite or is out of range.
pub fn flatten_to_grid(path: &Path, tol: f64) -> Result<Vec<PtUm>, GridError> {
    let mut out: Vec<PtUm> = Vec::new();
    for p in flatten(path, tol) {
        let g = PtUm::from_mm(p.x, p.y)?;
        if out.last() != Some(&g) {
            out.push(g);
        }
    }
    if path.closed && out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    Ok(out)
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "tests are the independent oracle and may use platform maths"
)]
mod tests;
