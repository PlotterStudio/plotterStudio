//! Ground truth for pen lightness.
//! Reference values come from the sRGB / CIE definitions (D65).

use super::*;

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() < tol
}

#[test]
fn black_is_zero_and_white_is_one_hundred() {
    assert!(near(lightness([0, 0, 0]), 0.0, 1e-9));
    assert!(near(lightness([255, 255, 255]), 100.0, 1e-6));
}

#[test]
fn middle_grey_is_about_fifty() {
    // sRGB 119 is the classic L* = 50 grey.
    assert!(
        near(lightness([119, 119, 119]), 50.0, 0.1),
        "{}",
        lightness([119, 119, 119])
    );
}

#[test]
fn primaries_match_published_lightness() {
    // CIELAB L* of the sRGB primaries (D65): red 53.24, green 87.73, blue 32.30.
    assert!(near(lightness([255, 0, 0]), 53.24, 0.05));
    assert!(near(lightness([0, 255, 0]), 87.73, 0.05));
    assert!(near(lightness([0, 0, 255]), 32.30, 0.05));
}

#[test]
fn yellow_is_lighter_than_black_ink() {
    assert!(lightness([255, 230, 0]) > lightness([0x23, 0x1F, 0x20]));
}

#[test]
fn the_dark_linear_segment_is_continuous() {
    // Around the CIE threshold the two branches must meet.
    let a = lightness([6, 6, 6]);
    let b = lightness([7, 7, 7]);
    assert!(a < b && b - a < 1.0);
}
