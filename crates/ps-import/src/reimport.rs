//! Re-import matching.
//!
//! When the source file changes, paths are matched old to new in three passes: (1) same source id,
//! (2) same structural fingerprint, (3) same layer and style with a Hausdorff distance under
//! 0.5 mm. Matched paths keep their operations and overrides, which is the caller's job; this
//! module only decides the pairing and summarises it so the user can review it before anything is
//! applied.

use ps_core::PathKey;
use ps_geometry::{Path, Point, flatten};

use crate::drawing::ImportedPath;

/// Largest Hausdorff distance for the proximity pass, in mm.
pub const PROXIMITY_MM: f64 = 0.5;
/// Spacing of the samples used to measure the distance, in mm.
const SAMPLE_STEP_MM: f64 = 0.05;
/// Most samples taken from one path, however long.
const MAX_SAMPLES: f64 = 4_096.0;
/// Tolerance when flattening curves for measuring, in mm.
const FLATTEN_TOL_MM: f64 = 0.005;

/// Samples a path at a fixed spacing along its flattened length, including both ends.
fn samples(path: &Path) -> Vec<Point> {
    let mut points = flatten(path, FLATTEN_TOL_MM);
    if path.closed
        && let Some(&first) = points.first()
    {
        points.push(first);
    }
    let mut out = Vec::new();
    let Some(&first) = points.first() else {
        return out;
    };
    out.push(first);
    let total: f64 = points.windows(2).map(|w| dist(w[0], w[1])).sum();
    let step_count = (total / SAMPLE_STEP_MM).ceil().clamp(1.0, MAX_SAMPLES);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a positive count clamped to 1..=4096"
    )]
    let step = step_count as usize;
    let spacing = total / step_count;
    let mut walked = 0.0;
    let mut next = spacing;
    for w in points.windows(2) {
        let len = dist(w[0], w[1]);
        while next <= walked + len + 1e-12 && len > 0.0 && out.len() <= step {
            let t = ((next - walked) / len).clamp(0.0, 1.0);
            out.push(Point::new(
                w[0].x + (w[1].x - w[0].x) * t,
                w[0].y + (w[1].y - w[0].y) * t,
            ));
            next += spacing;
        }
        walked += len;
    }
    out
}

fn dist(a: Point, b: Point) -> f64 {
    libm::hypot(b.x - a.x, b.y - a.y)
}

/// Distance from `p` to the polyline through `line`.
fn to_polyline(p: Point, line: &[Point]) -> f64 {
    if line.len() == 1 {
        return dist(p, line[0]);
    }
    line.windows(2)
        .map(|w| {
            let (dx, dy) = (w[1].x - w[0].x, w[1].y - w[0].y);
            let len2 = dx * dx + dy * dy;
            let t = if len2 == 0.0 {
                0.0
            } else {
                (((p.x - w[0].x) * dx + (p.y - w[0].y) * dy) / len2).clamp(0.0, 1.0)
            };
            dist(p, Point::new(w[0].x + dx * t, w[0].y + dy * t))
        })
        .fold(f64::INFINITY, f64::min)
}

/// The Hausdorff distance between two paths in mm, measured on points sampled every 0.05 mm
/// along their flattened outlines (so it is exact to about that spacing). An empty path is
/// infinitely far from everything.
#[must_use]
pub fn hausdorff(a: &Path, b: &Path) -> f64 {
    if a.segs.is_empty() || b.segs.is_empty() {
        return f64::INFINITY;
    }
    let (pa, pb) = (samples(a), samples(b));
    let directed = |from: &[Point], to: &[Point]| {
        from.iter()
            .map(|&p| to_polyline(p, to))
            .fold(0.0_f64, f64::max)
    };
    directed(&pa, &pb).max(directed(&pb, &pa))
}

/// Which pass matched a pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchPass {
    /// Same source id.
    SourceId,
    /// Same structural fingerprint.
    Fingerprint,
    /// Same layer and style, close geometry.
    Proximity,
}

/// One old path paired with a new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Matched {
    /// Index into the old paths.
    pub old: usize,
    /// Index into the new paths.
    pub new: usize,
    /// The pass that matched it.
    pub pass: MatchPass,
    /// Whether the geometry differs (so the user sees "changed").
    pub geometry_changed: bool,
}

/// The pairing of old and new paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reimport {
    /// The pairs, sorted by old index.
    pub matched: Vec<Matched>,
    /// New paths with no old counterpart.
    pub added: Vec<usize>,
    /// Old paths with no new counterpart.
    pub removed: Vec<usize>,
}

/// The counts shown in the re-import summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    /// Paths matched (changed or not).
    pub matched: usize,
    /// Matched paths whose geometry changed.
    pub changed_geometry: usize,
    /// Paths only in the new file.
    pub added: usize,
    /// Paths only in the old import.
    pub removed: usize,
}

impl Reimport {
    /// The summary counts.
    #[must_use]
    pub fn counts(&self) -> Counts {
        Counts {
            matched: self.matched.len(),
            changed_geometry: self.matched.iter().filter(|m| m.geometry_changed).count(),
            added: self.added.len(),
            removed: self.removed.len(),
        }
    }
}

fn same_geometry(a: &Path, b: &Path) -> bool {
    a == b || hausdorff(a, b) < 0.001
}

/// Matches the old import's paths to the new file's paths in three passes.
#[must_use]
pub fn match_paths(old: &[ImportedPath], new: &[ImportedPath]) -> Reimport {
    let mut old_taken = vec![false; old.len()];
    let mut new_taken = vec![false; new.len()];
    let mut matched: Vec<Matched> = Vec::new();

    let mut pair =
        |o: usize, n: usize, pass: MatchPass, taken: (&mut Vec<bool>, &mut Vec<bool>)| {
            taken.0[o] = true;
            taken.1[n] = true;
            matched.push(Matched {
                old: o,
                new: n,
                pass,
                geometry_changed: !same_geometry(&old[o].geom, &new[n].geom),
            });
        };

    // Passes 1 and 2: equal keys of the right kind.
    for (pass, is_kind) in [
        (
            MatchPass::SourceId,
            (|k: &PathKey| matches!(k, PathKey::Source(_))) as fn(&PathKey) -> bool,
        ),
        (MatchPass::Fingerprint, |k: &PathKey| {
            matches!(k, PathKey::Fingerprint(_))
        }),
    ] {
        for o in 0..old.len() {
            if old_taken[o] || !is_kind(&old[o].key) {
                continue;
            }
            if let Some(n) = (0..new.len()).find(|&n| !new_taken[n] && new[n].key == old[o].key) {
                pair(o, n, pass, (&mut old_taken, &mut new_taken));
            }
        }
    }

    // Pass 3: same layer and style within the proximity limit, nearest pair first, so the result
    // does not depend on the order of the paths.
    let mut candidates: Vec<(f64, usize, usize)> = Vec::new();
    for o in (0..old.len()).filter(|&o| !old_taken[o]) {
        for n in (0..new.len()).filter(|&n| !new_taken[n]) {
            if new[n].layer != old[o].layer || new[n].style != old[o].style {
                continue;
            }
            let d = hausdorff(&old[o].geom, &new[n].geom);
            if d < PROXIMITY_MM {
                candidates.push((d, o, n));
            }
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    for (_, o, n) in candidates {
        if !old_taken[o] && !new_taken[n] {
            pair(o, n, MatchPass::Proximity, (&mut old_taken, &mut new_taken));
        }
    }

    matched.sort_by_key(|m| m.old);
    Reimport {
        matched,
        added: (0..new.len()).filter(|&n| !new_taken[n]).collect(),
        removed: (0..old.len()).filter(|&o| !old_taken[o]).collect(),
    }
}

#[cfg(test)]
mod tests;
