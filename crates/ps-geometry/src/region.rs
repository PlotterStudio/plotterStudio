//! A filled region with a spatial index over its boundary, for asking whether a straight line stays
//! inside it.
//!
//! The question comes up once per candidate pair when pen-down links are chosen for a fill, so it
//! has to be cheap: the boundary edges are held in an R-tree and a query looks only at the edges
//! near the line. Everything is exact integer arithmetic on the µm grid.

use ps_core::PtUm;
use rstar::primitives::Line;
use rstar::{AABB, RTree};

use crate::boolean::Ring;

/// How far from the boundary a point may be, in µm, and still count as on it.
const ON_BOUNDARY_UM: i64 = 2;

/// The rings of a region (outer boundaries and holes) with an index of their edges.
#[derive(Debug, Clone)]
pub struct Region {
    edges: RTree<Line<[i64; 2]>>,
}

fn xy(p: PtUm) -> [i64; 2] {
    [p.x, p.y]
}

/// The sign of the turn from `a` to `b` to `c`: positive for left, negative for right.
fn turn(a: [i64; 2], b: [i64; 2], c: [i64; 2]) -> i128 {
    let cross = i128::from(b[0] - a[0]) * i128::from(c[1] - a[1])
        - i128::from(b[1] - a[1]) * i128::from(c[0] - a[0]);
    cross.signum()
}

/// True when the segments cross at a single point that is interior to both.
fn crosses(a: [i64; 2], b: [i64; 2], c: [i64; 2], d: [i64; 2]) -> bool {
    turn(a, b, c) * turn(a, b, d) < 0 && turn(c, d, a) * turn(c, d, b) < 0
}

/// Squared distance from `p` to the segment `a`..`b`.
#[expect(
    clippy::cast_precision_loss,
    reason = "grid coordinates are far below 2^53 µm, where i64 -> f64 is exact"
)]
fn distance2_to_segment(p: [i64; 2], a: [i64; 2], b: [i64; 2]) -> f64 {
    let (px, py) = (p[0] as f64, p[1] as f64);
    let (ax, ay) = (a[0] as f64, a[1] as f64);
    let (dx, dy) = (b[0] as f64 - ax, b[1] as f64 - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
    };
    let (qx, qy) = (ax + t * dx, ay + t * dy);
    (px - qx) * (px - qx) + (py - qy) * (py - qy)
}

impl Region {
    /// Indexes the edges of `rings`; a ring inside another is a hole.
    #[must_use]
    pub fn new(rings: &[Ring]) -> Self {
        let mut edges = Vec::new();
        for ring in rings.iter().filter(|r| r.len() >= 3) {
            for (i, &a) in ring.iter().enumerate() {
                let b = ring[(i + 1) % ring.len()];
                if a != b {
                    edges.push(Line::new(xy(a), xy(b)));
                }
            }
        }
        Self {
            edges: RTree::bulk_load(edges),
        }
    }

    /// True when `p` is inside the region or within a couple of µm of its boundary.
    #[must_use]
    pub fn contains(&self, p: PtUm) -> bool {
        let q = xy(p);
        let near = AABB::from_corners(
            [q[0] - ON_BOUNDARY_UM, q[1] - ON_BOUNDARY_UM],
            [q[0] + ON_BOUNDARY_UM, q[1] + ON_BOUNDARY_UM],
        );
        #[expect(
            clippy::cast_precision_loss,
            reason = "a couple of µm is exactly representable"
        )]
        let limit = (ON_BOUNDARY_UM * ON_BOUNDARY_UM) as f64;
        if self
            .edges
            .locate_in_envelope_intersecting(&near)
            .any(|e| distance2_to_segment(q, e.from, e.to) <= limit)
        {
            return true;
        }
        // Even-odd: a ray to the right crosses the edges an odd number of times from inside.
        let ray = AABB::from_corners([q[0], q[1]], [i64::MAX, q[1]]);
        let crossings = self
            .edges
            .locate_in_envelope_intersecting(&ray)
            .filter(|e| {
                let (a, b) = (e.from, e.to);
                if (a[1] > q[1]) == (b[1] > q[1]) {
                    return false;
                }
                // The edge crosses the line y = q.y; count it when the crossing is right of `q`.
                let side = turn(a, b, q);
                if b[1] > a[1] { side > 0 } else { side < 0 }
            })
            .count();
        crossings % 2 == 1
    }

    /// True when the straight line from `a` to `b` stays inside the region: no edge of the
    /// boundary crosses it and it is inside at its ends and at three points between.
    #[must_use]
    pub fn contains_segment(&self, a: PtUm, b: PtUm) -> bool {
        let (p, q) = (xy(a), xy(b));
        let span = AABB::from_corners(
            [p[0].min(q[0]), p[1].min(q[1])],
            [p[0].max(q[0]), p[1].max(q[1])],
        );
        if self
            .edges
            .locate_in_envelope_intersecting(&span)
            .any(|e| crosses(p, q, e.from, e.to))
        {
            return false;
        }
        // A line may pass through a vertex into the notch without crossing any edge properly, so
        // it is also sampled.
        (0..=4).all(|k| {
            let at = PtUm::new(a.x + (b.x - a.x) * k / 4, a.y + (b.y - a.y) * k / 4);
            self.contains(at)
        })
    }
}

#[cfg(test)]
mod tests;
