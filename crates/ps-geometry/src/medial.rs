//! Medial axis (centre lines) of thin polygons, from a segment Voronoi diagram.
//!
//! The axis of a polygon is the set of Voronoi edges between two of its sides that lie inside
//! it. Edges that end on a polygon vertex are the bisectors that run into corners ("spurs") and
//! are not part of the centre line, so they are dropped. Edges between a reflex vertex and a
//! side are parabolas; they are discretised with the library's own routine.

use std::collections::{BTreeMap, BTreeSet};

use boostvoronoi::prelude::{
    Builder, Line, Point as SitePoint, SourceCategory, VoronoiVisualUtils,
};
use boostvoronoi::utils::visual_utils::SimpleAffine;
use ps_core::PtUm;

use crate::boolean::Ring;
use crate::offset::um_i64;

/// Above this many (vertex × segment) pairs the inscribed radius is taken from the kept axis
/// only, instead of from every Voronoi vertex, to bound the work.
const RADIUS_BUDGET: usize = 4_000_000;

/// The centre lines of a polygon and how wide it gets.
#[derive(Debug, Clone, PartialEq)]
pub struct Medial {
    /// Centre lines as polylines on the µm grid, in a deterministic order.
    pub chains: Vec<Vec<PtUm>>,
    /// The radius of the largest circle that fits inside the polygon, in µm.
    pub max_radius_um: f64,
}

/// Why a medial axis could not be computed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MedialError {
    /// The shape has no ring with at least three distinct vertices.
    #[error("shape has no usable outline")]
    Degenerate,
    /// A coordinate does not fit the 32-bit integer range the Voronoi engine works in.
    #[error("coordinate is outside the 32-bit range of the Voronoi engine")]
    OutOfRange,
    /// The Voronoi engine rejected the input, for example because two sides cross.
    #[error("voronoi engine failed: {0}")]
    Engine(String),
}

fn engine_err(e: impl std::fmt::Display) -> MedialError {
    MedialError::Engine(e.to_string())
}

fn dedupe(ring: &[PtUm]) -> Vec<PtUm> {
    let mut out: Vec<PtUm> = Vec::with_capacity(ring.len());
    for &p in ring {
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

fn to_i32(v: i64) -> Result<i32, MedialError> {
    i32::try_from(v).map_err(|_| MedialError::OutOfRange)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "grid coordinates are far below 2^53 µm, where i64 -> f64 is exact"
)]
fn f(v: i64) -> f64 {
    v as f64
}

/// Even-odd point-in-polygon over all rings (outer ring and holes).
fn inside(rings: &[Vec<PtUm>], x: f64, y: f64) -> bool {
    let mut hit = false;
    for ring in rings {
        for (i, a) in ring.iter().enumerate() {
            let b = ring[(i + 1) % ring.len()];
            let (ay, by) = (f(a.y), f(b.y));
            if (ay > y) != (by > y) {
                let cross_x = f(a.x) + (y - ay) / (by - ay) * (f(b.x) - f(a.x));
                if x < cross_x {
                    hit = !hit;
                }
            }
        }
    }
    hit
}

fn dist_to_segment(px: f64, py: f64, from: [i32; 2], to: [i32; 2]) -> f64 {
    let (ax, ay) = (f64::from(from[0]), f64::from(from[1]));
    let (dx, dy) = (f64::from(to[0]) - ax, f64::from(to[1]) - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
    };
    libm::hypot(px - (ax + dx * t), py - (ay + dy * t))
}

/// One kept Voronoi edge: its two vertex ids and the polyline between them.
struct AxisEdge {
    from: usize,
    to: usize,
    points: Vec<[f64; 2]>,
}

/// Computes the medial axis of a polygon with holes.
///
/// `tol_um` bounds the distance between a parabolic edge and its polyline.
///
/// # Errors
/// [`MedialError`] if the shape has no usable outline, a coordinate is out of range, or the
/// Voronoi engine fails.
pub fn medial_axis(shape: &[Ring], tol_um: f64) -> Result<Medial, MedialError> {
    let rings: Vec<Vec<PtUm>> = shape
        .iter()
        .map(|r| dedupe(r))
        .filter(|r| r.len() >= 3)
        .collect();
    let mut segs: Vec<[i32; 4]> = Vec::new();
    for ring in &rings {
        for (i, a) in ring.iter().enumerate() {
            let b = ring[(i + 1) % ring.len()];
            segs.push([to_i32(a.x)?, to_i32(a.y)?, to_i32(b.x)?, to_i32(b.y)?]);
        }
    }
    if segs.is_empty() {
        return Err(MedialError::Degenerate);
    }

    let diagram = Builder::<i32>::default()
        .with_segments(segs.iter())
        .map_err(engine_err)?
        .build()
        .map_err(engine_err)?;

    let mut kept: Vec<AxisEdge> = Vec::new();
    for edge in diagram.edges() {
        let id = edge.id();
        if id.usize() > edge.twin().map_err(engine_err)?.usize()
            || !edge.is_primary()
            || !diagram.edge_is_finite(id).map_err(engine_err)?
        {
            continue;
        }
        let (Some(v0), Some(v1)) = (
            edge.vertex0(),
            diagram.edge_get_vertex1(id).map_err(engine_err)?,
        ) else {
            continue;
        };
        let a = diagram.vertex(v0).map_err(engine_err)?;
        let b = diagram.vertex(v1).map_err(engine_err)?;
        // Bisectors that end on an input vertex run into a corner: spurs, not centre lines.
        if a.is_site_point() || b.is_site_point() {
            continue;
        }
        if !inside(&rings, a.x(), a.y()) || !inside(&rings, b.x(), b.y()) {
            continue;
        }
        let mut points = vec![[a.x(), a.y()], [b.x(), b.y()]];
        if edge.is_curved() {
            discretize(&diagram, id, &segs, tol_um, &mut points).map_err(engine_err)?;
        }
        kept.push(AxisEdge {
            from: v0.usize(),
            to: v1.usize(),
            points,
        });
    }

    let max_radius_um = max_radius(&diagram, &rings, &segs, &kept);
    Ok(Medial {
        chains: chain(&kept),
        max_radius_um,
    })
}

/// Replaces the two end points of a parabolic edge by a polyline within `tol_um` of it.
fn discretize(
    diagram: &boostvoronoi::prelude::Diagram,
    id: boostvoronoi::prelude::EdgeIndex,
    segs: &[[i32; 4]],
    tol_um: f64,
    points: &mut Vec<[f64; 2]>,
) -> Result<(), boostvoronoi::BvError> {
    let twin = diagram.edge_get_twin(id)?;
    let mut point_site: Option<SitePoint<i32>> = None;
    let mut segment_site: Option<Line<i32>> = None;
    for cell_id in [diagram.edge_get_cell(id)?, diagram.edge_get_cell(twin)?] {
        let cell = diagram.cell(cell_id)?;
        let s = segs[cell.source_index().usize()];
        match cell.source_category() {
            SourceCategory::Segment => segment_site = Some(Line::from(s)),
            SourceCategory::SegmentStart => point_site = Some(SitePoint { x: s[0], y: s[1] }),
            SourceCategory::SegmentEnd => point_site = Some(SitePoint { x: s[2], y: s[3] }),
            SourceCategory::SinglePoint => {}
        }
    }
    if let (Some(p), Some(l)) = (point_site, segment_site) {
        VoronoiVisualUtils::discretize(&p, &l, tol_um, &SimpleAffine::default(), points);
    }
    Ok(())
}

/// Radius of the largest inscribed circle: the farthest any Voronoi vertex inside the polygon
/// is from its nearest side.
fn max_radius(
    diagram: &boostvoronoi::prelude::Diagram,
    rings: &[Vec<PtUm>],
    segs: &[[i32; 4]],
    kept: &[AxisEdge],
) -> f64 {
    let candidates: Vec<(f64, f64)> =
        if diagram.vertices().len().saturating_mul(segs.len()) <= RADIUS_BUDGET {
            diagram
                .vertices()
                .iter()
                .filter(|v| !v.is_site_point() && inside(rings, v.x(), v.y()))
                .map(|v| (v.x(), v.y()))
                .collect()
        } else {
            kept.iter()
                .flat_map(|e| e.points.iter().map(|p| (p[0], p[1])))
                .collect()
        };
    candidates
        .iter()
        .map(|&(x, y)| {
            segs.iter()
                .map(|s| dist_to_segment(x, y, [s[0], s[1]], [s[2], s[3]]))
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max)
}

/// Joins kept edges into maximal polylines, breaking at junctions.
fn chain(kept: &[AxisEdge]) -> Vec<Vec<PtUm>> {
    let mut at: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, e) in kept.iter().enumerate() {
        at.entry(e.from).or_default().push(i);
        at.entry(e.to).or_default().push(i);
    }
    let mut used: BTreeSet<usize> = BTreeSet::new();
    let mut out: Vec<Vec<PtUm>> = Vec::new();

    let walk = |start: usize, first: usize, used: &mut BTreeSet<usize>| {
        let mut points: Vec<PtUm> = Vec::new();
        let (mut node, mut edge) = (start, first);
        loop {
            used.insert(edge);
            let e = &kept[edge];
            let forward = e.from == node;
            let run: Vec<[f64; 2]> = if forward {
                e.points.clone()
            } else {
                e.points.iter().rev().copied().collect()
            };
            for p in run {
                let g = PtUm::new(um_i64(p[0]), um_i64(p[1]));
                if points.last() != Some(&g) {
                    points.push(g);
                }
            }
            node = if forward { e.to } else { e.from };
            let next = at[&node].iter().copied().find(|i| !used.contains(i));
            match next {
                Some(n) if at[&node].len() == 2 => edge = n,
                _ => break,
            }
        }
        points
    };

    // Open chains start at junctions and dead ends; what remains afterwards are closed loops.
    for (&node, edges) in &at {
        if edges.len() == 2 {
            continue;
        }
        for &first in edges {
            if !used.contains(&first) {
                out.push(walk(node, first, &mut used));
            }
        }
    }
    for (&node, edges) in &at {
        for &first in edges {
            if !used.contains(&first) {
                out.push(walk(node, first, &mut used));
            }
        }
    }
    out.retain(|c| c.len() >= 2);
    out
}

#[cfg(test)]
mod tests;
