//! Parameter resolution.
//!
//! Every parameter resolves through a fixed chain and the first layer that defines it wins:
//! operation override, mat override, project override, material preset (the machine-specific
//! value first, then the generic one), tool default. The result is then clamped to the machine
//! profile's limits; clamping never hides, because the pre-clamp value is kept and a warning is
//! raised. [`resolve`] is a pure function that the compiler and the inspector both call, so the
//! UI can never disagree with the G-code.

use std::collections::BTreeMap;

use ps_core::{Diagnostic, Diagnostics, L10nKey, LibRef};
use ps_library::{LibrarySnapshot, MachineProfile, ResolveError};

use crate::{JobSettings, Mat, Operation, ParamKey};

/// Which layer of the chain supplied a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamSource {
    /// The operation's own override.
    OperationOverride,
    /// The mat's override.
    MatOverride,
    /// The project's override.
    ProjectOverride,
    /// A material preset.
    MaterialPreset {
        /// The material.
        material: LibRef,
        /// True when the value came from the machine-specific part of the preset.
        machine_specific: bool,
    },
    /// The tool's default.
    ToolDefault {
        /// The tool.
        tool: LibRef,
    },
}

/// A parameter value with where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved<T> {
    /// The value in force (after any machine clamp).
    pub value: T,
    /// The chain layer that supplied it.
    pub source: ParamSource,
    /// The value before the machine limit changed it, when it did.
    pub clamped_by_machine: Option<T>,
}

/// Every parameter of an operation's tool, resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedParams {
    /// Resolved values by parameter name.
    pub params: BTreeMap<String, Resolved<f64>>,
    /// Warnings raised while resolving (machine clamps).
    pub warnings: Diagnostics,
}

/// Why parameters could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveParamsError {
    /// The operation's tool is not in the library.
    #[error(transparent)]
    MissingTool(#[from] ResolveError),
}

/// Limits a value to what the machine can do, returning the value and, if it changed, the
/// original. Only `force` (the profile's force range) and `speed` (up to the top speed) have
/// machine limits.
#[must_use]
pub fn clamp_to_machine(name: &str, value: f64, profile: &MachineProfile) -> (f64, Option<f64>) {
    let limited = match name {
        "force" => profile.force.clamp_gf(value),
        "speed" => value.min(profile.max_speed_mm_s),
        _ => value,
    };
    if limited.total_cmp(&value).is_eq() {
        (value, None)
    } else {
        (limited, Some(value))
    }
}

/// Resolves every parameter of the operation's tool.
///
/// # Errors
/// [`ResolveParamsError::MissingTool`] when the tool is not in the library. An unknown material
/// is ignored here; it is reported by the library-reference check.
pub fn resolve(
    op: &Operation,
    mat: &Mat,
    project: &JobSettings,
    libs: &LibrarySnapshot,
    profile: &MachineProfile,
) -> Result<ResolvedParams, ResolveParamsError> {
    let tool = libs.resolve_tool(&op.tool.tool)?.item;
    let material = mat
        .material
        .as_ref()
        .and_then(|r| libs.resolve_material(r).ok())
        .map(|r| r.item);
    let preset = material.as_ref().and_then(|m| {
        m.presets
            .iter()
            .find(|p| p.tool == tool.id && p.operation == op.kind.as_str())
    });

    let mut params = BTreeMap::new();
    let mut warnings = Diagnostics::new();
    for (name, spec) in &tool.parameters {
        let key = ParamKey::new(name);
        let layer = |o: &crate::ParamOverrides| o.get(&key).and_then(crate::ParamValue::as_f64);

        let (value, source) = if let Some(v) = layer(&op.overrides) {
            (v, ParamSource::OperationOverride)
        } else if let Some(v) = layer(&mat.overrides) {
            (v, ParamSource::MatOverride)
        } else if let Some(v) = layer(&project.overrides) {
            (v, ParamSource::ProjectOverride)
        } else if let Some((v, machine_specific)) =
            preset.zip(material.as_ref()).and_then(|(p, _)| {
                let specific = p.machines.get(&profile.id).and_then(|m| m.get(name));
                specific
                    .map(|v| (*v, true))
                    .or_else(|| p.params.get(name).map(|v| (*v, false)))
            })
        {
            let material = material.as_ref().map(|m| m.lib_ref());
            match material {
                Some(material) => (
                    v,
                    ParamSource::MaterialPreset {
                        material,
                        machine_specific,
                    },
                ),
                None => (
                    spec.default,
                    ParamSource::ToolDefault {
                        tool: tool.lib_ref(),
                    },
                ),
            }
        } else {
            (
                spec.default,
                ParamSource::ToolDefault {
                    tool: tool.lib_ref(),
                },
            )
        };

        let (used, clamped) = clamp_to_machine(name, value, profile);
        if let Some(requested) = clamped {
            warnings.push(
                Diagnostic::warning(L10nKey::new("diag-clamped-by-machine"))
                    .with_arg("param", name.clone())
                    .with_arg("requested", format!("{requested}"))
                    .with_arg("used", format!("{used}"))
                    .with_arg("machine", profile.id.clone()),
            );
        }
        params.insert(
            name.clone(),
            Resolved {
                value: used,
                source,
                clamped_by_machine: clamped,
            },
        );
    }
    Ok(ResolvedParams { params, warnings })
}

#[cfg(test)]
mod tests;
