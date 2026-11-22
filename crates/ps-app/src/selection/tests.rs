//! Ground truth for the selection model: a set of (object, optional path, optional node)
//! kept in app state, not in the document. Selection changes are not undo steps.

use ps_core::{ObjectId, PathKey};

use super::*;

fn id(ms: u64) -> ObjectId {
    ObjectId::at_unix_ms(ms)
}

#[test]
fn a_new_selection_is_empty() {
    let s = Selection::default();
    assert_eq!(s.len(), 0);
    assert_eq!(s.objects().len(), 0);
}

#[test]
fn selecting_an_object_replaces_the_selection() {
    let (a, b) = (id(1), id(2));
    let mut s = Selection::default();
    s.select_object(a);
    s.select_object(b);
    assert_eq!(s.objects(), vec![b]);
}

#[test]
fn adding_keeps_what_was_selected() {
    let (a, b) = (id(1), id(2));
    let mut s = Selection::default();
    s.select_object(a);
    s.add_object(b);
    let mut got = s.objects();
    got.sort();
    assert_eq!(got, vec![a, b]);
}

#[test]
fn toggling_adds_then_removes() {
    let a = id(1);
    let mut s = Selection::default();
    s.toggle_object(a);
    assert!(s.contains_object(a));
    s.toggle_object(a);
    assert!(!s.contains_object(a));
}

#[test]
fn clearing_empties_it() {
    let mut s = Selection::default();
    s.select_object(id(1));
    s.clear();
    assert!(s.is_empty());
}

#[test]
fn several_paths_of_one_object_count_as_one_selected_object() {
    let a = id(1);
    let mut s = Selection::default();
    s.add(Item {
        object: a,
        path: Some(PathKey::source("p1")),
        node: None,
    });
    s.add(Item {
        object: a,
        path: Some(PathKey::source("p2")),
        node: None,
    });
    assert_eq!(s.len(), 2);
    assert_eq!(s.objects(), vec![a]);
    assert!(s.contains_object(a));
}

#[test]
fn selecting_a_node_remembers_its_path_and_index() {
    let a = id(1);
    let mut s = Selection::default();
    s.select(Item {
        object: a,
        path: Some(PathKey::source("p1")),
        node: Some(3),
    });
    let items: Vec<&Item> = s.items().collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].node, Some(3));
}

#[test]
fn adding_the_same_item_twice_keeps_one() {
    let a = id(1);
    let mut s = Selection::default();
    s.add_object(a);
    s.add_object(a);
    assert_eq!(s.len(), 1);
}

#[test]
fn objects_that_no_longer_exist_are_dropped() {
    let (a, b, c) = (id(1), id(2), id(3));
    let mut s = Selection::default();
    s.add_object(a);
    s.add_object(b);
    s.add_object(c);
    s.retain_existing(|o| o != b);
    let mut got = s.objects();
    got.sort();
    assert_eq!(got, vec![a, c]);
}

#[test]
fn replacing_with_a_list_selects_exactly_those() {
    let (a, b, c) = (id(1), id(2), id(3));
    let mut s = Selection::default();
    s.add_object(a);
    s.set_objects(&[b, c]);
    let mut got = s.objects();
    got.sort();
    assert_eq!(got, vec![b, c]);
    s.set_objects(&[]);
    assert!(s.is_empty());
}

#[test]
fn a_selection_knows_whether_it_changed() {
    let a = id(1);
    let mut s = Selection::default();
    assert!(s.select_object(a), "from empty to one is a change");
    assert!(
        !s.select_object(a),
        "selecting what is already the whole selection is not"
    );
    // Ids made at the same millisecond still differ, so make the second one once.
    let b = id(2);
    assert!(s.add_object(b));
    assert!(!s.add_object(b));
}
