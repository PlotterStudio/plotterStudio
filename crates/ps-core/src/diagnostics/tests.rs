//! Ground truth for diagnostics: severity-ordered, located, localisable,
//! and deterministically sorted so compile output never depends on discovery order.

use super::*;
use crate::grid::PtUm;
use crate::ids::{MatId, ObjectId, PathKey};

#[test]
fn severity_orders_info_warning_error() {
    assert!(Severity::Info < Severity::Warning);
    assert!(Severity::Warning < Severity::Error);
}

#[test]
fn builders_set_severity_and_key() {
    let d = Diagnostic::warning(L10nKey::new("diag-outside-usable-area"));
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.key.as_str(), "diag-outside-usable-area");
    assert!(d.args.is_empty());
    assert_eq!(d.location, None);
    assert_eq!(
        Diagnostic::error(L10nKey::new("e")).severity,
        Severity::Error
    );
    assert_eq!(Diagnostic::info(L10nKey::new("i")).severity, Severity::Info);
}

#[test]
fn args_and_location_attach() {
    let mat = MatId::at_unix_ms(1_000);
    let obj = ObjectId::at_unix_ms(2_000);
    let d = Diagnostic::warning(L10nKey::new("diag-min-feature"))
        .with_arg("tool", "Fine-point blade")
        .with_arg("min_mm", "0.8")
        .at(Location::object(mat, obj).with_point(PtUm::new(1_000, 2_000)));
    assert_eq!(
        d.args.get("tool").map(String::as_str),
        Some("Fine-point blade")
    );
    // Args iterate in key order.
    let keys: Vec<_> = d.args.keys().map(String::as_str).collect();
    assert_eq!(keys, ["min_mm", "tool"]);
    let loc = d.location.unwrap();
    assert_eq!(loc.mat, Some(mat));
    assert_eq!(loc.object, Some(obj));
    assert_eq!(loc.at, Some(PtUm::new(1_000, 2_000)));
    assert_eq!(loc.path, None);
}

#[test]
fn location_can_name_a_path() {
    let loc = Location::object(MatId::at_unix_ms(1), ObjectId::at_unix_ms(2))
        .with_path(PathKey::source("p1"));
    assert_eq!(loc.path, Some(PathKey::source("p1")));
}

#[test]
fn l10n_key_serialises_as_a_string() {
    let k = L10nKey::new("cmd-undo");
    assert_eq!(serde_json::to_string(&k).unwrap(), "\"cmd-undo\"");
    assert_eq!(serde_json::from_str::<L10nKey>("\"cmd-undo\"").unwrap(), k);
    assert_eq!(k.to_string(), "cmd-undo");
}

#[test]
fn collection_counts_and_blocks_on_errors() {
    let mut set = Diagnostics::new();
    assert!(set.is_empty());
    assert!(!set.has_errors());
    set.push(Diagnostic::info(L10nKey::new("a")));
    set.push(Diagnostic::warning(L10nKey::new("b")));
    set.push(Diagnostic::warning(L10nKey::new("c")));
    assert!(!set.has_errors());
    assert_eq!(set.count(Severity::Warning), 2);
    assert_eq!(set.count(Severity::Info), 1);
    assert_eq!(set.count(Severity::Error), 0);
    set.push(Diagnostic::error(L10nKey::new("d")));
    assert!(set.has_errors());
    assert_eq!(set.len(), 4);
    assert_eq!(set.max_severity(), Some(Severity::Error));
}

#[test]
fn empty_collection_has_no_max_severity() {
    assert_eq!(Diagnostics::new().max_severity(), None);
}

#[test]
fn sorting_is_deterministic_and_most_severe_first() {
    let mut set = Diagnostics::new();
    set.push(Diagnostic::info(L10nKey::new("z")));
    set.push(Diagnostic::error(L10nKey::new("m")));
    set.push(Diagnostic::warning(L10nKey::new("b")));
    set.push(Diagnostic::error(L10nKey::new("a")));
    set.push(Diagnostic::warning(L10nKey::new("a")));
    set.sort();
    let order: Vec<_> = set
        .iter()
        .map(|d| (d.severity, d.key.as_str().to_owned()))
        .collect();
    assert_eq!(
        order,
        vec![
            (Severity::Error, "a".to_owned()),
            (Severity::Error, "m".to_owned()),
            (Severity::Warning, "a".to_owned()),
            (Severity::Warning, "b".to_owned()),
            (Severity::Info, "z".to_owned()),
        ]
    );
}

#[test]
fn sorting_is_independent_of_insertion_order() {
    let items = [
        Diagnostic::warning(L10nKey::new("x")).with_arg("n", "2"),
        Diagnostic::warning(L10nKey::new("x")).with_arg("n", "1"),
        Diagnostic::error(L10nKey::new("y")),
    ];
    let mut a: Diagnostics = items.iter().cloned().collect();
    let mut b: Diagnostics = items.iter().rev().cloned().collect();
    a.sort();
    b.sort();
    assert_eq!(a, b);
}

#[test]
fn extend_merges_collections() {
    let mut a = Diagnostics::new();
    a.push(Diagnostic::info(L10nKey::new("a")));
    let mut b = Diagnostics::new();
    b.push(Diagnostic::error(L10nKey::new("b")));
    a.extend(b);
    assert_eq!(a.len(), 2);
    assert!(a.has_errors());
}

#[test]
fn round_trips_through_json() {
    let d = Diagnostic::error(L10nKey::new("diag-missing-tool"))
        .with_arg("tool", "generic.drag-blade")
        .at(Location::default().with_point(PtUm::new(5, 6)));
    let json = serde_json::to_string(&d).unwrap();
    assert_eq!(serde_json::from_str::<Diagnostic>(&json).unwrap(), d);
}
