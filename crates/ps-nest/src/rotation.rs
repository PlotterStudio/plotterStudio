//! Pure helpers: rotation sets, grid-exact rotation, bounding boxes, convex hulls and the seed
//! rule. Rotation uses `libm`, so results are identical on every platform.

use ps_core::{ContentHash, PtUm};
use serde::{Deserialize, Serialize};

/// Which rotations an item may be placed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rotations {
    /// Upright only.
    None,
    /// 0°, 90°, 180° and 270°.
    Quarter,
    /// Any multiple of 5°.
    Free5,
    /// Grain lock: 0° and 180° only, so the grain keeps its direction.
    GrainLock,
}

/// Returns the allowed rotation angles in degrees, counter-clockwise, in a fixed order.
#[must_use]
pub fn angles(rotations: Rotations) -> Vec<f64> {
    match rotations {
        Rotations::None => vec![0.0],
        Rotations::Quarter => vec![0.0, 90.0, 180.0, 270.0],
        Rotations::GrainLock => vec![0.0, 180.0],
        Rotations::Free5 => (0..72).map(|i| f64::from(i) * 5.0).collect(),
    }
}

/// Rotates a ring counter-clockwise about the origin and rounds once to the µm grid.
///
/// Multiples of 90° are exact integer operations.
#[must_use]
pub fn rotate_ring(ring: &[PtUm], deg: f64) -> Vec<PtUm> {
    let turns = deg.rem_euclid(360.0);
    let quarters = turns / 90.0;
    if (quarters - quarters.round()).abs() < 1e-12 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "quarters is within 0..=4 and integral here"
        )]
        let q = quarters.round() as u8 % 4;
        return ring
            .iter()
            .map(|p| match q {
                0 => *p,
                1 => PtUm::new(-p.y, p.x),
                2 => PtUm::new(-p.x, -p.y),
                _ => PtUm::new(p.y, -p.x),
            })
            .collect();
    }
    let (sin, cos) = (libm::sin(turns.to_radians()), libm::cos(turns.to_radians()));
    ring.iter()
        .map(|p| {
            let (x, y) = (to_f64(p.x), to_f64(p.y));
            PtUm::new(round_um(x * cos - y * sin), round_um(x * sin + y * cos))
        })
        .collect()
}

#[expect(
    clippy::cast_precision_loss,
    reason = "grid coordinates are far below 2^53 µm, where i64 -> f64 is exact"
)]
pub(crate) fn to_f64(v: i64) -> f64 {
    v as f64
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "rotated coordinates stay within a few metres; `as` saturates anyway"
)]
fn round_um(v: f64) -> i64 {
    v.round() as i64
}

/// An axis-aligned bounding box on the µm grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// The corner with the smallest coordinates.
    pub min: PtUm,
    /// The corner with the largest coordinates.
    pub max: PtUm,
}

impl Bounds {
    /// Width in µm.
    #[must_use]
    pub fn width(&self) -> i64 {
        self.max.x - self.min.x
    }

    /// Height in µm.
    #[must_use]
    pub fn height(&self) -> i64 {
        self.max.y - self.min.y
    }

    /// The smallest box containing both.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            min: PtUm::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: PtUm::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }

    /// This box moved by `by`.
    #[must_use]
    pub fn translated(self, by: PtUm) -> Self {
        Self {
            min: self.min + by,
            max: self.max + by,
        }
    }

    /// True when `inner` lies inside this box (edges may touch).
    #[must_use]
    pub fn contains(&self, inner: &Self) -> bool {
        inner.min.x >= self.min.x
            && inner.min.y >= self.min.y
            && inner.max.x <= self.max.x
            && inner.max.y <= self.max.y
    }
}

/// Returns the bounding box of a ring, or `None` for an empty one.
#[must_use]
pub fn bbox(ring: &[PtUm]) -> Option<Bounds> {
    let first = *ring.first()?;
    Some(ring.iter().fold(
        Bounds {
            min: first,
            max: first,
        },
        |b, p| Bounds {
            min: PtUm::new(b.min.x.min(p.x), b.min.y.min(p.y)),
            max: PtUm::new(b.max.x.max(p.x), b.max.y.max(p.y)),
        },
    ))
}

fn cross(o: PtUm, a: PtUm, b: PtUm) -> i128 {
    i128::from(a.x - o.x) * i128::from(b.y - o.y) - i128::from(a.y - o.y) * i128::from(b.x - o.x)
}

/// Returns the convex hull, counter-clockwise from the lowest-leftmost point, without collinear
/// points (Andrew's monotone chain). Fewer than three distinct points are returned sorted.
#[must_use]
pub fn convex_hull(points: &[PtUm]) -> Vec<PtUm> {
    let mut pts = points.to_vec();
    pts.sort_unstable();
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut hull: Vec<PtUm> = Vec::with_capacity(pts.len() * 2);
    for &p in &pts {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0 {
            hull.pop();
        }
        hull.push(p);
    }
    let lower = hull.len() + 1;
    for &p in pts.iter().rev().skip(1) {
        while hull.len() >= lower && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0 {
            hull.pop();
        }
        hull.push(p);
    }
    hull.pop();
    hull
}

/// Derives the search seed from the job's content hash, so the same input always
/// gives the same layout.
#[must_use]
pub fn seed_from_hash(hash: &ContentHash) -> u64 {
    let bytes = hash.as_bytes();
    let head = bytes.first_chunk::<8>().copied().unwrap_or([0; 8]);
    u64::from_le_bytes(head)
}

#[cfg(test)]
mod tests;
