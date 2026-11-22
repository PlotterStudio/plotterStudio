//! Arranging objects: numeric entry from any of nine anchors, move, scale, rotate,
//! mirror, align and distribute, and grid and circular arrays.
//!
//! Each function takes the objects with their world-space bounds and object-to-mat transforms and
//! returns the new transforms (or the copies to add), leaving the commands to the caller, so the
//! maths is tested on plain numbers.

use ps_core::ObjectId;
use ps_geometry::{Affine, Point, Rect};

/// One of the nine points of a bounding box that numeric entry and scaling work from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// Top-left corner.
    TopLeft,
    /// Middle of the top edge.
    Top,
    /// Top-right corner.
    TopRight,
    /// Middle of the left edge.
    Left,
    /// The centre.
    Centre,
    /// Middle of the right edge.
    Right,
    /// Bottom-left corner.
    BottomLeft,
    /// Middle of the bottom edge.
    Bottom,
    /// Bottom-right corner.
    BottomRight,
}

impl Anchor {
    /// All nine anchors, row by row from the top left.
    pub const ALL: [Self; 9] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Left,
        Self::Centre,
        Self::Right,
        Self::BottomLeft,
        Self::Bottom,
        Self::BottomRight,
    ];
}

/// An axis to mirror or distribute along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Left to right.
    Horizontal,
    /// Top to bottom.
    Vertical,
}

/// How objects line up with each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignKind {
    /// Left edges.
    Left,
    /// Right edges.
    Right,
    /// Top edges.
    Top,
    /// Bottom edges.
    Bottom,
    /// Centres, left to right.
    CentreHorizontal,
    /// Centres, top to bottom.
    CentreVertical,
}

/// An object as the arrangement functions see it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Item {
    /// The object.
    pub id: ObjectId,
    /// Its bounds in mat space.
    pub bounds: Rect,
    /// Its object-to-mat transform.
    pub transform: Affine,
}

/// A copy an array adds: the object it copies and the copy's transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Copy {
    /// The object being copied.
    pub source: ObjectId,
    /// The copy's object-to-mat transform.
    pub transform: Affine,
}

/// A point of a rectangle.
#[must_use]
pub fn anchor_point(r: Rect, anchor: Anchor) -> Point {
    let (x0, y0, x1, y1) = (r.x0, r.y0, r.x1, r.y1);
    let (mx, my) = (x0.midpoint(x1), y0.midpoint(y1));
    match anchor {
        Anchor::TopLeft => Point::new(x0, y0),
        Anchor::Top => Point::new(mx, y0),
        Anchor::TopRight => Point::new(x1, y0),
        Anchor::Left => Point::new(x0, my),
        Anchor::Centre => Point::new(mx, my),
        Anchor::Right => Point::new(x1, my),
        Anchor::BottomLeft => Point::new(x0, y1),
        Anchor::Bottom => Point::new(mx, y1),
        Anchor::BottomRight => Point::new(x1, y1),
    }
}

/// The bounds of the whole selection.
#[must_use]
pub fn union_bounds(items: &[Item]) -> Option<Rect> {
    items.iter().map(|i| i.bounds).reduce(|a, b| a.union(b))
}

fn about(point: Point, apply: Affine) -> Affine {
    Affine::translate(point.to_vec2()) * apply * Affine::translate(-point.to_vec2())
}

fn all(items: &[Item], change: Affine) -> Vec<(ObjectId, Affine)> {
    items.iter().map(|i| (i.id, change * i.transform)).collect()
}

/// Moves the selection so that `anchor` of its bounds is at `target`.
#[must_use]
pub fn move_to(items: &[Item], anchor: Anchor, target: Point) -> Vec<(ObjectId, Affine)> {
    let Some(bounds) = union_bounds(items) else {
        return Vec::new();
    };
    let from = anchor_point(bounds, anchor);
    all(
        items,
        Affine::translate((target.x - from.x, target.y - from.y)),
    )
}

/// Scales the selection to `width` by `height` about `anchor`; with `lock_aspect` the height
/// follows the width. Nothing changes (an empty result) for a size that is not positive and
/// finite or a selection with no extent to scale.
#[must_use]
pub fn resize(
    items: &[Item],
    anchor: Anchor,
    width: f64,
    height: f64,
    lock_aspect: bool,
) -> Vec<(ObjectId, Affine)> {
    let Some(bounds) = union_bounds(items) else {
        return Vec::new();
    };
    let (w, h) = (bounds.width(), bounds.height());
    let sx = width / w;
    let sy = if lock_aspect { sx } else { height / h };
    let usable = |s: f64| s.is_finite() && s > 0.0;
    if w <= 0.0 || !usable(sx) || !usable(sy) || (!lock_aspect && h <= 0.0) {
        return Vec::new();
    }
    let sy = if lock_aspect && h <= 0.0 { 1.0 } else { sy };
    let origin = anchor_point(bounds, anchor);
    all(items, about(origin, Affine::scale_non_uniform(sx, sy)))
}

/// Rotates the selection by `degrees` about the centre of its bounds.
#[must_use]
pub fn rotate(items: &[Item], degrees: f64) -> Vec<(ObjectId, Affine)> {
    let Some(bounds) = union_bounds(items) else {
        return Vec::new();
    };
    let centre = anchor_point(bounds, Anchor::Centre);
    all(items, about(centre, Affine::rotate(degrees.to_radians())))
}

/// Mirrors the selection about the centre of its bounds.
#[must_use]
pub fn mirror(items: &[Item], axis: Axis) -> Vec<(ObjectId, Affine)> {
    let Some(bounds) = union_bounds(items) else {
        return Vec::new();
    };
    let centre = anchor_point(bounds, Anchor::Centre);
    let flip = match axis {
        Axis::Horizontal => Affine::scale_non_uniform(-1.0, 1.0),
        Axis::Vertical => Affine::scale_non_uniform(1.0, -1.0),
    };
    all(items, about(centre, flip))
}

/// Lines objects up with the selection's edge or centre.
#[must_use]
pub fn align(items: &[Item], kind: AlignKind) -> Vec<(ObjectId, Affine)> {
    let Some(bounds) = union_bounds(items) else {
        return Vec::new();
    };
    items
        .iter()
        .map(|i| {
            let b = i.bounds;
            let (dx, dy) = match kind {
                AlignKind::Left => (bounds.x0 - b.x0, 0.0),
                AlignKind::Right => (bounds.x1 - b.x1, 0.0),
                AlignKind::Top => (0.0, bounds.y0 - b.y0),
                AlignKind::Bottom => (0.0, bounds.y1 - b.y1),
                AlignKind::CentreHorizontal => {
                    (bounds.x0.midpoint(bounds.x1) - b.x0.midpoint(b.x1), 0.0)
                }
                AlignKind::CentreVertical => {
                    (0.0, bounds.y0.midpoint(bounds.y1) - b.y0.midpoint(b.y1))
                }
            };
            (i.id, Affine::translate((dx, dy)) * i.transform)
        })
        .collect()
}

/// Spaces objects so the gaps between their bounds are equal, keeping the first and last where
/// they are. Needs three or more objects; the result is empty otherwise.
#[must_use]
pub fn distribute(items: &[Item], axis: Axis) -> Vec<(ObjectId, Affine)> {
    if items.len() < 3 {
        return Vec::new();
    }
    let range = |b: Rect| match axis {
        Axis::Horizontal => (b.x0, b.x1),
        Axis::Vertical => (b.y0, b.y1),
    };
    let mut order: Vec<&Item> = items.iter().collect();
    order.sort_by(|a, b| range(a.bounds).0.total_cmp(&range(b.bounds).0));
    let first = range(order[0].bounds).0;
    let last = range(order[order.len() - 1].bounds).1;
    let sizes: f64 = order
        .iter()
        .map(|i| range(i.bounds).1 - range(i.bounds).0)
        .sum();
    #[expect(
        clippy::cast_precision_loss,
        reason = "object counts are far below 2^52"
    )]
    let gap = (last - first - sizes) / (order.len() - 1) as f64;
    let mut at = first;
    order
        .into_iter()
        .map(|i| {
            let (lo, hi) = range(i.bounds);
            let shift = at - lo;
            at += hi - lo + gap;
            let delta = match axis {
                Axis::Horizontal => (shift, 0.0),
                Axis::Vertical => (0.0, shift),
            };
            (i.id, Affine::translate(delta) * i.transform)
        })
        .collect()
}

/// The copies a `columns` by `rows` grid array adds, `dx` and `dy` apart; the original already
/// fills the first cell.
#[must_use]
pub fn grid_array(items: &[Item], columns: u32, rows: u32, dx: f64, dy: f64) -> Vec<Copy> {
    let mut out = Vec::new();
    for row in 0..rows {
        for column in 0..columns {
            if row == 0 && column == 0 {
                continue;
            }
            let offset = (f64::from(column) * dx, f64::from(row) * dy);
            for item in items {
                out.push(Copy {
                    source: item.id,
                    transform: Affine::translate(offset) * item.transform,
                });
            }
        }
    }
    out
}

/// The copies a circular array adds: `count` objects in total (the original included) spaced
/// evenly around `centre` over `total_degrees`. A full circle spaces them `360 / count` apart;
/// less than that ends on the last copy.
#[must_use]
pub fn circular_array(items: &[Item], count: u32, centre: Point, total_degrees: f64) -> Vec<Copy> {
    if count < 2 {
        return Vec::new();
    }
    let step = if total_degrees.abs() >= 360.0 {
        total_degrees / f64::from(count)
    } else {
        total_degrees / f64::from(count - 1)
    };
    let mut out = Vec::new();
    for k in 1..count {
        let turn = about(centre, Affine::rotate((step * f64::from(k)).to_radians()));
        for item in items {
            out.push(Copy {
                source: item.id,
                transform: turn * item.transform,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests;
