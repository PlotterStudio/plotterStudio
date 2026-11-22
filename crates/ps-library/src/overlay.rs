//! Overlays: a user's changes on top of an installed package, stored as a JSON Merge
//! Patch (RFC 7396) with a history. Package updates never erase tuned values because the patch
//! is re-applied to the new package.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Applies a JSON Merge Patch (RFC 7396) to `target`.
#[must_use]
pub fn merge_patch(target: Value, patch: &Value) -> Value {
    let Value::Object(patch_map) = patch else {
        return patch.clone();
    };
    let mut result = match target {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    for (key, value) in patch_map {
        if value.is_null() {
            result.remove(key);
        } else {
            let current = result.remove(key).unwrap_or(Value::Null);
            result.insert(key.clone(), merge_patch(current, value));
        }
    }
    Value::Object(result)
}

/// Builds the merge patch that turns `base` into `edited`.
///
/// Objects are compared key by key; any other difference replaces the whole value. A merge patch
/// cannot set a key to an explicit `null`, so such values are not representable.
#[must_use]
pub fn diff(base: &Value, edited: &Value) -> Value {
    match (base, edited) {
        (Value::Object(a), Value::Object(b)) => {
            let mut patch = Map::new();
            for key in a.keys().filter(|k| !b.contains_key(*k)) {
                patch.insert(key.clone(), Value::Null);
            }
            for (key, new) in b {
                match a.get(key) {
                    Some(old) if old == new => {}
                    Some(old) => {
                        patch.insert(key.clone(), diff(old, new));
                    }
                    None => {
                        patch.insert(key.clone(), new.clone());
                    }
                }
            }
            Value::Object(patch)
        }
        _ => edited.clone(),
    }
}

/// One change to an overlay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// When it happened (RFC 3339 text, supplied by the caller).
    pub when: String,
    /// Who made the change.
    pub who: String,
    /// The patch in force before the change.
    pub previous: Value,
}

/// A user's overlay on one library entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Overlay {
    /// The current merge patch (an empty object when there are no changes).
    pub patch: Value,
    /// Past states, oldest first.
    pub history: Vec<HistoryEntry>,
}

impl Default for Overlay {
    fn default() -> Self {
        Self::new()
    }
}

fn empty_patch() -> Value {
    Value::Object(Map::new())
}

impl Overlay {
    /// An overlay with no changes.
    #[must_use]
    pub fn new() -> Self {
        Self {
            patch: empty_patch(),
            history: Vec::new(),
        }
    }

    /// True when there are no changes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.patch == empty_patch()
    }

    /// The package value with the overlay applied.
    #[must_use]
    pub fn effective(&self, base: &Value) -> Value {
        merge_patch(base.clone(), &self.patch)
    }

    /// Records that the entry now reads `edited`: the patch becomes the difference from `base`.
    /// Does nothing (and records nothing) when the patch would not change.
    pub fn edit(&mut self, base: &Value, edited: &Value, when: &str, who: &str) {
        let patch = diff(base, edited);
        if patch == self.patch {
            return;
        }
        self.push(patch, when, who);
    }

    /// Drops all changes, going back to the package values. The old patch stays in the history.
    pub fn revert(&mut self, when: &str, who: &str) {
        if !self.is_empty() {
            self.push(empty_patch(), when, who);
        }
    }

    /// Goes back to the patch before the last change. Returns false when there is none.
    pub fn undo(&mut self) -> bool {
        match self.history.pop() {
            Some(entry) => {
                self.patch = entry.previous;
                true
            }
            None => false,
        }
    }

    fn push(&mut self, patch: Value, when: &str, who: &str) {
        let previous = std::mem::replace(&mut self.patch, patch);
        self.history.push(HistoryEntry {
            when: when.to_owned(),
            who: who.to_owned(),
            previous,
        });
    }
}

#[cfg(test)]
mod tests;
