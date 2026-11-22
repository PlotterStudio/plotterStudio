//! Ground truth for conflict rules: keyed by id and semantic version, a newer version
//! updates in place (overlays kept); the same or older version asks the user to replace, keep
//! both (the copy gets a new id suffix) or skip.

use std::collections::BTreeSet;

use semver::Version;

use super::*;

fn v(text: &str) -> Version {
    text.parse().unwrap()
}

#[test]
fn a_new_id_just_installs() {
    assert_eq!(resolve(None, &v("1.0.0")), Resolution::Install);
}

#[test]
fn a_newer_version_updates_in_place() {
    assert_eq!(
        resolve(Some(&v("1.0.0")), &v("1.0.1")),
        Resolution::UpdateInPlace
    );
    assert_eq!(
        resolve(Some(&v("1.2.0")), &v("1.10.0")),
        Resolution::UpdateInPlace
    );
    assert_eq!(
        resolve(Some(&v("1.0.0-beta")), &v("1.0.0")),
        Resolution::UpdateInPlace
    );
}

#[test]
fn the_same_or_an_older_version_asks_the_user() {
    assert_eq!(resolve(Some(&v("1.0.0")), &v("1.0.0")), Resolution::AskUser);
    assert_eq!(resolve(Some(&v("2.0.0")), &v("1.9.9")), Resolution::AskUser);
}

#[test]
fn a_copy_gets_a_new_id_suffix() {
    let taken = BTreeSet::new();
    assert_eq!(
        keep_both_id("community.kraft", &taken),
        "community.kraft-copy"
    );
}

#[test]
fn copy_suffixes_count_up_until_free() {
    let mut taken: BTreeSet<String> = ["a".to_owned(), "a-copy".to_owned()].into();
    assert_eq!(keep_both_id("a", &taken), "a-copy-2");
    taken.insert("a-copy-2".to_owned());
    assert_eq!(keep_both_id("a", &taken), "a-copy-3");
}

#[test]
fn copy_ids_stay_valid_library_ids() {
    let id = keep_both_id("pen.fineliner-black-0.4", &BTreeSet::new());
    assert!(ps_core::LibRef::new(id, v("1.0.0")).is_ok());
}

#[test]
fn choices_map_to_actions() {
    assert_eq!(action(Resolution::Install, None), Action::Install);
    assert_eq!(
        action(Resolution::UpdateInPlace, None),
        Action::UpdateKeepingOverlay
    );
    assert_eq!(
        action(Resolution::AskUser, Some(Choice::Replace)),
        Action::ReplaceKeepingOverlay
    );
    assert_eq!(
        action(Resolution::AskUser, Some(Choice::KeepBoth)),
        Action::InstallCopy
    );
    assert_eq!(
        action(Resolution::AskUser, Some(Choice::Skip)),
        Action::Skip
    );
    // Without an answer a conflict is never resolved silently.
    assert_eq!(action(Resolution::AskUser, None), Action::NeedsAnswer);
}
