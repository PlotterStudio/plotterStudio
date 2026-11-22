//! Compile-time booleans on the µm integer grid.
//!
//! Integer arithmetic makes the results identical on every platform. Rings are lists of
//! [`PtUm`] vertices (closed implicitly). A [`Shape`] is an outer ring followed by its holes;
//! results always have counter-clockwise outers and clockwise holes.

use i_overlay::core::fill_rule::FillRule as EngineFill;
use i_overlay::core::overlay::{Overlay, ShapeType};
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::i_float::int::point::IntPoint;
use i_overlay::string::clip::{ClipRule, IntClip};
use ps_core::PtUm;
use serde::{Deserialize, Serialize};

/// A closed ring of grid vertices.
pub type Ring = Vec<PtUm>;
/// An outer ring followed by hole rings.
pub type Shape = Vec<Ring>;

/// A boolean operation between a subject and a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolOp {
    /// Everything in either.
    Union,
    /// Only what is in both.
    Intersect,
    /// What is in the subject but not the clip.
    Difference,
    /// What is in exactly one of them.
    Xor,
}

/// How self-overlapping outlines decide what is inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillRule {
    /// Inside where the winding number is not zero.
    NonZero,
    /// Inside where the winding number is odd.
    EvenOdd,
}

impl FillRule {
    fn engine(self) -> EngineFill {
        match self {
            Self::NonZero => EngineFill::NonZero,
            Self::EvenOdd => EngineFill::EvenOdd,
        }
    }
}

impl BoolOp {
    fn engine(self) -> OverlayRule {
        match self {
            Self::Union => OverlayRule::Union,
            Self::Intersect => OverlayRule::Intersect,
            Self::Difference => OverlayRule::Difference,
            Self::Xor => OverlayRule::Xor,
        }
    }
}

fn to_engine(rings: &[Ring]) -> Vec<Vec<IntPoint<i64>>> {
    rings
        .iter()
        .map(|r| r.iter().map(|p| IntPoint::new(p.x, p.y)).collect())
        .collect()
}

fn from_engine(shapes: Vec<Vec<Vec<IntPoint<i64>>>>) -> Vec<Shape> {
    shapes
        .into_iter()
        .map(|shape| {
            shape
                .into_iter()
                .map(|ring| ring.into_iter().map(|p| PtUm::new(p.x, p.y)).collect())
                .collect()
        })
        .collect()
}

/// Applies a boolean operation to two sets of rings.
#[must_use]
pub fn boolean(subject: &[Ring], clip: &[Ring], op: BoolOp, rule: FillRule) -> Vec<Shape> {
    let mut overlay = Overlay::<i64>::new(subject.len() + clip.len());
    overlay.add_contours(&to_engine(subject), ShapeType::Subject);
    overlay.add_contours(&to_engine(clip), ShapeType::Clip);
    from_engine(overlay.overlay(op.engine(), rule.engine()))
}

/// Resolves overlaps and winding in one set of rings into clean shapes.
#[must_use]
pub fn normalize(rings: &[Ring], rule: FillRule) -> Vec<Shape> {
    let mut overlay = Overlay::<i64>::new(rings.len());
    overlay.add_contours(&to_engine(rings), ShapeType::Subject);
    from_engine(overlay.overlay(OverlayRule::Subject, rule.engine()))
}

/// Returns the portions of the polylines that lie inside the region (boundary included).
#[must_use]
pub fn clip_lines(lines: &[Vec<PtUm>], region: &[Ring], rule: FillRule) -> Vec<Vec<PtUm>> {
    let shapes = vec![to_engine(region)];
    let paths = to_engine(lines);
    let rule_out = ClipRule {
        invert: false,
        boundary_included: true,
    };
    shapes
        .clip_paths(&paths, rule.engine(), rule_out)
        .into_iter()
        .map(|p| p.into_iter().map(|q| PtUm::new(q.x, q.y)).collect())
        .collect()
}

/// Returns twice the signed area of a ring in µm² (positive when counter-clockwise).
///
/// Doubling keeps the value an integer for any grid polygon.
#[must_use]
pub fn ring_area2(ring: &[PtUm]) -> i128 {
    if ring.len() < 3 {
        return 0;
    }
    let mut sum = 0_i128;
    for (i, a) in ring.iter().enumerate() {
        let b = ring[(i + 1) % ring.len()];
        sum += i128::from(a.x) * i128::from(b.y) - i128::from(b.x) * i128::from(a.y);
    }
    sum
}

/// Returns twice the area of the shapes: each outer ring counts, each hole is subtracted.
#[must_use]
pub fn shapes_area2(shapes: &[Shape]) -> i128 {
    shapes
        .iter()
        .map(|shape| {
            let mut rings = shape.iter();
            let outer = rings.next().map_or(0, |r| ring_area2(r).abs());
            let holes: i128 = rings.map(|r| ring_area2(r).abs()).sum();
            outer - holes
        })
        .sum()
}

#[cfg(test)]
mod tests;
