//! Placement geometry and the fast grid layout ("Fill mat with copies" uses a grid
//! first and offers nesting as a refinement). All checks are exact integer geometry.

use ps_core::PtUm;
use ps_geometry::{BoolOp, FillRule, boolean, normalize, shapes_area2};

use crate::rotation::{Bounds, angles, bbox, rotate_ring};
use crate::{NestBin, NestItem, Placement};

type Ring = Vec<PtUm>;

/// Returns an item's rings as placed: rotated about the origin, then moved by the offset.
#[must_use]
pub fn placed_outline(item: &NestItem, placement: &Placement) -> Vec<Ring> {
    item.outline
        .iter()
        .map(|ring| {
            rotate_ring(ring, placement.rotation_deg)
                .into_iter()
                .map(|p| p + placement.offset)
                .collect()
        })
        .collect()
}

/// The bounding box of all of an item's rings after rotating by `deg`.
pub(crate) fn rotated_bounds(item: &NestItem, deg: f64) -> Option<Bounds> {
    item.outline
        .iter()
        .filter_map(|ring| bbox(&rotate_ring(ring, deg)))
        .reduce(Bounds::union)
}

fn item_area2(item: &NestItem) -> i128 {
    shapes_area2(&normalize(&item.outline, FillRule::NonZero))
}

pub(crate) fn bin_area2(bin: &NestBin) -> i128 {
    shapes_area2(&boolean(
        &bin.outline_rings(),
        &bin.keep_out_rings(),
        BoolOp::Difference,
        FillRule::NonZero,
    ))
}

/// Returns the fraction of the bin's usable area (outline minus keep-outs) covered by `items`.
#[must_use]
pub fn utilisation(bin: &NestBin, items: &[NestItem]) -> f64 {
    let usable = bin_area2(bin);
    if usable <= 0 {
        return 0.0;
    }
    let used: i128 = items.iter().map(item_area2).sum();
    #[expect(
        clippy::cast_precision_loss,
        reason = "areas are far below 2^53 µm², where i128 -> f64 is exact"
    )]
    let ratio = used as f64 / usable as f64;
    ratio
}

/// Lays copies of an item out in rows and columns, `spacing_um` apart, inside the bin's bounding
/// box, skipping cells that leave the bin or touch a keep-out.
///
/// Every allowed rotation is tried and the one giving the most copies wins; an upright layout
/// wins ties. Copies are returned row by row from the bin's top-left corner.
#[must_use]
pub fn grid_layout(bin: &NestBin, item: &NestItem, spacing_um: i64) -> Vec<Placement> {
    let Some(area) = bbox(&bin.outline) else {
        return Vec::new();
    };
    let outline = bin.outline_rings();
    let keep_outs = bin.keep_out_rings();
    let spacing = spacing_um.max(0);

    let mut best: Vec<Placement> = Vec::new();
    for deg in angles(item.rotations) {
        let Some(shape) = rotated_bounds(item, deg) else {
            continue;
        };
        let (w, h) = (shape.width(), shape.height());
        if w <= 0 || h <= 0 || w > area.width() || h > area.height() {
            continue;
        }
        let cols = (area.width() + spacing) / (w + spacing);
        let rows = (area.height() + spacing) / (h + spacing);
        let mut places = Vec::new();
        for row in 0..rows {
            for col in 0..cols {
                let cell = PtUm::new(
                    area.min.x + col * (w + spacing),
                    area.min.y + row * (h + spacing),
                );
                let placement = Placement {
                    item: item.id,
                    bin: 0,
                    rotation_deg: deg,
                    offset: cell - shape.min,
                };
                let rings = placed_outline(item, &placement);
                let outside = boolean(&rings, &outline, BoolOp::Difference, FillRule::NonZero);
                let clash = boolean(&rings, &keep_outs, BoolOp::Intersect, FillRule::NonZero);
                if shapes_area2(&outside) == 0 && shapes_area2(&clash) == 0 {
                    places.push(placement);
                }
            }
        }
        if places.len() > best.len() {
            best = places;
        }
    }
    best
}

#[cfg(test)]
mod tests;
