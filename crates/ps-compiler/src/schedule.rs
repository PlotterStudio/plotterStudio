//! Block ordering.
//!
//! Operations are grouped into blocks. The default stage order follows the design: overlay blocks,
//! surface operations (pens light to dark), perforate and wavy cuts, kiss cuts, through cuts. The
//! order is a topological sort over a constraint graph whose edges are user locks and safety
//! requirements; ties break by user row, then tool id, so the result is fully deterministic. When
//! a user lock conflicts with a safety edge the user order is kept and the edge is reported.

use std::cmp::Ordering;

use ps_model::{OpKind, OrderingMode};

/// The stage a block belongs to; lower runs first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    /// Operations that need a foil sheet or similar, wrapped by place and remove pauses.
    Overlay,
    /// Draw, fill, emboss, deboss, engrave, score.
    Surface,
    /// Perforate and wavy cuts.
    Perforate,
    /// Kiss cuts.
    KissCut,
    /// Through cuts.
    ThroughCut,
}

/// The stage an operation kind runs in.
#[must_use]
pub fn stage_of(kind: OpKind) -> Stage {
    match kind {
        OpKind::Foil | OpKind::FoilFill => Stage::Overlay,
        OpKind::Draw
        | OpKind::Fill
        | OpKind::Score
        | OpKind::Emboss
        | OpKind::Deboss
        | OpKind::Engrave => Stage::Surface,
        OpKind::Perforate | OpKind::WavyCut => Stage::Perforate,
        OpKind::KissCut => Stage::KissCut,
        OpKind::Cut => Stage::ThroughCut,
    }
}

/// A unit of scheduling: all the work of one operation row with one tool.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// The stage.
    pub stage: Stage,
    /// The user's row order.
    pub row: u64,
    /// The tool id, for tie-breaking.
    pub tool: String,
    /// CIELAB lightness of a pen colour, so pens run light to dark.
    pub lightness: Option<f64>,
}

/// Whether an edge comes from the user or from a safety rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// A user "run after" lock; always honoured.
    Lock,
    /// A release-safety requirement; dropped when it conflicts with locks.
    Safety,
}

/// "Block `before` must run before block `after`."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    /// The earlier block.
    pub before: usize,
    /// The later block.
    pub after: usize,
    /// Where the constraint comes from.
    pub kind: EdgeKind,
}

impl Edge {
    /// A user lock.
    #[must_use]
    pub fn lock(before: usize, after: usize) -> Self {
        Self {
            before,
            after,
            kind: EdgeKind::Lock,
        }
    }

    /// A safety edge.
    #[must_use]
    pub fn safety(before: usize, after: usize) -> Self {
        Self {
            before,
            after,
            kind: EdgeKind::Safety,
        }
    }
}

/// The locks form a cycle, so no order satisfies them.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the run-after locks contradict each other")]
pub struct Cycle;

/// The result of scheduling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    /// Block indices in run order.
    pub order: Vec<usize>,
    /// Safety edges the default (user) order would have violated, which the schedule fixed.
    pub reordered: Vec<(usize, usize)>,
    /// Safety edges that could not be honoured because user locks conflict with them.
    pub unresolved: Vec<(usize, usize)>,
}

fn compare(a: &Block, b: &Block, mode: OrderingMode) -> Ordering {
    let tail = || a.row.cmp(&b.row).then_with(|| a.tool.cmp(&b.tool));
    match mode {
        OrderingMode::StrictOrder => tail(),
        OrderingMode::MinimiseToolChanges => a.stage.cmp(&b.stage).then_with(|| {
            let light = match (a.lightness, b.lightness) {
                (Some(x), Some(y)) if a.stage == Stage::Surface => y.total_cmp(&x),
                _ => Ordering::Equal,
            };
            light.then_with(tail)
        }),
    }
}

/// Orders blocks: Kahn's algorithm that always takes the best ready block by (stage, lightness,
/// row, tool), after the user's own default order is computed to find which safety edges it
/// violated.
///
/// # Errors
///
/// [`Cycle`] when the lock edges alone are contradictory.
pub fn schedule(blocks: &[Block], edges: &[Edge], mode: OrderingMode) -> Result<Schedule, Cycle> {
    let n = blocks.len();
    let ranked = |remaining: &[usize]| -> Option<usize> {
        remaining
            .iter()
            .copied()
            .min_by(|&x, &y| compare(&blocks[x], &blocks[y], mode).then_with(|| x.cmp(&y)))
    };

    // The default order, ignoring safety edges, to report which ones it breaks.
    let locks: Vec<Edge> = edges
        .iter()
        .copied()
        .filter(|e| e.kind == EdgeKind::Lock)
        .collect();
    let (default_order, _) = kahn(n, &locks, &ranked)?;
    let mut pos = vec![0; n];
    for (i, &b) in default_order.iter().enumerate() {
        pos[b] = i;
    }

    let (order, unresolved) = kahn(n, edges, &ranked)?;
    let mut placed = vec![0; n];
    for (i, &b) in order.iter().enumerate() {
        placed[b] = i;
    }
    let mut reordered = Vec::new();
    for e in edges.iter().filter(|e| e.kind == EdgeKind::Safety) {
        if pos[e.before] > pos[e.after] && placed[e.before] < placed[e.after] {
            reordered.push((e.before, e.after));
        }
    }
    Ok(Schedule {
        order,
        reordered,
        unresolved,
    })
}

/// A topological order plus the safety edges that had to be relaxed to reach it.
type Ordered = (Vec<usize>, Vec<(usize, usize)>);

type Ranker<'a> = dyn Fn(&[usize]) -> Option<usize> + 'a;

/// Kahn's algorithm. When stuck, drops the incoming safety edges of the best remaining block and
/// reports them; if only lock edges remain, the graph has a cycle.
fn kahn(n: usize, edges: &[Edge], ranked: &Ranker<'_>) -> Result<Ordered, Cycle> {
    let mut live: Vec<bool> = vec![true; edges.len()];
    let mut done = vec![false; n];
    // The number of live edges into each block whose source is not placed yet, kept up to date
    // as blocks are placed and edges relaxed, instead of recounted from every edge each time.
    let mut indegree = vec![0_usize; n];
    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, e) in edges.iter().enumerate() {
        indegree[e.after] += 1;
        outgoing[e.before].push(i);
        incoming[e.after].push(i);
    }
    let mut order = Vec::with_capacity(n);
    let mut unresolved = Vec::new();
    while order.len() < n {
        let ready: Vec<usize> = (0..n).filter(|&b| !done[b] && indegree[b] == 0).collect();
        if let Some(next) = ranked(&ready) {
            done[next] = true;
            order.push(next);
            for &i in &outgoing[next] {
                if live[i] {
                    indegree[edges[i].after] -= 1;
                }
            }
            continue;
        }
        // Stuck: relax the safety edges into the best remaining block.
        let mut relaxed = false;
        let mut candidates: Vec<usize> = (0..n).filter(|&b| !done[b]).collect();
        while let Some(b) = ranked(&candidates) {
            let blockers: Vec<usize> = incoming[b]
                .iter()
                .copied()
                .filter(|&i| live[i] && !done[edges[i].before])
                .collect();
            if !blockers.is_empty() && blockers.iter().all(|&i| edges[i].kind == EdgeKind::Safety) {
                for i in blockers {
                    live[i] = false;
                    indegree[b] -= 1;
                    unresolved.push((edges[i].before, edges[i].after));
                }
                relaxed = true;
                break;
            }
            candidates.retain(|&c| c != b);
        }
        if !relaxed {
            return Err(Cycle);
        }
    }
    unresolved.sort_unstable();
    unresolved.dedup();
    Ok((order, unresolved))
}

#[cfg(test)]
mod tests;
