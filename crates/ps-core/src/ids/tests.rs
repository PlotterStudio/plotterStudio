//! Ground truth for identifiers: `UUIDv7`, stable across saves, distinct types.

use super::*;

macro_rules! id_tests {
    ($mod_name:ident, $ty:ident) => {
        mod $mod_name {
            use super::*;

            #[test]
            fn new_ids_are_uuid_v7_and_unique() {
                let a = $ty::new();
                let b = $ty::new();
                assert_eq!(a.as_uuid().get_version_num(), 7);
                assert_ne!(a, b);
            }

            #[test]
            fn timestamp_is_recoverable() {
                let id = $ty::at_unix_ms(1_700_000_123_456);
                assert_eq!(id.unix_ms(), Some(1_700_000_123_456));
            }

            #[test]
            fn later_timestamps_sort_later() {
                let early = $ty::at_unix_ms(1_000);
                let late = $ty::at_unix_ms(2_000);
                assert!(early < late);
            }

            #[test]
            fn display_and_parse_round_trip() {
                let id = $ty::at_unix_ms(42_000);
                let text = id.to_string();
                assert_eq!(text.len(), 36);
                assert_eq!(text.parse::<$ty>().unwrap(), id);
            }

            #[test]
            fn bad_text_is_rejected() {
                assert!("not-a-uuid".parse::<$ty>().is_err());
                assert!("".parse::<$ty>().is_err());
            }

            #[test]
            fn serialises_as_a_plain_string() {
                let id = $ty::from_uuid("0192f3a4-7c1e-7b2a-9f00-3d5c1a2b4e6f".parse().unwrap());
                let json = serde_json::to_string(&id).unwrap();
                assert_eq!(json, "\"0192f3a4-7c1e-7b2a-9f00-3d5c1a2b4e6f\"");
                assert_eq!(serde_json::from_str::<$ty>(&json).unwrap(), id);
            }
        }
    };
}

id_tests!(object_id, ObjectId);
id_tests!(mat_id, MatId);
id_tests!(op_id, OpId);
id_tests!(asset_id, AssetId);
id_tests!(op_row_id, OpRowId);
id_tests!(group_id, GroupId);
id_tests!(attach_set_id, AttachSetId);

#[test]
fn ordered_maps_iterate_ids_in_creation_order() {
    let ids: Vec<_> = (1..=5).map(|i| ObjectId::at_unix_ms(i * 1_000)).collect();
    let mut shuffled = vec![ids[3], ids[0], ids[4], ids[2], ids[1]];
    shuffled.sort();
    assert_eq!(shuffled, ids);
}

mod path_key {
    use super::*;
    use crate::hashing::hash_bytes;

    #[test]
    fn source_key_displays_with_prefix_and_parses_back() {
        let key = PathKey::source("path1234");
        assert_eq!(key.to_string(), "src:path1234");
        assert_eq!("src:path1234".parse::<PathKey>().unwrap(), key);
    }

    #[test]
    fn source_ids_may_contain_colons() {
        let key = PathKey::source("layer:1:rect");
        assert_eq!(key.to_string().parse::<PathKey>().unwrap(), key);
    }

    #[test]
    fn fingerprint_key_displays_hex_and_parses_back() {
        let key = PathKey::fingerprint(hash_bytes(b""));
        assert_eq!(
            key.to_string(),
            "fp:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
        assert_eq!(key.to_string().parse::<PathKey>().unwrap(), key);
    }

    #[test]
    fn malformed_keys_are_rejected() {
        assert!("path1234".parse::<PathKey>().is_err());
        assert!("fp:zz".parse::<PathKey>().is_err());
        assert!("src:".parse::<PathKey>().is_err());
    }

    #[test]
    fn serialises_as_its_display_string() {
        let key = PathKey::source("a");
        assert_eq!(serde_json::to_string(&key).unwrap(), "\"src:a\"");
        assert_eq!(serde_json::from_str::<PathKey>("\"src:a\"").unwrap(), key);
    }

    #[test]
    fn keys_have_a_total_deterministic_order() {
        let mut keys = [
            PathKey::fingerprint(hash_bytes(b"x")),
            PathKey::source("b"),
            PathKey::source("a"),
        ];
        keys.sort();
        assert_eq!(keys[0], PathKey::source("a"));
        assert_eq!(keys[1], PathKey::source("b"));
    }
}
