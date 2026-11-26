//! Ground truth for the command registry. One registry backs menus, shortcuts,
//! context menus and the command palette, so every action is reachable from the keyboard.
//! Default shortcuts follow vector editors (V select, A node, R rectangle, E ellipse, P pen,
//! arrows to nudge, Shift for 10x), with Cmd on macOS and Ctrl elsewhere. Users can rebind
//! shortcuts; conflicts are shown when assigning. The palette (Cmd/Ctrl+K) searches every command
//! by label in the current language.

use std::collections::BTreeMap;

use super::*;

fn registry() -> Registry {
    Registry::new(&BTreeMap::new())
}

fn labels(id: &str) -> String {
    // Stand-in for the localiser: "tool.select" -> "Select tool".
    match id {
        "tool.select" => "Select".to_owned(),
        "tool.node" => "Node tool".to_owned(),
        "tool.rectangle" => "Rectangle".to_owned(),
        "file.save" => "Save".to_owned(),
        "file.save-as" => "Save as…".to_owned(),
        other => other.to_owned(),
    }
}

fn all_enabled() -> Context {
    Context {
        has_selection: true,
        has_whole_objects: true,
        can_undo: true,
        can_redo: true,
        has_project: true,
        has_compile: true,
        device_ready: true,
    }
}

// ---- shortcuts ----

#[test]
fn a_shortcut_is_parsed_from_text() {
    let s = Shortcut::parse("Mod+Shift+Z").unwrap();
    assert!(s.command && s.shift && !s.alt);
    assert_eq!(s.key, "Z");
    let plain = Shortcut::parse("V").unwrap();
    assert!(!plain.command && !plain.shift && !plain.alt);
    assert_eq!(plain.key, "V");
}

#[test]
fn ctrl_cmd_and_mod_all_mean_the_command_key() {
    let a = Shortcut::parse("Ctrl+K").unwrap();
    let b = Shortcut::parse("Cmd+K").unwrap();
    let c = Shortcut::parse("Mod+K").unwrap();
    assert_eq!(a, b);
    assert_eq!(b, c);
}

#[test]
fn modifier_order_and_case_do_not_matter() {
    assert_eq!(
        Shortcut::parse("shift+mod+z"),
        Shortcut::parse("Mod+Shift+Z")
    );
    assert_eq!(
        Shortcut::parse("alt+shift+mod+left"),
        Shortcut::parse("Mod+Alt+Shift+Left")
    );
}

#[test]
fn bad_shortcuts_are_refused() {
    for bad in [
        "",
        "+",
        "Mod+",
        "Mod+Shift",
        "Mod+K+J",
        "Banana+K",
        "Mod++K",
    ] {
        assert_eq!(Shortcut::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn shortcuts_display_with_the_platforms_command_key() {
    let s = Shortcut::parse("Mod+Shift+Z").unwrap();
    assert_eq!(s.display(Platform::Windows), "Ctrl+Shift+Z");
    assert_eq!(s.display(Platform::Linux), "Ctrl+Shift+Z");
    assert_eq!(s.display(Platform::MacOs), "Cmd+Shift+Z");
    assert_eq!(
        Shortcut::parse("Delete")
            .unwrap()
            .display(Platform::Windows),
        "Delete"
    );
}

#[test]
fn displayed_text_parses_back_to_the_same_shortcut() {
    for text in ["Mod+K", "Shift+Left", "Mod+Alt+Shift+0", "V", "F6"] {
        let s = Shortcut::parse(text).unwrap();
        assert_eq!(
            Shortcut::parse(&s.display(Platform::Windows)),
            Some(s.clone()),
            "{text}"
        );
        assert_eq!(
            Shortcut::parse(&s.display(Platform::MacOs)),
            Some(s),
            "{text}"
        );
    }
}

#[test]
fn a_key_press_matches_the_shortcut_with_exactly_those_modifiers() {
    let s = Shortcut::parse("Mod+K").unwrap();
    assert!(s.matches(
        Mods {
            command: true,
            shift: false,
            alt: false
        },
        "K"
    ));
    assert!(!s.matches(
        Mods {
            command: false,
            shift: false,
            alt: false
        },
        "K"
    ));
    assert!(
        !s.matches(
            Mods {
                command: true,
                shift: true,
                alt: false
            },
            "K"
        ),
        "extra Shift"
    );
    assert!(!s.matches(
        Mods {
            command: true,
            shift: false,
            alt: false
        },
        "J"
    ));
    assert!(
        s.matches(
            Mods {
                command: true,
                shift: false,
                alt: false
            },
            "k"
        ),
        "key case is ignored"
    );
}

// ---- the registry ----

#[test]
fn command_ids_are_unique_and_every_command_has_a_label_key() {
    let r = registry();
    let mut ids: Vec<&str> = r.all().iter().map(|c| c.id).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total);
    assert!(
        r.all()
            .iter()
            .all(|c| c.label_key == format!("cmd-{}", c.id.replace('.', "-")))
    );
}

#[test]
fn the_vector_editor_shortcuts_are_bound() {
    let r = registry();
    for (key, id) in [
        ("V", "tool.select"),
        ("A", "tool.node"),
        ("R", "tool.rectangle"),
        ("E", "tool.ellipse"),
        ("P", "tool.pen"),
    ] {
        let s = Shortcut::parse(key).unwrap();
        assert_eq!(r.find_by_shortcut(&s).map(|c| c.id), Some(id), "{key}");
    }
}

#[test]
fn arrows_nudge_and_shift_makes_it_ten_times_bigger() {
    let r = registry();
    assert_eq!(
        r.find_by_shortcut(&Shortcut::parse("Left").unwrap())
            .map(|c| c.id),
        Some("nudge.left")
    );
    assert_eq!(
        r.find_by_shortcut(&Shortcut::parse("Shift+Left").unwrap())
            .map(|c| c.id),
        Some("nudge.left-big")
    );
    assert_eq!(nudge_distance_mm("nudge.left", 0.5), Some((-0.5, 0.0)));
    assert_eq!(nudge_distance_mm("nudge.left-big", 0.5), Some((-5.0, 0.0)));
    assert_eq!(nudge_distance_mm("nudge.down", 1.0), Some((0.0, 1.0)));
    assert_eq!(nudge_distance_mm("nudge.up-big", 1.0), Some((0.0, -10.0)));
    assert_eq!(nudge_distance_mm("file.save", 1.0), None);
}

#[test]
fn the_palette_opens_with_mod_k_and_undo_redo_follow_convention() {
    let r = registry();
    assert_eq!(
        r.find_by_shortcut(&Shortcut::parse("Mod+K").unwrap())
            .map(|c| c.id),
        Some("palette.open")
    );
    assert_eq!(
        r.find_by_shortcut(&Shortcut::parse("Mod+Z").unwrap())
            .map(|c| c.id),
        Some("edit.undo")
    );
    assert_eq!(
        r.find_by_shortcut(&Shortcut::parse("Mod+Shift+Z").unwrap())
            .map(|c| c.id),
        Some("edit.redo")
    );
    assert_eq!(
        r.find_by_shortcut(&Shortcut::parse("Mod+S").unwrap())
            .map(|c| c.id),
        Some("file.save")
    );
}

#[test]
fn the_defaults_have_no_conflicts() {
    assert_eq!(
        registry().conflicts(),
        Vec::<(Shortcut, Vec<&'static str>)>::new()
    );
}

#[test]
fn the_three_modes_and_the_panel_cycle_key_exist() {
    let r = registry();
    for id in [
        "mode.prepare",
        "mode.preview",
        "mode.export",
        "focus.next-panel",
    ] {
        assert!(r.get(id).is_some(), "{id}");
    }
    assert_eq!(
        r.find_by_shortcut(&Shortcut::parse("F6").unwrap())
            .map(|c| c.id),
        Some("focus.next-panel")
    );
}

#[test]
fn a_user_override_replaces_the_default_shortcut() {
    let mut overrides = BTreeMap::new();
    overrides.insert("tool.select".to_owned(), "Q".to_owned());
    let r = Registry::new(&overrides);
    assert_eq!(r.shortcut_of("tool.select"), Shortcut::parse("Q"));
    assert!(
        r.find_by_shortcut(&Shortcut::parse("V").unwrap()).is_none(),
        "V is free now"
    );
}

#[test]
fn an_empty_override_unbinds_the_command() {
    let mut overrides = BTreeMap::new();
    overrides.insert("tool.select".to_owned(), String::new());
    let r = Registry::new(&overrides);
    assert_eq!(r.shortcut_of("tool.select"), None);
}

#[test]
fn an_unparseable_or_unknown_override_is_ignored() {
    let mut overrides = BTreeMap::new();
    overrides.insert("tool.select".to_owned(), "Banana+Q".to_owned());
    overrides.insert("no.such.command".to_owned(), "Q".to_owned());
    let r = Registry::new(&overrides);
    assert_eq!(r.shortcut_of("tool.select"), Shortcut::parse("V"));
}

#[test]
fn assigning_a_taken_shortcut_reports_the_conflict_and_changes_nothing() {
    let mut r = registry();
    let taken = Shortcut::parse("R").unwrap();
    assert_eq!(
        r.assign("tool.select", Some(taken.clone())),
        Err(AssignError::Conflict {
            with: "tool.rectangle"
        })
    );
    assert_eq!(r.shortcut_of("tool.select"), Shortcut::parse("V"));
    assert_eq!(r.shortcut_of("tool.rectangle"), Some(taken));
}

#[test]
fn assigning_a_free_shortcut_works_and_can_be_reverted() {
    let mut r = registry();
    let q = Shortcut::parse("Mod+Alt+Q").unwrap();
    assert_eq!(r.assign("tool.select", Some(q.clone())), Ok(()));
    assert_eq!(r.shortcut_of("tool.select"), Some(q));
    r.reset("tool.select");
    assert_eq!(r.shortcut_of("tool.select"), Shortcut::parse("V"));
}

#[test]
fn assigning_to_an_unknown_command_is_an_error() {
    let mut r = registry();
    assert_eq!(
        r.assign("nope", Shortcut::parse("Q")),
        Err(AssignError::UnknownCommand)
    );
}

#[test]
fn reassigning_a_commands_own_shortcut_is_not_a_conflict() {
    let mut r = registry();
    assert_eq!(r.assign("tool.select", Shortcut::parse("V")), Ok(()));
}

#[test]
fn only_changed_shortcuts_are_saved_as_overrides() {
    let mut r = registry();
    assert!(r.overrides().is_empty());
    r.assign("tool.select", Shortcut::parse("Mod+Alt+Q"))
        .unwrap();
    r.assign("tool.node", None).unwrap();
    let o = r.overrides();
    assert_eq!(o.len(), 2);
    assert_eq!(o["tool.select"], "Mod+Alt+Q");
    assert_eq!(o["tool.node"], "");
    // They load back to the same registry.
    let again = Registry::new(&o);
    assert_eq!(
        again.shortcut_of("tool.select"),
        Shortcut::parse("Mod+Alt+Q")
    );
    assert_eq!(again.shortcut_of("tool.node"), None);
}

// ---- availability ----

#[test]
fn commands_are_enabled_by_what_they_need() {
    let r = registry();
    let none = Context::default();
    assert!(!r.is_enabled("edit.undo", &none));
    assert!(!r.is_enabled("edit.delete", &none));
    assert!(!r.is_enabled("file.export", &none));
    assert!(!r.is_enabled("machine.send", &none));
    assert!(r.is_enabled("file.new", &none), "needs nothing");
    assert!(r.is_enabled("palette.open", &none));

    let all = all_enabled();
    for id in [
        "edit.undo",
        "edit.redo",
        "edit.delete",
        "file.export",
        "machine.send",
        "file.save",
    ] {
        assert!(r.is_enabled(id, &all), "{id}");
    }
}

#[test]
fn undo_and_redo_are_enabled_independently() {
    let r = registry();
    let ctx = Context {
        can_undo: true,
        ..Context::default()
    };
    assert!(r.is_enabled("edit.undo", &ctx));
    assert!(!r.is_enabled("edit.redo", &ctx));
}

// ---- the palette ----

#[test]
fn the_palette_lists_every_command_for_an_empty_query() {
    let r = registry();
    let hits = r.search("", &labels, &all_enabled());
    assert_eq!(
        hits.len(),
        r.all().len(),
        "every action is reachable from the keyboard"
    );
}

#[test]
fn searching_matches_the_label_in_any_case() {
    let r = registry();
    let hits = r.search("rect", &labels, &all_enabled());
    assert_eq!(hits.first().map(|h| h.id), Some("tool.rectangle"));
    let upper = r.search("RECT", &labels, &all_enabled());
    assert_eq!(upper.first().map(|h| h.id), Some("tool.rectangle"));
}

#[test]
fn a_prefix_match_ranks_above_a_substring_match() {
    let r = registry();
    // "Save" starts with "sav"; "Save as…" also does; a label merely containing it ranks lower.
    let hits = r.search("sav", &labels, &all_enabled());
    let ids: Vec<&str> = hits.iter().map(|h| h.id).collect();
    let save = ids.iter().position(|i| *i == "file.save").unwrap();
    let save_as = ids.iter().position(|i| *i == "file.save-as").unwrap();
    assert!(save < save_as, "the shorter label first: {ids:?}");
}

#[test]
fn disabled_commands_are_found_but_listed_after_enabled_ones() {
    let r = registry();
    let ctx = Context {
        has_selection: false,
        ..all_enabled()
    };
    let hits = r.search("", &labels, &ctx);
    let first_disabled = hits.iter().position(|h| !h.enabled).unwrap();
    assert!(hits[first_disabled..].iter().all(|h| !h.enabled));
    assert!(hits.iter().any(|h| h.id == "edit.delete" && !h.enabled));
}

#[test]
fn a_query_that_matches_nothing_gives_nothing() {
    assert_eq!(
        registry().search("zzzzqqq", &labels, &all_enabled()).len(),
        0
    );
}

#[test]
fn each_hit_carries_its_shortcut_for_display() {
    let r = registry();
    let hit = r
        .search("Select", &labels, &all_enabled())
        .into_iter()
        .find(|h| h.id == "tool.select")
        .unwrap();
    assert_eq!(hit.shortcut, Shortcut::parse("V"));
}

#[test]
fn surrounding_space_in_the_query_is_ignored() {
    let r = registry();
    let a = r.search("  rect ", &labels, &all_enabled());
    let b = r.search("rect", &labels, &all_enabled());
    assert_eq!(a, b);
}
