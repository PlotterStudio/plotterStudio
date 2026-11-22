//! Ground truth for number formatting: numbers are formatted by our own formatter that
//! honours the user's decimal-separator setting and unit (mm or in), separately from the UI
//! language. The document always stores millimetres.

use ps_library::{DecimalSeparator, DisplayUnits};

use super::*;

fn mm() -> NumberFormat {
    NumberFormat::new(DisplayUnits::Millimetres, DecimalSeparator::Dot, "en")
}

fn mm_comma() -> NumberFormat {
    NumberFormat::new(DisplayUnits::Millimetres, DecimalSeparator::Comma, "en")
}

fn inches() -> NumberFormat {
    NumberFormat::new(DisplayUnits::Inches, DecimalSeparator::Dot, "en")
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn millimetres_show_up_to_three_decimals_without_trailing_zeros() {
    assert_eq!(mm().length(12.5), "12.5 mm");
    assert_eq!(mm().length(12.0), "12 mm");
    assert_eq!(mm().length(0.1234), "0.123 mm");
    assert_eq!(mm().length(0.0005), "0.001 mm");
}

#[test]
fn inches_show_up_to_four_decimals() {
    assert_eq!(inches().length(25.4), "1 in");
    assert_eq!(inches().length(12.7), "0.5 in");
    assert_eq!(inches().length(10.0), "0.3937 in");
}

#[test]
fn the_separator_follows_the_setting() {
    assert_eq!(mm_comma().length(12.5), "12,5 mm");
    assert_eq!(mm_comma().value(0.25), "0,25");
}

#[test]
fn automatic_separator_follows_the_locale() {
    let de = NumberFormat::new(DisplayUnits::Millimetres, DecimalSeparator::Auto, "de-DE");
    assert_eq!(de.length(1.5), "1,5 mm");
    let en = NumberFormat::new(DisplayUnits::Millimetres, DecimalSeparator::Auto, "en-US");
    assert_eq!(en.length(1.5), "1.5 mm");
}

#[test]
fn the_separator_is_independent_of_the_ui_language() {
    // A German UI with an explicit dot still shows dots.
    let f = NumberFormat::new(DisplayUnits::Millimetres, DecimalSeparator::Dot, "de-DE");
    assert_eq!(f.length(1.5), "1.5 mm");
}

#[test]
fn negative_zero_and_tiny_values_show_as_zero() {
    assert_eq!(mm().length(-0.0), "0 mm");
    assert_eq!(mm().length(-0.0004), "0 mm");
    assert_eq!(mm().length(-1.25), "-1.25 mm");
}

#[test]
fn non_finite_values_show_a_dash() {
    assert_eq!(mm().length(f64::NAN), "–");
    assert_eq!(mm().length(f64::INFINITY), "–");
}

#[test]
fn values_for_fields_carry_no_unit() {
    assert_eq!(mm().value(12.5), "12.5");
    assert_eq!(inches().value(25.4), "1");
}

#[test]
fn the_unit_label_follows_the_setting() {
    assert_eq!(mm().unit_label(), "mm");
    assert_eq!(inches().unit_label(), "in");
}

#[test]
fn typed_lengths_in_the_display_unit_become_millimetres() {
    assert!(close(mm().parse_length("12.5").unwrap(), 12.5));
    assert!(close(inches().parse_length("1").unwrap(), 25.4));
}

#[test]
fn typed_lengths_with_a_unit_override_the_display_unit() {
    assert!(close(mm().parse_length("1in").unwrap(), 25.4));
    assert!(close(mm().parse_length("1 in").unwrap(), 25.4));
    assert!(close(mm().parse_length("1\"").unwrap(), 25.4));
    assert!(close(mm().parse_length("2.5 cm").unwrap(), 25.0));
    assert!(close(inches().parse_length("10 mm").unwrap(), 10.0));
    assert!(close(inches().parse_length("10MM").unwrap(), 10.0));
}

#[test]
fn either_separator_is_accepted_when_typing() {
    assert!(close(mm().parse_length("12,5").unwrap(), 12.5));
    assert!(close(mm_comma().parse_length("12.5").unwrap(), 12.5));
}

#[test]
fn ambiguous_or_invalid_text_is_not_a_length() {
    for bad in [
        "",
        "  ",
        "abc",
        "1,234.5",
        "1.2.3",
        "12 furlongs",
        "--3",
        "1e400",
    ] {
        assert_eq!(mm().parse_length(bad), None, "{bad:?}");
    }
}

#[test]
fn signs_and_surrounding_space_are_fine() {
    assert!(close(mm().parse_length("  -3.5 mm ").unwrap(), -3.5));
    assert!(close(mm().parse_length("+4").unwrap(), 4.0));
}

#[test]
fn durations_read_naturally() {
    assert_eq!(format_duration_ms(0), "0 s");
    assert_eq!(format_duration_ms(12_000), "12 s");
    assert_eq!(format_duration_ms(59_400), "59 s");
    assert_eq!(format_duration_ms(60_000), "1 min 00 s");
    assert_eq!(format_duration_ms(185_000), "3 min 05 s");
    assert_eq!(format_duration_ms(3_600_000), "1 h 00 min");
    assert_eq!(format_duration_ms(3_725_000), "1 h 02 min");
}

#[test]
fn percentages_round_to_whole_numbers() {
    assert_eq!(format_percent(0.0), "0%");
    assert_eq!(format_percent(0.456), "46%");
    assert_eq!(format_percent(1.0), "100%");
    assert_eq!(format_percent(1.7), "100%");
    assert_eq!(format_percent(-1.0), "0%");
}
