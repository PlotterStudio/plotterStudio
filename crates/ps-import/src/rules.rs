//! The mapping-rule engine.
//!
//! A rule is a predicate on source style and layer plus the operation to assign. Rules run in
//! order and the first match wins. A path no rule matches gets a warning, never a silent default.
//! `pen:auto` maps to the nearest owned pen by CIEDE2000 after an exact-match pass.
//!
//! The "colour" of a path is its stroke colour, or its fill colour when it has no stroke.

use globset::Glob;
use ps_core::{Diagnostic, Diagnostics, L10nKey};
use ps_model::{MappingRule, Match, OpKind, SourceStyle};

use crate::colour::{delta_e2000, nearest, parse_hex, within};

/// A pen the user owns, for `pen:auto`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedPen {
    /// The pen's tool id.
    pub tool_id: String,
    /// The ink colour.
    pub colour: [u8; 3],
}

/// What a path was mapped to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mapping {
    /// A rule matched.
    Assigned {
        /// Index of the rule that matched.
        rule: usize,
        /// The operation.
        op: OpKind,
        /// The tool id (the chosen pen for `pen:auto`).
        tool: String,
        /// The pen's colour when `pen:auto` chose it.
        pen_colour: Option<[u8; 3]>,
    },
    /// No rule matched.
    Unmapped,
    /// `pen:auto` matched but the user owns no pens.
    NoOwnedPen,
    /// `pen:auto` matched but the path has no colour to match.
    NoColour,
}

fn effective_colour(style: &SourceStyle) -> Option<[u8; 3]> {
    style.stroke.or(style.fill)
}

/// True when `style` satisfies every predicate that is set on `m`.
#[must_use]
pub fn matches(m: &Match, style: &SourceStyle) -> bool {
    if let Some(hex) = &m.stroke {
        let Some(wanted) = parse_hex(hex) else {
            return false;
        };
        let Some(actual) = effective_colour(style) else {
            return false;
        };
        if !within(actual, wanted, m.tolerance_de.unwrap_or(0.0)) {
            return false;
        }
    }
    if let Some(pattern) = &m.layer {
        let Some(layer) = &style.layer else {
            return false;
        };
        let Ok(glob) = Glob::new(pattern) else {
            return false;
        };
        if !glob.compile_matcher().is_match(layer) {
            return false;
        }
    }
    if let Some(filled) = m.filled
        && style.fill.is_some() != filled
    {
        return false;
    }
    if let Some(dashed) = m.dashed
        && style.dash.is_empty() == dashed
    {
        return false;
    }
    if m.min_width_mm.is_some() || m.max_width_mm.is_some() {
        let Some(width) = style.stroke_width_mm else {
            return false;
        };
        if m.min_width_mm.is_some_and(|min| width < min)
            || m.max_width_mm.is_some_and(|max| width > max)
        {
            return false;
        }
    }
    true
}

fn choose_pen(colour: [u8; 3], pens: &[OwnedPen]) -> Option<&OwnedPen> {
    // Exact-match pass first, then the nearest by CIEDE2000.
    if let Some(exact) = pens.iter().find(|p| p.colour == colour) {
        return Some(exact);
    }
    let palette: Vec<[u8; 3]> = pens.iter().map(|p| p.colour).collect();
    nearest(colour, &palette).map(|i| &pens[i])
}

/// Maps one path's style with the first matching rule.
#[must_use]
pub fn map_style(rules: &[MappingRule], style: &SourceStyle, pens: &[OwnedPen]) -> Mapping {
    for (index, rule) in rules.iter().enumerate() {
        if !matches(&rule.matcher, style) {
            continue;
        }
        if rule.assign.is_auto_pen() {
            let Some(colour) = effective_colour(style) else {
                return Mapping::NoColour;
            };
            let Some(pen) = choose_pen(colour, pens) else {
                return Mapping::NoOwnedPen;
            };
            return Mapping::Assigned {
                rule: index,
                op: rule.assign.op,
                tool: pen.tool_id.clone(),
                pen_colour: Some(pen.colour),
            };
        }
        return Mapping::Assigned {
            rule: index,
            op: rule.assign.op,
            tool: rule.assign.tool.clone(),
            pen_colour: None,
        };
    }
    Mapping::Unmapped
}

/// Maps every style, with one warning for each path that could not be mapped.
#[must_use]
pub fn map_all(
    rules: &[MappingRule],
    styles: &[SourceStyle],
    pens: &[OwnedPen],
) -> (Vec<Mapping>, Diagnostics) {
    let mut diags = Diagnostics::new();
    let mapped = styles
        .iter()
        .map(|style| {
            let m = map_style(rules, style, pens);
            let key = match m {
                Mapping::Unmapped => Some("diag-import-unmapped"),
                Mapping::NoOwnedPen => Some("diag-import-no-owned-pen"),
                Mapping::NoColour => Some("diag-import-no-colour"),
                Mapping::Assigned { .. } => None,
            };
            if let Some(key) = key {
                diags.push(Diagnostic::warning(L10nKey::new(key)));
            }
            m
        })
        .collect();
    (mapped, diags)
}

/// A problem with a rule set, for the rule editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleIssue {
    /// Index of the rule.
    pub rule: usize,
    /// What is wrong.
    pub message: String,
}

/// Checks a rule set for rules that could never match or never be applied.
#[must_use]
pub fn check_rules(rules: &[MappingRule]) -> Vec<RuleIssue> {
    let mut out = Vec::new();
    for (i, rule) in rules.iter().enumerate() {
        let mut issue = |message: &str| {
            out.push(RuleIssue {
                rule: i,
                message: message.to_owned(),
            });
        };
        let m = &rule.matcher;
        if let Some(hex) = &m.stroke
            && parse_hex(hex).is_none()
        {
            issue("the colour is not a hex colour such as #FF0000");
        }
        if let Some(pattern) = &m.layer
            && Glob::new(pattern).is_err()
        {
            issue("the layer pattern is not a valid glob");
        }
        if m.tolerance_de.is_some_and(|t| t < 0.0 || !t.is_finite()) {
            issue("the colour tolerance must not be negative");
        }
        if let (Some(min), Some(max)) = (m.min_width_mm, m.max_width_mm)
            && min > max
        {
            issue("the smallest stroke width is above the largest");
        }
        if rule.assign.tool.is_empty() {
            issue("the rule assigns no tool");
        }
    }
    out
}

/// The CIEDE2000 distance between two colours, re-exported for the rule editor's preview.
#[must_use]
pub fn colour_distance(a: [u8; 3], b: [u8; 3]) -> f64 {
    delta_e2000(a, b)
}

#[cfg(test)]
mod tests;
