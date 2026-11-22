//! Geometry repair.
//!
//! Steps, in order: remove zero-length segments; close nearly closed filled paths; merge
//! duplicates (keeping the topmost); optionally join touching open paths; normalise the winding of
//! filled contours; flag self-intersections. Every repair is listed in the report so the import
//! dialog can show it and the app can undo the whole import as one step.
//!
//! Deviation from the original design: duplicates are found by a canonical-form map (vertex lists
//! normalised for start point and direction, on the µm grid) instead of R*-tree candidates
//! followed by pairwise comparison. The result is identical and the work is linear.

use std::collections::BTreeMap;

use ps_core::{PathKey, PtUm};
use ps_geometry::{Path, Point, Seg, flatten_to_grid};
use rstar::primitives::{GeomWithData, Rectangle};
use rstar::{AABB, RTree};

use crate::drawing::ImportedPath;

/// Tolerance for converting curves to grid polylines, in mm.
const GRID_TOL: f64 = 0.002;

/// What repair may do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RepairOptions {
    /// Largest gap that closes a filled path, in mm.
    pub close_tolerance_mm: f64,
    /// Segments shorter than this are removed, in mm.
    pub zero_length_mm: f64,
    /// Whether to join touching open paths of the same style (off by default).
    pub join_open: bool,
    /// How close end points must be to join, in mm.
    pub join_tolerance_mm: f64,
}

impl Default for RepairOptions {
    fn default() -> Self {
        Self {
            close_tolerance_mm: 0.05,
            zero_length_mm: 0.001,
            join_open: false,
            join_tolerance_mm: 0.05,
        }
    }
}

/// One thing repair did or noticed.
#[derive(Debug, Clone, PartialEq)]
pub enum RepairAction {
    /// A nearly closed filled path was closed.
    Closed {
        /// The path.
        key: PathKey,
        /// The gap that was closed, in mm.
        gap_mm: f64,
    },
    /// Zero-length segments were removed.
    ZeroLengthRemoved {
        /// The path.
        key: PathKey,
        /// How many segments.
        count: usize,
    },
    /// A duplicate was dropped in favour of the topmost copy.
    DuplicateMerged {
        /// The path that was kept.
        kept: PathKey,
        /// The path that was removed.
        removed: PathKey,
    },
    /// Two open paths were joined.
    Joined {
        /// The path that absorbed the other.
        into: PathKey,
        /// The path that was absorbed.
        from: PathKey,
    },
    /// A path crosses itself (reported, not changed).
    SelfIntersection {
        /// The path.
        key: PathKey,
        /// How many crossings.
        count: usize,
    },
    /// A filled contour's winding was reversed.
    Rewound {
        /// The path.
        key: PathKey,
    },
}

/// The repaired paths and what was done.
#[derive(Debug, Clone, PartialEq)]
pub struct Repaired {
    /// The paths after repair, in their original order.
    pub items: Vec<ImportedPath>,
    /// Every repair and finding.
    pub report: Vec<RepairAction>,
}

fn dist(a: Point, b: Point) -> f64 {
    libm::hypot(b.x - a.x, b.y - a.y)
}

// ---- zero-length segments ----

fn drop_zero_length(item: &mut ImportedPath, min_mm: f64, report: &mut Vec<RepairAction>) {
    let kept: Vec<Seg> = item
        .geom
        .segments()
        .filter(|(from, seg)| seg.length(*from) >= min_mm)
        .map(|(_, seg)| *seg)
        .collect();
    let removed = item.geom.segs.len() - kept.len();
    if removed > 0 {
        item.geom.segs = kept;
        report.push(RepairAction::ZeroLengthRemoved {
            key: item.key.clone(),
            count: removed,
        });
    }
}

// ---- closing ----

fn close_gap(item: &mut ImportedPath, tolerance_mm: f64, report: &mut Vec<RepairAction>) {
    let g = &mut item.geom;
    if g.closed || item.style.fill.is_none() || g.segs.is_empty() {
        return;
    }
    let gap = dist(g.start, g.end());
    if gap >= tolerance_mm {
        return;
    }
    if gap > 0.0 {
        let start = g.start;
        if let Some(last) = g.segs.last_mut() {
            match last {
                Seg::Line { to } | Seg::Arc { to, .. } | Seg::Cubic { to, .. } => *to = start,
            }
        }
    }
    g.closed = true;
    report.push(RepairAction::Closed {
        key: item.key.clone(),
        gap_mm: gap,
    });
}

// ---- duplicates ----

fn xy(p: PtUm) -> (i64, i64) {
    (p.x, p.y)
}

/// A vertex list normalised for start point and direction, so equal shapes compare equal.
fn canonical(points: &[PtUm], closed: bool) -> Vec<(i64, i64)> {
    let forward: Vec<(i64, i64)> = points.iter().map(|&p| xy(p)).collect();
    let mut backward = forward.clone();
    backward.reverse();
    if !closed {
        return forward.min(backward);
    }
    let rotated = |v: &[(i64, i64)]| -> Vec<(i64, i64)> {
        let at = v
            .iter()
            .enumerate()
            .min_by_key(|&(_, p)| *p)
            .map_or(0, |(i, _)| i);
        v[at..].iter().chain(&v[..at]).copied().collect()
    };
    rotated(&forward).min(rotated(&backward))
}

/// Whether a path is closed, with its vertices normalised for start point and direction.
type Shape = (bool, Vec<(i64, i64)>);

fn merge_duplicates(items: Vec<ImportedPath>, report: &mut Vec<RepairAction>) -> Vec<ImportedPath> {
    let shapes: Vec<Shape> = items
        .iter()
        .map(|i| {
            let pts = flatten_to_grid(&i.geom, GRID_TOL).unwrap_or_default();
            (i.geom.closed, canonical(&pts, i.geom.closed))
        })
        .collect();
    let keys: Vec<PathKey> = items.iter().map(|i| i.key.clone()).collect();
    // The topmost (last) path with each shape wins.
    let mut winner: BTreeMap<&Shape, usize> = BTreeMap::new();
    for (i, shape) in shapes.iter().enumerate() {
        winner.insert(shape, i);
    }
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.into_iter().enumerate() {
        let top = winner[&shapes[i]];
        if top == i {
            out.push(item);
        } else {
            report.push(RepairAction::DuplicateMerged {
                kept: keys[top].clone(),
                removed: keys[i].clone(),
            });
        }
    }
    out
}

// ---- joining ----

fn orientations(a: &Path, b: &Path, tol: f64) -> Option<(bool, bool)> {
    // (reverse a, reverse b) so that a ends where b starts.
    if dist(a.end(), b.start) <= tol {
        Some((false, false))
    } else if dist(a.end(), b.end()) <= tol {
        Some((false, true))
    } else if dist(a.start, b.start) <= tol {
        Some((true, false))
    } else if dist(a.start, b.end()) <= tol {
        Some((true, true))
    } else {
        None
    }
}

fn join_open(
    items: Vec<ImportedPath>,
    tol: f64,
    report: &mut Vec<RepairAction>,
) -> Vec<ImportedPath> {
    let mut items: Vec<Option<ImportedPath>> = items.into_iter().map(Some).collect();
    loop {
        let mut joined = false;
        'scan: for i in 0..items.len() {
            for j in (i + 1)..items.len() {
                let (Some(a), Some(b)) = (&items[i], &items[j]) else {
                    continue;
                };
                if a.geom.closed
                    || b.geom.closed
                    || a.geom.segs.is_empty()
                    || b.geom.segs.is_empty()
                    || a.style != b.style
                    || a.layer != b.layer
                {
                    continue;
                }
                let Some((flip_a, flip_b)) = orientations(&a.geom, &b.geom, tol) else {
                    continue;
                };
                let first = if flip_a {
                    a.geom.reversed()
                } else {
                    a.geom.clone()
                };
                let second = if flip_b {
                    b.geom.reversed()
                } else {
                    b.geom.clone()
                };
                let mut path = first;
                path.segs.extend(second.segs);
                if dist(path.start, path.end()) <= tol && path.segs.len() > 1 {
                    let start = path.start;
                    if let Some(last) = path.segs.last_mut() {
                        match last {
                            Seg::Line { to } | Seg::Arc { to, .. } | Seg::Cubic { to, .. } => {
                                *to = start;
                            }
                        }
                    }
                    path.closed = true;
                }
                let (into, from) = (a.key.clone(), b.key.clone());
                let mut merged = a.clone();
                merged.geom = path;
                report.push(RepairAction::Joined { into, from });
                items[i] = Some(merged);
                items[j] = None;
                joined = true;
                break 'scan;
            }
        }
        if !joined {
            break;
        }
    }
    items.into_iter().flatten().collect()
}

// ---- winding ----

fn point_in_ring(p: PtUm, ring: &[PtUm]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let mut inside = false;
    for (i, &a) in ring.iter().enumerate() {
        let b = ring[(i + 1) % ring.len()];
        if (a.y > p.y) != (b.y > p.y) {
            let cross = i128::from(b.x - a.x) * i128::from(p.y - a.y)
                - i128::from(p.x - a.x) * i128::from(b.y - a.y);
            if (b.y > a.y && cross > 0) || (b.y < a.y && cross < 0) {
                inside = !inside;
            }
        }
    }
    inside
}

fn normalise_winding(items: &mut [ImportedPath], report: &mut Vec<RepairAction>) {
    let rings: Vec<Option<Vec<PtUm>>> = items
        .iter()
        .map(|i| {
            (i.geom.closed && i.style.fill.is_some())
                .then(|| flatten_to_grid(&i.geom, GRID_TOL).ok())
                .flatten()
        })
        .collect();
    for i in 0..items.len() {
        let Some(ring) = &rings[i] else {
            continue;
        };
        let Some(&probe) = ring.first() else {
            continue;
        };
        let depth = rings
            .iter()
            .enumerate()
            .filter(|(j, other)| {
                *j != i
                    && other
                        .as_ref()
                        .is_some_and(|o| point_in_ring(probe, o) && ring_area(o) > ring_area(ring))
            })
            .count();
        let want_ccw = depth % 2 == 0;
        let is_ccw = items[i].geom.signed_area() > 0.0;
        if is_ccw != want_ccw {
            items[i].geom = items[i].geom.reversed();
            report.push(RepairAction::Rewound {
                key: items[i].key.clone(),
            });
        }
    }
}

fn ring_area(ring: &[PtUm]) -> i128 {
    let mut twice: i128 = 0;
    for (i, &a) in ring.iter().enumerate() {
        let b = ring[(i + 1) % ring.len()];
        twice += i128::from(a.x) * i128::from(b.y) - i128::from(b.x) * i128::from(a.y);
    }
    twice.abs()
}

// ---- self-intersections ----

fn orient(a: PtUm, b: PtUm, c: PtUm) -> i128 {
    i128::from(b.x - a.x) * i128::from(c.y - a.y) - i128::from(b.y - a.y) * i128::from(c.x - a.x)
}

/// True when two segments cross at a point interior to both.
fn properly_cross(a: PtUm, b: PtUm, c: PtUm, d: PtUm) -> bool {
    orient(a, b, c).signum() * orient(a, b, d).signum() < 0
        && orient(c, d, a).signum() * orient(c, d, b).signum() < 0
}

fn crossings(points: &[PtUm], closed: bool) -> usize {
    let mut edges: Vec<(PtUm, PtUm)> = points.windows(2).map(|w| (w[0], w[1])).collect();
    if closed && points.len() > 2 {
        edges.push((points[points.len() - 1], points[0]));
    }
    let boxes: Vec<GeomWithData<Rectangle<[i64; 2]>, usize>> = edges
        .iter()
        .enumerate()
        .map(|(i, (a, b))| {
            GeomWithData::new(
                Rectangle::from_corners([a.x.min(b.x), a.y.min(b.y)], [a.x.max(b.x), a.y.max(b.y)]),
                i,
            )
        })
        .collect();
    let tree = RTree::bulk_load(boxes);
    let mut count = 0;
    for (i, (a, b)) in edges.iter().enumerate() {
        let envelope =
            AABB::from_corners([a.x.min(b.x), a.y.min(b.y)], [a.x.max(b.x), a.y.max(b.y)]);
        for hit in tree.locate_in_envelope_intersecting(&envelope) {
            let j = hit.data;
            let adjacent = j <= i + 1 || (closed && i == 0 && j + 1 == edges.len());
            if !adjacent && properly_cross(*a, *b, edges[j].0, edges[j].1) {
                count += 1;
            }
        }
    }
    count
}

fn flag_self_intersections(items: &[ImportedPath], report: &mut Vec<RepairAction>) {
    for item in items {
        let Ok(points) = flatten_to_grid(&item.geom, GRID_TOL) else {
            continue;
        };
        let count = crossings(&points, item.geom.closed);
        if count > 0 {
            report.push(RepairAction::SelfIntersection {
                key: item.key.clone(),
                count,
            });
        }
    }
}

/// Repairs imported paths. The order of the surviving paths is unchanged.
#[must_use]
pub fn repair(items: Vec<ImportedPath>, options: &RepairOptions) -> Repaired {
    let mut report = Vec::new();
    let mut items = items;
    for item in &mut items {
        drop_zero_length(item, options.zero_length_mm, &mut report);
    }
    for item in &mut items {
        close_gap(item, options.close_tolerance_mm, &mut report);
    }
    let mut items = merge_duplicates(items, &mut report);
    if options.join_open {
        items = join_open(items, options.join_tolerance_mm, &mut report);
    }
    normalise_winding(&mut items, &mut report);
    flag_self_intersections(&items, &mut report);
    Repaired { items, report }
}

#[cfg(test)]
mod tests;
