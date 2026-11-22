//! Ground truth for the spatial index: hit testing, rubber-band selection and
//! nearest-neighbour queries, with deterministic result order.

use super::*;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
    Rect::new(x0, y0, x1, y1)
}

fn index() -> SpatialIndex<u32> {
    let mut ix = SpatialIndex::new();
    ix.insert(3, rect(20.0, 0.0, 30.0, 10.0));
    ix.insert(1, rect(0.0, 0.0, 10.0, 10.0));
    ix.insert(2, rect(5.0, 5.0, 15.0, 15.0));
    ix
}

#[test]
fn counts_inserted_entries() {
    let ix = index();
    assert_eq!(ix.len(), 3);
    assert!(!ix.is_empty());
    assert!(SpatialIndex::<u32>::new().is_empty());
}

#[test]
fn rect_query_returns_intersecting_ids_in_order() {
    assert_eq!(index().query_rect(rect(8.0, 8.0, 12.0, 12.0)), vec![1, 2]);
    assert_eq!(
        index().query_rect(rect(-5.0, -5.0, 100.0, 100.0)),
        vec![1, 2, 3]
    );
    let far = index().query_rect(rect(100.0, 100.0, 110.0, 110.0));
    assert_eq!(far.len(), 0);
}

#[test]
fn touching_edges_count_as_intersecting() {
    assert_eq!(
        index().query_rect(rect(10.0, 0.0, 20.0, 10.0)),
        vec![1, 2, 3]
    );
}

#[test]
fn point_query_uses_the_radius() {
    // (7, 7) is inside entries 1 and 2.
    assert_eq!(index().query_point(Point::new(7.0, 7.0), 0.0), vec![1, 2]);
    // (17, 5) is 2 mm right of entry 2 and 3 mm left of entry 3.
    assert_eq!(index().query_point(Point::new(17.0, 5.0), 2.5), vec![2]);
    assert_eq!(index().query_point(Point::new(17.0, 5.0), 3.5), vec![2, 3]);
}

#[test]
fn nearest_picks_the_closest_box() {
    let ix = index();
    assert_eq!(ix.nearest(Point::new(18.0, 5.0)), Some(3)); // 2 from entry 3, 3 from entry 2
    assert_eq!(ix.nearest(Point::new(-4.0, 5.0)), Some(1));
    assert_eq!(SpatialIndex::<u32>::new().nearest(Point::ZERO), None);
}

#[test]
fn nearest_breaks_ties_by_smaller_id() {
    let mut ix = SpatialIndex::new();
    ix.insert(9, rect(10.0, 0.0, 11.0, 1.0));
    ix.insert(4, rect(-11.0, 0.0, -10.0, 1.0));
    assert_eq!(ix.nearest(Point::new(0.0, 0.5)), Some(4));
}

#[test]
fn removed_entries_disappear() {
    let mut ix = index();
    assert!(ix.remove(&2));
    assert!(!ix.remove(&2));
    assert_eq!(ix.len(), 2);
    assert_eq!(ix.query_rect(rect(8.0, 8.0, 12.0, 12.0)), vec![1]);
}

#[test]
fn bulk_build_matches_incremental_inserts() {
    let items = vec![
        (3, rect(20.0, 0.0, 30.0, 10.0)),
        (1, rect(0.0, 0.0, 10.0, 10.0)),
        (2, rect(5.0, 5.0, 15.0, 15.0)),
    ];
    let bulk = SpatialIndex::from_entries(items);
    assert_eq!(bulk.query_rect(rect(8.0, 8.0, 12.0, 12.0)), vec![1, 2]);
    assert_eq!(bulk.len(), 3);
}

#[test]
fn duplicate_ids_are_replaced() {
    let mut ix = index();
    ix.insert(1, rect(100.0, 100.0, 110.0, 110.0));
    assert_eq!(ix.len(), 3);
    assert_eq!(ix.query_rect(rect(0.0, 0.0, 1.0, 1.0)), Vec::<u32>::new());
    assert_eq!(ix.query_rect(rect(105.0, 105.0, 106.0, 106.0)), vec![1]);
}
