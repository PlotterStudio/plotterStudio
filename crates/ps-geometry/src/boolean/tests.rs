//! Ground truth for compile-time booleans on the µm grid. Areas are exact
//! integers, computed by hand from axis-aligned rectangles.

use proptest::prelude::*;

use super::*;

/// A counter-clockwise rectangle in µm.
fn sq(x0: i64, y0: i64, x1: i64, y1: i64) -> Ring {
    vec![
        PtUm::new(x0, y0),
        PtUm::new(x1, y0),
        PtUm::new(x1, y1),
        PtUm::new(x0, y1),
    ]
}

/// Total area of the shapes, in µm², doubled (so it stays an integer).
fn area2(shapes: &[Shape]) -> i128 {
    shapes_area2(shapes)
}

const MM2: i128 = 1_000_000; // one mm² in µm²

fn a() -> Vec<Ring> {
    vec![sq(0, 0, 4_000, 4_000)] // 16 mm²
}

fn b() -> Vec<Ring> {
    vec![sq(2_000, 2_000, 6_000, 6_000)] // 16 mm², overlap 4 mm²
}

#[test]
fn ring_area_is_signed_and_doubled() {
    let r = sq(0, 0, 4_000, 3_000);
    assert_eq!(ring_area2(&r), 2 * 12 * MM2);
    let mut rev = r.clone();
    rev.reverse();
    assert_eq!(ring_area2(&rev), -2 * 12 * MM2);
    assert_eq!(ring_area2(&[]), 0);
    assert_eq!(ring_area2(&[PtUm::new(1, 1), PtUm::new(5, 5)]), 0);
}

#[test]
fn union_of_overlapping_squares() {
    let u = boolean(&a(), &b(), BoolOp::Union, FillRule::NonZero);
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].len(), 1); // no holes
    assert_eq!(area2(&u), 2 * 28 * MM2); // 16 + 16 - 4
}

#[test]
fn intersection_of_overlapping_squares() {
    let i = boolean(&a(), &b(), BoolOp::Intersect, FillRule::NonZero);
    assert_eq!(i.len(), 1);
    assert_eq!(area2(&i), 2 * 4 * MM2);
    let mut xs: Vec<i64> = i[0][0].iter().map(|p| p.x).collect();
    xs.sort_unstable();
    xs.dedup();
    assert_eq!(xs, vec![2_000, 4_000]);
}

#[test]
fn difference_is_subject_minus_clip() {
    let d = boolean(&a(), &b(), BoolOp::Difference, FillRule::NonZero);
    assert_eq!(area2(&d), 2 * 12 * MM2);
    let d = boolean(&b(), &a(), BoolOp::Difference, FillRule::NonZero);
    assert_eq!(area2(&d), 2 * 12 * MM2);
}

#[test]
fn exclusion_is_the_symmetric_difference() {
    let x = boolean(&a(), &b(), BoolOp::Xor, FillRule::NonZero);
    assert_eq!(area2(&x), 2 * 24 * MM2);
}

#[test]
fn disjoint_squares_give_two_shapes() {
    let far = vec![sq(10_000, 0, 12_000, 2_000)];
    let u = boolean(&a(), &far, BoolOp::Union, FillRule::NonZero);
    assert_eq!(u.len(), 2);
    assert_eq!(area2(&u), 2 * 20 * MM2);
}

#[test]
fn intersection_with_nothing_is_empty() {
    let result = boolean(&a(), &[], BoolOp::Intersect, FillRule::NonZero);
    assert_eq!(result.len(), 0);
    let far = vec![sq(10_000, 0, 12_000, 2_000)];
    let result = boolean(&a(), &far, BoolOp::Intersect, FillRule::NonZero);
    assert_eq!(result.len(), 0);
}

#[test]
fn union_with_nothing_is_the_shape_itself() {
    let u = boolean(&a(), &[], BoolOp::Union, FillRule::NonZero);
    assert_eq!(area2(&u), 2 * 16 * MM2);
}

#[test]
fn a_union_a_is_a() {
    let u = boolean(&a(), &a(), BoolOp::Union, FillRule::NonZero);
    assert_eq!(area2(&u), 2 * 16 * MM2);
}

#[test]
fn subtracting_an_inner_square_leaves_a_hole() {
    let big = vec![sq(0, 0, 10_000, 10_000)]; // 100 mm²
    let small = vec![sq(2_000, 2_000, 4_000, 4_000)]; // 4 mm²
    let d = boolean(&big, &small, BoolOp::Difference, FillRule::NonZero);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].len(), 2); // outer + hole
    assert_eq!(area2(&d), 2 * 96 * MM2);
    // Outer counter-clockwise (positive), hole clockwise (negative).
    assert!(ring_area2(&d[0][0]) > 0);
    assert!(ring_area2(&d[0][1]) < 0);
}

#[test]
fn fill_rules_differ_on_nested_same_direction_rings() {
    // Two counter-clockwise rings, one inside the other: winding number 2 in the middle.
    let rings = vec![sq(0, 0, 10_000, 10_000), sq(2_000, 2_000, 8_000, 8_000)];
    let solid = normalize(&rings, FillRule::NonZero);
    assert_eq!(solid.len(), 1);
    assert_eq!(solid[0].len(), 1);
    assert_eq!(area2(&solid), 2 * 100 * MM2);
    let holed = normalize(&rings, FillRule::EvenOdd);
    assert_eq!(holed[0].len(), 2);
    assert_eq!(area2(&holed), 2 * (100 - 36) * MM2);
}

#[test]
fn normalize_merges_overlapping_rings() {
    let merged = normalize(&[a()[0].clone(), b()[0].clone()], FillRule::NonZero);
    assert_eq!(merged.len(), 1);
    assert_eq!(area2(&merged), 2 * 28 * MM2);
}

#[test]
fn clip_keeps_the_part_of_a_line_inside() {
    let region = vec![sq(0, 0, 10_000, 10_000)];
    let line = vec![PtUm::new(-5_000, 1_000), PtUm::new(15_000, 1_000)];
    let pieces = clip_lines(&[line], &region, FillRule::NonZero);
    assert_eq!(pieces.len(), 1);
    let mut xs: Vec<i64> = pieces[0].iter().map(|p| p.x).collect();
    xs.sort_unstable();
    assert_eq!((xs[0], *xs.last().unwrap()), (0, 10_000));
    assert!(pieces[0].iter().all(|p| p.y == 1_000));
}

#[test]
fn clip_splits_a_line_at_a_hole() {
    let region = normalize(
        &[sq(0, 0, 10_000, 10_000), {
            let mut hole = sq(4_000, 4_000, 6_000, 6_000);
            hole.reverse();
            hole
        }],
        FillRule::NonZero,
    );
    let rings: Vec<Ring> = region.into_iter().flatten().collect();
    let line = vec![PtUm::new(-1_000, 5_000), PtUm::new(11_000, 5_000)];
    let mut pieces = clip_lines(&[line], &rings, FillRule::NonZero);
    pieces.sort_by_key(|p| p.iter().map(|q| q.x).min());
    assert_eq!(pieces.len(), 2);
    let ends = |p: &Vec<PtUm>| {
        let xs: Vec<i64> = p.iter().map(|q| q.x).collect();
        (*xs.iter().min().unwrap(), *xs.iter().max().unwrap())
    };
    assert_eq!(ends(&pieces[0]), (0, 4_000));
    assert_eq!(ends(&pieces[1]), (6_000, 10_000));
}

#[test]
fn clip_drops_lines_outside() {
    let region = vec![sq(0, 0, 10_000, 10_000)];
    let line = vec![PtUm::new(-5_000, 20_000), PtUm::new(15_000, 20_000)];
    let result = clip_lines(&[line], &region, FillRule::NonZero);
    assert_eq!(result.len(), 0);
}

proptest! {
    /// area(A ∪ B) = area(A) + area(B) − area(A ∩ B), exactly, on the integer grid.
    #[test]
    fn inclusion_exclusion_holds(
        ax in -5_000_i64..5_000, ay in -5_000_i64..5_000, aw in 1_i64..6_000, ah in 1_i64..6_000,
        bx in -5_000_i64..5_000, by in -5_000_i64..5_000, bw in 1_i64..6_000, bh in 1_i64..6_000,
    ) {
        let ra = vec![sq(ax, ay, ax + aw, ay + ah)];
        let rb = vec![sq(bx, by, bx + bw, by + bh)];
        let union = area2(&boolean(&ra, &rb, BoolOp::Union, FillRule::NonZero));
        let inter = area2(&boolean(&ra, &rb, BoolOp::Intersect, FillRule::NonZero));
        let area_a = i128::from(aw) * i128::from(ah) * 2;
        let area_b = i128::from(bw) * i128::from(bh) * 2;
        prop_assert_eq!(union, area_a + area_b - inter);
    }

    /// A \ B and A ∩ B partition A.
    #[test]
    fn difference_and_intersection_partition_the_subject(
        ax in -5_000_i64..5_000, ay in -5_000_i64..5_000, aw in 1_i64..6_000, ah in 1_i64..6_000,
        bx in -5_000_i64..5_000, by in -5_000_i64..5_000, bw in 1_i64..6_000, bh in 1_i64..6_000,
    ) {
        let ra = vec![sq(ax, ay, ax + aw, ay + ah)];
        let rb = vec![sq(bx, by, bx + bw, by + bh)];
        let diff = area2(&boolean(&ra, &rb, BoolOp::Difference, FillRule::NonZero));
        let inter = area2(&boolean(&ra, &rb, BoolOp::Intersect, FillRule::NonZero));
        prop_assert_eq!(diff + inter, i128::from(aw) * i128::from(ah) * 2);
    }
}
