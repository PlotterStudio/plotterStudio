//! Safety checks.
//!
//! * Release safety: for every through-cut contour, any other block whose paths lie inside (or
//!   cross into) the region it frees must run before it. Each becomes a "must run before" edge.
//! * Machine limits: every move inside the work area and outside keep-outs; force within the
//!   profile range (values are clamped and reported, never silently changed).
//!
//! All tests are exact integer geometry on the µm grid.

use ps_core::PtUm;

use crate::order::point_in_ring;
use crate::schedule::Edge;

/// An axis-aligned rectangle in µm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left.
    pub x0: i64,
    /// Top.
    pub y0: i64,
    /// Right.
    pub x1: i64,
    /// Bottom.
    pub y1: i64,
}

/// A block that frees regions: its closed through-cut contours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cutter {
    /// The block.
    pub block: usize,
    /// The closed contours it cuts.
    pub rings: Vec<Vec<PtUm>>,
}

/// A block's drawn polylines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drawn {
    /// The block.
    pub block: usize,
    /// Its paths.
    pub paths: Vec<Vec<PtUm>>,
}

fn orient(a: PtUm, b: PtUm, c: PtUm) -> i128 {
    i128::from(b.x - a.x) * i128::from(c.y - a.y) - i128::from(b.y - a.y) * i128::from(c.x - a.x)
}

fn within(a: PtUm, b: PtUm, p: PtUm) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// True when the closed segments `a-b` and `c-d` share at least one point.
#[must_use]
pub fn segments_cross(a: PtUm, b: PtUm, c: PtUm, d: PtUm) -> bool {
    let (o1, o2, o3, o4) = (
        orient(a, b, c),
        orient(a, b, d),
        orient(c, d, a),
        orient(c, d, b),
    );
    if o1.signum() * o2.signum() < 0 && o3.signum() * o4.signum() < 0 {
        return true;
    }
    (o1 == 0 && within(a, b, c))
        || (o2 == 0 && within(a, b, d))
        || (o3 == 0 && within(c, d, a))
        || (o4 == 0 && within(c, d, b))
}

fn touches_region(path: &[PtUm], ring: &[PtUm]) -> bool {
    if path.iter().any(|&p| point_in_ring(p, ring)) {
        return true;
    }
    path.windows(2).any(|w| {
        ring.iter()
            .enumerate()
            .any(|(i, &a)| segments_cross(w[0], w[1], a, ring[(i + 1) % ring.len()]))
    })
}

/// The "must run before" edges that release safety requires, sorted and without duplicates.
#[must_use]
pub fn release_edges(cutters: &[Cutter], drawn: &[Drawn]) -> Vec<Edge> {
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for cutter in cutters {
        for other in drawn.iter().filter(|d| d.block != cutter.block) {
            let freed = cutter
                .rings
                .iter()
                .any(|ring| other.paths.iter().any(|path| touches_region(path, ring)));
            if freed {
                pairs.push((other.block, cutter.block));
            }
        }
    }
    pairs.sort_unstable();
    pairs.dedup();
    pairs.into_iter().map(|(b, a)| Edge::safety(b, a)).collect()
}

// ---- machine limits ----

/// The machine's physical limits in µm and gram-force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineLimits {
    /// Work area width.
    pub work_w_um: i64,
    /// Work area height.
    pub work_h_um: i64,
    /// Regions the tool must stay out of.
    pub keep_outs: Vec<Rect>,
    /// Lowest allowed force.
    pub force_min_gf: u32,
    /// Highest allowed force.
    pub force_max_gf: u32,
}

/// A way a move can break the machine's limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Violation {
    /// Outside the work area.
    OutsideWorkArea,
    /// Inside a keep-out region.
    InKeepOut,
}

fn strictly_inside(p: PtUm, r: &Rect) -> bool {
    p.x > r.x0 && p.x < r.x1 && p.y > r.y0 && p.y < r.y1
}

/// Checks a single point against the work area and keep-outs; edges of a keep-out are allowed.
#[must_use]
pub fn check_point(p: PtUm, limits: &MachineLimits) -> Option<Violation> {
    if p.x < 0 || p.y < 0 || p.x > limits.work_w_um || p.y > limits.work_h_um {
        return Some(Violation::OutsideWorkArea);
    }
    limits
        .keep_outs
        .iter()
        .any(|r| strictly_inside(p, r))
        .then_some(Violation::InKeepOut)
}

/// Checks a straight move, including a pass through a keep-out between its ends.
#[must_use]
pub fn check_segment(a: PtUm, b: PtUm, limits: &MachineLimits) -> Option<Violation> {
    if let Some(v) = check_point(a, limits).or_else(|| check_point(b, limits)) {
        return Some(v);
    }
    for r in &limits.keep_outs {
        let corners = [
            PtUm::new(r.x0, r.y0),
            PtUm::new(r.x1, r.y0),
            PtUm::new(r.x1, r.y1),
            PtUm::new(r.x0, r.y1),
        ];
        let crosses_interior = (0..4).any(|i| {
            let (c, d) = (corners[i], corners[(i + 1) % 4]);
            segments_cross(a, b, c, d) && !(within(c, d, a) && orient(c, d, a) == 0)
        });
        // A segment that enters the rectangle must cross its boundary twice or end inside, and
        // endpoints inside were handled above, so a proper crossing of two sides means it passes
        // through; a single touch is only an edge contact.
        let hits = (0..4)
            .filter(|&i| {
                let (c, d) = (corners[i], corners[(i + 1) % 4]);
                orient(a, b, c).signum() * orient(a, b, d).signum() < 0
                    && orient(c, d, a).signum() * orient(c, d, b).signum() < 0
            })
            .count();
        if crosses_interior && hits >= 2 {
            return Some(Violation::InKeepOut);
        }
    }
    None
}

/// Clamps a force into the profile range; the flag says whether it changed.
#[must_use]
pub fn clamp_force(gf: u32, limits: &MachineLimits) -> (u32, bool) {
    let clamped = gf.clamp(limits.force_min_gf, limits.force_max_gf);
    (clamped, clamped != gf)
}

#[cfg(test)]
mod tests;
