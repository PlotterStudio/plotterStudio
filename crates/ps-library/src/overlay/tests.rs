//! Ground truth for overlays: user changes stored as a JSON Merge Patch
//! (RFC 7396) over a package, with history so "revert to package values" is one action.
//! The first block is the RFC's own example table (appendix A).

use proptest::prelude::*;
use serde_json::{Value, json};

use super::*;

fn j(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn rfc_7396_appendix_a_examples() {
    let cases = [
        (r#"{"a":"b"}"#, r#"{"a":"c"}"#, r#"{"a":"c"}"#),
        (r#"{"a":"b"}"#, r#"{"b":"c"}"#, r#"{"a":"b","b":"c"}"#),
        (r#"{"a":"b"}"#, r#"{"a":null}"#, r"{}"),
        (r#"{"a":"b","b":"c"}"#, r#"{"a":null}"#, r#"{"b":"c"}"#),
        (r#"{"a":["b"]}"#, r#"{"a":"c"}"#, r#"{"a":"c"}"#),
        (r#"{"a":"c"}"#, r#"{"a":["b"]}"#, r#"{"a":["b"]}"#),
        (
            r#"{"a":{"b":"c"}}"#,
            r#"{"a":{"b":"d","c":null}}"#,
            r#"{"a":{"b":"d"}}"#,
        ),
        (r#"{"a":[{"b":"c"}]}"#, r#"{"a":[1]}"#, r#"{"a":[1]}"#),
        (r#"["a","b"]"#, r#"["c","d"]"#, r#"["c","d"]"#),
        (r#"{"a":"b"}"#, r#"["c"]"#, r#"["c"]"#),
        (r#"{"a":"foo"}"#, "null", "null"),
        (r#"{"a":"foo"}"#, r#""bar""#, r#""bar""#),
        (r#"{"e":null}"#, r#"{"a":1}"#, r#"{"e":null,"a":1}"#),
        (r"[1,2]", r#"{"a":"b","c":null}"#, r#"{"a":"b"}"#),
        (r"{}", r#"{"a":{"bb":{"ccc":null}}}"#, r#"{"a":{"bb":{}}}"#),
    ];
    for (target, patch, result) in cases {
        assert_eq!(
            merge_patch(j(target), &j(patch)),
            j(result),
            "{target} + {patch}"
        );
    }
}

#[test]
fn diff_of_equal_values_is_an_empty_patch() {
    let v = json!({"a": 1, "b": {"c": [1, 2]}});
    assert_eq!(diff(&v, &v), json!({}));
}

#[test]
fn diff_names_only_what_changed() {
    let base = json!({"force": 210, "speed": 100, "name": "Kraft"});
    let edited = json!({"force": 250, "speed": 100, "name": "Kraft"});
    assert_eq!(diff(&base, &edited), json!({"force": 250}));
}

#[test]
fn diff_marks_removed_keys_with_null() {
    let base = json!({"a": 1, "b": 2});
    let edited = json!({"a": 1});
    assert_eq!(diff(&base, &edited), json!({"b": null}));
}

#[test]
fn diff_recurses_into_objects_but_replaces_arrays() {
    let base = json!({"geometry": {"x": 1, "y": 2}, "ops": ["cut"]});
    let edited = json!({"geometry": {"x": 1, "y": 3}, "ops": ["cut", "score"]});
    assert_eq!(
        diff(&base, &edited),
        json!({"geometry": {"y": 3}, "ops": ["cut", "score"]})
    );
}

#[test]
fn applying_a_diff_reproduces_the_edit() {
    let base = json!({"a": {"b": 1, "c": 2}, "d": [1], "e": "x"});
    let edited = json!({"a": {"b": 5}, "d": [2], "f": true});
    assert_eq!(merge_patch(base.clone(), &diff(&base, &edited)), edited);
}

fn arb_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Bool(true)),
        (-100_i64..100).prop_map(|n| json!(n)),
        "[a-c]{0,3}".prop_map(Value::String),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        prop::collection::btree_map("[a-d]", inner, 0..4)
            .prop_map(|m| Value::Object(m.into_iter().collect()))
    })
}

proptest! {
    /// For any two objects, patching the first with their diff gives the second.
    #[test]
    fn diff_then_patch_round_trips(a in arb_json(), b in arb_json()) {
        // Merge patch cannot express an explicit null value, so skip documents that hold one.
        prop_assume!(!a.to_string().contains("null") && !b.to_string().contains("null"));
        prop_assert_eq!(merge_patch(a.clone(), &diff(&a, &b)), b);
    }
}

// ---- overlays with history ----

#[test]
fn a_new_overlay_changes_nothing() {
    let base = json!({"force": 210});
    let o = Overlay::new();
    assert!(o.is_empty());
    assert_eq!(o.effective(&base), base);
}

#[test]
fn editing_stores_only_the_difference() {
    let base = json!({"force": 210, "speed": 100});
    let mut o = Overlay::new();
    o.edit(
        &base,
        &json!({"force": 250, "speed": 100}),
        "2026-11-17T10:00:00Z",
        "ilya",
    );
    assert_eq!(o.patch, json!({"force": 250}));
    assert_eq!(o.effective(&base), json!({"force": 250, "speed": 100}));
    assert!(!o.is_empty());
}

#[test]
fn history_records_who_when_and_the_previous_patch() {
    let base = json!({"force": 210});
    let mut o = Overlay::new();
    o.edit(&base, &json!({"force": 230}), "t1", "ilya");
    o.edit(&base, &json!({"force": 250}), "t2", "ilya");
    assert_eq!(o.history.len(), 2);
    assert_eq!(o.history[0].previous, json!({}));
    assert_eq!(o.history[1].previous, json!({"force": 230}));
    assert_eq!(
        (o.history[1].when.as_str(), o.history[1].who.as_str()),
        ("t2", "ilya")
    );
}

#[test]
fn an_edit_that_changes_nothing_adds_no_history() {
    let base = json!({"force": 210});
    let mut o = Overlay::new();
    o.edit(&base, &base, "t1", "ilya");
    assert!(o.is_empty());
    assert_eq!(o.history.len(), 0);
}

#[test]
fn revert_goes_back_to_the_package_values_in_one_action() {
    let base = json!({"force": 210, "speed": 100});
    let mut o = Overlay::new();
    o.edit(&base, &json!({"force": 250, "speed": 90}), "t1", "ilya");
    o.revert("t2", "ilya");
    assert!(o.is_empty());
    assert_eq!(o.effective(&base), base);
    // Reverting is itself recorded, so it can be undone.
    assert_eq!(
        o.history.last().unwrap().previous,
        json!({"force": 250, "speed": 90})
    );
}

#[test]
fn undo_restores_the_previous_patch() {
    let base = json!({"force": 210});
    let mut o = Overlay::new();
    o.edit(&base, &json!({"force": 230}), "t1", "a");
    o.edit(&base, &json!({"force": 250}), "t2", "a");
    assert!(o.undo());
    assert_eq!(o.effective(&base), json!({"force": 230}));
    assert!(o.undo());
    assert!(o.is_empty());
    assert!(!o.undo());
}

#[test]
fn an_overlay_survives_a_package_update() {
    // The user tuned the force; the package then gains a new key. Both show up.
    let v1 = json!({"force": 210, "speed": 100});
    let mut o = Overlay::new();
    o.edit(&v1, &json!({"force": 250, "speed": 100}), "t1", "a");
    let v2 = json!({"force": 210, "speed": 100, "passes": 2});
    assert_eq!(
        o.effective(&v2),
        json!({"force": 250, "speed": 100, "passes": 2})
    );
}

#[test]
fn overlays_serialise_to_json_and_back() {
    let base = json!({"force": 210});
    let mut o = Overlay::new();
    o.edit(&base, &json!({"force": 250}), "t1", "a");
    let text = serde_json::to_string(&o).unwrap();
    assert_eq!(serde_json::from_str::<Overlay>(&text).unwrap(), o);
}
