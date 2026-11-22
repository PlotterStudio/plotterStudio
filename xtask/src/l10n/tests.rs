//! Ground truth for the localisation check: every locale offers the same messages as
//! the English source, each with the same placeholders, and no file repeats an id or fails to
//! parse.

use super::*;

const EN: &str = "title = Plotter Studio\nhello = Hello { $name }\ncount = { $n } items\n";

#[test]
fn matching_locales_have_no_problems() {
    let de = "title = Plotter Studio\nhello = Hallo { $name }\ncount = { $n } Objekte\n";
    assert_eq!(check(&[("en", EN), ("de", de)]), Vec::<String>::new());
}

#[test]
fn a_missing_message_is_reported_by_locale_and_id() {
    let de = "title = Plotter Studio\nhello = Hallo { $name }\n";
    let problems = check(&[("en", EN), ("de", de)]);
    assert_eq!(
        problems,
        vec!["de is missing the message `count`".to_owned()]
    );
}

#[test]
fn a_message_the_source_does_not_have_is_reported() {
    let de = "title = x\nhello = { $name }\ncount = { $n }\nextra = y\n";
    let problems = check(&[("en", EN), ("de", de)]);
    assert_eq!(
        problems,
        vec!["de has the message `extra`, which en does not".to_owned()]
    );
}

#[test]
fn a_placeholder_that_differs_is_reported() {
    let de = "title = x\nhello = Hallo { $person }\ncount = { $n }\n";
    let problems = check(&[("en", EN), ("de", de)]);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("hello")
            && problems[0].contains("name")
            && problems[0].contains("person"),
        "{problems:?}"
    );
}

#[test]
fn a_repeated_id_in_one_file_is_reported() {
    let en = "a = 1\na = 2\n";
    let problems = check(&[("en", en)]);
    assert_eq!(
        problems,
        vec!["en defines the message `a` more than once".to_owned()]
    );
}

#[test]
fn a_file_that_does_not_parse_is_reported() {
    let problems = check(&[("en", "title = fine\nbroken { = \n")]);
    assert!(
        problems.iter().any(|p| p.starts_with("en does not parse")),
        "{problems:?}"
    );
}

#[test]
fn multi_line_messages_and_comments_are_one_message_each() {
    let en = "# a comment\nlong =\n    First { $x }\n    second line\nnext = Next\n";
    let de = "long =\n    Erste { $x }\n    zweite\nnext = Weiter\n";
    assert_eq!(check(&[("en", en), ("de", de)]), Vec::<String>::new());
}

#[test]
fn terms_and_attributes_are_not_counted_as_messages() {
    let en = "-brand = Plotter\nname = { -brand }\n    .title = Title\n";
    let de = "-brand = Plotter
name = { -brand }
    .title = Titel
";
    assert_eq!(check(&[("en", en), ("de", de)]), Vec::<String>::new());
}

#[test]
fn the_first_locale_is_the_source() {
    // With no other locale there is nothing to compare, only the source's own checks.
    assert_eq!(check(&[("en", EN)]), Vec::<String>::new());
    assert_eq!(check(&[]), Vec::<String>::new());
}

#[test]
fn the_locales_folder_is_read_with_english_first() {
    let dir = tempfile::tempdir().unwrap();
    let locales = dir.path().join("crates/ps-app/locales");
    for (name, text) in [
        ("en", "a = A\nb = B\n"),
        ("de", "a = A\n"),
        ("fr", "a = A\nb = B\n"),
    ] {
        std::fs::create_dir_all(locales.join(name)).unwrap();
        std::fs::write(locales.join(name).join("main.ftl"), text).unwrap();
    }
    let problems = check_dir(dir.path()).unwrap();
    assert_eq!(problems, vec!["de is missing the message `b`".to_owned()]);
}

#[test]
fn a_repository_without_locales_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(check_dir(dir.path()).is_err());
}

#[test]
fn the_real_locales_are_consistent() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    assert_eq!(check_dir(root).unwrap(), Vec::<String>::new());
}
