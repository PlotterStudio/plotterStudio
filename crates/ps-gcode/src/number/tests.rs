//! Ground truth for number formatting: coordinates in mm with exactly three decimals,
//! printed from integer micrometres, never from floats; tangent angles in degrees with three
//! decimals from milli-degrees.

use super::*;

#[test]
fn micrometres_print_as_millimetres_with_three_decimals() {
    assert_eq!(um(0), "0.000");
    assert_eq!(um(1), "0.001");
    assert_eq!(um(12_345), "12.345");
    assert_eq!(um(-12_345), "-12.345");
    assert_eq!(um(12_500), "12.500");
    assert_eq!(um(1_000_000), "1000.000");
    assert_eq!(um(999), "0.999");
}

#[test]
fn small_negative_values_keep_their_sign() {
    assert_eq!(um(-1), "-0.001");
    assert_eq!(um(-999), "-0.999");
    assert_eq!(um(-1_000), "-1.000");
}

#[test]
fn the_extremes_do_not_overflow() {
    assert_eq!(um(i64::MAX), "9223372036854775.807");
    assert_eq!(um(i64::MIN), "-9223372036854775.808");
}

#[test]
fn milli_degrees_print_as_degrees_with_three_decimals() {
    assert_eq!(mdeg(0), "0.000");
    assert_eq!(mdeg(90_000), "90.000");
    assert_eq!(mdeg(-45_500), "-45.500");
    assert_eq!(mdeg(540_001), "540.001");
}

#[test]
fn trimmed_numbers_drop_trailing_zeros_for_previews() {
    assert_eq!(um_trimmed(12_500), "12.5");
    assert_eq!(um_trimmed(12_000), "12");
    assert_eq!(um_trimmed(-12_345), "-12.345");
    assert_eq!(um_trimmed(0), "0");
    assert_eq!(um_trimmed(-500), "-0.5");
}

#[test]
fn comment_text_is_made_safe_for_a_parenthesised_message() {
    assert_eq!(pause_text("Load pen: Fine liner"), "Load pen: Fine liner");
    assert_eq!(pause_text("one\ntwo"), "one; two");
    assert_eq!(pause_text("Blade (45°)"), "Blade [45°]");
    assert_eq!(pause_text("a\r\nb"), "a; b");
    assert_eq!(pause_text("  spaced  "), "spaced");
}
