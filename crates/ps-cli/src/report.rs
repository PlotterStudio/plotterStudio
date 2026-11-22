//! Printing diagnostics: one stable line each, with the key scripts can match on, or JSON.

use std::fmt::Write as _;
use std::io::Write;

use ps_core::{Diagnostic, Diagnostics, Severity};
use serde_json::{Value, json};

fn severity_word(s: Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

/// `severity key arg=value ... @mat=… object=… path=…`
pub(crate) fn line(d: &Diagnostic) -> String {
    let mut out = format!("{} {}", severity_word(d.severity), d.key.as_str());
    for (name, value) in &d.args {
        let _ = write!(out, " {name}={value}");
    }
    if let Some(loc) = &d.location {
        let mut parts = Vec::new();
        if let Some(mat) = loc.mat {
            parts.push(format!("mat={mat:?}"));
        }
        if let Some(object) = loc.object {
            parts.push(format!("object={object:?}"));
        }
        if let Some(path) = &loc.path {
            parts.push(format!("path={path}"));
        }
        if let Some(at) = loc.at {
            parts.push(format!("at={},{}", at.x, at.y));
        }
        if !parts.is_empty() {
            let _ = write!(out, " @{}", parts.join(" "));
        }
    }
    out
}

/// The number of errors and warnings.
pub(crate) fn counts(diags: &Diagnostics) -> (usize, usize) {
    (diags.count(Severity::Error), diags.count(Severity::Warning))
}

/// Prints every diagnostic, one per line.
pub(crate) fn print(w: &mut dyn Write, diags: &Diagnostics) {
    for d in diags {
        let _ = writeln!(w, "{}", line(d));
    }
}

/// The diagnostics as a JSON array.
pub(crate) fn to_json(diags: &Diagnostics) -> Value {
    Value::Array(
        diags
            .iter()
            .map(|d| {
                json!({
                    "severity": severity_word(d.severity),
                    "key": d.key.as_str(),
                    "args": d.args,
                    "location": d.location.as_ref().map(|l| json!({
                        "mat": l.mat.map(|m| format!("{m:?}")),
                        "object": l.object.map(|o| format!("{o:?}")),
                        "path": l.path.as_ref().map(ToString::to_string),
                    })),
                })
            })
            .collect(),
    )
}
