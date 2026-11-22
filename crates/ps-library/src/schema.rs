//! Schemas and validation.
//!
//! JSON Schemas (draft 2020-12) are generated from the Rust types and published under stable
//! `$id` URLs. A file is migrated, validated against its schema (reporting every error at once
//! with its JSON pointer), deserialised, and finally checked semantically.

use std::collections::{BTreeMap, BTreeSet};

use ps_core::LibRef;
use semver::Version;
use serde_json::Value;

#[cfg(test)]
use crate::Textures;
use crate::migrate::{MigrateError, to_current};
use crate::{LibraryKind, MachineProfile, Material, Tool, ToolFamily};

/// Units a parameter may declare.
const UNITS: [&str; 8] = ["gf", "mm/s", "mm", "°C", "ms", "deg", "%", "s"];

/// One problem found in a library file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// RFC 6901 JSON pointer to the offending value (empty for the whole document).
    pub pointer: String,
    /// What is wrong.
    pub message: String,
}

impl Issue {
    /// Creates an issue.
    #[must_use]
    pub fn new(pointer: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            pointer: pointer.into(),
            message: message.into(),
        }
    }
}

/// Groups issues by the file they came from, keeping each file's order.
#[must_use]
pub fn group_by_file(issues: Vec<(String, Issue)>) -> BTreeMap<String, Vec<Issue>> {
    let mut out: BTreeMap<String, Vec<Issue>> = BTreeMap::new();
    for (file, issue) in issues {
        out.entry(file).or_default().push(issue);
    }
    out
}

/// The published JSON Schema of a kind, with its stable `$id`.
#[must_use]
pub fn schema_json(kind: LibraryKind) -> Value {
    let schema = match kind {
        LibraryKind::Tool => schemars::schema_for!(Tool),
        LibraryKind::Material => schemars::schema_for!(Material),
        LibraryKind::Profile => schemars::schema_for!(MachineProfile),
    };
    let mut value = serde_json::to_value(schema).unwrap_or(Value::Null);
    if let Some(obj) = value.as_object_mut() {
        obj.insert("$id".to_owned(), Value::from(kind.schema_id()));
    }
    value
}

/// Validates a JSON value against the kind's schema, returning every error, sorted by pointer.
#[must_use]
pub fn validate(kind: LibraryKind, instance: &Value) -> Vec<Issue> {
    let schema = schema_json(kind);
    let validator = match jsonschema::validator_for(&schema) {
        Ok(v) => v,
        Err(e) => return vec![Issue::new("", format!("schema could not be compiled: {e}"))],
    };
    let mut issues: Vec<Issue> = validator
        .iter_errors(instance)
        .map(|e| Issue::new(e.instance_path().to_string(), e.to_string()))
        .collect();
    issues.sort_by(|a, b| (&a.pointer, &a.message).cmp(&(&b.pointer, &b.message)));
    issues
}

fn positive(v: Option<f64>, pointer: &str, out: &mut Vec<Issue>) {
    if let Some(x) = v
        && x.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
    {
        out.push(Issue::new(pointer, "must be greater than zero"));
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Semantic checks on a tool: ranges, units, pen and blade requirements.
#[must_use]
pub fn check_tool(t: &Tool) -> Vec<Issue> {
    let mut out = Vec::new();
    if !valid_id(&t.id) {
        out.push(Issue::new(
            "/id",
            "ids use letters, digits, '.', '-' and '_' only",
        ));
    }
    if t.operations.is_empty() {
        out.push(Issue::new(
            "/operations",
            "a tool needs at least one operation",
        ));
    }
    if t.holders.is_empty() {
        out.push(Issue::new("/holders", "a tool needs at least one holder"));
    }
    for (name, p) in &t.parameters {
        let base = format!("/parameters/{name}");
        if p.min > p.max {
            out.push(Issue::new(format!("{base}/min"), "min is above max"));
        }
        if p.default < p.min || p.default > p.max {
            out.push(Issue::new(
                format!("{base}/default"),
                "default is outside min..max",
            ));
        }
        if p.kind == crate::ParamType::Integer
            && [p.min, p.max, p.default].iter().any(|v| v.fract() != 0.0)
        {
            out.push(Issue::new(
                format!("{base}/default"),
                "an integer parameter needs whole numbers",
            ));
        }
        if let Some(unit) = &p.unit
            && !UNITS.contains(&unit.as_str())
        {
            out.push(Issue::new(
                format!("{base}/unit"),
                format!("unknown unit {unit:?}"),
            ));
        }
    }
    let g = &t.geometry;
    positive(g.blade_offset_mm, "/geometry/blade_offset_mm", &mut out);
    positive(g.min_feature_mm, "/geometry/min_feature_mm", &mut out);
    positive(g.tip_width_mm, "/geometry/tip_width_mm", &mut out);
    positive(g.wheel_radius_mm, "/geometry/wheel_radius_mm", &mut out);
    if t.family == ToolFamily::Pen {
        if g.tip_width_mm.is_none() {
            out.push(Issue::new(
                "/geometry/tip_width_mm",
                "pens need a tip width for fills",
            ));
        }
        match &t.pen {
            None => out.push(Issue::new("/pen", "pens need colour details")),
            Some(pen) if pen.colour.rgb().is_none() => {
                out.push(Issue::new(
                    "/pen/colour/hex",
                    "colour must look like #RRGGBB",
                ));
            }
            Some(_) => {}
        }
    }
    if t.drive == crate::DriveType::Drag && g.blade_offset_mm.is_none() {
        out.push(Issue::new(
            "/geometry/blade_offset_mm",
            "drag blades need a blade offset",
        ));
    }
    out
}

/// Semantic checks on a material; `files` are the texture files that are present.
#[must_use]
pub fn check_material(m: &Material, files: &BTreeSet<String>) -> Vec<Issue> {
    let mut out = Vec::new();
    if let Some(t) = m.thickness_mm
        && t <= 0.0
    {
        out.push(Issue::new("/thickness_mm", "must be greater than zero"));
    }
    if let Some(tex) = &m.textures {
        if tex.scale_mm <= 0.0 {
            out.push(Issue::new(
                "/textures/scale_mm",
                "must be greater than zero",
            ));
        }
        let named = [
            ("base_color", &tex.base_color),
            ("normal", &tex.normal),
            ("roughness", &tex.roughness),
            ("height", &tex.height),
            ("metallic", &tex.metallic),
        ];
        for (field, file) in named {
            if let Some(f) = file
                && !files.contains(f)
            {
                out.push(Issue::new(
                    format!("/textures/{field}"),
                    format!("texture file {f:?} is missing"),
                ));
            }
        }
    }
    for (i, p) in m.presets.iter().enumerate() {
        if p.params.is_empty() {
            out.push(Issue::new(
                format!("/presets/{i}/params"),
                "a preset needs values",
            ));
        }
        for (k, v) in &p.params {
            if *v < 0.0 {
                out.push(Issue::new(
                    format!("/presets/{i}/params/{k}"),
                    "must not be negative",
                ));
            }
        }
    }
    out
}

/// Semantic checks on a machine profile.
#[must_use]
pub fn check_profile(p: &MachineProfile) -> Vec<Issue> {
    let mut out = Vec::new();
    positive(Some(p.work_area.width_mm), "/work_area/width_mm", &mut out);
    positive(
        Some(p.work_area.height_mm),
        "/work_area/height_mm",
        &mut out,
    );
    if p.holders.is_empty() {
        out.push(Issue::new(
            "/holders",
            "a machine needs at least one holder",
        ));
    }
    for (i, s) in p.mat_sizes.iter().enumerate() {
        positive(
            Some(s.width_mm),
            &format!("/mat_sizes/{i}/width_mm"),
            &mut out,
        );
        positive(
            Some(s.height_mm),
            &format!("/mat_sizes/{i}/height_mm"),
            &mut out,
        );
    }
    if p.force.min_gf > p.force.max_gf {
        out.push(Issue::new("/force/min_gf", "min is above max"));
    }
    positive(Some(p.force.units_per_gf), "/force/units_per_gf", &mut out);
    positive(
        Some(p.gcode.chord_tolerance_mm),
        "/gcode/chord_tolerance_mm",
        &mut out,
    );
    if !p.gcode.dialect.starts_with("ps-gcode/") {
        out.push(Issue::new(
            "/gcode/dialect",
            "dialect must be ps-gcode/<version>",
        ));
    }
    if p.heated && p.heating_rate_c_s.is_none() {
        out.push(Issue::new(
            "/heating_rate_c_s",
            "heated machines need a heating rate",
        ));
    }
    out
}

/// A loaded, validated library entry.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// A tool.
    Tool(Tool),
    /// A material.
    Material(Material),
    /// A machine profile.
    Profile(MachineProfile),
}

impl Entry {
    /// The kind of entry.
    #[must_use]
    pub fn kind(&self) -> LibraryKind {
        match self {
            Self::Tool(_) => LibraryKind::Tool,
            Self::Material(_) => LibraryKind::Material,
            Self::Profile(_) => LibraryKind::Profile,
        }
    }

    /// The entry's id.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Tool(t) => &t.id,
            Self::Material(m) => &m.id,
            Self::Profile(p) => &p.id,
        }
    }

    /// The entry's version.
    #[must_use]
    pub fn version(&self) -> &Version {
        match self {
            Self::Tool(t) => &t.version,
            Self::Material(m) => &m.version,
            Self::Profile(p) => &p.version,
        }
    }

    /// The entry as a library reference.
    #[must_use]
    pub fn lib_ref(&self) -> LibRef {
        match self {
            Self::Tool(t) => t.lib_ref(),
            Self::Material(m) => m.lib_ref(),
            Self::Profile(p) => p.lib_ref(),
        }
    }

    /// Loads text: migrates, validates against the schema, deserialises, then checks semantics.
    ///
    /// Structural errors are all reported together and stop the later stages.
    ///
    /// # Errors
    /// Every [`Issue`] of the first stage that fails.
    pub fn from_json(kind: LibraryKind, text: &str) -> Result<Self, Vec<Issue>> {
        Self::from_json_with_files(kind, text, &BTreeSet::new())
    }

    /// Like [`Entry::from_json`], with the set of texture files present for materials.
    ///
    /// # Errors
    /// As for [`Entry::from_json`].
    pub fn from_json_with_files(
        kind: LibraryKind,
        text: &str,
        files: &BTreeSet<String>,
    ) -> Result<Self, Vec<Issue>> {
        let raw: Value = serde_json::from_str(text)
            .map_err(|e| vec![Issue::new("", format!("not valid JSON: {e}"))])?;
        let value = to_current(kind, raw).map_err(|e| {
            let pointer = match e {
                MigrateError::NotObject => "",
                _ => "/schema_version",
            };
            vec![Issue::new(pointer, e.to_string())]
        })?;
        let issues = validate(kind, &value);
        if !issues.is_empty() {
            return Err(issues);
        }
        let fail = |e: serde_json::Error| vec![Issue::new("", e.to_string())];
        let (entry, issues) = match kind {
            LibraryKind::Tool => {
                let t: Tool = serde_json::from_value(value).map_err(fail)?;
                let issues = check_tool(&t);
                (Self::Tool(t), issues)
            }
            LibraryKind::Material => {
                let m: Material = serde_json::from_value(value).map_err(fail)?;
                let issues = check_material(&m, files);
                (Self::Material(m), issues)
            }
            LibraryKind::Profile => {
                let p: MachineProfile = serde_json::from_value(value).map_err(fail)?;
                let issues = check_profile(&p);
                (Self::Profile(p), issues)
            }
        };
        if issues.is_empty() {
            Ok(entry)
        } else {
            Err(issues)
        }
    }
}

#[cfg(test)]
mod tests;
