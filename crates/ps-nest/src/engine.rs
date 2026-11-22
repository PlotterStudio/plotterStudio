//! The nesting engine: bottom-left fill on the `jagua-rs` collision engine, improved
//! by a seeded search over item orders with an iteration budget.
//!
//! * Items are outlines grown by half the spacing; the bin outline is grown by the same amount so
//!   a grown item can sit flush against the mat edge. Attached sets nest as one item, using the
//!   outer boundary of their union (their convex hull when the set is not connected).
//! * Candidate positions are the bin corner and the corners of already placed items and
//!   keep-outs; the first collision-free one is slid up and left until it stops.
//! * Every integer coordinate is converted to `f32` millimetres only to ask the collision engine
//!   a yes/no question; the placement itself stays on the µm grid.

use jagua_rs::collision_detection::CDEConfig;
use jagua_rs::collision_detection::hazards::filter::NoFilter;
use jagua_rs::entities::{Container, InferiorQualityZone, Item, Layout};
use jagua_rs::geometry::fail_fast::SPSurrogateConfig;
use jagua_rs::geometry::geo_enums::RotationRange;
use jagua_rs::geometry::geo_traits::Transformable;
use jagua_rs::geometry::primitives::{Point as JaguaPoint, SPolygon};
use jagua_rs::geometry::shape_modification::{ShapeModifyConfig, ShapeModifyMode};
use jagua_rs::geometry::{DTransformation, OriginalShape, Transformation};
use ps_core::PtUm;
use ps_geometry::{FillRule, Join, normalize, offset_shapes, ring_area2};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::layout::utilisation;
use crate::rotation::{Bounds, angles, bbox, convex_hull, rotate_ring};
use crate::{NestBin, NestItem, NestParams, Nesting, Placement};

/// Largest slide step: 2^20 µm, about a kilometre, far beyond any mat.
const SLIDE_START_POWER: u32 = 20;
/// Distance in µm that candidate positions keep from an edge: the collision engine counts
/// touching as colliding, so a position flush with an edge is never accepted. Sliding then
/// closes the gap to within a micrometre of the engine's own tolerance.
const GUARD_UM: i64 = 20;
/// How many times the slide alternates between up and left.
const SLIDE_PASSES: usize = 3;

#[expect(
    clippy::cast_precision_loss,
    reason = "mat coordinates are far below 2^24 µm (16 m), where i64 -> f32 is exact enough for a yes/no query"
)]
fn mm_f32(um: i64) -> f32 {
    um as f32 / 1_000.0
}

/// Builds a collision-engine polygon from a ring, or `None` if it is not a valid simple polygon.
fn jagua_polygon(ring: &[PtUm]) -> Option<SPolygon> {
    let mut points: Vec<JaguaPoint> = Vec::with_capacity(ring.len());
    for p in ring {
        let q = JaguaPoint(mm_f32(p.x), mm_f32(p.y));
        if !points.contains(&q) {
            points.push(q);
        }
    }
    SPolygon::new(points).ok()
}

fn original(poly: SPolygon) -> OriginalShape {
    OriginalShape {
        shape: poly,
        pre_transform: DTransformation::empty(),
        modify_mode: ShapeModifyMode::Inflate,
        modify_config: ShapeModifyConfig::default(),
    }
}

/// The outer boundary of a set of rings, grown by `half` µm.
fn grown_outline(rings: &[Vec<PtUm>], half: i64) -> Option<Vec<PtUm>> {
    let shapes = normalize(rings, FillRule::NonZero);
    let outer: Vec<PtUm> = match shapes.as_slice() {
        [single] => single.first()?.clone(),
        _ => convex_hull(
            &shapes
                .iter()
                .flatten()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
        ),
    };
    if outer.len() < 3 {
        return None;
    }
    if half == 0 {
        return Some(outer);
    }
    offset_shapes(&[vec![outer]], half, Join::Round)
        .into_iter()
        .filter_map(|s| s.into_iter().next())
        .max_by_key(|r| ring_area2(r).abs())
}

/// One allowed rotation of an item, prepared once.
struct Variant {
    deg: f64,
    bounds: Bounds,
    poly: SPolygon,
}

struct PreparedItem {
    index: usize,
    area2: i128,
    variants: Vec<Variant>,
    usable: bool,
}

struct PreparedBin {
    container: Option<Container>,
    bounds: Bounds,
    keep_out_bounds: Vec<Bounds>,
}

fn prepare_bin(bin: &NestBin, half: i64) -> PreparedBin {
    let grown = grown_outline(&bin.outline_rings(), half);
    let bounds = grown.as_deref().and_then(bbox).unwrap_or(Bounds {
        min: PtUm::new(0, 0),
        max: PtUm::new(0, 0),
    });
    let keep_out_bounds = bin.keep_outs.iter().filter_map(|r| bbox(r)).collect();
    let container = grown.as_deref().and_then(jagua_polygon).and_then(|outer| {
        let zones: Vec<InferiorQualityZone> = {
            let holes: Vec<OriginalShape> = bin
                .keep_outs
                .iter()
                .filter_map(|r| jagua_polygon(r))
                .map(original)
                .collect();
            if holes.is_empty() {
                Vec::new()
            } else {
                InferiorQualityZone::new(0, holes).into_iter().collect()
            }
        };
        let cde = CDEConfig {
            quadtree_depth: 4,
            cd_threshold: 16,
            item_surrogate_config: SPSurrogateConfig::none(),
        };
        Container::new(0, original(outer), zones, cde).ok()
    });
    PreparedBin {
        container,
        bounds,
        keep_out_bounds,
    }
}

struct BinState {
    layout: Option<Layout>,
    placed_bounds: Vec<Bounds>,
}

/// One complete greedy pass over the items in `order`.
struct Outcome {
    placements: Vec<Placement>,
    placed_items: Vec<usize>,
    unplaced_items: Vec<usize>,
    area2: i128,
}

/// The polygon moved by `by`, without recomputing its derived data.
fn at(poly: &SPolygon, by: PtUm) -> SPolygon {
    poly.transform_clone(&Transformation::from_translation((
        mm_f32(by.x),
        mm_f32(by.y),
    )))
}

fn feasible(layout: &Layout, poly: &SPolygon, by: PtUm) -> bool {
    !layout.cde().detect_poly_collision(&at(poly, by), &NoFilter)
}

/// Slides a feasible position as far as it goes in direction `dir`, with halving steps.
fn slide(layout: &Layout, v: &Variant, bin: &Bounds, mut t: PtUm, dir: PtUm) -> PtUm {
    for power in (0..=SLIDE_START_POWER).rev() {
        let step = 1_i64 << power;
        loop {
            let next = PtUm::new(t.x + dir.x * step, t.y + dir.y * step);
            if !bin.contains(&v.bounds.translated(next)) || !feasible(layout, &v.poly, next) {
                break;
            }
            t = next;
        }
    }
    t
}

/// Finds the best position of one rotation in one bin, if any: `(offset, placed bounds)`.
fn position(
    layout: &Layout,
    bin: &PreparedBin,
    placed: &[Bounds],
    v: &Variant,
) -> Option<(PtUm, Bounds)> {
    let floor = bin.bounds.min;
    let mut candidates = vec![floor];
    for b in placed {
        candidates.push(PtUm::new(b.max.x, b.min.y));
        candidates.push(PtUm::new(b.min.x, b.max.y));
    }
    for k in &bin.keep_out_bounds {
        candidates.push(PtUm::new(k.max.x, floor.y));
        candidates.push(PtUm::new(floor.x, k.max.y));
        candidates.push(PtUm::new(k.max.x, k.min.y));
        candidates.push(PtUm::new(k.min.x, k.max.y));
    }
    candidates.sort_by_key(|c| (c.y, c.x));
    candidates.dedup();

    for c in candidates {
        let t = c + PtUm::new(GUARD_UM, GUARD_UM) - v.bounds.min;
        if !bin.bounds.contains(&v.bounds.translated(t)) || !feasible(layout, &v.poly, t) {
            continue;
        }
        let mut t = t;
        for _ in 0..SLIDE_PASSES {
            let before = t;
            t = slide(layout, v, &bin.bounds, t, PtUm::new(0, -1));
            t = slide(layout, v, &bin.bounds, t, PtUm::new(-1, 0));
            if t == before {
                break;
            }
        }
        return Some((t, v.bounds.translated(t)));
    }
    None
}

fn run(
    order: &[usize],
    items: &[NestItem],
    prepared: &[PreparedItem],
    bins: &[PreparedBin],
    next_id: &mut usize,
) -> Outcome {
    let mut states: Vec<BinState> = bins
        .iter()
        .map(|b| BinState {
            layout: b.container.clone().map(Layout::new),
            placed_bounds: Vec::new(),
        })
        .collect();
    let mut out = Outcome {
        placements: Vec::new(),
        placed_items: Vec::new(),
        unplaced_items: Vec::new(),
        area2: 0,
    };

    'items: for &pi in order {
        let p = &prepared[pi];
        if !p.usable {
            out.unplaced_items.push(p.index);
            continue;
        }
        for (b, bin) in bins.iter().enumerate() {
            let state = &mut states[b];
            let Some(layout) = state.layout.as_mut() else {
                continue;
            };
            // The best rotation in this bin: lowest, then leftmost.
            let mut best: Option<(&Variant, PtUm, Bounds)> = None;
            for v in &p.variants {
                if let Some((t, placed)) = position(layout, bin, &state.placed_bounds, v) {
                    let better = best.as_ref().is_none_or(|(_, _, cur)| {
                        (placed.min.y, placed.min.x) < (cur.min.y, cur.min.x)
                    });
                    if better {
                        best = Some((v, t, placed));
                    }
                }
            }
            if let Some((v, t, placed)) = best {
                let item = Item::new(
                    *next_id,
                    original(at(&v.poly, t)),
                    RotationRange::None,
                    None,
                    SPSurrogateConfig::none(),
                );
                *next_id += 1;
                if let Ok(item) = item {
                    layout.place_item(&item, DTransformation::empty());
                    state.placed_bounds.push(placed);
                    out.placements.push(Placement {
                        item: items[p.index].id,
                        bin: b,
                        rotation_deg: v.deg,
                        offset: t,
                    });
                    out.placed_items.push(p.index);
                    out.area2 += p.area2;
                    continue 'items;
                }
            }
        }
        out.unplaced_items.push(p.index);
    }
    out
}

fn prepare_item(index: usize, item: &NestItem, half: i64) -> PreparedItem {
    let area2 = ps_geometry::shapes_area2(&normalize(&item.outline, FillRule::NonZero));
    let variants: Vec<Variant> = grown_outline(&item.outline, half)
        .map(|grown| {
            angles(item.rotations)
                .into_iter()
                .filter_map(|deg| {
                    let ring = rotate_ring(&grown, deg);
                    Some(Variant {
                        deg,
                        bounds: bbox(&ring)?,
                        poly: jagua_polygon(&ring)?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    PreparedItem {
        index,
        area2,
        usable: !variants.is_empty(),
        variants,
    }
}

/// Perturbs an order: swap two items or move one to a new position.
fn perturb(order: &mut [usize], rng: &mut ChaCha8Rng) {
    let n = order.len();
    if n < 2 {
        return;
    }
    let (i, j) = (rng.gen_range(0..n), rng.gen_range(0..n));
    if rng.gen_bool(0.5) {
        order.swap(i, j);
    } else if i < j {
        order[i..=j].rotate_left(1);
    } else {
        order[j..=i].rotate_right(1);
    }
}

/// Nests `items` onto `bins`.
///
/// Items are tried largest first; then up to `params.iterations` alternative orders are tried
/// (seeded by `params.seed`) and the best layout is kept. `cancel` is polled before each
/// alternative; when it returns true the best layout so far is returned, so a cancelled run is
/// the only way two runs with the same input can differ.
#[must_use]
pub fn nest(
    bins: &[NestBin],
    items: &[NestItem],
    params: &NestParams,
    cancel: &dyn Fn() -> bool,
) -> Nesting {
    let half = params.spacing_um.max(0) / 2;
    let prepared_bins: Vec<PreparedBin> = bins.iter().map(|b| prepare_bin(b, half)).collect();
    let prepared: Vec<PreparedItem> = items
        .iter()
        .enumerate()
        .map(|(index, item)| prepare_item(index, item, half))
        .collect();

    // Largest first; ties by caller id so the start does not depend on input order.
    let mut order: Vec<usize> = (0..prepared.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(prepared[i].area2), items[i].id));

    let mut next_id = 0;
    let mut best = run(&order, items, &prepared, &prepared_bins, &mut next_id);
    let mut rng = ChaCha8Rng::seed_from_u64(params.seed);
    let mut best_order = order;
    for _ in 0..params.iterations {
        if cancel() {
            break;
        }
        // Explore from the best order found so far, not from a worse neighbour.
        let mut candidate = best_order.clone();
        perturb(&mut candidate, &mut rng);
        let attempt = run(&candidate, items, &prepared, &prepared_bins, &mut next_id);
        if attempt.area2 > best.area2 {
            best = attempt;
            best_order = candidate;
        }
    }

    let mut unplaced: Vec<u32> = best.unplaced_items.iter().map(|&i| items[i].id).collect();
    unplaced.sort_unstable();
    let utilisation_per_bin: Vec<f64> = bins
        .iter()
        .enumerate()
        .map(|(b, bin)| {
            let in_bin: Vec<NestItem> = best
                .placements
                .iter()
                .zip(&best.placed_items)
                .filter(|(p, _)| p.bin == b)
                .map(|(_, &i)| items[i].clone())
                .collect();
            utilisation(bin, &in_bin)
        })
        .collect();
    Nesting {
        placements: best.placements,
        unplaced,
        utilisation: utilisation_per_bin,
    }
}

#[cfg(test)]
mod tests;
