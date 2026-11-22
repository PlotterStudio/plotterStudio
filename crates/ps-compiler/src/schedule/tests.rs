//! Ground truth for block ordering: stages in the documented order (overlay, surface,
//! perforate/wavy, kiss cut, through cut), pens light to dark, ties by user row then tool id,
//! a topological sort over lock and safety edges, and user locks winning over safety edges.

use ps_model::OpKind;

use super::*;

fn block(kind: OpKind, row: u64, tool: &str) -> Block {
    Block {
        stage: stage_of(kind),
        row,
        tool: tool.to_owned(),
        lightness: None,
    }
}

fn pen(row: u64, tool: &str, lightness: f64) -> Block {
    Block {
        lightness: Some(lightness),
        ..block(OpKind::Draw, row, tool)
    }
}

#[test]
fn every_operation_belongs_to_a_stage() {
    assert_eq!(stage_of(OpKind::Foil), Stage::Overlay);
    assert_eq!(stage_of(OpKind::FoilFill), Stage::Overlay);
    for k in [
        OpKind::Draw,
        OpKind::Fill,
        OpKind::Emboss,
        OpKind::Deboss,
        OpKind::Engrave,
        OpKind::Score,
    ] {
        assert_eq!(stage_of(k), Stage::Surface, "{k:?}");
    }
    assert_eq!(stage_of(OpKind::Perforate), Stage::Perforate);
    assert_eq!(stage_of(OpKind::WavyCut), Stage::Perforate);
    assert_eq!(stage_of(OpKind::KissCut), Stage::KissCut);
    assert_eq!(stage_of(OpKind::Cut), Stage::ThroughCut);
}

#[test]
fn stages_are_ranked_in_the_documented_order() {
    assert!(Stage::Overlay < Stage::Surface);
    assert!(Stage::Surface < Stage::Perforate);
    assert!(Stage::Perforate < Stage::KissCut);
    assert!(Stage::KissCut < Stage::ThroughCut);
}

#[test]
fn blocks_run_in_stage_order_whatever_the_row_order() {
    let blocks = [
        block(OpKind::Cut, 0, "blade"),
        block(OpKind::KissCut, 1, "blade"),
        block(OpKind::Draw, 2, "pen"),
        block(OpKind::Foil, 3, "foil"),
        block(OpKind::Perforate, 4, "blade"),
    ];
    let s = schedule(&blocks, &[], OrderingMode::MinimiseToolChanges).unwrap();
    assert_eq!(s.order, vec![3, 2, 4, 1, 0]);
}

#[test]
fn pens_go_light_to_dark_within_the_surface_stage() {
    let blocks = [
        pen(0, "black", 5.0),
        pen(1, "yellow", 90.0),
        pen(2, "red", 45.0),
    ];
    let s = schedule(&blocks, &[], OrderingMode::MinimiseToolChanges).unwrap();
    assert_eq!(s.order, vec![1, 2, 0]);
}

#[test]
fn ties_break_by_row_then_tool_id() {
    let blocks = [
        block(OpKind::Cut, 5, "b"),
        block(OpKind::Cut, 5, "a"),
        block(OpKind::Cut, 2, "z"),
    ];
    let s = schedule(&blocks, &[], OrderingMode::MinimiseToolChanges).unwrap();
    assert_eq!(s.order, vec![2, 1, 0]);
}

#[test]
fn strict_order_follows_rows_and_ignores_stage_and_lightness() {
    let blocks = [
        block(OpKind::Cut, 0, "blade"),
        pen(1, "yellow", 90.0),
        pen(2, "black", 5.0),
    ];
    let s = schedule(&blocks, &[], OrderingMode::StrictOrder).unwrap();
    assert_eq!(s.order, vec![0, 1, 2]);
}

#[test]
fn a_lock_forces_one_block_after_another() {
    let blocks = [pen(0, "a", 10.0), pen(1, "b", 90.0)];
    // Light-to-dark would put block 1 first; the lock says 1 runs after 0.
    let edges = [Edge::lock(0, 1)];
    let s = schedule(&blocks, &edges, OrderingMode::MinimiseToolChanges).unwrap();
    assert_eq!(s.order, vec![0, 1]);
}

#[test]
fn a_safety_edge_reorders_and_reports_what_the_default_order_violated() {
    let blocks = [
        block(OpKind::Cut, 0, "blade"),
        block(OpKind::Cut, 1, "blade"),
    ];
    // Block 1 must run before block 0 (it lies inside the piece block 0 frees).
    let edges = [Edge::safety(1, 0)];
    let s = schedule(&blocks, &edges, OrderingMode::MinimiseToolChanges).unwrap();
    assert_eq!(s.order, vec![1, 0]);
    assert_eq!(s.reordered, vec![(1, 0)]);
    assert_eq!(s.unresolved.len(), 0);
}

#[test]
fn a_satisfied_safety_edge_reports_nothing() {
    let blocks = [
        block(OpKind::Cut, 0, "blade"),
        block(OpKind::Cut, 1, "blade"),
    ];
    let s = schedule(
        &blocks,
        &[Edge::safety(0, 1)],
        OrderingMode::MinimiseToolChanges,
    )
    .unwrap();
    assert_eq!(s.order, vec![0, 1]);
    assert_eq!(s.reordered.len(), 0);
}

#[test]
fn locks_win_over_conflicting_safety_edges() {
    let blocks = [
        block(OpKind::Cut, 0, "blade"),
        block(OpKind::Cut, 1, "blade"),
    ];
    // The user locked 0 before 1, safety wants 1 before 0: keep the user order and say so.
    let edges = [Edge::lock(0, 1), Edge::safety(1, 0)];
    let s = schedule(&blocks, &edges, OrderingMode::MinimiseToolChanges).unwrap();
    assert_eq!(s.order, vec![0, 1]);
    assert_eq!(s.unresolved, vec![(1, 0)]);
}

#[test]
fn contradictory_locks_are_an_error() {
    let blocks = [block(OpKind::Cut, 0, "a"), block(OpKind::Cut, 1, "b")];
    let edges = [Edge::lock(0, 1), Edge::lock(1, 0)];
    assert!(schedule(&blocks, &edges, OrderingMode::MinimiseToolChanges).is_err());
}

#[test]
fn a_chain_of_safety_edges_across_stages_is_followed() {
    // A pen block must precede a cut block that the stage order already puts last.
    let blocks = [block(OpKind::Cut, 0, "blade"), pen(1, "pen", 10.0)];
    let s = schedule(
        &blocks,
        &[Edge::safety(1, 0)],
        OrderingMode::MinimiseToolChanges,
    )
    .unwrap();
    assert_eq!(s.order, vec![1, 0]);
    assert_eq!(s.reordered.len(), 0, "stage order already satisfied it");
}

#[test]
fn an_empty_schedule_is_fine() {
    let s = schedule(&[], &[], OrderingMode::MinimiseToolChanges).unwrap();
    assert_eq!(s.order.len(), 0);
}

#[test]
fn scheduling_is_deterministic() {
    let blocks: Vec<Block> = (0..30)
        .map(|i| {
            if i % 3 == 0 {
                block(OpKind::Cut, i, "blade")
            } else {
                pen(i, "pen", f64::from((i * 37 % 100) as u32))
            }
        })
        .collect();
    let edges = [Edge::safety(5, 3), Edge::lock(7, 4)];
    let a = schedule(&blocks, &edges, OrderingMode::MinimiseToolChanges).unwrap();
    let b = schedule(&blocks, &edges, OrderingMode::MinimiseToolChanges).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.order.len(), 30);
}

#[test]
fn a_long_chain_of_safety_edges_is_ordered_quickly_and_in_chain_order() {
    // Rows run backwards, so only the chain of edges decides the order: block i must follow i-1.
    let n = 3_000_u64;
    let blocks: Vec<Block> = (0..n).map(|i| block(OpKind::Cut, n - i, "blade")).collect();
    let edges: Vec<Edge> = (1..blocks.len())
        .map(|i| Edge {
            before: i - 1,
            after: i,
            kind: EdgeKind::Safety,
        })
        .collect();
    let started = std::time::Instant::now();
    let s = schedule(&blocks, &edges, OrderingMode::MinimiseToolChanges).unwrap();
    assert_eq!(s.order, (0..blocks.len()).collect::<Vec<_>>());
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "ordering 3,000 blocks took {:?}",
        started.elapsed()
    );
}
