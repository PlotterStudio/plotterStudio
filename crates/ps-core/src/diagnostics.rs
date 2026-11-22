//! Diagnostics: problems found while editing, compiling or exporting.
//!
//! A diagnostic carries a localisation key plus arguments rather than finished text,
//! and an optional location so the canvas can draw a badge. Collections sort deterministically
//! so compiler output never depends on the order problems were discovered.

use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::grid::PtUm;
use crate::ids::{MatId, ObjectId, PathKey};

/// A key naming a localisable message (a Fluent message id).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct L10nKey(Cow<'static, str>);

impl L10nKey {
    /// Creates a key from a static or owned string.
    #[must_use]
    pub fn new(key: impl Into<Cow<'static, str>>) -> Self {
        Self(key.into())
    }

    /// Returns the key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for L10nKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How serious a diagnostic is. Ordered from least to most severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Worth knowing; never blocks anything.
    Info,
    /// Something is probably wrong; export is still allowed.
    Warning,
    /// A blocking problem; export is refused.
    Error,
}

/// Where a diagnostic applies, so the UI can link to it.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Location {
    /// The mat concerned.
    pub mat: Option<MatId>,
    /// The object concerned.
    pub object: Option<ObjectId>,
    /// The path concerned.
    pub path: Option<PathKey>,
    /// A point on the mat, in µm.
    pub at: Option<PtUm>,
}

impl Location {
    /// A location naming an object on a mat.
    #[must_use]
    pub fn object(mat: MatId, object: ObjectId) -> Self {
        Self {
            mat: Some(mat),
            object: Some(object),
            ..Self::default()
        }
    }

    /// Adds a point on the mat.
    #[must_use]
    pub fn with_point(mut self, at: PtUm) -> Self {
        self.at = Some(at);
        self
    }

    /// Adds a path.
    #[must_use]
    pub fn with_path(mut self, path: PathKey) -> Self {
        self.path = Some(path);
        self
    }
}

/// One problem report.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Diagnostic {
    /// How serious it is.
    pub severity: Severity,
    /// The message to show, as a localisation key.
    pub key: L10nKey,
    /// Values substituted into the message, in key order.
    pub args: BTreeMap<String, String>,
    /// Where it applies, if anywhere in particular.
    pub location: Option<Location>,
}

impl Diagnostic {
    fn with_severity(severity: Severity, key: L10nKey) -> Self {
        Self {
            severity,
            key,
            args: BTreeMap::new(),
            location: None,
        }
    }

    /// Creates an informational diagnostic.
    #[must_use]
    pub fn info(key: L10nKey) -> Self {
        Self::with_severity(Severity::Info, key)
    }

    /// Creates a warning.
    #[must_use]
    pub fn warning(key: L10nKey) -> Self {
        Self::with_severity(Severity::Warning, key)
    }

    /// Creates a blocking error.
    #[must_use]
    pub fn error(key: L10nKey) -> Self {
        Self::with_severity(Severity::Error, key)
    }

    /// Adds a message argument.
    #[must_use]
    pub fn with_arg(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.args.insert(name.into(), value.into());
        self
    }

    /// Sets the location.
    #[must_use]
    pub fn at(mut self, location: Location) -> Self {
        self.location = Some(location);
        self
    }
}

/// A collection of diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Diagnostics(Vec<Diagnostic>);

impl Diagnostics {
    /// Creates an empty collection.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a diagnostic.
    pub fn push(&mut self, d: Diagnostic) {
        self.0.push(d);
    }

    /// Returns the number of diagnostics.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true when there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns true when any diagnostic is an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.0.iter().any(|d| d.severity == Severity::Error)
    }

    /// Counts diagnostics of one severity.
    #[must_use]
    pub fn count(&self, severity: Severity) -> usize {
        self.0.iter().filter(|d| d.severity == severity).count()
    }

    /// Returns the highest severity present.
    #[must_use]
    pub fn max_severity(&self) -> Option<Severity> {
        self.0.iter().map(|d| d.severity).max()
    }

    /// Sorts most severe first, then by key, arguments and location.
    pub fn sort(&mut self) {
        self.0.sort_by(|a, b| {
            (Reverse(a.severity), &a.key, &a.args, &a.location).cmp(&(
                Reverse(b.severity),
                &b.key,
                &b.args,
                &b.location,
            ))
        });
    }

    /// Iterates in the current order.
    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.0.iter()
    }
}

impl FromIterator<Diagnostic> for Diagnostics {
    fn from_iter<I: IntoIterator<Item = Diagnostic>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl Extend<Diagnostic> for Diagnostics {
    fn extend<I: IntoIterator<Item = Diagnostic>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests;
