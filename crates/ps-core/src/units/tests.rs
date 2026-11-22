//! Ground truth for the physical-quantity newtypes and tolerance constants.

use super::*;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn mm_adds_subtracts_and_scales() {
    assert_eq!(Mm(1.5) + Mm(2.0), Mm(3.5));
    assert_eq!(Mm(5.0) - Mm(2.0), Mm(3.0));
    assert_eq!(-Mm(2.0), Mm(-2.0));
    assert_eq!(Mm(2.0) * 3.0, Mm(6.0));
    assert_eq!(Mm(9.0) / 3.0, Mm(3.0));
    assert!(close(Mm(9.0) / Mm(3.0), 3.0)); // a ratio of lengths is a plain number
}

#[test]
fn mm_converts_to_and_from_inches() {
    assert!(close(Mm::from_inches(1.0).get(), 25.4));
    assert!(close(Mm::from_inches(12.0).get(), 304.8)); // the 12 in mat edge
    assert!(close(Mm(609.6).to_inches(), 24.0)); // the 24 in mat edge
}

#[test]
fn mm_per_sec_converts_to_mm_per_min() {
    assert!(close(MmPerSec(100.0).to_mm_per_min(), 6_000.0)); // F6000 in the 9.1 example
    assert!(close(MmPerSec(50.0).to_mm_per_min(), 3_000.0)); // F3000
}

#[test]
fn degrees_convert_to_radians() {
    assert!(close(Degrees(180.0).to_radians(), std::f64::consts::PI));
    assert!(close(
        Degrees(90.0).to_radians(),
        std::f64::consts::FRAC_PI_2
    ));
    assert!(close(
        Degrees::from_radians(std::f64::consts::PI).get(),
        180.0
    ));
}

#[test]
fn degrees_wrap_into_half_open_range() {
    assert_eq!(Degrees(190.0).wrapped(), Degrees(-170.0));
    assert_eq!(Degrees(-190.0).wrapped(), Degrees(170.0));
    assert_eq!(Degrees(180.0).wrapped(), Degrees(180.0));
    assert_eq!(Degrees(-180.0).wrapped(), Degrees(180.0));
    assert_eq!(Degrees(540.0).wrapped(), Degrees(180.0));
    assert_eq!(Degrees(0.0).wrapped(), Degrees(0.0));
    assert_eq!(Degrees(360.0).wrapped(), Degrees(0.0));
}

#[test]
fn unwrap_near_keeps_the_angle_continuous() {
    // The tool must never spin 360 degrees to cross the +/-180 boundary.
    assert_eq!(Degrees(-170.0).unwrap_near(Degrees(170.0)), Degrees(190.0));
    assert_eq!(Degrees(170.0).unwrap_near(Degrees(-170.0)), Degrees(-190.0));
    assert_eq!(Degrees(90.0).unwrap_near(Degrees(0.0)), Degrees(90.0));
    assert_eq!(Degrees(-170.0).unwrap_near(Degrees(190.0)), Degrees(190.0));
    // Already near: unchanged.
    assert_eq!(Degrees(45.0).unwrap_near(Degrees(40.0)), Degrees(45.0));
}

#[test]
fn unwrap_near_never_moves_more_than_half_a_turn() {
    let mut prev = Degrees(0.0);
    // A path that winds around several times in 30 degree steps.
    for step in 0..40 {
        let raw = Degrees(f64::from(step) * 30.0).wrapped();
        let next = raw.unwrap_near(prev);
        assert!((next.get() - prev.get()).abs() <= 180.0 + 1e-9);
        prev = next;
    }
    assert!(close(prev.get(), 1_170.0)); // 39 steps of 30 degrees, never spun back
}

#[test]
fn quantities_serialise_as_bare_numbers() {
    assert_eq!(serde_json::to_string(&Mm(1.5)).unwrap(), "1.5");
    assert_eq!(serde_json::to_string(&GramsForce(350.0)).unwrap(), "350.0");
    let back: Celsius = serde_json::from_str("120.0").unwrap();
    assert_eq!(back, Celsius(120.0));
    let ms: Millis = serde_json::from_str("80").unwrap();
    assert_eq!(ms, Millis(80));
}

#[test]
fn quantities_display_with_their_unit() {
    assert_eq!(Mm(12.5).to_string(), "12.5 mm");
    assert_eq!(GramsForce(350.0).to_string(), "350 gf");
    assert_eq!(MmPerSec(100.0).to_string(), "100 mm/s");
    assert_eq!(MmPerSec2(500.0).to_string(), "500 mm/s²");
    assert_eq!(Celsius(120.0).to_string(), "120 °C");
    assert_eq!(Millis(80).to_string(), "80 ms");
    assert_eq!(Degrees(45.0).to_string(), "45°");
}

#[test]
fn precision_budget_sums_to_the_prd_limit() {
    // The stages must add up to the 0.01 mm cumulative error.
    let total = tolerance::BEZIER_TO_ARC
        + tolerance::FLATTEN
        + tolerance::OFFSET
        + tolerance::GRID_SNAP
        + tolerance::RESERVE;
    assert!(close(total.get(), 0.010));
    assert!(close(tolerance::PROCESSING_BUDGET.get(), 0.010));
}

#[test]
fn tolerance_constants_match_the_design_document() {
    assert_eq!(tolerance::BEZIER_TO_ARC, Mm(0.002));
    assert_eq!(tolerance::FLATTEN, Mm(0.002));
    assert_eq!(tolerance::OFFSET, Mm(0.002));
    assert_eq!(tolerance::GRID_SNAP, Mm(0.0005));
    assert_eq!(tolerance::RESERVE, Mm(0.0035));
    assert_eq!(tolerance::CLOSE_PATH, Mm(0.05));
    assert_eq!(tolerance::DUPLICATE, Mm(0.001));
    assert_eq!(tolerance::ZERO_LENGTH_SEGMENT, Mm(0.001));
    assert_eq!(tolerance::CHORD, Mm(0.01));
    assert_eq!(tolerance::HIT_TEST_SCREEN_PX, 4.0);
    assert_eq!(tolerance::SNAP_SCREEN_PX, 8.0);
}

#[test]
fn hit_test_tolerance_scales_with_zoom() {
    // 4 screen px at 2 px/mm is 2 mm; at 8 px/mm it is 0.5 mm.
    assert!(close(tolerance::px_to_mm(4.0, 2.0).get(), 2.0));
    assert!(close(tolerance::px_to_mm(4.0, 8.0).get(), 0.5));
}
