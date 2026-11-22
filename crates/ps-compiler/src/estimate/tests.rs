//! Ground truth for estimation: time from the trapezoidal/look-ahead motion model plus
//! tool-up/down times, dwells and heating time; user pauses are counted separately; a per-machine
//! `time_scale` adjusts machine time; cut length is reported per tool slot; tool changes are
//! counted; utilisation is a share in parts per million. Expected values are closed-form.

use ps_core::{MatId, ObjectId, OpId, PathKey, PtUm};
use ps_job::{Estimate, HolderId, PauseMsg, SrcRef, Step};

use super::*;

const MM: i64 = 1_000;

fn p(x: i64, y: i64) -> PtUm {
    PtUm::new(x * MM, y * MM)
}

fn src() -> SrcRef {
    SrcRef {
        mat: MatId::at_unix_ms(1),
        object: ObjectId::at_unix_ms(2),
        path: PathKey::source("p"),
        op: OpId::at_unix_ms(3),
        move_index: 0,
    }
}

fn line(x: i64, y: i64, mm_per_s: u32) -> Step {
    Step::Line {
        to: p(x, y),
        feed_mm_min: mm_per_s * 60,
        a_mdeg: None,
        src: src(),
    }
}

fn input(steps: &[Step]) -> EstimateInput<'_> {
    EstimateInput {
        steps,
        limits: MotionLimits {
            max_speed_mm_s: 500.0,
            max_travel_speed_mm_s: 500.0,
            accel_mm_s2: 1_000.0,
            junction_deviation_mm: 0.05,
        },
        tool_up_ms: 100,
        tool_down_ms: 150,
        heating_rate_c_s: None,
        ambient_c: 25,
        time_scale: 1.0,
        pause_allowance_ms: 20_000,
        accel_by_speed: &[],
        vector_ms_by_speed: &[],
    }
}

fn near(a: u64, b: u64, tol: u64) -> bool {
    a.abs_diff(b) <= tol
}

#[test]
fn an_empty_program_takes_no_time() {
    let e = estimate(&input(&[]));
    assert_eq!(e, Estimate::default());
}

#[test]
fn a_rapid_travel_is_a_triangle_profile_at_the_machine_limits() {
    // 100 mm rest to rest at a = 1000 mm/s^2 never reaches 500 mm/s: 2 sqrt(L / a) = 0.6325 s.
    let steps = [Step::Travel { to: p(100, 0) }];
    let e = estimate(&input(&steps));
    assert!(near(e.machine_time_ms, 632, 1), "{}", e.machine_time_ms);
}

#[test]
fn a_drawn_line_adds_tool_down_and_up_times() {
    // 100 mm at 100 mm/s: 0.1 + 0.9 + 0.1 = 1.1 s, plus 150 ms down and 100 ms up.
    let steps = [
        Step::ToolDown { force_gf: 100 },
        line(100, 0, 100),
        Step::ToolUp,
    ];
    let e = estimate(&input(&steps));
    assert_eq!(e.machine_time_ms, 1_350);
}

#[test]
fn collinear_drawn_moves_are_planned_together() {
    let one = [Step::ToolDown { force_gf: 1 }, line(100, 0, 100)];
    let two = [
        Step::ToolDown { force_gf: 1 },
        line(50, 0, 100),
        line(100, 0, 100),
    ];
    assert_eq!(
        estimate(&input(&one)).machine_time_ms,
        estimate(&input(&two)).machine_time_ms
    );
}

#[test]
fn a_corner_costs_time() {
    let straight = [
        Step::ToolDown { force_gf: 1 },
        line(50, 0, 100),
        line(100, 0, 100),
    ];
    let corner = [
        Step::ToolDown { force_gf: 1 },
        line(50, 0, 100),
        line(50, 50, 100),
    ];
    assert!(
        estimate(&input(&corner)).machine_time_ms > estimate(&input(&straight)).machine_time_ms
    );
}

#[test]
fn dwells_add_their_duration() {
    let steps = [Step::Dwell { ms: 500 }, Step::Dwell { ms: 250 }];
    assert_eq!(estimate(&input(&steps)).machine_time_ms, 750);
}

#[test]
fn waiting_for_the_heater_uses_the_heating_rate() {
    // From 25 to 225 degrees at 5 degrees per second: 40 s.
    let steps = [Step::Temperature {
        celsius: 225,
        wait: true,
    }];
    let mut i = input(&steps);
    i.heating_rate_c_s = Some(5.0);
    assert_eq!(estimate(&i).machine_time_ms, 40_000);
}

#[test]
fn setting_a_temperature_without_waiting_costs_nothing() {
    let steps = [Step::Temperature {
        celsius: 225,
        wait: false,
    }];
    let mut i = input(&steps);
    i.heating_rate_c_s = Some(5.0);
    assert_eq!(estimate(&i).machine_time_ms, 0);
}

#[test]
fn a_second_wait_at_the_same_temperature_is_free() {
    let steps = [
        Step::Temperature {
            celsius: 100,
            wait: true,
        },
        Step::Temperature {
            celsius: 100,
            wait: true,
        },
    ];
    let mut i = input(&steps);
    i.heating_rate_c_s = Some(5.0);
    assert_eq!(estimate(&i).machine_time_ms, 15_000);
}

#[test]
fn the_time_scale_adjusts_machine_time() {
    let steps = [Step::Dwell { ms: 1_000 }];
    let mut i = input(&steps);
    i.time_scale = 1.1;
    assert_eq!(estimate(&i).machine_time_ms, 1_100);
}

#[test]
fn user_pauses_are_counted_separately() {
    let steps = [
        Step::Pause(PauseMsg::new("hold on")),
        Step::Dwell { ms: 1_000 },
        Step::LoadTool {
            slot: 2,
            holder: HolderId::new("a"),
            pause: Some(PauseMsg::new("swap")),
        },
        Step::LoadTool {
            slot: 1,
            holder: HolderId::new("b"),
            pause: None,
        },
    ];
    let e = estimate(&input(&steps));
    assert_eq!(e.machine_time_ms, 1_000);
    assert_eq!(e.with_changes_ms, 1_000 + 2 * 20_000);
    assert_eq!(e.tool_changes, 1, "only a tool swap is a tool change");
}

#[test]
fn drawn_length_is_reported_per_tool_slot() {
    let steps = [
        Step::LoadTool {
            slot: 1,
            holder: HolderId::new("a"),
            pause: None,
        },
        Step::ToolDown { force_gf: 1 },
        line(100, 0, 100),
        Step::ToolUp,
        Step::Travel { to: p(0, 50) },
        Step::LoadTool {
            slot: 2,
            holder: HolderId::new("b"),
            pause: None,
        },
        Step::ToolDown { force_gf: 1 },
        line(0, 80, 100),
    ];
    let e = estimate(&input(&steps));
    assert_eq!(e.cut_length_um.get(&1), Some(&100_000));
    assert_eq!(e.cut_length_um.get(&2), Some(&30_000));
    assert_eq!(e.cut_length_um.len(), 2);
}

#[test]
fn arc_length_uses_the_radius_and_sweep() {
    // Counter-clockwise half circle of radius 10 mm from (10, 0) to (-10, 0).
    let steps = [
        Step::Travel { to: p(10, 0) },
        Step::LoadTool {
            slot: 1,
            holder: HolderId::new("a"),
            pause: None,
        },
        Step::ToolDown { force_gf: 1 },
        Step::Arc {
            to: p(-10, 0),
            center: p(0, 0),
            ccw: true,
            feed_mm_min: 6_000,
            src: src(),
        },
    ];
    let e = estimate(&input(&steps));
    let len = e.cut_length_um[&1];
    assert!(len.abs_diff(31_416) <= 1, "{len}");
}

#[test]
fn a_clockwise_arc_takes_the_long_way_round() {
    // Clockwise from (10, 0) to (0, 10) about the origin is three quarters of a circle.
    let steps = [
        Step::Travel { to: p(10, 0) },
        Step::LoadTool {
            slot: 1,
            holder: HolderId::new("a"),
            pause: None,
        },
        Step::ToolDown { force_gf: 1 },
        Step::Arc {
            to: p(0, 10),
            center: p(0, 0),
            ccw: false,
            feed_mm_min: 6_000,
            src: src(),
        },
    ];
    let len = estimate(&input(&steps)).cut_length_um[&1];
    assert!(len.abs_diff(47_124) <= 1, "{len}");
}

#[test]
fn travel_between_strokes_is_not_drawn_length() {
    let steps = [
        Step::LoadTool {
            slot: 1,
            holder: HolderId::new("a"),
            pause: None,
        },
        Step::Travel { to: p(500, 500) },
    ];
    assert_eq!(estimate(&input(&steps)).cut_length_um.len(), 0);
}

#[test]
fn utilisation_is_the_covered_share_in_parts_per_million() {
    assert_eq!(utilisation_ppm(250, 1_000), 250_000);
    assert_eq!(utilisation_ppm(0, 1_000), 0);
    assert_eq!(
        utilisation_ppm(2_000, 1_000),
        1_000_000,
        "capped at the whole mat"
    );
    assert_eq!(utilisation_ppm(5, 0), 0, "no mat, no share");
}

#[test]
fn estimation_is_deterministic() {
    let steps = [
        Step::ToolDown { force_gf: 1 },
        line(40, 10, 80),
        line(20, 70, 80),
        Step::ToolUp,
    ];
    assert_eq!(estimate(&input(&steps)), estimate(&input(&steps)));
}

fn input_with_limits(
    steps: &[Step],
    max_speed_mm_s: f64,
    max_travel_speed_mm_s: f64,
) -> EstimateInput<'_> {
    let mut i = input(steps);
    i.limits.max_speed_mm_s = max_speed_mm_s;
    i.limits.max_travel_speed_mm_s = max_travel_speed_mm_s;
    i
}

#[test]
fn a_rapid_travel_goes_at_the_travel_limit_not_the_drawing_limit() {
    // 100 mm rest to rest at 1000 mm/s^2 with a 50 mm/s limit: 1.25 mm to get up to speed and as
    // much to stop, 97.5 mm at 50 mm/s = 1.95 s, and 0.05 s each way: 2.05 s.
    let steps = [Step::Travel { to: p(100, 0) }];
    let e = estimate(&input_with_limits(&steps, 500.0, 50.0));
    assert!(near(e.machine_time_ms, 2050, 1), "{}", e.machine_time_ms);
}

#[test]
fn a_rapid_travel_may_be_faster_than_drawing() {
    // Drawing tops out at 100 mm/s and travel at 200: 100 mm is 20 mm up to speed, 20 mm down and
    // 60 mm at 200 mm/s = 0.3 s, with 0.2 s each way: 0.7 s.
    let steps = [Step::Travel { to: p(100, 0) }];
    let e = estimate(&input_with_limits(&steps, 100.0, 200.0));
    assert!(near(e.machine_time_ms, 700, 1), "{}", e.machine_time_ms);
}

#[test]
fn a_slow_travel_limit_does_not_slow_a_drawn_line() {
    // As `a_drawn_line_adds_tool_down_and_up_times`: 0.1 + 0.9 + 0.1 s plus 150 ms and 100 ms.
    let steps = [
        Step::ToolDown { force_gf: 100 },
        line(100, 0, 100),
        Step::ToolUp,
    ];
    let e = estimate(&input_with_limits(&steps, 500.0, 50.0));
    assert!(near(e.machine_time_ms, 1350, 2), "{}", e.machine_time_ms);
}

#[test]
fn a_machine_that_accelerates_harder_at_speed_is_planned_with_that_acceleration() {
    // 100 mm at 100 mm/s with 300 mm/s^2 at that speed (the table): rest to rest, 16.67 mm to get
    // up to speed and as much to stop, 66.67 mm at 100 mm/s = 0.6667 s, and 0.3333 s each way:
    // 1.3333 s, plus 150 ms down and 100 ms up. At the flat 1000 mm/s^2 it would be 1.1 s.
    let steps = [
        Step::ToolDown { force_gf: 100 },
        line(100, 0, 100),
        Step::ToolUp,
    ];
    let table = [(50.0, 100.0), (100.0, 300.0)];
    let mut i = input(&steps);
    i.accel_by_speed = &table;
    let e = estimate(&i);
    assert!(near(e.machine_time_ms, 1_583, 2), "{}", e.machine_time_ms);
}

fn three_collinear_lines() -> [Step; 5] {
    [
        Step::ToolDown { force_gf: 100 },
        line(100, 0, 100),
        line(200, 0, 100),
        line(300, 0, 100),
        Step::ToolUp,
    ]
}

#[test]
fn every_drawn_vector_costs_the_machine_a_fixed_time() {
    // 300 mm at 100 mm/s rest to rest with 1000 mm/s^2: 0.1 s each way and 2.9 s between, 3.1 s;
    // plus 150 ms down and 100 ms up: 3.35 s. Three vectors at 10 ms each (the table) add 30 ms.
    let steps = three_collinear_lines();
    let plain = estimate(&input(&steps));
    assert!(
        near(plain.machine_time_ms, 3_350, 2),
        "{}",
        plain.machine_time_ms
    );
    let table = [(100.0, 10.0)];
    let mut i = input(&steps);
    i.vector_ms_by_speed = &table;
    let costed = estimate(&i);
    assert!(
        near(costed.machine_time_ms, 3_380, 2),
        "{}",
        costed.machine_time_ms
    );
}

#[test]
fn the_cost_of_a_vector_follows_the_speed_it_is_drawn_at() {
    // 20 ms a vector at 50 mm/s, 10 ms at 100: at 75 mm/s it is 15 ms, so three vectors cost 45 ms.
    let steps = [
        Step::ToolDown { force_gf: 100 },
        line(75, 0, 75),
        line(150, 0, 75),
        line(225, 0, 75),
        Step::ToolUp,
    ];
    let table = [(50.0, 20.0), (100.0, 10.0)];
    let plain = estimate(&input(&steps));
    let mut i = input(&steps);
    i.vector_ms_by_speed = &table;
    let costed = estimate(&i);
    assert!(
        near(costed.machine_time_ms - plain.machine_time_ms, 45, 2),
        "{} against {}",
        costed.machine_time_ms,
        plain.machine_time_ms
    );
}
