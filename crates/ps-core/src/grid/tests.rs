//! Ground truth for the µm grid: round half away from zero once, print
//! integers with an inserted decimal point, never format floats.

use super::*;
use proptest::prelude::*;

#[test]
fn whole_and_fractional_mm_convert_exactly() {
    assert_eq!(um_from_mm(12.345), Ok(12_345));
    assert_eq!(um_from_mm(-12.345), Ok(-12_345));
    assert_eq!(um_from_mm(0.0), Ok(0));
    assert_eq!(um_from_mm(304.8), Ok(304_800)); // 12 in
}

#[test]
fn rounding_is_half_away_from_zero() {
    // 0.0625 mm is exactly representable and equals 62.5 µm.
    assert_eq!(um_from_mm(0.0625), Ok(63));
    assert_eq!(um_from_mm(-0.0625), Ok(-63));
    // Just under the half-way point rounds toward zero.
    assert_eq!(um_from_mm(0.0624), Ok(62));
    assert_eq!(um_from_mm(-0.0624), Ok(-62));
}

#[test]
fn negative_zero_becomes_zero() {
    assert_eq!(um_from_mm(-0.0), Ok(0));
    assert_eq!(um_from_mm(-0.0001), Ok(0));
}

#[test]
fn non_finite_input_is_rejected() {
    assert_eq!(um_from_mm(f64::NAN), Err(GridError::NotFinite));
    assert_eq!(um_from_mm(f64::INFINITY), Err(GridError::NotFinite));
    assert_eq!(um_from_mm(f64::NEG_INFINITY), Err(GridError::NotFinite));
}

#[test]
fn out_of_range_input_is_rejected() {
    // 1e13 mm is 1e16 µm, beyond the 2^53 range where f64 holds whole µm exactly.
    assert_eq!(um_from_mm(1.0e13), Err(GridError::OutOfRange));
    assert_eq!(um_from_mm(-1.0e13), Err(GridError::OutOfRange));
    // A 10 km work area is far inside the range.
    assert_eq!(um_from_mm(1.0e7), Ok(10_000_000_000));
}

#[test]
fn mm_from_um_divides_by_one_thousand() {
    assert!((mm_from_um(12_345) - 12.345).abs() < 1e-12);
    assert!((mm_from_um(-1) + 0.001).abs() < 1e-12);
    assert!(mm_from_um(0).abs() < f64::EPSILON);
}

#[test]
fn format_um_inserts_the_decimal_point() {
    assert_eq!(format_um(12_345), "12.345");
    assert_eq!(format_um(-12_345), "-12.345"); // the documented example
    assert_eq!(format_um(0), "0.000");
    assert_eq!(format_um(5), "0.005");
    assert_eq!(format_um(-5), "-0.005");
    assert_eq!(format_um(1_000), "1.000");
    assert_eq!(format_um(62_500), "62.500");
}

#[test]
fn format_um_handles_the_extreme_values() {
    assert_eq!(format_um(i64::MAX), "9223372036854775.807");
    assert_eq!(format_um(i64::MIN), "-9223372036854775.808");
}

#[test]
fn format_mdeg_prints_three_decimals_from_milli_degrees() {
    assert_eq!(format_mdeg(3_500), "3.500");
    assert_eq!(format_mdeg(-90_000), "-90.000");
    assert_eq!(format_mdeg(0), "0.000");
    assert_eq!(format_mdeg(-1), "-0.001");
    assert_eq!(format_mdeg(360_000), "360.000");
}

#[test]
fn mdeg_from_degrees_rounds_half_away_from_zero() {
    assert_eq!(mdeg_from_degrees(90.0), Ok(90_000));
    assert_eq!(mdeg_from_degrees(-179.5), Ok(-179_500));
    // 0.0625 deg is 62.5 mdeg exactly.
    assert_eq!(mdeg_from_degrees(0.0625), Ok(63));
    assert_eq!(mdeg_from_degrees(f64::NAN), Err(GridError::NotFinite));
}

#[test]
fn points_do_arithmetic_on_the_grid() {
    let a = PtUm::new(1_000, -2_000);
    let b = PtUm::new(250, 500);
    assert_eq!(a + b, PtUm::new(1_250, -1_500));
    assert_eq!(a - b, PtUm::new(750, -2_500));
    assert_eq!(-a, PtUm::new(-1_000, 2_000));
}

#[test]
fn points_convert_from_and_to_mm() {
    let p = PtUm::from_mm(12.5, 20.0).unwrap();
    assert_eq!(p, PtUm::new(12_500, 20_000));
    let (x, y) = p.to_mm();
    assert!((x - 12.5).abs() < 1e-12);
    assert!((y - 20.0).abs() < 1e-12);
    assert_eq!(PtUm::from_mm(f64::NAN, 0.0), Err(GridError::NotFinite));
}

#[test]
fn squared_distance_uses_wide_integers() {
    // 3-4-5 triangle in µm.
    assert_eq!(PtUm::new(0, 0).dist_sq(PtUm::new(3_000, 4_000)), 25_000_000);
    // Would overflow i64 if computed naively.
    let far = PtUm::new(i64::MAX / 2, 0);
    assert_eq!(
        PtUm::new(0, 0).dist_sq(far),
        i128::from(i64::MAX / 2) * i128::from(i64::MAX / 2)
    );
}

#[test]
fn points_order_by_x_then_y() {
    let mut pts = vec![
        PtUm::new(2, 1),
        PtUm::new(1, 5),
        PtUm::new(1, -5),
        PtUm::new(2, 0),
    ];
    pts.sort();
    assert_eq!(
        pts,
        vec![
            PtUm::new(1, -5),
            PtUm::new(1, 5),
            PtUm::new(2, 0),
            PtUm::new(2, 1)
        ]
    );
}

#[test]
fn points_serialise_as_a_pair_of_integers() {
    let json = serde_json::to_string(&PtUm::new(5, -6)).unwrap();
    assert_eq!(json, r#"{"x":5,"y":-6}"#);
    let back: PtUm = serde_json::from_str(&json).unwrap();
    assert_eq!(back, PtUm::new(5, -6));
}

proptest! {
    /// Converting a whole-µm value to mm and back is lossless (half a µm of slack covers f64).
    #[test]
    fn um_to_mm_to_um_round_trips(um in -1_000_000_000_000_i64..1_000_000_000_000_i64) {
        prop_assert_eq!(um_from_mm(mm_from_um(um)), Ok(um));
    }

    /// The grid snap moves a value by at most half a grid step (the 0.0005 mm budget in 6.2).
    #[test]
    fn snapping_error_is_at_most_half_a_micrometre(mm in -10_000.0_f64..10_000.0) {
        let um = um_from_mm(mm).unwrap();
        prop_assert!((mm_from_um(um) - mm).abs() <= 0.000_5 + 1e-9);
    }

    /// Formatting then reading the digits back gives the integer, for every value.
    #[test]
    fn format_um_round_trips_through_text(um in any::<i64>()) {
        let text = format_um(um);
        let negative = text.starts_with('-');
        let digits: String = text.chars().filter(char::is_ascii_digit).collect();
        let magnitude: i128 = digits.parse().unwrap();
        let value = if negative { -magnitude } else { magnitude };
        prop_assert_eq!(value, i128::from(um));
        // Exactly three decimals, always.
        let (_, frac) = text.rsplit_once('.').unwrap();
        prop_assert_eq!(frac.len(), 3);
    }
}
