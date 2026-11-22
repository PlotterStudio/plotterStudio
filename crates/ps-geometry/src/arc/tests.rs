//! Ground truth for circular-arc helpers. Every expected value is a closed-form result
//! (unit circle, quarter and half turns), not output of the implementation.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use super::*;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn near_pt(a: Point, b: Point) -> bool {
    near(a.x, b.x) && near(a.y, b.y)
}

#[test]
fn bulge_is_tan_of_a_quarter_sweep() {
    assert!(near(bulge_from_sweep(PI), 1.0)); // semicircle
    assert!(near(bulge_from_sweep(FRAC_PI_2), 2.0_f64.sqrt() - 1.0)); // tan(22.5 deg)
    assert!(near(bulge_from_sweep(-PI), -1.0)); // clockwise is negative
    assert!(near(bulge_from_sweep(0.0), 0.0));
}

#[test]
fn sweep_from_bulge_inverts_bulge_from_sweep() {
    assert!(near(sweep_from_bulge(1.0), PI));
    assert!(near(sweep_from_bulge(-1.0), -PI));
    for sweep in [0.1, 1.0, FRAC_PI_2, 3.0, -2.5] {
        assert!(near(sweep_from_bulge(bulge_from_sweep(sweep)), sweep));
    }
}

#[test]
fn radius_from_chord_and_bulge() {
    // Semicircle over a chord of 2 has radius 1.
    assert!(near(
        radius_from_bulge(pt(0.0, 0.0), pt(2.0, 0.0), 1.0),
        1.0
    ));
    // Quarter circle between (1,0) and (0,1): chord sqrt(2), radius 1.
    let b = bulge_from_sweep(FRAC_PI_2);
    assert!(near(radius_from_bulge(pt(1.0, 0.0), pt(0.0, 1.0), b), 1.0));
    // Sign of the bulge does not change the radius.
    assert!(near(radius_from_bulge(pt(1.0, 0.0), pt(0.0, 1.0), -b), 1.0));
}

#[test]
fn center_depends_on_direction() {
    let b = bulge_from_sweep(FRAC_PI_2);
    // Counter-clockwise quarter from (1,0) to (0,1) is about the origin.
    assert!(near_pt(
        center_from_bulge(pt(1.0, 0.0), pt(0.0, 1.0), b).unwrap(),
        pt(0.0, 0.0)
    ));
    // Clockwise between the same points is the short way round the other centre, (1,1).
    assert!(near_pt(
        center_from_bulge(pt(1.0, 0.0), pt(0.0, 1.0), -b).unwrap(),
        pt(1.0, 1.0)
    ));
}

#[test]
fn zero_bulge_has_no_center() {
    assert_eq!(center_from_bulge(pt(0.0, 0.0), pt(1.0, 0.0), 0.0), None);
}

#[test]
fn bulge_from_arc_round_trips() {
    let center = pt(0.0, 0.0);
    let b = bulge_from_arc(pt(1.0, 0.0), pt(0.0, 1.0), center, true);
    assert!(near(b, 2.0_f64.sqrt() - 1.0));
    let b = bulge_from_arc(pt(1.0, 0.0), pt(0.0, 1.0), center, false);
    // Clockwise from (1,0) to (0,1) is three quarters of a turn: -tan(135/2 deg ... ) = -tan(3pi/8).
    assert!(near(b, -(3.0 * PI / 8.0).tan()));
}

#[test]
fn sweep_follows_the_stated_direction() {
    let o = pt(0.0, 0.0);
    assert!(near(sweep(pt(1.0, 0.0), pt(0.0, 1.0), o, true), FRAC_PI_2));
    assert!(near(
        sweep(pt(1.0, 0.0), pt(0.0, 1.0), o, false),
        -3.0 * FRAC_PI_2
    ));
    assert!(near(sweep(pt(1.0, 0.0), pt(-1.0, 0.0), o, true), PI));
    assert!(near(sweep(pt(1.0, 0.0), pt(-1.0, 0.0), o, false), -PI));
}

#[test]
fn coincident_endpoints_mean_a_full_turn() {
    let o = pt(0.0, 0.0);
    assert!(near(sweep(pt(1.0, 0.0), pt(1.0, 0.0), o, true), TAU));
    assert!(near(sweep(pt(1.0, 0.0), pt(1.0, 0.0), o, false), -TAU));
}

#[test]
fn point_at_angle_walks_the_circle() {
    let c = pt(2.0, 3.0);
    assert!(near_pt(point_at(c, 5.0, 0.0), pt(7.0, 3.0)));
    assert!(near_pt(point_at(c, 5.0, FRAC_PI_2), pt(2.0, 8.0)));
    assert!(near_pt(point_at(c, 5.0, PI), pt(-3.0, 3.0)));
}

#[test]
fn quarter_circle_is_one_cubic_with_the_standard_handle() {
    // kappa = 4/3 * tan(pi/8) = 0.5522847498...
    let cubics = to_cubics(pt(1.0, 0.0), pt(0.0, 1.0), pt(0.0, 0.0), true);
    assert_eq!(cubics.len(), 1);
    let [c1, c2, end] = cubics[0];
    assert!(near_pt(c1, pt(1.0, 0.552_284_749_830_793_4)));
    assert!(near_pt(c2, pt(0.552_284_749_830_793_4, 1.0)));
    assert!(near_pt(end, pt(0.0, 1.0)));
}

#[test]
fn cubic_count_is_one_per_quarter_turn_or_part_of_one() {
    let o = pt(0.0, 0.0);
    assert_eq!(to_cubics(pt(1.0, 0.0), pt(-1.0, 0.0), o, true).len(), 2); // half
    assert_eq!(to_cubics(pt(1.0, 0.0), pt(1.0, 0.0), o, true).len(), 4); // full
    assert_eq!(to_cubics(pt(1.0, 0.0), pt(0.0, -1.0), o, false).len(), 1); // cw quarter
}

#[test]
fn cubics_end_exactly_on_the_requested_end_point() {
    let cubics = to_cubics(pt(1.0, 0.0), pt(-1.0, 0.0), pt(0.0, 0.0), true);
    assert_eq!(cubics.last().unwrap()[2], pt(-1.0, 0.0));
    // The join between the two halves is the top of the circle.
    assert!(near_pt(cubics[0][2], pt(0.0, 1.0)));
}

#[test]
fn clockwise_cubics_go_the_other_way_round() {
    let cubics = to_cubics(pt(1.0, 0.0), pt(-1.0, 0.0), pt(0.0, 0.0), false);
    assert!(near_pt(cubics[0][2], pt(0.0, -1.0)));
}

#[test]
fn three_points_define_a_direction_and_centre() {
    let (c, ccw) = from_three_points(pt(1.0, 0.0), pt(0.0, 1.0), pt(-1.0, 0.0)).unwrap();
    assert!(near_pt(c, pt(0.0, 0.0)));
    assert!(ccw);
    let (c, ccw) = from_three_points(pt(1.0, 0.0), pt(0.0, -1.0), pt(-1.0, 0.0)).unwrap();
    assert!(near_pt(c, pt(0.0, 0.0)));
    assert!(!ccw);
}

#[test]
fn collinear_points_have_no_arc() {
    assert_eq!(
        from_three_points(pt(0.0, 0.0), pt(1.0, 1.0), pt(2.0, 2.0)),
        None
    );
    assert_eq!(
        from_three_points(pt(0.0, 0.0), pt(0.0, 0.0), pt(2.0, 2.0)),
        None
    );
}

#[test]
fn step_for_tolerance_bounds_the_chord_error() {
    // r = 10, tol = 0.002: sagitta r(1 - cos(step/2)) = tol  =>  step ~ 2*sqrt(2*tol/r) = 0.04.
    let step = max_step(10.0, 0.002);
    assert!((step - 0.04).abs() < 1e-4);
    // Exactly meeting the tolerance, not exceeding it.
    let sag = 10.0 * (1.0 - libm::cos(step / 2.0));
    assert!(sag <= 0.002 + 1e-12);
}

#[test]
fn step_is_capped_for_tiny_radii_or_huge_tolerances() {
    assert!(near(max_step(1.0, 5.0), PI)); // tolerance larger than the radius: half turn
    assert!(near(max_step(0.0, 0.002), PI)); // degenerate radius
}
