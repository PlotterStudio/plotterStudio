//! Fill generation for pens, foil, engraving, debossing and embossing.
//!
//! The region is inset by half the tip width so the ink edge lands on the shape boundary, then
//! filled with a pattern at spacing `s = t × (1 − o)`. Output strokes are polylines on the µm
//! grid. Thin parts narrower than the inset (medial-axis centre lines) are not generated here:
//! they need a Voronoi dependency that is not part of this crate yet.

use std::collections::BTreeMap;

use ps_core::PtUm;
use serde::{Deserialize, Serialize};

use crate::boolean::{FillRule, Ring, Shape, clip_lines};
use crate::offset::{Join, offset_shapes, um_f64, um_i64};

/// Largest overlap accepted; beyond this the spacing would collapse to nothing.
const MAX_OVERLAP: f64 = 0.95;
/// A safety cap on concentric rings, far above anything a mat can need.
const MAX_RINGS: usize = 100_000;
const MAX_RINGS_F64: f64 = 100_000.0;

/// The pattern used to fill a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    /// Parallel hatch lines.
    Parallel,
    /// Hatch plus a second hatch at 90°.
    Cross,
    /// Hatch lines linked end to end where the link lies inside the region.
    ZigZag,
    /// Rings offset inward until nothing is left.
    Concentric,
}

/// Parameters of a fill (the `FillSpec` of an operation).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FillSpec {
    /// The pattern.
    pub pattern: Pattern,
    /// Hatch angle in degrees, counter-clockwise from the x axis.
    pub angle_deg: f64,
    /// Overlap between adjacent passes, 0 to 1 (default 0.15 in the UI).
    pub overlap: f64,
    /// Also draw the boundary of the inset region.
    pub outline_pass: bool,
}

/// Returns the pass spacing `t × (1 − o)` for a tip width and overlap, in mm.
///
/// The overlap is clamped to `0..=0.95` so the spacing stays positive and never exceeds the tip.
#[must_use]
pub fn spacing(tip: f64, overlap: f64) -> f64 {
    tip * (1.0 - overlap.clamp(0.0, MAX_OVERLAP))
}

/// Returns how many parallel passes a stroke of `width` needs: `ceil((w − t) / s) + 1`.
#[must_use]
pub fn stroke_passes(width: f64, tip: f64, spacing: f64) -> u32 {
    if width <= tip || spacing <= 0.0 {
        return 1;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a positive, finite ratio; saturating conversion is the intent"
    )]
    let extra = ((width - tip) / spacing).ceil() as u32;
    extra.saturating_add(1)
}

/// Insets a region by half the tip width, so the ink edge lands on the boundary.
#[must_use]
pub fn inset_region(shapes: &[Shape], tip_mm: f64) -> Vec<Shape> {
    let Ok(half) = ps_core::um_from_mm(tip_mm / 2.0) else {
        return Vec::new();
    };
    offset_shapes(shapes, -half, Join::Round)
}

/// Generates the stroke polylines that fill `region` with a pen of the given tip width.
///
/// Open polylines are strokes; polylines whose first and last points coincide are closed rings.
#[must_use]
pub fn generate(region: &[Shape], tip_mm: f64, spec: &FillSpec) -> Vec<Vec<PtUm>> {
    let inset = inset_region(region, tip_mm);
    if inset.is_empty() {
        return Vec::new();
    }
    let rings: Vec<Ring> = inset.iter().flatten().cloned().collect();
    let step_um = spacing(tip_mm, spec.overlap) * 1_000.0;

    let mut out = match spec.pattern {
        Pattern::Parallel => hatch(&rings, step_um, spec.angle_deg),
        Pattern::Cross => {
            let mut lines = hatch(&rings, step_um, spec.angle_deg);
            lines.extend(hatch(&rings, step_um, spec.angle_deg + 90.0));
            lines
        }
        Pattern::ZigZag => zigzag(&rings, step_um, spec.angle_deg),
        Pattern::Concentric => concentric(inset.clone(), step_um),
    };
    if spec.outline_pass {
        out.extend(rings.iter().map(|r| closed(r)));
    }
    out
}

/// Rotates a grid point counter-clockwise about the origin. Multiples of 90° are exact.
fn rotate(pt: PtUm, deg: f64) -> PtUm {
    let turns = deg.rem_euclid(360.0);
    let quarters = turns / 90.0;
    if (quarters - quarters.round()).abs() < 1e-12 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "quarters is within 0..=4 and integral here"
        )]
        let q = quarters.round() as u8 % 4;
        return match q {
            0 => pt,
            1 => PtUm::new(-pt.y, pt.x),
            2 => PtUm::new(-pt.x, -pt.y),
            _ => PtUm::new(pt.y, -pt.x),
        };
    }
    let (sin, cos) = (libm::sin(turns.to_radians()), libm::cos(turns.to_radians()));
    let (fx, fy) = (um_f64(pt.x), um_f64(pt.y));
    PtUm::new(um_i64(fx * cos - fy * sin), um_i64(fx * sin + fy * cos))
}

fn closed(ring: &[PtUm]) -> Vec<PtUm> {
    let mut line = ring.to_vec();
    if let Some(&first) = ring.first() {
        line.push(first);
    }
    line
}

/// Hatch pieces grouped by row in the hatch frame (rotated so lines are horizontal), each row
/// ordered by x. The key is the row's y.
fn hatch_rows(rings: &[Ring], step_um: f64, angle_deg: f64) -> BTreeMap<i64, Vec<[PtUm; 2]>> {
    let rotated: Vec<Ring> = rings
        .iter()
        .map(|r| r.iter().map(|&p| rotate(p, -angle_deg)).collect())
        .collect();
    let pts = || rotated.iter().flatten();
    let (Some(y_min), Some(y_max), Some(x_min), Some(x_max)) = (
        pts().map(|p| p.y).min(),
        pts().map(|p| p.y).max(),
        pts().map(|p| p.x).min(),
        pts().map(|p| p.x).max(),
    ) else {
        return BTreeMap::new();
    };

    // Rows run from the first edge of the region to the last, spaced evenly at no more than the
    // requested step: a fixed step would leave a bare strip along the far edge whenever the
    // height is not a whole number of steps, so the step shrinks a little to land on it.
    let span = um_f64(y_max - y_min);
    let count = if step_um > 0.0 && span > 0.0 {
        let n = (span / step_um).ceil();
        if n.is_finite() {
            n.min(MAX_RINGS_F64)
        } else {
            1.0
        }
    } else {
        1.0
    };
    let step = if span > 0.0 { span / count } else { step_um };
    let mut lines = Vec::new();
    let mut k = 0.0_f64;
    while k <= count {
        let y = if k >= count {
            y_max
        } else {
            y_min + um_i64(k * step)
        };
        lines.push(vec![PtUm::new(x_min - 1, y), PtUm::new(x_max + 1, y)]);
        k += 1.0;
    }

    let mut rows: BTreeMap<i64, Vec<[PtUm; 2]>> = BTreeMap::new();
    for piece in clip_lines(&lines, &rotated, FillRule::NonZero) {
        let (Some(&a), Some(&b)) = (piece.first(), piece.last()) else {
            continue;
        };
        if a == b {
            continue;
        }
        let ordered = if a.x <= b.x { [a, b] } else { [b, a] };
        rows.entry(ordered[0].y).or_default().push(ordered);
    }
    for pieces in rows.values_mut() {
        pieces.sort_by_key(|[a, _]| a.x);
    }
    rows
}

fn hatch(rings: &[Ring], step_um: f64, angle_deg: f64) -> Vec<Vec<PtUm>> {
    hatch_rows(rings, step_um, angle_deg)
        .into_values()
        .flatten()
        .map(|[a, b]| vec![rotate(a, angle_deg), rotate(b, angle_deg)])
        .collect()
}

fn zigzag(rings: &[Ring], step_um: f64, angle_deg: f64) -> Vec<Vec<PtUm>> {
    let rows = hatch_rows(rings, step_um, angle_deg);
    let reach2 = (2.0 * step_um) * (2.0 * step_um);
    let mut strokes: Vec<Vec<PtUm>> = Vec::new();
    let mut current: Vec<PtUm> = Vec::new();

    for (i, pieces) in rows.values().enumerate() {
        let forward = i % 2 == 0;
        let ordered: Vec<(PtUm, PtUm)> = if forward {
            pieces.iter().map(|&[a, b]| (a, b)).collect()
        } else {
            pieces.iter().rev().map(|&[a, b]| (b, a)).collect()
        };
        for (start, end) in ordered {
            let linked = current
                .last()
                .is_some_and(|&last| can_link(rings, last, start, reach2));
            if linked {
                current.extend([start, end]);
            } else {
                if !current.is_empty() {
                    strokes.push(std::mem::take(&mut current));
                }
                current = vec![start, end];
            }
        }
    }
    if !current.is_empty() {
        strokes.push(current);
    }
    for stroke in &mut strokes {
        for p in stroke.iter_mut() {
            *p = rotate(*p, angle_deg);
        }
    }
    strokes
}

/// A link between two row ends is allowed when it is shorter than `2s` and lies in the region.
fn can_link(rings: &[Ring], a: PtUm, b: PtUm, reach2: f64) -> bool {
    #[expect(
        clippy::cast_precision_loss,
        reason = "squared link lengths are tiny compared to 2^53"
    )]
    let d2 = a.dist_sq(b) as f64;
    d2 < reach2 && lies_inside(rings, a, b)
}

/// How much of a line may be missing from its clip before it counts as leaving the region (µm).
/// The clipper rounds where a line crosses the boundary to the grid, and the ends of a link lie on
/// the boundary themselves, so an exact match is too much to ask on slanted or curved edges.
const INSIDE_SLACK_UM: f64 = 10.0;

/// True when the straight line from `a` to `b` lies wholly inside the region `rings` (non-zero
/// winding), give or take [`INSIDE_SLACK_UM`]: a pen-down move along it cannot leave ink outside
/// the shape.
fn lies_inside(rings: &[Ring], a: PtUm, b: PtUm) -> bool {
    if a == b {
        return true;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "grid coordinates are far below 2^53 µm, where i64 -> f64 is exact"
    )]
    let length = |p: PtUm, q: PtUm| libm::hypot((q.x - p.x) as f64, (q.y - p.y) as f64);
    let inside: f64 = clip_lines(&[vec![a, b]], rings, FillRule::NonZero)
        .iter()
        .map(|piece| match (piece.first(), piece.last()) {
            (Some(&p), Some(&q)) => length(p, q),
            _ => 0.0,
        })
        .sum();
    inside >= length(a, b) - INSIDE_SLACK_UM
}

fn concentric(mut region: Vec<Shape>, step_um: f64) -> Vec<Vec<PtUm>> {
    let step = um_i64(step_um).max(1);
    let mut out = Vec::new();
    while !region.is_empty() && out.len() < MAX_RINGS {
        out.extend(region.iter().flatten().map(|r| closed(r)));
        region = offset_shapes(&region, -step, Join::Round);
    }
    out
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "tests are the independent oracle and may use platform maths"
)]
mod tests;
