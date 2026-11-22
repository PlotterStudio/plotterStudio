//! Per-machine calibration: what the calibration wizard measures, kept apart
//! from tool and material definitions and applied by the compiler.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Issue;

/// An X/Y offset in mm.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Xy {
    /// X offset.
    pub x_mm: f64,
    /// Y offset.
    pub y_mm: f64,
}

fn one() -> f64 {
    1.0
}

/// Calibration values for one machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    /// Measured blade offsets by tool id.
    #[serde(default)]
    pub blade_offsets_mm: BTreeMap<String, f64>,
    /// Measured pen-to-blade offsets by holder id.
    #[serde(default)]
    pub holder_offsets_mm: BTreeMap<String, Xy>,
    /// Multiplier applied to every force.
    #[serde(default = "one")]
    pub force_scale: f64,
    /// Multiplier that brings time estimates in line with measured runs.
    #[serde(default = "one")]
    pub time_scale: f64,
}

impl Default for Calibration {
    fn default() -> Self {
        Self {
            blade_offsets_mm: BTreeMap::new(),
            holder_offsets_mm: BTreeMap::new(),
            force_scale: 1.0,
            time_scale: 1.0,
        }
    }
}

impl Calibration {
    /// The blade offset for a tool: the measured one, else `default`.
    #[must_use]
    pub fn blade_offset(&self, tool_id: &str, default: f64) -> f64 {
        self.blade_offsets_mm
            .get(tool_id)
            .copied()
            .unwrap_or(default)
    }

    /// The offset of a holder (zero when never measured).
    #[must_use]
    pub fn holder_offset(&self, holder_id: &str) -> Xy {
        self.holder_offsets_mm
            .get(holder_id)
            .copied()
            .unwrap_or_default()
    }

    /// A force with the calibration applied.
    #[must_use]
    pub fn scaled_force(&self, gf: f64) -> f64 {
        gf * self.force_scale
    }

    /// Reports values that make no sense.
    #[must_use]
    pub fn validate(&self) -> Vec<Issue> {
        let mut out = Vec::new();
        let ok = |v: f64| v.is_finite() && v > 0.0;
        if !ok(self.force_scale) {
            out.push(Issue::new("/force_scale", "must be a positive number"));
        }
        if !ok(self.time_scale) {
            out.push(Issue::new("/time_scale", "must be a positive number"));
        }
        for (tool, v) in &self.blade_offsets_mm {
            if !v.is_finite() || *v < 0.0 {
                out.push(Issue::new(
                    format!("/blade_offsets_mm/{tool}"),
                    "must not be negative",
                ));
            }
        }
        out
    }
}

/// The calibration file name for a machine id.
#[must_use]
pub fn file_name(machine_id: &str) -> String {
    format!("{machine_id}.json")
}

#[cfg(test)]
mod tests;
