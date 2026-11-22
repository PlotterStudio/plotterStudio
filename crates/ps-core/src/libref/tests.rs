//! Ground truth for library references: `id@version`, as printed in the G-code
//! header (`; machine generic.cutter-medium@1.0.0`).

use super::*;
use semver::Version;

fn r(id: &str, v: &str) -> LibRef {
    LibRef::new(id, v.parse().unwrap()).unwrap()
}

#[test]
fn displays_as_id_at_version() {
    assert_eq!(
        r("generic.cutter-medium", "1.0.0").to_string(),
        "generic.cutter-medium@1.0.0"
    );
    assert_eq!(
        r("community.cardstock-80lb", "2.1.0").to_string(),
        "community.cardstock-80lb@2.1.0"
    );
}

#[test]
fn parses_the_display_form() {
    let parsed: LibRef = "pen.fineliner-black-0.4@1.2.3".parse().unwrap();
    assert_eq!(parsed.id(), "pen.fineliner-black-0.4");
    assert_eq!(*parsed.version(), Version::new(1, 2, 3));
}

#[test]
fn prereleases_round_trip() {
    let parsed: LibRef = "a.b@1.0.0-beta.2".parse().unwrap();
    assert_eq!(parsed.to_string(), "a.b@1.0.0-beta.2");
}

#[test]
fn rejects_malformed_references() {
    for bad in [
        "",
        "no-version",
        "@1.0.0",
        "id@",
        "id@1.0",
        "id@x.y.z",
        "a b@1.0.0",
        "a@b@1.0.0",
    ] {
        assert!(bad.parse::<LibRef>().is_err(), "{bad:?}");
    }
}

#[test]
fn ids_cannot_be_empty_or_contain_separators() {
    assert!(LibRef::new("", Version::new(1, 0, 0)).is_err());
    assert!(LibRef::new("has@at", Version::new(1, 0, 0)).is_err());
    assert!(LibRef::new("has space", Version::new(1, 0, 0)).is_err());
    assert!(LibRef::new("ok.id-1_x", Version::new(1, 0, 0)).is_ok());
}

#[test]
fn versions_compare_as_semver_not_as_text() {
    assert!(r("a", "1.10.0") > r("a", "1.2.0"));
    assert!(r("a", "2.0.0") > r("a", "1.99.99"));
    assert!(r("a", "1.0.0-alpha") < r("a", "1.0.0"));
}

#[test]
fn ordering_is_by_id_then_version() {
    let mut v = [r("b", "1.0.0"), r("a", "2.0.0"), r("a", "1.0.0")];
    v.sort();
    assert_eq!(
        v.iter().map(ToString::to_string).collect::<Vec<_>>(),
        ["a@1.0.0", "a@2.0.0", "b@1.0.0"]
    );
}

#[test]
fn same_id_ignores_the_version() {
    assert!(r("a", "1.0.0").same_id(&r("a", "9.0.0")));
    assert!(!r("a", "1.0.0").same_id(&r("b", "1.0.0")));
}

#[test]
fn serialises_as_one_string() {
    let json = serde_json::to_string(&r("generic.drag-blade", "1.0.0")).unwrap();
    assert_eq!(json, "\"generic.drag-blade@1.0.0\"");
    let back: LibRef = serde_json::from_str(&json).unwrap();
    assert_eq!(back, r("generic.drag-blade", "1.0.0"));
    assert!(serde_json::from_str::<LibRef>("\"nope\"").is_err());
}
