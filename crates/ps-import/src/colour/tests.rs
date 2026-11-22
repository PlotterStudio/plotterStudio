//! Ground truth for colour matching: colours match exactly or within a CIEDE2000
//! tolerance. The Lab pairs and their expected differences come from the published CIEDE2000
//! test data (Sharma, Wu and Dalal, 2005), the standard check for implementations of the formula.

use super::*;

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() < tol
}

#[test]
fn published_reference_pairs_match_to_four_decimals() {
    let cases: [([f64; 3], [f64; 3], f64); 6] = [
        ([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485], 2.0425),
        ([50.0, 3.1571, -77.2803], [50.0, 0.0, -82.7485], 2.8615),
        ([50.0, 2.8361, -74.0200], [50.0, 0.0, -82.7485], 3.4412),
        ([50.0, -1.3802, -84.2814], [50.0, 0.0, -82.7485], 1.0000),
        ([50.0, 0.0, 0.0], [50.0, -1.0, 2.0], 2.3669),
        (
            [22.7233, 20.0904, -46.6940],
            [23.0331, 14.9730, -42.5619],
            2.0373,
        ),
    ];
    for (a, b, want) in cases {
        let got = delta_e2000_lab(a, b);
        assert!(near(got, want, 5e-4), "{a:?} vs {b:?}: {got} (want {want})");
    }
}

#[test]
fn the_difference_is_symmetric() {
    let (a, b) = ([60.2574, -34.0099, 36.2677], [60.4626, -34.1751, 39.4387]);
    assert!(near(delta_e2000_lab(a, b), delta_e2000_lab(b, a), 1e-9));
}

#[test]
fn identical_colours_have_no_difference() {
    assert!(near(delta_e2000([12, 200, 99], [12, 200, 99]), 0.0, 1e-9));
}

#[test]
fn black_and_white_are_one_hundred_apart() {
    assert!(near(delta_e2000([0, 0, 0], [255, 255, 255]), 100.0, 0.01));
}

#[test]
fn close_blues_are_closer_than_blue_and_red() {
    let near_blue = delta_e2000([0, 0, 255], [10, 10, 250]);
    let far = delta_e2000([0, 0, 255], [255, 0, 0]);
    assert!(near_blue < 5.0 && far > 40.0, "{near_blue} {far}");
}

#[test]
fn hex_colours_parse_in_short_and_long_form() {
    assert_eq!(parse_hex("#231F20"), Some([0x23, 0x1F, 0x20]));
    assert_eq!(parse_hex("231f20"), Some([0x23, 0x1F, 0x20]));
    assert_eq!(parse_hex("#0f8"), Some([0x00, 0xFF, 0x88]));
    assert_eq!(parse_hex("#12345"), None);
    assert_eq!(parse_hex("#GG0000"), None);
    assert_eq!(parse_hex(""), None);
}

#[test]
fn hex_colours_print_in_upper_case() {
    assert_eq!(to_hex([0x23, 0x1F, 0x0A]), "#231F0A");
}

#[test]
fn a_zero_tolerance_means_exact() {
    assert!(within([10, 20, 30], [10, 20, 30], 0.0));
    assert!(!within([10, 20, 30], [10, 20, 31], 0.0));
}

#[test]
fn a_tolerance_accepts_nearby_colours() {
    assert!(within([0, 0, 255], [10, 10, 250], 5.0));
    assert!(!within([0, 0, 255], [255, 0, 0], 5.0));
}

#[test]
fn the_nearest_colour_is_chosen_and_ties_go_to_the_first() {
    let palette = [[255, 0, 0], [0, 255, 0], [0, 0, 255]];
    assert_eq!(nearest([250, 10, 10], &palette), Some(0));
    assert_eq!(nearest([10, 10, 240], &palette), Some(2));
    assert_eq!(nearest([1, 2, 3], &[]), None);
    assert_eq!(nearest([5, 5, 5], &[[9, 9, 9], [9, 9, 9]]), Some(0));
}
