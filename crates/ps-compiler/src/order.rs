//! Ordering helpers.
//!
//! * Cut containment: closed cut contours form a containment forest; cuts are emitted children
//!   before parents, smaller siblings before larger ones, so pieces stay held to the mat until
//!   their last cut.
//! * Path order within a tool group: greedy nearest neighbour from the current head position over
//!   an R*-tree of candidate start points, then 2-opt and Or-opt sweeps for a fixed number of
//!   sweeps (never a wall-clock time), so the result never depends on machine speed.
//!
//! Everything is exact integer geometry on the µm grid, or `libm` distances.

use ps_core::PtUm;
use rstar::RTree;
use rstar::primitives::GeomWithData;

/// A distance below which a move is not an improvement (µm).
const EPS: f64 = 1e-9;
/// The longest run of consecutive paths Or-opt moves at once.
const OR_OPT_MAX_SEGMENT: usize = 3;

// ---- containment ----

/// Twice the absolute area of a ring in µm².
fn area2(ring: &[PtUm]) -> i128 {
    ps_geometry::ring_area2(ring).abs()
}

/// True when `p` lies inside the ring or on its boundary (exact integer arithmetic).
#[must_use]
pub fn point_in_ring(p: PtUm, ring: &[PtUm]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let mut inside = false;
    for (i, &a) in ring.iter().enumerate() {
        let b = ring[(i + 1) % ring.len()];
        let cross = i128::from(b.x - a.x) * i128::from(p.y - a.y)
            - i128::from(p.x - a.x) * i128::from(b.y - a.y);
        let within = p.x >= a.x.min(b.x)
            && p.x <= a.x.max(b.x)
            && p.y >= a.y.min(b.y)
            && p.y <= a.y.max(b.y);
        if cross == 0 && within {
            return true; // on an edge or vertex
        }
        if (a.y > p.y) != (b.y > p.y) && ((b.y > a.y && cross > 0) || (b.y < a.y && cross < 0)) {
            inside = !inside;
        }
    }
    inside
}

/// The containment forest of a set of closed rings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forest {
    /// For each ring, the smallest ring that contains it.
    pub parent: Vec<Option<usize>>,
    /// For each ring, the rings it directly contains, in index order.
    pub children: Vec<Vec<usize>>,
}

/// Builds the containment forest: a ring's parent is the smallest strictly larger ring that
/// holds all of its vertices.
#[must_use]
pub fn containment(rings: &[Vec<PtUm>]) -> Forest {
    let areas: Vec<i128> = rings.iter().map(|r| area2(r)).collect();
    let mut parent: Vec<Option<usize>> = vec![None; rings.len()];
    for (i, child) in rings.iter().enumerate() {
        let mut best: Option<usize> = None;
        for (j, outer) in rings.iter().enumerate() {
            if i == j || areas[j] <= areas[i] || !child.iter().all(|&v| point_in_ring(v, outer)) {
                continue;
            }
            if best.is_none_or(|b| areas[j] < areas[b]) {
                best = Some(j);
            }
        }
        parent[i] = best;
    }
    let mut children = vec![Vec::new(); rings.len()];
    for (i, p) in parent.iter().enumerate() {
        if let Some(p) = p {
            children[*p].push(i);
        }
    }
    Forest { parent, children }
}

fn visit(node: usize, forest: &Forest, areas: &[i128], out: &mut Vec<usize>) {
    let mut kids = forest.children[node].clone();
    kids.sort_by_key(|&i| (areas[i], i));
    for k in kids {
        visit(k, forest, areas, out);
    }
    out.push(node);
}

/// The order to cut closed contours: post-order over the containment forest, siblings (and
/// roots) by ascending area, ties by index.
#[must_use]
pub fn emission_order(rings: &[Vec<PtUm>]) -> Vec<usize> {
    let forest = containment(rings);
    let areas: Vec<i128> = rings.iter().map(|r| area2(r)).collect();
    let mut roots: Vec<usize> = (0..rings.len())
        .filter(|&i| forest.parent[i].is_none())
        .collect();
    roots.sort_by_key(|&i| (areas[i], i));
    let mut out = Vec::with_capacity(rings.len());
    for r in roots {
        visit(r, &forest, &areas, &mut out);
    }
    out
}

// ---- choosing where a closed path starts ----

/// A closed path rotated to start at its vertex nearest `from`; open paths come back unchanged.
#[must_use]
pub fn start_nearest(path: &ps_geometry::Path, from: ps_geometry::Point) -> ps_geometry::Path {
    if !path.closed || path.segs.len() < 2 {
        return path.clone();
    }
    // Vertex k is the end of segment k - 1; vertex 0 is the start.
    let vertices: Vec<ps_geometry::Point> = std::iter::once(path.start)
        .chain(path.segs.iter().map(ps_geometry::Seg::to))
        .take(path.segs.len())
        .collect();
    let nearest = vertices
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            ps_geometry::arc::dist(**a, from).total_cmp(&ps_geometry::arc::dist(**b, from))
        })
        .map_or(0, |(i, _)| i);
    if nearest == 0 {
        return path.clone();
    }
    let mut rotated = ps_geometry::Path::new(vertices[nearest]);
    rotated.segs.extend(path.segs[nearest..].iter().copied());
    rotated.segs.extend(path.segs[..nearest].iter().copied());
    rotated.closed = true;
    rotated
}

// ---- path ordering ----

/// A path to order: where it starts and ends, and whether it may be drawn backwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    /// First point.
    pub start: PtUm,
    /// Last point.
    pub end: PtUm,
    /// Whether the tool may traverse it from `end` to `start`.
    pub reversible: bool,
}

/// One stop of a tour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visit {
    /// Index into the item list.
    pub index: usize,
    /// True when the path is drawn from its end to its start.
    pub reversed: bool,
}

fn entry(items: &[Item], v: Visit) -> PtUm {
    if v.reversed {
        items[v.index].end
    } else {
        items[v.index].start
    }
}

fn exit(items: &[Item], v: Visit) -> PtUm {
    if v.reversed {
        items[v.index].start
    } else {
        items[v.index].end
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "grid coordinates are far below 2^53 µm, where i64 -> f64 is exact"
)]
fn dist(a: PtUm, b: PtUm) -> f64 {
    libm::hypot((b.x - a.x) as f64, (b.y - a.y) as f64)
}

/// Total travel of a tour from `head`: head to the first entry, then exit to entry between
/// consecutive paths (the drawing itself is not counted).
#[must_use]
pub fn tour_length(items: &[Item], order: &[Visit], head: PtUm) -> f64 {
    let mut at = head;
    let mut total = 0.0;
    for &v in order {
        total += dist(at, entry(items, v));
        at = exit(items, v);
    }
    total
}

type Entry = GeomWithData<[f64; 2], (usize, bool)>;

#[expect(
    clippy::cast_precision_loss,
    reason = "grid coordinates are far below 2^53 µm, where i64 -> f64 is exact"
)]
fn coords(p: PtUm) -> [f64; 2] {
    [p.x as f64, p.y as f64]
}

/// Greedy nearest-neighbour order from `head`, using an R*-tree over the candidate ends.
///
/// Reversible paths may be entered from either end; ties go to the lower index (and to the start
/// of a path before its end).
#[must_use]
pub fn nearest_neighbour(items: &[Item], head: PtUm) -> Vec<Visit> {
    let mut entries: Vec<Entry> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        entries.push(GeomWithData::new(coords(it.start), (i, false)));
        if it.reversible {
            entries.push(GeomWithData::new(coords(it.end), (i, true)));
        }
    }
    let mut tree = RTree::bulk_load(entries);
    let mut order = Vec::with_capacity(items.len());
    let mut at = head;
    while !tree.size().eq(&0) {
        let mut best: Option<(f64, Entry)> = None;
        for (e, d2) in tree.nearest_neighbor_iter_with_distance_2(&coords(at)) {
            match &best {
                Some((bd, _)) if d2 > *bd => break,
                Some((_, cur)) if (e.data.0, e.data.1) >= (cur.data.0, cur.data.1) => {}
                _ => best = Some((d2, *e)),
            }
        }
        let Some((_, chosen)) = best else {
            break;
        };
        let (index, reversed) = chosen.data;
        for e in [(index, false), (index, true)] {
            let key = items
                .get(index)
                .map(|it| if e.1 { it.end } else { it.start });
            if let Some(p) = key {
                let probe: Entry = GeomWithData::new(coords(p), e);
                tree.remove(&probe);
            }
        }
        let v = Visit { index, reversed };
        at = exit(items, v);
        order.push(v);
    }
    order
}

/// Improves a tour in place with 2-opt and Or-opt, first improvement in a fixed scan order, for
/// at most `max_iters` accepted moves. Returns how many moves were made.
///
/// The tour never gets longer, and paths that may not be reversed are never reversed.
pub fn improve(items: &[Item], order: &mut Vec<Visit>, head: PtUm, max_iters: usize) -> usize {
    let mut used = 0;
    while used < max_iters && (two_opt_step(items, order, head) || or_opt_step(items, order, head))
    {
        used += 1;
    }
    used
}

fn flip(items: &[Item], v: Visit) -> Visit {
    Visit {
        index: v.index,
        reversed: !v.reversed && items[v.index].reversible
            || v.reversed && !items[v.index].reversible,
    }
}

fn two_opt_step(items: &[Item], order: &mut [Visit], head: PtUm) -> bool {
    let n = order.len();
    for i in 0..n {
        let left = if i == 0 {
            head
        } else {
            exit(items, order[i - 1])
        };
        for j in i + 1..n {
            if order[i..=j].iter().any(|v| !items[v.index].reversible) {
                continue;
            }
            let right = order.get(j + 1).map(|&v| entry(items, v));
            let before = dist(left, entry(items, order[i]))
                + right.map_or(0.0, |r| dist(exit(items, order[j]), r));
            let after = dist(left, exit(items, order[j]))
                + right.map_or(0.0, |r| dist(entry(items, order[i]), r));
            if after < before - EPS {
                order[i..=j].reverse();
                for v in &mut order[i..=j] {
                    *v = flip(items, *v);
                }
                return true;
            }
        }
    }
    false
}

fn or_opt_step(items: &[Item], order: &mut Vec<Visit>, head: PtUm) -> bool {
    let n = order.len();
    for len in 1..=OR_OPT_MAX_SEGMENT.min(n) {
        for i in 0..=n - len {
            let seg = &order[i..i + len];
            let rest: Vec<Visit> = order[..i]
                .iter()
                .chain(&order[i + len..])
                .copied()
                .collect();
            let left = if i == 0 {
                head
            } else {
                exit(items, rest[i - 1])
            };
            let right = rest.get(i).map(|&v| entry(items, v));
            let (seg_in, seg_out) = (entry(items, seg[0]), exit(items, seg[len - 1]));
            let removed = dist(left, seg_in) + right.map_or(0.0, |r| dist(seg_out, r))
                - right.map_or(0.0, |r| dist(left, r));
            let can_flip = seg.iter().all(|v| items[v.index].reversible);
            for q in 0..=rest.len() {
                let a = if q == 0 {
                    head
                } else {
                    exit(items, rest[q - 1])
                };
                let b = rest.get(q).map(|&v| entry(items, v));
                for reversed in [false, true] {
                    if reversed && !can_flip {
                        continue;
                    }
                    if !reversed && q == i {
                        continue; // the same place
                    }
                    let (sin, sout) = if reversed {
                        (seg_out, seg_in)
                    } else {
                        (seg_in, seg_out)
                    };
                    let added = dist(a, sin) + b.map_or(0.0, |x| dist(sout, x))
                        - b.map_or(0.0, |x| dist(a, x));
                    if added < removed - EPS {
                        let mut moved: Vec<Visit> = seg.to_vec();
                        if reversed {
                            moved.reverse();
                            for v in &mut moved {
                                *v = flip(items, *v);
                            }
                        }
                        let mut next = rest;
                        for (k, v) in moved.into_iter().enumerate() {
                            next.insert(q + k, v);
                        }
                        *order = next;
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// What it costs to move the tool from one point to another between paths.
pub(crate) trait Metric {
    /// The cost of the move; it must not depend on the direction.
    fn cost(&self, a: PtUm, b: PtUm) -> f64;
}

/// Plain distance in µm.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Straight;

impl Metric for Straight {
    fn cost(&self, a: PtUm, b: PtUm) -> f64 {
        dist(a, b)
    }
}

/// Whether the path at `v` may be turned round.
fn can_turn(items: &[Item], v: Visit) -> bool {
    items[v.index].reversible
}

/// Where the tool is after the first `k` visits of `order`.
fn exit_before(items: &[Item], order: &[Visit], head: PtUm, k: usize) -> PtUm {
    if k == 0 {
        head
    } else {
        exit(items, order[k - 1])
    }
}

/// 2-opt over the whole tour in one sweep: every `i..=j` reversal that shortens the tour is made
/// as it is found and the sweep goes on from there, instead of starting again. Returns whether
/// anything changed.
fn two_opt_sweep<M: Metric>(m: &M, items: &[Item], order: &mut [Visit], head: PtUm) -> bool {
    let n = order.len();
    let mut changed = false;
    for i in 0..n {
        for j in i..n {
            if !can_turn(items, order[j]) {
                break;
            }
            let left = exit_before(items, order, head, i);
            let right = order.get(j + 1).map(|&v| entry(items, v));
            let (first, last) = (entry(items, order[i]), exit(items, order[j]));
            let before = m.cost(left, first) + right.map_or(0.0, |r| m.cost(last, r));
            let after = m.cost(left, last) + right.map_or(0.0, |r| m.cost(first, r));
            if after < before - EPS {
                order[i..=j].reverse();
                for v in &mut order[i..=j] {
                    v.reversed = !v.reversed;
                }
                changed = true;
            }
        }
    }
    changed
}

/// Or-opt over the whole tour in one sweep: runs of up to [`OR_OPT_MAX_SEGMENT`] paths are moved
/// (and turned round if that helps) to where they cost least, without allocating.
fn or_opt_sweep<M: Metric>(m: &M, items: &[Item], order: &mut [Visit], head: PtUm) -> bool {
    let n = order.len();
    let mut changed = false;
    for len in 1..=OR_OPT_MAX_SEGMENT.min(n) {
        let mut i = 0;
        while i + len <= n {
            if try_move_segment(m, items, order, head, i, len) {
                changed = true;
            } else {
                i += 1;
            }
        }
    }
    changed
}

/// Moves `order[first..first + len]` to its cheapest other place if that shortens the tour.
fn try_move_segment<M: Metric>(
    metric: &M,
    items: &[Item],
    order: &mut [Visit],
    head: PtUm,
    first: usize,
    len: usize,
) -> bool {
    let n = order.len();
    let left = exit_before(items, order, head, first);
    let right = order.get(first + len).map(|&v| entry(items, v));
    let (seg_in, seg_out) = (
        entry(items, order[first]),
        exit(items, order[first + len - 1]),
    );
    let removed = metric.cost(left, seg_in) + right.map_or(0.0, |to| metric.cost(seg_out, to))
        - right.map_or(0.0, |to| metric.cost(left, to));
    let can_flip = order[first..first + len]
        .iter()
        .all(|&visit| can_turn(items, visit));
    let mut best: Option<(f64, usize, bool)> = None;
    for slot in (0..=n).filter(|slot| *slot < first || *slot > first + len) {
        let prev = exit_before(items, order, head, slot);
        let next = order.get(slot).map(|&v| entry(items, v));
        let gap = next.map_or(0.0, |to| metric.cost(prev, to));
        for reversed in [false, true] {
            if reversed && !can_flip {
                continue;
            }
            let (sin, sout) = if reversed {
                (seg_out, seg_in)
            } else {
                (seg_in, seg_out)
            };
            let gain = removed
                - (metric.cost(prev, sin) + next.map_or(0.0, |to| metric.cost(sout, to)) - gap);
            if gain > EPS && best.is_none_or(|(best_gain, _, _)| gain > best_gain + EPS) {
                best = Some((gain, slot, reversed));
            }
        }
    }
    let Some((_, slot, reversed)) = best else {
        return false;
    };
    let at = if slot > first {
        order[first..slot].rotate_left(len);
        slot - len
    } else {
        order[slot..first + len].rotate_right(len);
        slot
    };
    if reversed {
        order[at..at + len].reverse();
        for v in &mut order[at..at + len] {
            v.reversed = !v.reversed;
        }
    }
    true
}

/// Local search to a standstill: 2-opt and Or-opt sweeps until a sweep changes nothing, or
/// `max_sweeps` sweeps have run. Returns the number of sweeps made.
pub fn improve_sweeps(items: &[Item], order: &mut [Visit], head: PtUm, max_sweeps: usize) -> usize {
    improve_sweeps_by(&Straight, items, order, head, max_sweeps)
}

/// [`improve_sweeps`] under another [`Metric`], for tours whose cost is not plain distance.
pub(crate) fn improve_sweeps_by<M: Metric>(
    m: &M,
    items: &[Item],
    order: &mut [Visit],
    head: PtUm,
    max_sweeps: usize,
) -> usize {
    let mut sweeps = 0;
    while sweeps < max_sweeps {
        sweeps += 1;
        let a = two_opt_sweep(m, items, order, head);
        let b = or_opt_sweep(m, items, order, head);
        if !(a || b) {
            break;
        }
    }
    sweeps
}

#[cfg(test)]
mod tests;
