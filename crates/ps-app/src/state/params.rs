//! The settings of an operations-panel row: its tool's parameters resolved through the chain,
//! and edits that set or clear an operation-level override on every operation of the
//! row. The same `resolve` the compiler uses supplies the values, so the panel cannot disagree
//! with the G-code.

use ps_core::OpRowId;
use ps_library::ParamType;
use ps_model::{
    Command as ModelCommand, Operation, OverrideTarget, ParamKey, ParamSource, ParamValue,
    SetOverride,
};

use super::AppState;

/// Where a parameter's value comes from, for the badge beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamOrigin {
    /// The tool's own default.
    Default,
    /// The mat's material preset.
    Preset,
    /// An override by the user, on the operation, the mat or the project.
    Changed,
}

/// One parameter of a row's tool, ready to show.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamView {
    /// The parameter's name in the tool definition, such as `force`.
    pub name: String,
    /// Its unit, such as `gf`; empty for none.
    pub unit: String,
    /// Smallest value the tool allows.
    pub min: f64,
    /// Largest value the tool allows.
    pub max: f64,
    /// The increment a slider moves by.
    pub step: f64,
    /// The value in force.
    pub value: f64,
    /// Where it comes from.
    pub origin: ParamOrigin,
    /// True when the user overrode it on the row's operations.
    pub modified: bool,
}

impl ParamView {
    /// A setting that is not a tool parameter, shaped like one so the panel can show it.
    pub(crate) fn plain(name: &str, value: f64, min: f64, max: f64, step: f64, unit: &str) -> Self {
        Self {
            name: name.to_owned(),
            unit: unit.to_owned(),
            min,
            max,
            step,
            value,
            origin: ParamOrigin::Default,
            modified: false,
        }
    }
}

impl AppState {
    /// The operations of `row` on the current mat, with where each lives.
    pub(super) fn row_operations(
        &self,
        row: OpRowId,
    ) -> Vec<(
        ps_core::ObjectId,
        ps_core::PathKey,
        std::sync::Arc<Operation>,
    )> {
        let mat = self.current_mat();
        let mut out = Vec::new();
        for object in mat.objects.values() {
            for path in object.paths.iter() {
                for op in &path.ops {
                    if op.row == row {
                        out.push((object.id, path.key.clone(), std::sync::Arc::new(op.clone())));
                    }
                }
            }
        }
        out
    }

    /// The tool's parameters for `row`, resolved as the compiler would. Empty without a machine
    /// or when the tool is not in the library.
    #[must_use]
    pub fn row_params(&self, row: OpRowId) -> Vec<ParamView> {
        let Some(profile) = self.profile.as_deref() else {
            return Vec::new();
        };
        let ops = self.row_operations(row);
        let Some((_, _, op)) = ops.first() else {
            return Vec::new();
        };
        let mat = self.current_mat();
        let doc = self.store.document();
        let Ok(resolved) = ps_model::resolve(op, &mat, &doc.settings, &self.libs, profile) else {
            return Vec::new();
        };
        let Ok(tool) = self.libs.resolve_tool(&op.tool.tool) else {
            return Vec::new();
        };
        let mut out: Vec<ParamView> = tool
            .item
            .parameters
            .iter()
            .filter_map(|(name, spec)| {
                let got = resolved.params.get(name)?;
                let overridden = ops
                    .iter()
                    .any(|(_, _, o)| o.overrides.get(&ParamKey::new(name)).is_some());
                let step = match spec.kind {
                    ParamType::Integer => 1.0,
                    ParamType::Number => nice_step(spec.max - spec.min),
                };
                Some(ParamView {
                    name: name.clone(),
                    unit: spec.unit.clone().unwrap_or_default(),
                    min: spec.min,
                    max: spec.max,
                    step,
                    value: got.value,
                    origin: match got.source {
                        ParamSource::ToolDefault { .. } => ParamOrigin::Default,
                        ParamSource::MaterialPreset { .. } => ParamOrigin::Preset,
                        _ => ParamOrigin::Changed,
                    },
                    modified: overridden,
                })
            })
            .collect();
        // The tool's own order is by name; put the common ones first, as the design lists them.
        let rank = |n: &str| ["force", "speed", "passes"].iter().position(|k| *k == n);
        out.sort_by_key(|p| (rank(&p.name).unwrap_or(usize::MAX), p.name.clone()));
        out
    }

    /// Sets `name` to `value` on every operation of `row`; false when the tool has no such
    /// parameter or the value is outside its range.
    pub fn set_row_param(&mut self, row: OpRowId, name: &str, value: f64, now_ms: u64) -> bool {
        let Some(view) = self.row_params(row).into_iter().find(|p| p.name == name) else {
            return false;
        };
        if !(value.is_finite() && value >= view.min && value <= view.max) {
            return false;
        }
        self.override_row(row, name, Some(&ParamValue::Number(value)), now_ms)
    }

    /// Removes the operation-level override of `name` from every operation of `row`.
    pub fn reset_row_param(&mut self, row: OpRowId, name: &str, now_ms: u64) -> bool {
        self.override_row(row, name, None, now_ms)
    }

    /// Removes every operation-level override from `row`.
    pub fn reset_row_params(&mut self, row: OpRowId, now_ms: u64) -> bool {
        let names: Vec<String> = self
            .row_params(row)
            .into_iter()
            .filter(|p| p.modified)
            .map(|p| p.name)
            .collect();
        let cmds = names
            .iter()
            .flat_map(|n| self.override_commands(row, n, None))
            .collect::<Vec<_>>();
        !cmds.is_empty() && self.commit(&cmds, now_ms)
    }

    fn override_commands(
        &self,
        row: OpRowId,
        name: &str,
        value: Option<&ParamValue>,
    ) -> Vec<Box<dyn ModelCommand>> {
        self.row_operations(row)
            .into_iter()
            .map(|(object, path, op)| {
                Box::new(SetOverride {
                    target: OverrideTarget::Operation {
                        mat: self.mat,
                        object,
                        path,
                        op: op.id,
                    },
                    key: ParamKey::new(name),
                    value: value.cloned(),
                }) as Box<dyn ModelCommand>
            })
            .collect()
    }

    fn override_row(
        &mut self,
        row: OpRowId,
        name: &str,
        value: Option<&ParamValue>,
        now_ms: u64,
    ) -> bool {
        let cmds = self.override_commands(row, name, value);
        !cmds.is_empty() && self.commit(&cmds, now_ms)
    }
}

/// A slider step for a real-valued range: a power of ten (or half of one) giving about a
/// couple of hundred stops.
fn nice_step(range: f64) -> f64 {
    if !(range.is_finite() && range > 0.0) {
        return 1.0;
    }
    let raw = range / 200.0;
    let decade = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * decade)
        .find(|s| *s >= raw)
        .unwrap_or(decade * 10.0)
}
