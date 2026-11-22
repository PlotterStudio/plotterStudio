//! Thin parts of a fill region.
//!
//! A pen of tip width `t` fills a region by hatching the region inset by `t/2`. Whatever that
//! cannot reach is `R − grow(R′, t/2)`: narrow strips and small dots, but also a small sliver at
//! every convex corner, because a round tip cannot reach a sharp corner. The specification says parts
//! wider than a dot are drawn as single centre-line strokes and parts smaller than the tip are
//! flagged; it gives no thresholds, so this module fixes them (design decision):
//!
//! * A leftover part is a *feature* when the largest circle that fits in it has a diameter of at
//!   least [`FEATURE_FRACTION`] × the tip (half the tip). Smaller parts, including every corner
//!   sliver, are ignored silently: they are far below what the pen can draw and not worth a
//!   warning.
//! * A feature whose centre line is at least [`MIN_STROKE_TIPS`] × the tip long is drawn along
//!   that centre line (the pen centred in it, so ink width `t` covers a part narrower than `t`).
//! * Any other feature is *flagged*, not drawn.
//!
//! * Dead-end branches of the axis whose tip lies on the boundary (clearance under
//!   [`SPUR_RATIO`] × the branch's own largest clearance) are boundary noise, such as the jagged
//!   mouth where a tail leaves a body, and are pruned. A real strip end keeps its full
//!   half-width at the tip and is never pruned.
//!
//! If the Voronoi engine cannot process a part, it is flagged when its area is at least a
//! quarter of a tip-diameter square, and ignored otherwise.

use std::collections::BTreeMap;

use ps_core::PtUm;

use crate::boolean::{BoolOp, FillRule, Ring, Shape, boolean, shapes_area2};
use crate::fill::{FillSpec, generate, inset_region};
use crate::medial::medial_axis;
use crate::offset::{Join, offset_shapes, um_f64};

/// A leftover part is a feature when its inscribed diameter is at least this fraction of the tip.
pub const FEATURE_FRACTION: f64 = 0.5;
/// A feature is drawn when its centre line is at least this many tip widths long.
pub const MIN_STROKE_TIPS: f64 = 1.0;
/// A dead-end branch is pruned when its tip clearance is below this fraction of its maximum.
pub const SPUR_RATIO: f64 = 0.5;
/// Discretisation tolerance for parabolic axis edges, in µm.
const AXIS_TOL_UM: f64 = 2.0;

/// The centre-line strokes for thin parts and the parts that could not be drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct ThinParts {
    /// Centre-line polylines on the µm grid.
    pub strokes: Vec<Vec<PtUm>>,
    /// Features smaller than the tip, left undrawn so the UI can warn about them.
    pub flagged: Vec<Shape>,
}

/// A complete fill: hatch or pattern strokes plus thin-part centre lines, and what was flagged.
#[derive(Debug, Clone, PartialEq)]
pub struct Fill {
    /// Every stroke to draw.
    pub strokes: Vec<Vec<PtUm>>,
    /// Features the pen cannot draw (see [`ThinParts::flagged`]).
    pub flagged: Vec<Shape>,
}

fn rings_of(shapes: &[Shape]) -> Vec<Ring> {
    shapes.iter().flatten().cloned().collect()
}

fn length(chain: &[PtUm]) -> f64 {
    chain
        .windows(2)
        .map(|w| libm::hypot(um_f64(w[1].x - w[0].x), um_f64(w[1].y - w[0].y)))
        .sum()
}

/// Finds the parts of `region` that a pen of `tip_mm` cannot reach, and draws or flags them.
#[must_use]
pub fn thin_parts(region: &[Shape], tip_mm: f64) -> ThinParts {
    let mut result = ThinParts {
        strokes: Vec::new(),
        flagged: Vec::new(),
    };
    let Ok(radius_um) = ps_core::um_from_mm(tip_mm / 2.0) else {
        return result;
    };
    // A pen with no width (or one that rounds to none on the µm grid) reaches nothing, so every
    // sliver would look like a feature; there is nothing sensible to draw or flag.
    if tip_mm.is_nan() || tip_mm <= 0.0 || radius_um <= 0 {
        return result;
    }
    let diameter_um = um_f64(radius_um) * 2.0;

    let inner = inset_region(region, tip_mm);
    let reached = offset_shapes(&inner, radius_um, Join::Round);
    let leftovers = boolean(
        &rings_of(region),
        &rings_of(&reached),
        BoolOp::Difference,
        FillRule::NonZero,
    );

    // Area (in µm²) a part must have to be worth flagging when it cannot be analysed.
    let min_area2 = 2.0 * (diameter_um / 2.0) * (diameter_um / 2.0);
    for part in leftovers {
        if let Ok(axis) = medial_axis(&part, AXIS_TOL_UM) {
            if axis.max_radius_um * 2.0 < FEATURE_FRACTION * diameter_um {
                continue; // a corner sliver or a speck
            }
            let chains = prune_spurs(axis.chains, &part);
            let axis_length: f64 = chains.iter().map(|c| length(c)).sum();
            if axis_length >= MIN_STROKE_TIPS * diameter_um {
                result.strokes.extend(chains);
            } else {
                result.flagged.push(part);
            }
        } else {
            #[expect(clippy::cast_precision_loss, reason = "areas are far below 2^53 µm²")]
            let area2 = shapes_area2(std::slice::from_ref(&part)) as f64;
            if area2 >= min_area2 {
                result.flagged.push(part);
            }
        }
    }
    result
}

/// Distance from a point to the nearest side of the part, in µm.
fn clearance(p: PtUm, part: &Shape) -> f64 {
    let mut best = f64::INFINITY;
    for ring in part {
        for (i, a) in ring.iter().enumerate() {
            let b = ring[(i + 1) % ring.len()];
            let (ax, ay) = (um_f64(a.x), um_f64(a.y));
            let (dx, dy) = (um_f64(b.x) - ax, um_f64(b.y) - ay);
            let len2 = dx * dx + dy * dy;
            let (px, py) = (um_f64(p.x), um_f64(p.y));
            let t = if len2 == 0.0 {
                0.0
            } else {
                (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
            };
            best = best.min(libm::hypot(px - (ax + dx * t), py - (ay + dy * t)));
        }
    }
    best
}

/// Removes boundary-noise spurs from the axis and joins what is left at degree-2 junctions.
fn prune_spurs(mut chains: Vec<Vec<PtUm>>, part: &Shape) -> Vec<Vec<PtUm>> {
    loop {
        let mut degree: BTreeMap<PtUm, usize> = BTreeMap::new();
        for c in &chains {
            for end in [c.first(), c.last()].into_iter().flatten() {
                *degree.entry(*end).or_default() += 1;
            }
        }
        let spur = chains.iter().position(|c| {
            let (Some(&a), Some(&b)) = (c.first(), c.last()) else {
                return false;
            };
            // A spur has exactly one free tip; the other end joins another chain.
            let leaf = match (degree[&a] == 1, degree[&b] == 1) {
                (true, false) => a,
                (false, true) => b,
                _ => return false,
            };
            let widest = c.iter().map(|&p| clearance(p, part)).fold(0.0, f64::max);
            clearance(leaf, part) < SPUR_RATIO * widest
        });
        match spur {
            Some(i) => {
                chains.remove(i);
            }
            None => break,
        }
    }
    join_at_pairs(chains)
}

/// Joins chains that meet at a point where exactly two chain ends coincide.
fn join_at_pairs(mut chains: Vec<Vec<PtUm>>) -> Vec<Vec<PtUm>> {
    chains.retain(|c| !c.is_empty());
    'again: loop {
        for i in 0..chains.len() {
            for j in (i + 1)..chains.len() {
                let (ai, bi) = (chains[i][0], chains[i][chains[i].len() - 1]);
                let (aj, bj) = (chains[j][0], chains[j][chains[j].len() - 1]);
                let ends = chains.iter().flat_map(|c| [c[0], c[c.len() - 1]]);
                let count = |p: PtUm| ends.clone().filter(|&q| q == p).count();
                let joint = [(ai, aj), (ai, bj), (bi, aj), (bi, bj)]
                    .into_iter()
                    .find(|&(p, q)| p == q && count(p) == 2);
                let Some((p, _)) = joint else { continue };
                let mut left = chains[i].clone();
                let mut right = chains[j].clone();
                if left[0] == p {
                    left.reverse();
                }
                if right[right.len() - 1] == p {
                    right.reverse();
                }
                left.extend(right.into_iter().skip(1));
                chains.remove(j);
                chains[i] = left;
                continue 'again;
            }
        }
        return chains;
    }
}

/// Generates the full fill of a region: the pattern from [`generate`] plus centre lines for the
/// thin parts the pattern cannot reach, with the undrawable features flagged.
#[must_use]
pub fn fill(region: &[Shape], tip_mm: f64, spec: &FillSpec) -> Fill {
    let mut strokes = generate(region, tip_mm, spec);
    let thin = thin_parts(region, tip_mm);
    strokes.extend(thin.strokes);
    Fill {
        strokes,
        flagged: thin.flagged,
    }
}

#[cfg(test)]
mod tests;
