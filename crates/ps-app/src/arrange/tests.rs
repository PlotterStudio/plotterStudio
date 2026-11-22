//! Ground truth for arranging objects: numeric entry from any of nine anchors, move,
//! rotate, scale, mirror, align and distribute, grid and circular arrays. Every function takes the
//! objects with their world bounds and transforms and returns the new transforms, so the results
//! are checked by hand-computed coordinates.

use ps_core::ObjectId;
use ps_geometry::{Affine, Point, Rect};

use super::*;

fn id(ms: u64) -> ObjectId {
    ObjectId::at_unix_ms(ms)
}

/// An object whose geometry is a rectangle in world space, with an identity-plus-offset transform.
fn item(who: ObjectId, left: f64, top: f64, width: f64, height: f64) -> Item {
    Item {
        id: who,
        bounds: Rect::new(left, top, left + width, top + height),
        transform: Affine::translate((left, top)),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// Where a point of the object's own geometry ends up under a transform.
fn map(t: Affine, p: (f64, f64)) -> (f64, f64) {
    let q = t * Point::new(p.0, p.1);
    (q.x, q.y)
}

fn find(result: &[(ObjectId, Affine)], who: ObjectId) -> Affine {
    result.iter().find(|(i, _)| *i == who).unwrap().1
}

#[test]
fn the_nine_anchors_are_the_corners_edge_midpoints_and_centre() {
    let r = Rect::new(0.0, 0.0, 10.0, 20.0);
    let expect = [
        (Anchor::TopLeft, 0.0, 0.0),
        (Anchor::Top, 5.0, 0.0),
        (Anchor::TopRight, 10.0, 0.0),
        (Anchor::Left, 0.0, 10.0),
        (Anchor::Centre, 5.0, 10.0),
        (Anchor::Right, 10.0, 10.0),
        (Anchor::BottomLeft, 0.0, 20.0),
        (Anchor::Bottom, 5.0, 20.0),
        (Anchor::BottomRight, 10.0, 20.0),
    ];
    for (a, x, y) in expect {
        let p = anchor_point(r, a);
        assert!(close(p.x, x) && close(p.y, y), "{a:?}: {p:?}");
    }
    assert_eq!(Anchor::ALL.len(), 9);
}

#[test]
fn the_selection_bounds_are_the_union_of_the_objects() {
    let items = [
        item(id(1), 0.0, 0.0, 10.0, 10.0),
        item(id(2), 20.0, 5.0, 10.0, 10.0),
    ];
    let b = union_bounds(&items).unwrap();
    assert!(close(b.x0, 0.0) && close(b.y0, 0.0) && close(b.x1, 30.0) && close(b.y1, 15.0));
    assert!(union_bounds(&[]).is_none());
}

#[test]
fn moving_puts_the_chosen_anchor_at_the_target() {
    let a = id(1);
    let items = [item(a, 10.0, 10.0, 20.0, 10.0)];
    // Put the centre (20, 15) at (100, 100): everything moves by (80, 85).
    let out = move_to(&items, Anchor::Centre, Point::new(100.0, 100.0));
    let t = find(&out, a);
    assert_eq!(map(t, (0.0, 0.0)), (90.0, 95.0));
    // And from the top-left corner.
    let out = move_to(&items, Anchor::TopLeft, Point::new(0.0, 0.0));
    assert_eq!(map(find(&out, a), (0.0, 0.0)), (0.0, 0.0));
}

#[test]
fn moving_several_objects_keeps_their_arrangement() {
    let (a, b) = (id(1), id(2));
    let items = [
        item(a, 0.0, 0.0, 10.0, 10.0),
        item(b, 20.0, 0.0, 10.0, 10.0),
    ];
    let out = move_to(&items, Anchor::TopLeft, Point::new(50.0, 50.0));
    assert_eq!(map(find(&out, a), (0.0, 0.0)), (50.0, 50.0));
    assert_eq!(map(find(&out, b), (0.0, 0.0)), (70.0, 50.0));
}

#[test]
fn resizing_scales_about_the_chosen_anchor() {
    let a = id(1);
    let items = [item(a, 10.0, 10.0, 20.0, 10.0)];
    // Double the width and keep the left edge fixed.
    let out = resize(&items, Anchor::Left, 40.0, 10.0, false);
    let t = find(&out, a);
    assert_eq!(map(t, (0.0, 0.0)), (10.0, 10.0));
    assert_eq!(map(t, (20.0, 10.0)), (50.0, 20.0));
    // The same resize anchored at the right edge keeps the right edge fixed.
    let out = resize(&items, Anchor::Right, 40.0, 10.0, false);
    let t = find(&out, a);
    assert_eq!(map(t, (20.0, 0.0)), (30.0, 10.0));
    assert_eq!(map(t, (0.0, 0.0)), (-10.0, 10.0));
}

#[test]
fn a_locked_aspect_ratio_derives_the_height_from_the_width() {
    let a = id(1);
    let items = [item(a, 0.0, 0.0, 20.0, 10.0)];
    let out = resize(&items, Anchor::TopLeft, 40.0, 999.0, true);
    let t = find(&out, a);
    assert_eq!(map(t, (20.0, 10.0)), (40.0, 20.0));
}

#[test]
fn resizing_to_nothing_or_from_nothing_changes_nothing() {
    let a = id(1);
    let flat = [Item {
        id: a,
        bounds: Rect::new(0.0, 0.0, 10.0, 0.0),
        transform: Affine::IDENTITY,
    }];
    assert!(
        resize(&flat, Anchor::Centre, 20.0, 20.0, false).is_empty(),
        "no height to scale"
    );
    let items = [item(a, 0.0, 0.0, 10.0, 10.0)];
    assert!(
        resize(&items, Anchor::Centre, 0.0, 10.0, false).is_empty(),
        "zero width refused"
    );
    assert_eq!(
        resize(&items, Anchor::Centre, f64::NAN, 10.0, false).len(),
        0
    );
}

#[test]
fn rotating_turns_the_selection_about_its_centre() {
    let a = id(1);
    let items = [item(a, 0.0, 0.0, 20.0, 10.0)]; // centre (10, 5)
    let out = rotate(&items, 90.0);
    let t = find(&out, a);
    // The corner (0,0) is 10 left and 5 above the centre; a quarter turn (y down, clockwise on
    // screen) puts it 5 right of and 10 above... check by the rotation matrix.
    let (x, y) = map(t, (0.0, 0.0));
    assert!(close(x, 15.0) && close(y, -5.0), "({x}, {y})");
    // The centre stays where it was.
    let (cx, cy) = map(t, (10.0, 5.0));
    assert!(close(cx, 10.0) && close(cy, 5.0));
}

#[test]
fn rotating_by_a_full_turn_changes_nothing() {
    let a = id(1);
    let items = [item(a, 3.0, 4.0, 20.0, 10.0)];
    let t = find(&rotate(&items, 360.0), a);
    let (x, y) = map(t, (0.0, 0.0));
    assert!(close(x, 3.0) && close(y, 4.0), "({x}, {y})");
}

#[test]
fn mirroring_flips_about_the_selection_centre() {
    let (a, b) = (id(1), id(2));
    let items = [
        item(a, 0.0, 0.0, 10.0, 10.0),
        item(b, 20.0, 0.0, 10.0, 10.0),
    ]; // centre x = 15
    let out = mirror(&items, Axis::Horizontal);
    // The left square's left edge x=0 maps to x=30, the far edge of the selection.
    assert!(close(map(find(&out, a), (0.0, 0.0)).0, 30.0));
    assert!(close(map(find(&out, b), (10.0, 0.0)).0, 0.0));
    let up = mirror(&items, Axis::Vertical);
    // Vertical mirror about y = 5 swaps top and bottom of each.
    assert!(close(map(find(&up, a), (0.0, 0.0)).1, 10.0));
}

#[test]
fn aligning_moves_every_object_to_the_selection_edge_or_centre() {
    let (a, b, c) = (id(1), id(2), id(3));
    let items = [
        item(a, 0.0, 0.0, 10.0, 10.0),
        item(b, 20.0, 5.0, 20.0, 10.0),
        item(c, 50.0, 30.0, 10.0, 10.0),
    ];
    // Selection spans x 0..60, y 0..40.
    let left = align(&items, AlignKind::Left);
    assert!(close(map(find(&left, c), (0.0, 0.0)).0, 0.0));
    let right = align(&items, AlignKind::Right);
    assert!(
        close(map(find(&right, a), (0.0, 0.0)).0, 50.0),
        "10 wide, right edge at 60"
    );
    assert!(
        close(map(find(&right, b), (0.0, 0.0)).0, 40.0),
        "20 wide, right edge at 60"
    );
    let top = align(&items, AlignKind::Top);
    assert!(close(map(find(&top, c), (0.0, 0.0)).1, 0.0));
    let bottom = align(&items, AlignKind::Bottom);
    assert!(close(map(find(&bottom, a), (0.0, 0.0)).1, 30.0));
    let centre_h = align(&items, AlignKind::CentreHorizontal);
    assert!(
        close(map(find(&centre_h, a), (0.0, 0.0)).0, 25.0),
        "centre x 30, width 10"
    );
    let centre_v = align(&items, AlignKind::CentreVertical);
    assert!(
        close(map(find(&centre_v, a), (0.0, 0.0)).1, 15.0),
        "centre y 20, height 10"
    );
}

#[test]
fn aligning_one_object_or_none_changes_nothing() {
    assert_eq!(align(&[], AlignKind::Left).len(), 0);
    let one = [item(id(1), 5.0, 5.0, 10.0, 10.0)];
    let out = align(&one, AlignKind::Right);
    let (x, y) = map(find(&out, one[0].id), (0.0, 0.0));
    assert!(close(x, 5.0) && close(y, 5.0));
}

#[test]
fn distributing_makes_the_gaps_equal_and_keeps_the_outer_objects() {
    let (a, b, c) = (id(1), id(2), id(3));
    let items = [
        item(a, 0.0, 0.0, 10.0, 10.0),
        item(b, 12.0, 0.0, 10.0, 10.0),
        item(c, 60.0, 0.0, 10.0, 10.0),
    ];
    // Total span 70; objects take 30; two gaps of 20.
    let out = distribute(&items, Axis::Horizontal);
    assert!(close(map(find(&out, a), (0.0, 0.0)).0, 0.0));
    assert!(close(map(find(&out, b), (0.0, 0.0)).0, 30.0));
    assert!(close(map(find(&out, c), (0.0, 0.0)).0, 60.0));
}

#[test]
fn distribution_goes_by_position_not_by_the_order_given() {
    let (a, b, c) = (id(1), id(2), id(3));
    let items = [
        item(c, 60.0, 0.0, 10.0, 10.0),
        item(a, 0.0, 0.0, 10.0, 10.0),
        item(b, 12.0, 0.0, 10.0, 10.0),
    ];
    let out = distribute(&items, Axis::Horizontal);
    assert!(close(map(find(&out, b), (0.0, 0.0)).0, 30.0));
}

#[test]
fn distributing_fewer_than_three_objects_changes_nothing() {
    let two = [
        item(id(1), 0.0, 0.0, 10.0, 10.0),
        item(id(2), 40.0, 0.0, 10.0, 10.0),
    ];
    assert_eq!(distribute(&two, Axis::Horizontal).len(), 0);
}

#[test]
fn a_grid_array_makes_copies_at_each_step_but_not_over_the_original() {
    let a = id(1);
    let items = [item(a, 10.0, 10.0, 5.0, 5.0)];
    let copies = grid_array(&items, 3, 2, 20.0, 15.0);
    // 3 columns by 2 rows is 6 cells; the original already occupies one.
    assert_eq!(copies.len(), 5);
    let mut origins: Vec<(f64, f64)> = copies
        .iter()
        .map(|c| map(c.transform, (0.0, 0.0)))
        .collect();
    origins.sort_by(|p, q| p.partial_cmp(q).unwrap());
    let mut expect = vec![
        (30.0, 10.0),
        (50.0, 10.0),
        (10.0, 25.0),
        (30.0, 25.0),
        (50.0, 25.0),
    ];
    expect.sort_by(|p, q| p.partial_cmp(q).unwrap());
    assert_eq!(origins, expect);
    assert!(copies.iter().all(|c| c.source == a));
}

#[test]
fn a_one_by_one_grid_makes_no_copies() {
    assert_eq!(
        grid_array(&[item(id(1), 0.0, 0.0, 5.0, 5.0)], 1, 1, 10.0, 10.0).len(),
        0
    );
    assert_eq!(
        grid_array(&[item(id(1), 0.0, 0.0, 5.0, 5.0)], 0, 3, 10.0, 10.0).len(),
        0
    );
}

#[test]
fn a_circular_array_spaces_copies_evenly_around_a_centre() {
    let a = id(1);
    // A point 10 to the right of the centre, four copies in total: the original plus three.
    let items = [item(a, 10.0, 0.0, 0.0, 0.0)];
    let copies = circular_array(&items, 4, Point::new(0.0, 0.0), 360.0);
    assert_eq!(copies.len(), 3);
    let mut points: Vec<(f64, f64)> = copies
        .iter()
        .map(|c| {
            let (x, y) = map(c.transform, (0.0, 0.0));
            ((x * 1e6).round() / 1e6, (y * 1e6).round() / 1e6)
        })
        .collect();
    points.sort_by(|p, q| p.partial_cmp(q).unwrap());
    let mut expect = vec![(0.0, 10.0), (-10.0, 0.0), (0.0, -10.0)];
    expect.sort_by(|p, q| p.partial_cmp(q).unwrap());
    assert_eq!(points, expect);
}

#[test]
fn a_partial_circular_array_ends_on_the_last_step() {
    let a = id(1);
    let items = [item(a, 10.0, 0.0, 0.0, 0.0)];
    // Quarter circle, three objects in total: 0, 45 and 90 degrees.
    let copies = circular_array(&items, 3, Point::new(0.0, 0.0), 90.0);
    assert_eq!(copies.len(), 2);
    let last = copies
        .iter()
        .map(|c| map(c.transform, (0.0, 0.0)))
        .fold((0.0, 0.0), |best, p| if p.1 > best.1 { p } else { best });
    assert!(close(last.0, 0.0) && close(last.1, 10.0), "{last:?}");
}

#[test]
fn a_circular_array_of_fewer_than_two_makes_no_copies() {
    let items = [item(id(1), 10.0, 0.0, 0.0, 0.0)];
    assert_eq!(
        circular_array(&items, 1, Point::new(0.0, 0.0), 360.0).len(),
        0
    );
    assert_eq!(
        circular_array(&items, 0, Point::new(0.0, 0.0), 360.0).len(),
        0
    );
}
