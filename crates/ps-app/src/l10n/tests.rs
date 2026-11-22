//! Ground truth for localisation: all strings are Fluent messages compiled into the
//! binary; the locale comes from the system and can be overridden. A CI check fails if a message
//! is used in code but missing from the English file, and reports untranslated keys per language.

use std::collections::BTreeMap;

use super::*;
use crate::Registry;

const EN: &str = "\
hello = Hello
greeting = Hello, { $name }!
paths = { $count ->
    [one] { $count } path
   *[other] { $count } paths
}
only-english = Only here
";

const DE: &str = "\
hello = Hallo
greeting = Hallo, { $name }!
paths = { $count ->
    [one] { $count } Pfad
   *[other] { $count } Pfade
}
";

fn loc(locale: &str) -> Localizer {
    Localizer::from_sources(locale, &[("en", EN), ("de", DE)])
}

#[test]
fn a_message_is_looked_up_in_the_chosen_language() {
    assert_eq!(loc("de").tr("hello"), "Hallo");
    assert_eq!(loc("en").tr("hello"), "Hello");
}

#[test]
fn a_regional_tag_falls_back_to_its_language() {
    assert_eq!(loc("de-AT").tr("hello"), "Hallo");
    assert_eq!(loc("en-GB").tr("hello"), "Hello");
}

#[test]
fn an_unknown_language_falls_back_to_english() {
    assert_eq!(loc("xx").tr("hello"), "Hello");
    assert_eq!(loc("system-default-garbage!!").tr("hello"), "Hello");
}

#[test]
fn a_message_missing_in_the_language_falls_back_to_english() {
    assert_eq!(loc("de").tr("only-english"), "Only here");
}

#[test]
fn a_message_missing_everywhere_shows_its_key() {
    assert_eq!(loc("de").tr("no-such-message"), "no-such-message");
}

#[test]
fn arguments_are_substituted_without_bidi_marks() {
    let text = loc("en").tr_args("greeting", &[("name", Arg::Text("Ada"))]);
    assert_eq!(text, "Hello, Ada!");
    assert!(!text.contains('\u{2068}') && !text.contains('\u{2069}'));
}

#[test]
fn plural_forms_follow_the_language() {
    let one = loc("en").tr_args("paths", &[("count", Arg::Number(1.0))]);
    let many = loc("en").tr_args("paths", &[("count", Arg::Number(3.0))]);
    assert_eq!(one, "1 path");
    assert_eq!(many, "3 paths");
    assert_eq!(
        loc("de").tr_args("paths", &[("count", Arg::Number(2.0))]),
        "2 Pfade"
    );
}

#[test]
fn a_missing_argument_does_not_panic() {
    let text = loc("en").tr("greeting");
    assert!(text.starts_with("Hello"), "{text}");
}

#[test]
fn has_says_whether_a_key_exists_in_the_chain() {
    let l = loc("de");
    assert!(l.has("hello"));
    assert!(l.has("only-english"), "found through the English fallback");
    assert!(!l.has("nope"));
}

#[test]
fn keys_used_in_code_but_missing_from_english_are_reported() {
    let l = loc("en");
    assert_eq!(
        l.missing_in_english(&["hello", "nope", "paths", "also-nope"]),
        vec!["also-nope", "nope"]
    );
}

#[test]
fn untranslated_keys_are_listed_per_language() {
    let l = loc("en");
    assert_eq!(l.untranslated("de"), vec!["only-english"]);
    assert_eq!(l.untranslated("en"), Vec::<String>::new());
    assert_eq!(
        l.untranslated("fr").len(),
        4,
        "a language with no file has everything to do"
    );
}

#[test]
fn the_available_languages_are_listed() {
    let mut langs = loc("en").languages();
    langs.sort();
    assert_eq!(langs, vec!["de", "en"]);
}

#[test]
fn a_broken_file_does_not_take_the_app_down() {
    let l = Localizer::from_sources("en", &[("en", EN), ("de", "this is = {{ not valid")]);
    assert_eq!(l.tr("hello"), "Hello");
}

// ---- the shipped English file ----

#[test]
fn every_command_label_exists_in_the_built_in_english_file() {
    let l = Localizer::new("en");
    let missing: Vec<&str> = Registry::new(&BTreeMap::default())
        .all()
        .iter()
        .map(|c| c.label_key)
        .filter(|k| !l.has(k))
        .collect();
    assert_eq!(missing, Vec::<&str>::new());
}

#[test]
fn the_built_in_english_file_has_the_ui_basics() {
    let l = Localizer::new("en");
    for key in [
        "app-name",
        "mode-prepare",
        "mode-preview",
        "mode-export",
        "palette-placeholder",
        "warnings-count",
    ] {
        assert!(l.has(key), "{key}");
    }
    assert_eq!(l.tr("app-name"), "Plotter Studio");
}

#[test]
fn the_label_for_a_command_id_comes_from_its_key() {
    let l = Localizer::new("en");
    assert_eq!(l.command_label("file.save"), "Save");
    assert_eq!(l.command_label("tool.select"), "Select");
    assert_eq!(l.command_label("not.a.command"), "not.a.command");
}

// ---- the CI check: every message used in code exists in English ----

/// Message keys appear in the source as string literals that look like `some-key-name`.
fn message_keys_in(source: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut rest = source;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('"') else { break };
        let literal = &after[..end];
        let looks_like_key = literal.contains('-')
            && literal.len() > 3
            && literal
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !literal.starts_with('-')
            && !literal.ends_with('-');
        if looks_like_key {
            keys.push(literal.to_owned());
        }
        rest = &after[end + 1..];
    }
    keys.sort();
    keys.dedup();
    keys
}

#[test]
fn every_message_key_used_by_the_window_layer_is_in_the_english_file() {
    let l = Localizer::new("en");
    let mut used = Vec::new();
    for source in [
        include_str!("../ui.rs"),
        include_str!("../ui/extras.rs"),
        include_str!("../shell.rs"),
    ] {
        used.extend(message_keys_in(source));
    }
    assert!(!used.is_empty(), "the scan found the keys");
    let used: Vec<&str> = used.iter().map(String::as_str).collect();
    assert_eq!(l.missing_in_english(&used), Vec::<String>::new());
}

#[test]
fn the_key_scan_finds_keys_and_ignores_other_strings() {
    let found = message_keys_in(
        r#"loc.tr("mode-prepare"); let a = "Hello World"; x("snake_case"); y("a-b")"#,
    );
    assert_eq!(found, vec!["mode-prepare"]);
}

#[test]
fn every_operation_kind_has_a_localised_name() {
    let l = Localizer::new("en");
    for kind in ps_model::OpKind::ALL {
        assert!(l.has(&format!("op-{}", kind.as_str())), "{kind:?}");
    }
}

#[test]
fn the_pseudo_locale_accents_text_and_keeps_placeholders_and_ids() {
    let source = "greeting = Hello { $name }, you have { $count ->\n    [one] one file\n   *[other] { $count } files\n}\n# a comment\n";
    let out = pseudo_localize(source);
    assert!(out.starts_with("greeting = "), "{out}");
    assert!(out.contains("Héllö"), "{out}");
    assert!(
        out.contains("{ $name }") && out.contains("{ $count }"),
        "{out}"
    );
    assert!(out.contains("[one]") && out.contains("*[other]"), "{out}");
    assert!(out.contains("# a comment"), "{out}");
    // It is still valid Fluent with the same messages.
    let loc = Localizer::from_sources("en", &[("en", &out)]);
    assert!(loc.has("greeting"));
}

#[test]
fn the_whole_english_file_survives_pseudo_localisation() {
    let english = include_str!("../../locales/en/main.ftl");
    let pseudo = pseudo_localize(english);
    let a = Localizer::from_sources("en", &[("en", english)]);
    let b = Localizer::from_sources("en", &[("en", &pseudo)]);
    let mut checked = 0;
    for line in english
        .lines()
        .filter(|l| l.starts_with(|c: char| c.is_ascii_alphabetic()))
    {
        if let Some((id, _)) = line.split_once('=') {
            let id = id.trim();
            assert!(a.has(id) == b.has(id), "{id} lost");
            checked += 1;
        }
    }
    assert!(checked > 100);
}

#[test]
fn the_en_xa_tag_selects_the_pseudo_locale() {
    let normal = Localizer::new("en").tr("app-name");
    let pseudo = Localizer::new("en-XA").tr("app-name");
    assert_ne!(normal, pseudo);
}
