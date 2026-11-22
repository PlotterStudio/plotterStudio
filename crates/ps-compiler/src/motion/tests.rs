//! Ground truth for the motion planner: a trapezoidal velocity profile per move with
//! a look-ahead planner (forward and backward passes, like GRBL) where corner speed is limited by
//! junction deviation. All expected times are closed-form physics.

use super::*;

fn near(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() < eps
}

const A: f64 = 1_000.0; // mm/s²

fn mv(len: f64, dir: (f64, f64), feed: f64) -> Move {
    Move {
        length_mm: len,
        dir: Dir { x: dir.0, y: dir.1 },
        feed_mm_s: feed,
    }
}

fn limits() -> MotionLimits {
    MotionLimits {
        max_speed_mm_s: 500.0,
        max_travel_speed_mm_s: 500.0,
        accel_mm_s2: A,
        junction_deviation_mm: 0.05,
    }
}

// ---- segment time ----

#[test]
fn a_trapezoid_accelerates_cruises_and_brakes() {
    // 100 mm from rest to rest at 100 mm/s: 5 mm to speed up (0.1 s), 5 mm to stop (0.1 s),
    // 90 mm cruising (0.9 s).
    let t = segment_time(100.0, 0.0, 100.0, 0.0, A);
    assert!(near(t, 1.1, 1e-9), "{t}");
}

#[test]
fn a_move_too_short_to_reach_cruise_is_a_triangle() {
    // 5 mm from rest to rest: peak speed sqrt(a L) = 70.7 mm/s, time 2 sqrt(L / a).
    let t = segment_time(5.0, 0.0, 100.0, 0.0, A);
    assert!(near(t, 2.0 * (5.0_f64 / A).sqrt(), 1e-9), "{t}");
}

#[test]
fn entry_and_exit_speeds_shorten_the_move() {
    // 100 mm, entering and leaving at 100 mm/s, cruise 100: constant speed, 1 s.
    assert!(near(segment_time(100.0, 100.0, 100.0, 100.0, A), 1.0, 1e-9));
    // Entering at 50 and leaving at 0 with cruise 100: (100² - 50²) / (2a) = 3.75 mm to reach
    // 100 from 50 (0.05 s), 5 mm to stop (0.1 s).
    let t = segment_time(100.0, 50.0, 100.0, 0.0, A);
    let expected = 0.05 + (100.0 - 3.75 - 5.0) / 100.0 + 0.1;
    assert!(near(t, expected, 1e-9), "{t} vs {expected}");
}

#[test]
fn a_zero_length_move_takes_no_time() {
    assert_eq!(segment_time(0.0, 0.0, 100.0, 0.0, A), 0.0);
}

// ---- junction speed ----

#[test]
fn a_straight_junction_is_limited_only_by_the_neighbours() {
    let v = junction_speed(
        Dir { x: 1.0, y: 0.0 },
        Dir { x: 1.0, y: 0.0 },
        A,
        0.05,
        80.0,
    );
    assert!(near(v, 80.0, 1e-9));
}

#[test]
fn a_right_angle_corner_follows_the_junction_deviation_formula() {
    // cos(theta) = 0, sin(theta / 2) = sqrt(1/2); v^2 = a d s / (1 - s) = 1000 * 0.05 * 2.4142 = 120.7
    let v = junction_speed(
        Dir { x: 1.0, y: 0.0 },
        Dir { x: 0.0, y: 1.0 },
        A,
        0.05,
        1_000.0,
    );
    let s = 0.5_f64.sqrt();
    assert!(near(v, (A * 0.05 * s / (1.0 - s)).sqrt(), 1e-9), "{v}");
    assert!(near(v, 10.987, 1e-3), "{v}");
}

#[test]
fn a_reversal_is_taken_very_slowly() {
    let v = junction_speed(
        Dir { x: 1.0, y: 0.0 },
        Dir { x: -1.0, y: 0.0 },
        A,
        0.05,
        100.0,
    );
    assert!(v <= MIN_JUNCTION_SPEED + 1e-12, "{v}");
}

#[test]
fn sharper_corners_are_slower() {
    let slow = junction_speed(
        Dir { x: 1.0, y: 0.0 },
        Dir {
            x: -0.9,
            y: 0.435_889_894_354_067_4,
        },
        A,
        0.05,
        1_000.0,
    );
    let medium = junction_speed(
        Dir { x: 1.0, y: 0.0 },
        Dir { x: 0.0, y: 1.0 },
        A,
        0.05,
        1_000.0,
    );
    let gentle = junction_speed(
        Dir { x: 1.0, y: 0.0 },
        Dir {
            x: 0.9,
            y: 0.435_889_894_354_067_4,
        },
        A,
        0.05,
        1_000.0,
    );
    assert!(slow < medium && medium < gentle);
}

#[test]
fn a_bigger_deviation_allows_faster_corners() {
    let tight = junction_speed(
        Dir { x: 1.0, y: 0.0 },
        Dir { x: 0.0, y: 1.0 },
        A,
        0.01,
        1_000.0,
    );
    let loose = junction_speed(
        Dir { x: 1.0, y: 0.0 },
        Dir { x: 0.0, y: 1.0 },
        A,
        0.1,
        1_000.0,
    );
    assert!(tight < loose);
}

// ---- look-ahead plan ----

#[test]
fn two_collinear_moves_take_as_long_as_one() {
    let one = plan(&[mv(20.0, (1.0, 0.0), 100.0)], &limits());
    let two = plan(
        &[mv(10.0, (1.0, 0.0), 100.0), mv(10.0, (1.0, 0.0), 100.0)],
        &limits(),
    );
    assert!(near(one.total_s, 0.3, 1e-9), "{}", one.total_s);
    assert!(near(two.total_s, one.total_s, 1e-9), "{}", two.total_s);
}

#[test]
fn the_plan_starts_and_ends_at_rest() {
    let p = plan(
        &[mv(10.0, (1.0, 0.0), 100.0), mv(10.0, (1.0, 0.0), 100.0)],
        &limits(),
    );
    assert_eq!(p.junction_speeds.first().copied(), Some(0.0));
    assert_eq!(p.junction_speeds.last().copied(), Some(0.0));
    assert_eq!(p.junction_speeds.len(), 3);
}

#[test]
fn a_corner_slows_the_job_down() {
    let straight = plan(
        &[mv(50.0, (1.0, 0.0), 100.0), mv(50.0, (1.0, 0.0), 100.0)],
        &limits(),
    );
    let corner = plan(
        &[mv(50.0, (1.0, 0.0), 100.0), mv(50.0, (0.0, 1.0), 100.0)],
        &limits(),
    );
    assert!(corner.total_s > straight.total_s);
    // The corner speed is exactly the junction speed (it is reachable within 50 mm).
    assert!(
        near(corner.junction_speeds[1], 10.987, 1e-3),
        "{:?}",
        corner.junction_speeds
    );
}

#[test]
fn junction_speeds_never_exceed_what_acceleration_allows() {
    // Two very short collinear moves cannot reach their cruise speed.
    let p = plan(
        &[mv(1.0, (1.0, 0.0), 400.0), mv(1.0, (1.0, 0.0), 400.0)],
        &limits(),
    );
    let v = p.junction_speeds[1];
    assert!(v <= (2.0 * A * 1.0).sqrt() + 1e-9, "{v}");
}

#[test]
fn the_planner_never_exceeds_the_machine_maximum_speed() {
    let p = plan(&[mv(1_000.0, (1.0, 0.0), 10_000.0)], &limits());
    // Cruise is capped at 500 mm/s: 125 mm up, 125 mm down, 750 mm at 500 = 1.5 s + 1 s.
    assert!(near(p.total_s, 0.5 + 0.5 + 1.5, 1e-9), "{}", p.total_s);
}

#[test]
fn an_empty_plan_takes_no_time() {
    let p = plan(&[], &limits());
    assert_eq!(p.total_s, 0.0);
    assert_eq!(p.junction_speeds, vec![0.0]);
}

#[test]
fn planning_is_deterministic() {
    let moves: Vec<Move> = (0..50)
        .map(|i| {
            let a = f64::from(i) * 0.7;
            mv(2.0 + f64::from(i % 5), (a.cos(), a.sin()), 60.0)
        })
        .collect();
    assert_eq!(plan(&moves, &limits()), plan(&moves, &limits()));
}

// ---- acceleration that depends on the speed ----

#[test]
fn acceleration_is_read_off_the_table_between_its_points() {
    let table = [(50.0, 100.0), (100.0, 300.0)];
    // Halfway between the two points is halfway between their accelerations.
    assert!((accel_at(&table, 1_000.0, 75.0) - 200.0).abs() < 1e-9);
    assert!((accel_at(&table, 1_000.0, 50.0) - 100.0).abs() < 1e-9);
    assert!((accel_at(&table, 1_000.0, 100.0) - 300.0).abs() < 1e-9);
}

#[test]
fn acceleration_beyond_the_table_is_held_and_below_it_falls_towards_zero() {
    let table = [(50.0, 100.0), (100.0, 300.0)];
    assert!((accel_at(&table, 1_000.0, 500.0) - 300.0).abs() < 1e-9);
    // Below the first point, in proportion to speed: half the speed, half the acceleration.
    assert!((accel_at(&table, 1_000.0, 25.0) - 50.0).abs() < 1e-9);
}

#[test]
fn without_a_table_acceleration_is_the_machines_one_number() {
    assert!((accel_at(&[], 1_000.0, 75.0) - 1_000.0).abs() < 1e-9);
}

// ---- the time the machine spends on each vector ----

#[test]
fn the_cost_of_a_vector_is_read_off_the_table_and_held_beyond_it() {
    let table = [(50.0, 20.0), (100.0, 10.0)];
    assert!(
        (vector_ms_at(&table, 75.0) - 15.0).abs() < 1e-9,
        "between the points"
    );
    assert!((vector_ms_at(&table, 50.0) - 20.0).abs() < 1e-9);
    // Held at the ends, unlike acceleration, which falls towards zero with the speed.
    assert!((vector_ms_at(&table, 10.0) - 20.0).abs() < 1e-9);
    assert!((vector_ms_at(&table, 500.0) - 10.0).abs() < 1e-9);
    assert!((vector_ms_at(&[], 75.0)).abs() < 1e-9, "no table, no cost");
}
