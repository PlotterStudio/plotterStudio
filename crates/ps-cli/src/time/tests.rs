//! Ground truth for the UTC timestamp the CLI stamps into a bundle's manifest (RFC 3339, whole
//! seconds). Reference values are well-known Unix times.

use super::*;

#[test]
fn the_epoch_is_the_first_of_january_1970() {
    assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
}

#[test]
fn well_known_instants() {
    assert_eq!(rfc3339_utc(1_000_000_000), "2001-09-09T01:46:40Z");
    assert_eq!(rfc3339_utc(1_700_000_000), "2023-11-14T22:13:20Z");
    assert_eq!(rfc3339_utc(2_000_000_000), "2033-05-18T03:33:20Z");
}

#[test]
fn leap_days_are_counted() {
    assert_eq!(
        rfc3339_utc(951_782_400),
        "2000-02-29T00:00:00Z",
        "2000 is a leap year"
    );
    assert_eq!(rfc3339_utc(951_868_800), "2000-03-01T00:00:00Z");
    assert_eq!(rfc3339_utc(1_078_099_200), "2004-03-01T00:00:00Z");
    assert_eq!(
        rfc3339_utc(4_107_542_400),
        "2100-03-01T00:00:00Z",
        "2100 is not a leap year"
    );
}

#[test]
fn the_last_second_of_a_year_rolls_over() {
    assert_eq!(rfc3339_utc(1_704_067_199), "2023-12-31T23:59:59Z");
    assert_eq!(rfc3339_utc(1_704_067_200), "2024-01-01T00:00:00Z");
}
