//! Ground truth for hashing: BLAKE3, with a canonical, platform-independent
//! feed so memo keys and input hashes never change between operating systems.

use super::*;

const EMPTY_BLAKE3: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";

#[test]
fn hash_of_empty_input_is_the_published_blake3_value() {
    assert_eq!(hash_bytes(b"").to_string(), EMPTY_BLAKE3);
}

#[test]
fn hex_round_trips() {
    let h = hash_bytes(b"plotter");
    let text = h.to_string();
    assert_eq!(text.len(), 64);
    assert_eq!(text.parse::<ContentHash>().unwrap(), h);
}

#[test]
fn bad_hex_is_rejected() {
    assert!("abc".parse::<ContentHash>().is_err());
    assert!(
        "zz49b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262af13"
            .parse::<ContentHash>()
            .is_err()
    );
}

#[test]
fn bytes_view_matches_the_digest() {
    let h = hash_bytes(b"");
    assert_eq!(h.as_bytes()[0], 0xaf);
    assert_eq!(h.as_bytes()[31], 0x62);
    assert_eq!(ContentHash::from_bytes(*h.as_bytes()), h);
}

#[test]
fn serialises_as_hex_string() {
    let h = hash_bytes(b"");
    assert_eq!(
        serde_json::to_string(&h).unwrap(),
        format!("\"{EMPTY_BLAKE3}\"")
    );
    assert_eq!(
        serde_json::from_str::<ContentHash>(&format!("\"{EMPTY_BLAKE3}\"")).unwrap(),
        h
    );
}

#[test]
fn integers_are_fed_little_endian() {
    let mut h = StableHasher::new();
    h.write_u32(1);
    assert_eq!(h.finish(), hash_bytes(&[1, 0, 0, 0]));

    let mut h = StableHasher::new();
    h.write_u64(0x0102_0304_0506_0708);
    assert_eq!(h.finish(), hash_bytes(&[8, 7, 6, 5, 4, 3, 2, 1]));

    let mut h = StableHasher::new();
    h.write_i64(-1);
    assert_eq!(h.finish(), hash_bytes(&[0xff; 8]));
}

#[test]
fn strings_are_length_prefixed() {
    let mut h = StableHasher::new();
    h.write_str("ab");
    let mut expected = 2_u64.to_le_bytes().to_vec();
    expected.extend_from_slice(b"ab");
    assert_eq!(h.finish(), hash_bytes(&expected));
}

#[test]
fn framing_prevents_concatenation_collisions() {
    let mut a = StableHasher::new();
    a.write_str("ab");
    a.write_str("c");
    let mut b = StableHasher::new();
    b.write_str("a");
    b.write_str("bc");
    assert_ne!(a.finish(), b.finish());
}

#[test]
fn bool_and_option_are_tagged() {
    let mut t = StableHasher::new();
    t.write_bool(true);
    assert_eq!(t.finish(), hash_bytes(&[1]));

    let mut none = StableHasher::new();
    none.write_option_u32(None);
    assert_eq!(none.finish(), hash_bytes(&[0]));

    let mut some = StableHasher::new();
    some.write_option_u32(Some(7));
    assert_eq!(some.finish(), hash_bytes(&[1, 7, 0, 0, 0]));
}

#[test]
fn floats_hash_by_bits_with_zero_and_nan_normalised() {
    let hash_f = |v: f64| {
        let mut h = StableHasher::new();
        h.write_f64(v);
        h.finish()
    };
    assert_eq!(hash_f(0.0), hash_f(-0.0));
    assert_eq!(hash_f(f64::NAN), hash_f(-f64::NAN));
    assert_eq!(
        hash_f(f64::NAN),
        hash_f(f64::from_bits(0x7ff8_0000_0000_0001))
    );
    assert_ne!(hash_f(1.0), hash_f(2.0));
    // 1.0 is 0x3FF0_0000_0000_0000.
    assert_eq!(
        hash_f(1.0),
        hash_bytes(&0x3ff0_0000_0000_0000_u64.to_le_bytes())
    );
}

#[test]
fn nested_hashes_and_slices_are_framed() {
    let inner = hash_bytes(b"x");
    let mut h = StableHasher::new();
    h.write_hash(&inner);
    assert_eq!(h.finish(), hash_bytes(inner.as_bytes()));

    let mut h = StableHasher::new();
    h.write_bytes(&[9, 9]);
    let mut expected = 2_u64.to_le_bytes().to_vec();
    expected.extend_from_slice(&[9, 9]);
    assert_eq!(h.finish(), hash_bytes(&expected));
}

#[test]
fn same_feed_gives_same_hash() {
    let build = || {
        let mut h = StableHasher::new();
        h.write_str("job");
        h.write_f64(0.34);
        h.write_u32(3);
        h.finish()
    };
    assert_eq!(build(), build());
}
