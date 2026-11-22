//! Ground truth for ordering helpers: cuts are emitted children before parents,
//! smaller siblings before larger ones, so pieces stay held to the mat until their last cut;
//! paths within a tool group go nearest-neighbour, then 2-opt/Or-opt improved for a fixed number
//! of iterations (never wall-clock time), so the result never depends on machine speed.

use proptest::prelude::*;
use ps_core::PtUm;

use super::*;

const MM: i64 = 1_000;

fn rect(x0: i64, y0: i64, x1: i64, y1: i64) -> Vec<PtUm> {
    vec![
        PtUm::new(x0 * MM, y0 * MM),
        PtUm::new(x1 * MM, y0 * MM),
        PtUm::new(x1 * MM, y1 * MM),
        PtUm::new(x0 * MM, y1 * MM),
    ]
}

fn p(x: i64, y: i64) -> PtUm {
    PtUm::new(x * MM, y * MM)
}

// ---- point in ring ----

#[test]
fn points_inside_outside_and_on_the_boundary() {
    let sq = rect(0, 0, 10, 10);
    assert!(point_in_ring(p(5, 5), &sq));
    assert!(!point_in_ring(p(15, 5), &sq));
    assert!(!point_in_ring(p(-1, 5), &sq));
    assert!(
        point_in_ring(p(0, 5), &sq),
        "an edge point counts as inside"
    );
    assert!(point_in_ring(p(10, 10), &sq), "a vertex counts as inside");
}

#[test]
fn concave_rings_are_handled() {
    // An L shape: the notch (6..10, 6..10) is outside.
    let l = vec![p(0, 0), p(10, 0), p(10, 5), p(5, 5), p(5, 10), p(0, 10)];
    assert!(point_in_ring(p(2, 8), &l));
    assert!(point_in_ring(p(8, 2), &l));
    assert!(!point_in_ring(p(8, 8), &l));
}

#[test]
fn degenerate_rings_contain_nothing() {
    assert!(!point_in_ring(p(0, 0), &[]));
    assert!(!point_in_ring(p(0, 0), &[p(0, 0), p(1, 1)]));
}

// ---- containment forest ----

#[test]
fn the_forest_finds_each_rings_smallest_container() {
    let rings = vec![
        rect(0, 0, 100, 100),  // 0: outer
        rect(10, 10, 60, 60),  // 1: inside 0
        rect(20, 20, 40, 40),  // 2: inside 1 (and 0)
        rect(70, 70, 90, 90),  // 3: inside 0, beside 1
        rect(200, 0, 210, 10), // 4: separate
    ];
    let f = containment(&rings);
    assert_eq!(f.parent, vec![None, Some(0), Some(1), Some(0), None]);
    assert_eq!(f.children[0], vec![1, 3]);
    assert_eq!(f.children[1], vec![2]);
    assert_eq!(f.children[4].len(), 0);
}

#[test]
fn children_are_emitted_before_their_parents() {
    let rings = vec![
        rect(0, 0, 100, 100),
        rect(10, 10, 60, 60),
        rect(20, 20, 40, 40),
    ];
    assert_eq!(emission_order(&rings), vec![2, 1, 0]);
}

#[test]
fn smaller_siblings_come_before_larger_ones() {
    let rings = vec![
        rect(0, 0, 100, 100), // 0 big root
        rect(10, 10, 20, 20), // 1 small child of 0
        rect(30, 30, 80, 80), // 2 larger child of 0
        rect(200, 0, 205, 5), // 3 tiny separate root
    ];
    // Roots by area: 3 (tiny), then 0. Within 0: child 1 (small) before child 2.
    assert_eq!(emission_order(&rings), vec![3, 1, 2, 0]);
}

#[test]
fn deep_nesting_empties_inside_out() {
    let rings: Vec<Vec<PtUm>> = (0..5)
        .map(|i| rect(i * 10, i * 10, 100 - i * 10, 100 - i * 10))
        .collect();
    assert_eq!(emission_order(&rings), vec![4, 3, 2, 1, 0]);
}

#[test]
fn input_order_does_not_change_the_emission_shape() {
    let a = vec![rect(0, 0, 100, 100), rect(10, 10, 20, 20)];
    let b = vec![rect(10, 10, 20, 20), rect(0, 0, 100, 100)];
    // Index 1 of `a` is the inner ring; index 0 of `b` is the inner ring.
    assert_eq!(emission_order(&a), vec![1, 0]);
    assert_eq!(emission_order(&b), vec![0, 1]);
}

#[test]
fn an_empty_set_has_no_order() {
    assert_eq!(emission_order(&[]), Vec::<usize>::new());
    assert_eq!(containment(&[]).parent.len(), 0);
}

// ---- nearest neighbour ----

fn seg(x0: i64, x1: i64) -> Item {
    Item {
        start: p(x0, 0),
        end: p(x1, 0),
        reversible: true,
    }
}

#[test]
fn nearest_neighbour_walks_along_a_line() {
    let items = [seg(1, 2), seg(5, 6), seg(3, 4)];
    let order = nearest_neighbour(&items, p(0, 0));
    let idx: Vec<usize> = order.iter().map(|v| v.index).collect();
    assert_eq!(idx, [0, 2, 1]);
    assert!(order.iter().all(|v| !v.reversed));
    // Travel: 0 -> 1, 2 -> 3, 4 -> 5 = 3 mm.
    assert!((tour_length(&items, &order, p(0, 0)) - 3_000.0).abs() < 1e-9);
}

#[test]
fn a_reversible_path_is_entered_from_the_nearer_end() {
    let items = [Item {
        start: p(9, 0),
        end: p(1, 0),
        reversible: true,
    }];
    let order = nearest_neighbour(&items, p(0, 0));
    assert!(order[0].reversed);
    assert!((tour_length(&items, &order, p(0, 0)) - 1_000.0).abs() < 1e-9);
}

#[test]
fn a_non_reversible_path_keeps_its_direction() {
    let items = [Item {
        start: p(9, 0),
        end: p(1, 0),
        reversible: false,
    }];
    let order = nearest_neighbour(&items, p(0, 0));
    assert!(!order[0].reversed);
    assert!((tour_length(&items, &order, p(0, 0)) - 9_000.0).abs() < 1e-9);
}

#[test]
fn ties_go_to_the_lower_index() {
    let items = [
        Item {
            start: p(5, 0),
            end: p(5, 0),
            reversible: true,
        },
        Item {
            start: p(-5, 0),
            end: p(-5, 0),
            reversible: true,
        },
    ];
    let order = nearest_neighbour(&items, p(0, 0));
    assert_eq!(order[0].index, 0);
}

#[test]
fn every_item_is_visited_exactly_once() {
    let items: Vec<Item> = (0..20).map(|i| seg(i * 3, i * 3 + 1)).collect();
    let mut seen: Vec<usize> = nearest_neighbour(&items, p(30, 0))
        .iter()
        .map(|v| v.index)
        .collect();
    seen.sort_unstable();
    assert_eq!(seen, (0..20).collect::<Vec<_>>());
}

#[test]
fn nothing_to_order_gives_an_empty_tour() {
    assert_eq!(nearest_neighbour(&[], p(0, 0)).len(), 0);
    assert_eq!(tour_length(&[], &[], p(0, 0)), 0.0);
}

// ---- 2-opt and Or-opt ----

fn dot(x: i64, y: i64) -> Item {
    Item {
        start: p(x, y),
        end: p(x, y),
        reversible: true,
    }
}

fn visits(order: &[usize]) -> Vec<Visit> {
    order
        .iter()
        .map(|&index| Visit {
            index,
            reversed: false,
        })
        .collect()
}

#[test]
fn two_opt_uncrosses_a_crossing_tour() {
    let items = [dot(0, 0), dot(10, 0), dot(10, 10), dot(0, 10)];
    let mut order = visits(&[0, 2, 1, 3]); // the diagonals cross
    let before = tour_length(&items, &order, p(0, 0));
    improve(&items, &mut order, p(0, 0), 100);
    let after = tour_length(&items, &order, p(0, 0));
    assert!(before > 38_000.0, "{before}");
    assert!((after - 30_000.0).abs() < 1e-6, "{after}");
}

#[test]
fn or_opt_moves_a_stray_item_into_place() {
    // Points 0..8 on a line, but item 4 sits at the far end of the order.
    let items: Vec<Item> = (0..9).map(|i| dot(i * 10, 0)).collect();
    let mut order = visits(&[0, 1, 2, 3, 5, 6, 7, 8, 4]);
    let before = tour_length(&items, &order, p(0, 0));
    improve(&items, &mut order, p(0, 0), 1_000);
    let after = tour_length(&items, &order, p(0, 0));
    assert!(after < before);
    assert!(
        (after - 80_000.0).abs() < 1e-6,
        "perfect line tour is 80 mm, got {after}"
    );
}

#[test]
fn zero_iterations_change_nothing() {
    let items = [dot(0, 0), dot(10, 0), dot(10, 10), dot(0, 10)];
    let mut order = visits(&[0, 2, 1, 3]);
    let used = improve(&items, &mut order, p(0, 0), 0);
    assert_eq!(used, 0);
    assert_eq!(order, visits(&[0, 2, 1, 3]));
}

#[test]
fn the_iteration_budget_is_respected() {
    let items: Vec<Item> = (0..30)
        .map(|i| dot((i * 37) % 101, (i * 53) % 97))
        .collect();
    let mut order = visits(&(0..30).collect::<Vec<_>>());
    let used = improve(&items, &mut order, p(0, 0), 7);
    assert!(used <= 7);
}

#[test]
fn improvement_is_deterministic() {
    let items: Vec<Item> = (0..25)
        .map(|i| dot((i * 37) % 101, (i * 53) % 97))
        .collect();
    let start = visits(&(0..25).collect::<Vec<_>>());
    let (mut a, mut b) = (start.clone(), start);
    improve(&items, &mut a, p(0, 0), 500);
    improve(&items, &mut b, p(0, 0), 500);
    assert_eq!(a, b);
}

#[test]
fn improving_respects_directions_that_may_not_flip() {
    let items = [
        Item {
            start: p(0, 0),
            end: p(10, 0),
            reversible: false,
        },
        Item {
            start: p(20, 0),
            end: p(30, 0),
            reversible: false,
        },
    ];
    let mut order = visits(&[0, 1]);
    improve(&items, &mut order, p(0, 0), 100);
    assert!(order.iter().all(|v| !v.reversed));
}

proptest! {
    /// 2-opt/Or-opt never lengthens the tour and always visits every item once.
    #[test]
    fn improvement_never_gets_worse(coords in prop::collection::vec((0_i64..200, 0_i64..200), 1..25)) {
        let items: Vec<Item> = coords.iter().map(|&(x, y)| dot(x, y)).collect();
        let mut order = nearest_neighbour(&items, p(0, 0));
        let before = tour_length(&items, &order, p(0, 0));
        improve(&items, &mut order, p(0, 0), 200);
        let after = tour_length(&items, &order, p(0, 0));
        prop_assert!(after <= before + 1e-6);
        let mut seen: Vec<usize> = order.iter().map(|v| v.index).collect();
        seen.sort_unstable();
        prop_assert_eq!(seen, (0..items.len()).collect::<Vec<_>>());
    }
}

// ---- choosing where a closed path starts ----

mod closed_starts {
    use ps_geometry::{Path, Point};

    use crate::order::start_nearest;

    #[test]
    fn a_closed_path_starts_at_the_vertex_nearest_the_head() {
        let square = Path::rect(0.0, 0.0, 10.0, 10.0);
        let rotated = start_nearest(&square, Point::new(12.0, 11.0));
        assert_eq!(rotated.start, Point::new(10.0, 10.0));
        assert!(rotated.closed);
        assert_eq!(
            rotated.end(),
            rotated.start,
            "still closed at its new start"
        );
    }

    #[test]
    fn rotation_keeps_the_shape_the_same_length_and_winding() {
        let square = Path::rect(0.0, 0.0, 10.0, 10.0);
        let rotated = start_nearest(&square, Point::new(-3.0, 14.0));
        assert!((rotated.length() - square.length()).abs() < 1e-9);
        assert!((rotated.signed_area() - square.signed_area()).abs() < 1e-9);
        assert_eq!(rotated.start, Point::new(0.0, 10.0));
    }

    #[test]
    fn a_path_already_starting_there_is_unchanged() {
        let square = Path::rect(0.0, 0.0, 10.0, 10.0);
        assert_eq!(start_nearest(&square, Point::new(-1.0, -1.0)), square);
    }

    #[test]
    fn open_paths_are_left_alone() {
        let line = Path::polyline(
            &[
                Point::new(0.0, 0.0),
                Point::new(5.0, 0.0),
                Point::new(5.0, 5.0),
            ],
            false,
        );
        assert_eq!(start_nearest(&line, Point::new(5.0, 5.0)), line);
    }

    #[test]
    fn curves_and_arcs_survive_rotation() {
        let circle = Path::circle(Point::new(0.0, 0.0), 10.0);
        let rotated = start_nearest(&circle, Point::new(-30.0, 0.0));
        assert!((rotated.length() - circle.length()).abs() < 1e-6);
        assert!(rotated.start.x < -5.0, "{:?}", rotated.start);
    }
}
