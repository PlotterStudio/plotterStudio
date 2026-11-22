//! The knife: a straight cut through a path splits it where it crosses.
//!
//! An open path becomes open pieces. A closed shape cut in two places becomes two closed halves,
//! each closed along the cut and keeping the shape's winding; a closed shape cut in more places
//! becomes open pieces, one between each pair of neighbouring crossings.

use crate::flatten::flatten;
use crate::{Path, Point, Seg};

/// How finely curves are followed when looking for crossings, in millimetres.
const TOLERANCE: f64 = 1e-4;
/// Distances closer than this are the same place, in millimetres.
const SAME: f64 = 1e-9;

fn cross(a: (f64, f64), b: (f64, f64)) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

/// The distances along `path` at which it crosses the straight cut from `a` to `b`, ascending.
/// A path that only touches the cut at a vertex and stays on one side does not cross it.
#[must_use]
pub fn crossings(path: &Path, from_pt: Point, to_pt: Point) -> Vec<f64> {
    let r = (to_pt.x - from_pt.x, to_pt.y - from_pt.y);
    let cut_len2 = r.0 * r.0 + r.1 * r.1;
    if cut_len2 < SAME {
        return Vec::new();
    }
    let side = |q: Point| cross(r, (q.x - from_pt.x, q.y - from_pt.y));
    let mut found: Vec<f64> = Vec::new();
    let mut at = 0.0;
    for (from, seg) in path.segments() {
        let len = seg.length(from);
        let mini = match *seg {
            Seg::Line { to } => Path::polyline(&[from, to], false),
            Seg::Arc { to, center, ccw } => Path::new(from).arc_to(to, center, ccw),
            Seg::Cubic { c1, c2, to } => Path::new(from).cubic_to(c1, c2, to),
        };
        let pts = flatten(&mini, TOLERANCE);
        let lengths: Vec<f64> = pts
            .windows(2)
            .map(|w| crate::arc::dist(w[0], w[1]))
            .collect();
        let total: f64 = lengths.iter().sum();
        let mut walked = 0.0;
        for (w, l) in pts.windows(2).zip(&lengths) {
            let (s0, s1) = (side(w[0]), side(w[1]));
            // Zero counts as the non-negative side, so a vertex on the cut is crossed once.
            if (s0 < 0.0) != (s1 < 0.0) {
                let u = s0 / (s0 - s1);
                let hit = Point::new(
                    w[0].x + u * (w[1].x - w[0].x),
                    w[0].y + u * (w[1].y - w[0].y),
                );
                let t = ((hit.x - from_pt.x) * r.0 + (hit.y - from_pt.y) * r.1) / cut_len2;
                if (-SAME..=1.0 + SAME).contains(&t) {
                    let along = if total > 0.0 {
                        (walked + u * l) / total
                    } else {
                        0.0
                    };
                    found.push(at + along * len);
                }
            }
            walked += l;
        }
        at += len;
    }
    // Two crossings at the same place are a touch from one side, not a cut.
    found.sort_by(f64::total_cmp);
    let mut out: Vec<f64> = Vec::new();
    let mut i = 0;
    while i < found.len() {
        if i + 1 < found.len() && (found[i + 1] - found[i]).abs() < SAME {
            i += 2;
        } else {
            out.push(found[i]);
            i += 1;
        }
    }
    // On a closed path the end is the start.
    if path.closed {
        for d in &mut out {
            if (*d - at).abs() < SAME {
                *d = 0.0;
            }
        }
        out.sort_by(f64::total_cmp);
    }
    out
}

/// Joins open pieces end to start into one path.
fn joined(parts: &[&Path]) -> Path {
    let mut path = Path::new(parts[0].start);
    for part in parts {
        path.segs.extend(part.segs.iter().copied());
    }
    path
}

fn closed_by_chord(path: Path) -> Path {
    let start = path.start;
    if path.end() == start {
        path.closed()
    } else {
        path.line_to(start).closed()
    }
}

/// Cuts `path` along the straight line from `a` to `b`. `None` when the cut does not cross it.
#[must_use]
pub fn knife(path: &Path, a: Point, b: Point) -> Option<Vec<Path>> {
    let cuts = crossings(path, a, b);
    if cuts.is_empty() {
        return None;
    }
    if !path.closed {
        let pieces = path.split_at_lengths(&cuts);
        return (pieces.len() > 1).then_some(pieces);
    }
    // A closed path: split as if open, then rejoin the ends across the original start.
    let mut open = path.clone();
    open.closed = false;
    let mut pieces: Vec<Path> = open
        .split_at_lengths(&cuts)
        .into_iter()
        .filter(|p| !p.segs.is_empty())
        .collect();
    let starts_at_cut = cuts.first().is_some_and(|d| *d < SAME);
    if !starts_at_cut && pieces.len() > 1 {
        // The last piece runs on into the first across the start.
        let first = pieces.remove(0);
        let last = pieces.pop()?;
        pieces.push(joined(&[&last, &first]));
    }
    if pieces.len() < 2 {
        return None;
    }
    if cuts.len() == 2 {
        return Some(pieces.into_iter().map(closed_by_chord).collect());
    }
    Some(pieces)
}

#[cfg(test)]
mod tests;
