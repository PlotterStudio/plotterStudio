//! Conflict rules for installing library entries.
//!
//! The same rules apply to tools, materials and profiles, keyed by id and semantic version.

use std::collections::BTreeSet;

use semver::Version;

/// What the version comparison alone decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Nothing with this id is installed.
    Install,
    /// The incoming version is newer: update in place, keeping overlays.
    UpdateInPlace,
    /// The same or an older version is installed: the user decides.
    AskUser,
}

/// The user's answer to a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Replace the installed entry.
    Replace,
    /// Install the incoming entry under a new id as well.
    KeepBoth,
    /// Leave things as they are.
    Skip,
}

/// What to do, once any answer is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Install as a new entry.
    Install,
    /// Update the installed entry and re-apply its overlay.
    UpdateKeepingOverlay,
    /// Replace the installed entry and re-apply its overlay.
    ReplaceKeepingOverlay,
    /// Install under a fresh id.
    InstallCopy,
    /// Do nothing.
    Skip,
    /// The user has not answered yet; nothing may be changed.
    NeedsAnswer,
}

/// Decides by version alone.
#[must_use]
pub fn resolve(existing: Option<&Version>, incoming: &Version) -> Resolution {
    match existing {
        None => Resolution::Install,
        Some(have) if incoming > have => Resolution::UpdateInPlace,
        Some(_) => Resolution::AskUser,
    }
}

/// Combines the version decision with the user's answer.
#[must_use]
pub fn action(resolution: Resolution, choice: Option<Choice>) -> Action {
    match (resolution, choice) {
        (Resolution::Install, _) => Action::Install,
        (Resolution::UpdateInPlace, _) => Action::UpdateKeepingOverlay,
        (Resolution::AskUser, Some(Choice::Replace)) => Action::ReplaceKeepingOverlay,
        (Resolution::AskUser, Some(Choice::KeepBoth)) => Action::InstallCopy,
        (Resolution::AskUser, Some(Choice::Skip)) => Action::Skip,
        (Resolution::AskUser, None) => Action::NeedsAnswer,
    }
}

/// An id for a kept-both copy: `<id>-copy`, then `<id>-copy-2`, `-3` … until it is free.
#[must_use]
pub fn keep_both_id(id: &str, taken: &BTreeSet<String>) -> String {
    let first = format!("{id}-copy");
    if !taken.contains(&first) {
        return first;
    }
    let mut n = 2_u32;
    loop {
        let candidate = format!("{first}-{n}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests;
