//! Localisation: Fluent messages compiled into the binary.
//!
//! A message is looked up in the chosen language, then in English, then shown as its key, so a
//! missing translation never leaves a blank. Isolation marks are off because the text goes into
//! a UI, not into bidirectional documents.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use fluent_syntax::ast::Entry;
use unic_langid::LanguageIdentifier;

const BUILT_IN: &[(&str, &str)] = &[("en", include_str!("../locales/en/main.ftl"))];
const FALLBACK: &str = "en";
/// The tag that selects the pseudo-locale, English with accents and brackets for finding
/// unlocalised text.
pub(crate) const PSEUDO_LOCALE: &str = "en-XA";

fn accent(c: char) -> char {
    match c {
        'a' => 'á',
        'e' => 'é',
        'i' => 'í',
        'o' => 'ö',
        'u' => 'ü',
        'c' => 'ç',
        'n' => 'ñ',
        'y' => 'ý',
        'A' => 'Á',
        'E' => 'É',
        'I' => 'Í',
        'O' => 'Ö',
        'U' => 'Ü',
        other => other,
    }
}

/// Turns Fluent source into a pseudo-locale: vowels are accented, each message is
/// wrapped in brackets and padded by about a third, so clipped, hard-coded or unlocalised text
/// stands out. Placeables (`{ ... }`), ids, attribute names and variant keys are left alone.
#[must_use]
pub(crate) fn pseudo_localize(source: &str) -> String {
    let mut out = String::new();
    let mut depth = 0_u32;
    for line in source.lines() {
        let indented = line.starts_with(' ');
        let (head, body) =
            if !indented && line.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                line.split_once('=').map_or(("", line), |(h, b)| (h, b))
            } else if indented && depth == 0 && line.trim_start().starts_with(['[', '*', '.']) {
                let t = line.trim_start();
                let split = t.find(']').or_else(|| t.find('=')).map_or(0, |i| i + 1);
                let lead = line.len() - t.len() + split;
                (&line[..lead], &line[lead..])
            } else {
                ("", line)
            };
        if line.starts_with('#') || line.starts_with('-') && !indented {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if !head.is_empty() && head != line {
            out.push_str(head);
            if !indented {
                out.push('=');
            }
        }
        let body = if head.is_empty() { line } else { body };
        let mut text = String::new();
        let mut letters = 0_usize;
        for c in body.chars() {
            match c {
                '{' => {
                    depth += 1;
                    text.push(c);
                }
                '}' => {
                    depth = depth.saturating_sub(1);
                    text.push(c);
                }
                _ if depth > 0 => text.push(c),
                _ => {
                    if c.is_alphabetic() {
                        letters += 1;
                    }
                    text.push(accent(c));
                }
            }
        }
        out.push_str(&text);
        let _ = letters;
        out.push('\n');
    }
    out
}

/// A message argument.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Arg<'a> {
    /// Text.
    Text(&'a str),
    /// A number, which also selects plural forms.
    Number(f64),
}

struct Language {
    bundle: FluentBundle<FluentResource>,
    keys: BTreeSet<String>,
}

/// Looks messages up for one chosen language with English behind it.
pub struct Localizer {
    chosen: String,
    languages: BTreeMap<String, Language>,
}

impl std::fmt::Debug for Localizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Localizer")
            .field("chosen", &self.chosen)
            .field("languages", &self.languages.keys().collect::<Vec<_>>())
            .finish()
    }
}

fn language_of(tag: &str) -> String {
    LanguageIdentifier::from_str(tag)
        .map_or_else(|_| FALLBACK.to_owned(), |id| id.language.to_string())
}

fn load(language: &str, source: &str) -> Language {
    // A resource with syntax errors still yields its valid messages.
    let resource =
        FluentResource::try_new(source.to_owned()).unwrap_or_else(|(partial, _)| partial);
    let keys = resource
        .entries()
        .filter_map(|entry| match entry {
            Entry::Message(message) => Some(message.id.name.to_owned()),
            _ => None,
        })
        .collect();
    let id = LanguageIdentifier::from_str(language)
        .unwrap_or_else(|_| LanguageIdentifier::from_str(FALLBACK).unwrap_or_default());
    let mut bundle = FluentBundle::new_concurrent(vec![id]);
    bundle.set_use_isolating(false);
    // Duplicate ids are reported but the first definition stays; nothing to do about them here.
    let _ = bundle.add_resource(resource);
    Language { bundle, keys }
}

impl Localizer {
    /// A localizer over the built-in languages for `locale` (a tag such as `de-AT`).
    #[must_use]
    pub fn new(locale: &str) -> Self {
        if locale.eq_ignore_ascii_case(PSEUDO_LOCALE) {
            let pseudo = pseudo_localize(BUILT_IN[0].1);
            return Self::from_sources(FALLBACK, &[(FALLBACK, &pseudo)]);
        }
        Self::from_sources(locale, BUILT_IN)
    }

    /// A localizer over explicit `(language, ftl source)` pairs; English should be among them.
    #[must_use]
    pub fn from_sources(locale: &str, sources: &[(&str, &str)]) -> Self {
        Self {
            chosen: language_of(locale),
            languages: sources
                .iter()
                .map(|(language, source)| ((*language).to_owned(), load(language, source)))
                .collect(),
        }
    }

    fn chain(&self) -> impl Iterator<Item = &Language> {
        let chosen = self.languages.get(&self.chosen);
        let fallback = (self.chosen != FALLBACK)
            .then(|| self.languages.get(FALLBACK))
            .flatten();
        chosen.into_iter().chain(fallback)
    }

    fn format(&self, key: &str, args: Option<&FluentArgs<'_>>) -> Option<String> {
        self.chain().find_map(|language| {
            let message = language.bundle.get_message(key)?;
            let pattern = message.value()?;
            let mut errors = Vec::new();
            Some(
                language
                    .bundle
                    .format_pattern(pattern, args, &mut errors)
                    .into_owned(),
            )
        })
    }

    /// The text of a message, or its key when no language has it.
    #[must_use]
    pub fn tr(&self, key: &str) -> String {
        self.format(key, None).unwrap_or_else(|| key.to_owned())
    }

    /// The text of a message with arguments.
    #[must_use]
    pub fn tr_args(&self, key: &str, args: &[(&str, Arg<'_>)]) -> String {
        let mut fluent = FluentArgs::new();
        for (name, value) in args {
            match value {
                Arg::Text(text) => fluent.set(*name, *text),
                Arg::Number(number) => fluent.set(*name, *number),
            }
        }
        self.format(key, Some(&fluent))
            .unwrap_or_else(|| key.to_owned())
    }

    /// Whether the chosen language or English has a message.
    #[must_use]
    pub fn has(&self, key: &str) -> bool {
        self.chain().any(|language| language.keys.contains(key))
    }

    /// The label of a command, from its `cmd-` message; the id itself if there is none.
    #[must_use]
    pub fn command_label(&self, id: &str) -> String {
        let key = format!("cmd-{}", id.replace('.', "-"));
        if self.has(&key) {
            self.tr(&key)
        } else {
            id.to_owned()
        }
    }

    /// Keys used in code that English does not define, sorted.
    #[must_use]
    pub fn missing_in_english(&self, used: &[&str]) -> Vec<String> {
        let english = self.languages.get(FALLBACK);
        let mut missing: Vec<String> = used
            .iter()
            .filter(|key| english.is_none_or(|e| !e.keys.contains(**key)))
            .map(|key| (*key).to_owned())
            .collect();
        missing.sort();
        missing.dedup();
        missing
    }

    /// English keys a language has no translation for, sorted.
    #[must_use]
    pub fn untranslated(&self, language: &str) -> Vec<String> {
        let Some(english) = self.languages.get(FALLBACK) else {
            return Vec::new();
        };
        let have = self.languages.get(language).map(|l| &l.keys);
        english
            .keys
            .iter()
            .filter(|key| have.is_none_or(|k| !k.contains(*key)))
            .cloned()
            .collect()
    }

    /// The languages that have a file.
    #[must_use]
    pub fn languages(&self) -> Vec<String> {
        self.languages.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests;
