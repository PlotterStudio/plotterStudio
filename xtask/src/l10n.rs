//! `cargo xtask l10n-check`: every locale under `crates/ps-app/locales` must offer the
//! same messages as English, with the same placeholders, and parse cleanly.

use std::collections::{BTreeMap, BTreeSet};

/// One message: its id and the placeholders it uses.
type Messages = Vec<(String, BTreeSet<String>)>;

fn is_id_start(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// The messages in a Fluent file, found line by line: a message starts at an unindented
/// `id =` line and runs through the indented lines after it. Terms (`-id`) and comments are not
/// messages.
fn messages(text: &str) -> Messages {
    let mut out: Messages = Vec::new();
    for line in text.lines() {
        let starts_message = line.chars().next().is_some_and(is_id_start) && line.contains('=');
        if starts_message {
            let id: String = line
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            out.push((id, BTreeSet::new()));
        } else if !line.starts_with(' ') && !line.is_empty() {
            // A term, a comment or a stray line ends the current message.
            out.push((String::new(), BTreeSet::new()));
        }
        if let Some((_, vars)) = out.last_mut() {
            let mut chars = line.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '$' {
                    let name: String = std::iter::from_fn(|| {
                        chars.next_if(|n| n.is_ascii_alphanumeric() || *n == '-' || *n == '_')
                    })
                    .collect();
                    if !name.is_empty() {
                        vars.insert(name);
                    }
                }
            }
        }
    }
    out.retain(|(id, _)| !id.is_empty());
    out
}

/// What is wrong across the locales; the first is the source the others are compared with.
pub(crate) fn check(locales: &[(&str, &str)]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut parsed: Vec<(&str, BTreeMap<String, BTreeSet<String>>)> = Vec::new();
    for (name, text) in locales {
        if let Err((_, errors)) = fluent_syntax::parser::parse(*text) {
            let first = errors
                .first()
                .map_or_else(String::new, |e| format!("{:?}", e.kind));
            problems.push(format!("{name} does not parse: {first}"));
            continue;
        }
        let mut map = BTreeMap::new();
        for (id, vars) in messages(text) {
            if map.insert(id.clone(), vars).is_some() {
                problems.push(format!("{name} defines the message `{id}` more than once"));
            }
        }
        parsed.push((name, map));
    }
    let Some(((source_name, source), rest)) = parsed.split_first() else {
        return problems;
    };
    for (name, map) in rest {
        for (id, vars) in source {
            match map.get(id) {
                None => problems.push(format!("{name} is missing the message `{id}`")),
                Some(have) if have != vars => problems.push(format!(
                    "{name} uses different placeholders in `{id}`: {source_name} has {vars:?}, {name} has {have:?}"
                )),
                Some(_) => {}
            }
        }
        for id in map.keys().filter(|id| !source.contains_key(*id)) {
            problems.push(format!(
                "{name} has the message `{id}`, which {source_name} does not"
            ));
        }
    }
    problems
}

/// Reads every locale under `root/crates/ps-app/locales` (English first) and checks them.
pub(crate) fn check_dir(root: &std::path::Path) -> anyhow::Result<Vec<String>> {
    use anyhow::Context;
    let folder = root.join("crates/ps-app/locales");
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .with_context(|| format!("reading {}", folder.display()))?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names.sort_by_key(|n| n != "en");
    let mut texts = Vec::new();
    for name in &names {
        let path = folder.join(name).join("main.ftl");
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        texts.push((name.clone(), text));
    }
    let locales: Vec<(&str, &str)> = texts
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();
    Ok(check(&locales))
}

#[cfg(test)]
mod tests;
