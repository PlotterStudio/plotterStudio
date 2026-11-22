//! Joining fill strokes into pen-down chains.
//!
//! A hatch fill is one stroke per row, so the pen would lift and hop to the next row hundreds of
//! times. Where the end of one row lies a few row spacings from the start of the next, and the
//! straight line between them stays inside the region being filled, the pen can simply stay down:
//! the extra ink is inside the fill anyway.
//!
//! Which rows end up next to each other is decided by a short tour over the strokes (nearest
//! neighbour, then 2-opt and Or-opt) in which a possible join costs almost nothing and a pen lift
//! costs a fixed extra distance. Runs of the tour whose hops are all joins become one stroke.
//! Everything is integer geometry or fixed-order, so the result is the same on every machine.
//!
//! A join between two rows that run back to back, about one row spacing apart, is a hairpin. A
//! machine that slows at every corner (one hobby cutter measured loses 0.2 s to each sharp one) does better
//! if the hairpin is a semicircle of gentle chords: both rows are cut back by the radius so the
//! turn stays inside the shape, and the chords turn by at most the profile's step.

use ps_core::PtUm;
use ps_geometry::Region;
use rstar::RTree;
use rstar::primitives::GeomWithData;
use rustc_hash::FxHashSet;

use crate::order::{Item, Metric, improve_sweeps_by, nearest_neighbour};

/// What a pen lift costs the tour, as extra distance in µm: the time to raise and lower the tool.
const LIFT_UM: f64 = 3_000.0;
/// A pen-down join costs this fraction of its length, so that of two joins the shorter wins.
const JOIN_FRACTION: f64 = 0.02;
/// Pair comparisons the tour may spend; the local search stops sweeping when they are used up.
const WORK: usize = 30_000_000;
/// The most sweeps of the local search.
const MAX_SWEEPS: usize = 12;

type Key = (i64, i64);

fn key(p: PtUm) -> Key {
    (p.x, p.y)
}

fn pair(a: PtUm, b: PtUm) -> (Key, Key) {
    let (a, b) = (key(a), key(b));
    if a <= b { (a, b) } else { (b, a) }
}

/// Where the tour starts: nowhere in particular. It lies far outside any mat, and moving from it
/// costs nothing, so the tour is free to begin at whichever end suits it.
const START: PtUm = PtUm {
    x: -1_000_000_000_000,
    y: -1_000_000_000_000,
};

/// The cost of moving between two stroke ends: nearly free where the pen can stay down.
struct Joins {
    joinable: FxHashSet<(Key, Key)>,
}

impl Metric for Joins {
    #[expect(
        clippy::cast_precision_loss,
        reason = "grid coordinates are far below 2^53 µm, where i64 -> f64 is exact"
    )]
    fn cost(&self, a: PtUm, b: PtUm) -> f64 {
        if a == START || b == START {
            return 0.0;
        }
        let d = libm::hypot((b.x - a.x) as f64, (b.y - a.y) as f64);
        if self.joinable.contains(&pair(a, b)) {
            d * JOIN_FRACTION
        } else {
            d + LIFT_UM
        }
    }
}

/// Every pair of stroke ends, from different strokes, that are within `reach_um` of each other with
/// the straight line between them inside `region`.
fn joinable_pairs(strokes: &[Vec<PtUm>], region: &Region, reach_um: i64) -> FxHashSet<(Key, Key)> {
    type End = GeomWithData<[i64; 2], (usize, PtUm)>;
    let ends: Vec<End> = strokes
        .iter()
        .enumerate()
        .flat_map(|(i, s)| [s[0], s[s.len() - 1]].map(|p| End::new([p.x, p.y], (i, p))))
        .collect();
    let tree = RTree::bulk_load(ends.clone());
    let reach2 = reach_um.saturating_mul(reach_um);
    let mut out = FxHashSet::default();
    for end in &ends {
        let (i, p) = end.data;
        for near in tree.locate_within_distance(*end.geom(), reach2) {
            let (j, q) = near.data;
            if i == j || out.contains(&pair(p, q)) {
                continue;
            }
            if region.contains_segment(p, q) {
                out.insert(pair(p, q));
            }
        }
    }
    out
}

/// The least cosine of the angle between two rows for them to count as running back to back.
const HAIRPIN_COS: f64 = -0.97;
/// The most the join may lean from square across the rows, as a cosine.
const ACROSS_COS: f64 = 0.35;
/// The shortest row, in µm, either side of a join that is rounded: on rows shorter than this the
/// turns follow one another so closely that the machine jerks (seen on a real fill, 2026-11-21).
const TURN_MIN_ROW_UM: f64 = 8_000.0;
/// The smallest and largest gap, in µm, between two rows that is rounded into a turn.
const TURN_UM: (f64, f64) = (100.0, 3_000.0);

/// Joins `strokes` end to start where the gap is at most `reach_um` and the straight line across
/// it lies inside `region`. Strokes are only ever turned round, never cut, so every point of every
/// input stroke is still drawn.
#[must_use]
pub fn connect(strokes: Vec<Vec<PtUm>>, region: &Region, reach_um: i64) -> Vec<Vec<PtUm>> {
    connect_rounded(strokes, region, reach_um, None)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "grid coordinates are far below 2^53 µm, where i64 -> f64 is exact"
)]
fn at(p: PtUm) -> (f64, f64) {
    (p.x as f64, p.y as f64)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is a rounded grid coordinate, far below 2^63 µm"
)]
fn grid(x: f64, y: f64) -> PtUm {
    PtUm::new(libm::round(x) as i64, libm::round(y) as i64)
}

fn unit((x, y): (f64, f64)) -> Option<(f64, f64)> {
    let length = libm::hypot(x, y);
    (length > 0.0).then(|| (x / length, y / length))
}

/// The chords of a semicircle whose chords turn by at most `step_deg`: 2 to 360.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is a whole number held to 2..=360 on the line above"
)]
fn chord_count(step_deg: f64) -> u32 {
    libm::ceil(180.0 / step_deg).clamp(2.0, 360.0) as u32
}

/// If the chain's last row and the next stroke's first row make a hairpin, replaces the sharp join
/// with a semicircle: cuts both rows back by the radius and puts the turn's points on the chain.
/// Leaves both alone when rounding is not possible or the arc would leave the region.
fn round_hairpin(chain: &mut Vec<PtUm>, stroke: &mut [PtUm], region: &Region, step_deg: f64) {
    let (n, m) = (chain.len(), stroke.len());
    if n < 2 || m < 2 || step_deg.is_nan() || step_deg <= 0.0 {
        return;
    }
    let (back, tail) = (at(chain[n - 2]), at(chain[n - 1]));
    let (first, next) = (at(stroke[0]), at(stroke[1]));
    let (Some(row), Some(other)) = (
        unit((tail.0 - back.0, tail.1 - back.1)),
        unit((next.0 - first.0, next.1 - first.1)),
    ) else {
        return;
    };
    if row.0 * other.0 + row.1 * other.1 > HAIRPIN_COS {
        return;
    }
    let gap = libm::hypot(first.0 - tail.0, first.1 - tail.1);
    if !(TURN_UM.0..=TURN_UM.1).contains(&gap) {
        return;
    }
    let across = ((first.0 - tail.0) / gap, (first.1 - tail.1) / gap);
    if (across.0 * row.0 + across.1 * row.1).abs() > ACROSS_COS {
        return;
    }
    let radius = gap / 2.0;
    let row_len = libm::hypot(tail.0 - back.0, tail.1 - back.1);
    let next_len = libm::hypot(next.0 - first.0, next.1 - first.1);
    if row_len < TURN_MIN_ROW_UM || next_len < TURN_MIN_ROW_UM {
        return;
    }
    // Both rows are cut back by the radius, so the turn's far side is where the rows ended.
    let tail_in = (tail.0 - row.0 * radius, tail.1 - row.1 * radius);
    let first_in = (first.0 + other.0 * radius, first.1 + other.1 * radius);
    let Some(side) = unit((first_in.0 - tail_in.0, first_in.1 - tail_in.1)) else {
        return;
    };
    let centre = (
        f64::midpoint(tail_in.0, first_in.0),
        f64::midpoint(tail_in.1, first_in.1),
    );
    let turn_radius = libm::hypot(first_in.0 - tail_in.0, first_in.1 - tail_in.1) / 2.0;
    let count = chord_count(step_deg);
    let chords = f64::from(count);
    let mut turn = vec![grid(tail_in.0, tail_in.1)];
    for k in 1..count {
        let theta = core::f64::consts::PI * f64::from(k) / chords;
        let (c, s) = (libm::cos(theta), libm::sin(theta));
        turn.push(grid(
            centre.0 + turn_radius * (-side.0 * c + row.0 * s),
            centre.1 + turn_radius * (-side.1 * c + row.1 * s),
        ));
    }
    turn.push(grid(first_in.0, first_in.1));
    if !turn.windows(2).all(|w| region.contains_segment(w[0], w[1])) {
        return;
    }
    chain.pop();
    chain.extend(turn[..turn.len() - 1].iter().copied());
    stroke[0] = turn[turn.len() - 1];
}

/// [`connect`], and where `turn_step_deg` is set, hairpins between joined rows are drawn as
/// semicircles whose chords turn by at most that many degrees.
#[must_use]
pub(crate) fn connect_rounded(
    strokes: Vec<Vec<PtUm>>,
    region: &Region,
    reach_um: i64,
    turn_step_deg: Option<f64>,
) -> Vec<Vec<PtUm>> {
    let strokes: Vec<Vec<PtUm>> = strokes.into_iter().filter(|s| !s.is_empty()).collect();
    if strokes.is_empty() {
        return Vec::new();
    }
    let items: Vec<Item> = strokes
        .iter()
        .map(|s| Item {
            start: s[0],
            end: s[s.len() - 1],
            reversible: true,
        })
        .collect();
    let joins = Joins {
        joinable: joinable_pairs(&strokes, region, reach_um),
    };
    let mut tour = nearest_neighbour(&items, START);
    let work = items
        .len()
        .saturating_mul(items.len())
        .saturating_mul(3)
        .max(1);
    let sweeps = (WORK / work).clamp(1, MAX_SWEEPS);
    improve_sweeps_by(&joins, &items, &mut tour, START, sweeps);

    let mut chains: Vec<Vec<PtUm>> = Vec::new();
    for visit in tour {
        let mut stroke = strokes[visit.index].clone();
        if visit.reversed {
            stroke.reverse();
        }
        let joined = chains.last_mut().is_some_and(|chain| {
            let (Some(&tail), Some(&first)) = (chain.last(), stroke.first()) else {
                return false;
            };
            if !joins.joinable.contains(&pair(tail, first)) {
                return false;
            }
            if let Some(step) = turn_step_deg {
                round_hairpin(chain, &mut stroke, region, step);
            }
            chain.extend(stroke.iter().copied());
            true
        });
        if !joined {
            chains.push(stroke);
        }
    }
    chains
}

#[cfg(test)]
mod tests;
