//! The job IR: the compiler's output, ordered moves per mat with links back to the
//! source objects, before any G-code text exists.
//!
//! Coordinates are integer micrometres ([`PtUm`]), so a job never depends on float formatting.

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, TAU};

use ps_core::{ContentHash, Diagnostics, LibRef, MatId, ObjectId, OpId, PathKey, PtUm};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Links a move back to the object, path and operation that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SrcRef {
    /// The mat.
    pub mat: MatId,
    /// The placed object.
    pub object: ObjectId,
    /// The path within the object.
    pub path: PathKey,
    /// The operation.
    pub op: OpId,
    /// Index of this move among the operation's moves.
    pub move_index: u32,
}

/// A message shown to the user when the job pauses (for example "Load pen: … into clamp A").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PauseMsg(String);

impl PauseMsg {
    /// Creates a message.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The message text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.0
    }
}

/// Names a tool holder (a clamp) of a machine profile.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct HolderId(String);

impl HolderId {
    /// Creates a holder id.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One row of a mat's tool table: which tool sits in which holder, under which `T` number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ToolEntry {
    /// The slot, as in `T<slot> M6`.
    pub slot: u8,
    /// The tool definition used.
    #[schemars(with = "String")]
    pub tool: LibRef,
    /// The holder it goes in.
    pub holder: HolderId,
    /// A human-readable label for pauses and headers.
    pub label: String,
    /// The colour and tip size this use of a pen has, when they differ from the tool's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen: Option<PenVariant>,
}

/// How a use of a pen differs from the pen in the library: another ink colour or tip size. Two uses
/// with different variants are different slots, because the user swaps the pen between them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PenVariant {
    /// `#RRGGBB` of the ink.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
    /// The tip width in micrometres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_width_um: Option<u32>,
}

/// Time and length estimates for one mat. All integers, so they hash and compare
/// exactly.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Estimate {
    /// Machine time, excluding user pauses.
    pub machine_time_ms: u64,
    /// Machine time plus tool-change pauses.
    pub with_changes_ms: u64,
    /// Drawn length per tool slot, in µm.
    pub cut_length_um: BTreeMap<u8, u64>,
    /// Number of tool changes.
    pub tool_changes: u32,
    /// Share of the mat covered by material in use, in parts per million.
    pub utilisation_ppm: u32,
}

/// One step of a mat's program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Put a tool in a holder, optionally pausing to tell the user.
    LoadTool {
        /// The tool slot.
        slot: u8,
        /// The holder.
        holder: HolderId,
        /// Message for the user, if a pause is needed.
        pause: Option<PauseMsg>,
    },
    /// Stop until the user continues.
    Pause(PauseMsg),
    /// Set a heated tool's temperature.
    Temperature {
        /// Target in °C.
        celsius: i32,
        /// Wait until it is reached.
        wait: bool,
    },
    /// Wait.
    Dwell {
        /// Milliseconds.
        ms: u32,
    },
    /// Lower the tool with a force.
    ToolDown {
        /// Force in grams-force.
        force_gf: u32,
    },
    /// Raise the tool.
    ToolUp,
    /// Move without drawing.
    Travel {
        /// Destination.
        to: PtUm,
    },
    /// Draw a straight line.
    Line {
        /// End point.
        to: PtUm,
        /// Feed rate in mm/min.
        feed_mm_min: u32,
        /// Tangent angle in milli-degrees, for tangential tools.
        a_mdeg: Option<i64>,
        /// Where the move came from.
        src: SrcRef,
    },
    /// Draw a circular arc.
    Arc {
        /// End point.
        to: PtUm,
        /// Centre of the circle.
        center: PtUm,
        /// Counter-clockwise when true.
        ccw: bool,
        /// Feed rate in mm/min.
        feed_mm_min: u32,
        /// Where the move came from.
        src: SrcRef,
    },
}

/// An axis-aligned box in µm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// Smallest corner.
    pub min: PtUm,
    /// Largest corner.
    pub max: PtUm,
}

impl Step {
    /// The point the tool is at after this step, for moves.
    #[must_use]
    pub fn end_point(&self) -> Option<PtUm> {
        match self {
            Self::Travel { to } | Self::Line { to, .. } | Self::Arc { to, .. } => Some(*to),
            _ => None,
        }
    }

    /// True for steps that draw (lines and arcs).
    #[must_use]
    pub fn is_drawn(&self) -> bool {
        matches!(self, Self::Line { .. } | Self::Arc { .. })
    }
}

/// One mat's program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatJob {
    /// The mat.
    pub mat: MatId,
    /// Tools used, by slot.
    pub tool_table: Vec<ToolEntry>,
    /// The program.
    pub steps: Vec<Step>,
    /// Estimates.
    pub estimate: Estimate,
}

/// A compiled job: one program per mat plus diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    /// Programs, in run order.
    pub mats: Vec<MatJob>,
    /// Problems found while compiling; errors block export.
    pub diagnostics: Diagnostics,
    /// Hash of everything the compile read, for caching and the search seed.
    pub input_hash: ContentHash,
}

impl Job {
    /// Finds a mat's program.
    #[must_use]
    pub fn mat(&self, id: MatId) -> Option<&MatJob> {
        self.mats.iter().find(|m| m.mat == id)
    }

    /// True when a blocking error was reported.
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        self.diagnostics.has_errors()
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "grid coordinates are far below 2^53 µm, where i64 -> f64 is exact"
)]
fn f(v: i64) -> f64 {
    v as f64
}

fn dist(a: PtUm, b: PtUm) -> f64 {
    libm::hypot(f(b.x - a.x), f(b.y - a.y))
}

/// Signed sweep of an arc, counter-clockwise positive; coincident ends mean a full turn.
fn sweep(from: PtUm, to: PtUm, center: PtUm, ccw: bool) -> f64 {
    if from == to {
        return if ccw { TAU } else { -TAU };
    }
    let a0 = libm::atan2(f(from.y - center.y), f(from.x - center.x));
    let a1 = libm::atan2(f(to.y - center.y), f(to.x - center.x));
    if ccw {
        let d = (a1 - a0).rem_euclid(TAU);
        if d == 0.0 { TAU } else { d }
    } else {
        let d = (a0 - a1).rem_euclid(TAU);
        if d == 0.0 { -TAU } else { -d }
    }
}

impl MatJob {
    /// Walks the moves, calling `visit(from, step)` with the point each one starts at.
    fn walk(&self, mut visit: impl FnMut(PtUm, &Step)) {
        let mut at: Option<PtUm> = None;
        for step in &self.steps {
            let Some(to) = step.end_point() else {
                continue;
            };
            if let Some(from) = at {
                visit(from, step);
            }
            at = Some(to);
        }
    }

    /// Total length of drawn moves in µm.
    #[must_use]
    pub fn drawn_length_um(&self) -> f64 {
        let mut total = 0.0;
        self.walk(|from, step| match step {
            Step::Line { to, .. } => total += dist(from, *to),
            Step::Arc {
                to, center, ccw, ..
            } => {
                total += dist(from, *center) * sweep(from, *to, *center, *ccw).abs();
            }
            _ => {}
        });
        total
    }

    /// Total length of travel moves in µm.
    #[must_use]
    pub fn travel_length_um(&self) -> f64 {
        let mut total = 0.0;
        self.walk(|from, step| {
            if let Step::Travel { to } = step {
                total += dist(from, *to);
            }
        });
        total
    }

    /// The box around every move, including the bulge of arcs.
    #[must_use]
    pub fn bounds(&self) -> Option<Bounds> {
        let mut b: Option<Bounds> = None;
        let mut add = |p: PtUm| {
            b = Some(b.map_or(Bounds { min: p, max: p }, |c| Bounds {
                min: PtUm::new(c.min.x.min(p.x), c.min.y.min(p.y)),
                max: PtUm::new(c.max.x.max(p.x), c.max.y.max(p.y)),
            }));
        };
        let mut at: Option<PtUm> = None;
        for step in &self.steps {
            if let Step::Arc {
                to, center, ccw, ..
            } = step
                && let Some(from) = at
            {
                let radius = dist(from, *center);
                let total = sweep(from, *to, *center, *ccw).abs();
                let a0 = libm::atan2(f(from.y - center.y), f(from.x - center.x));
                for k in 0..4 {
                    let axis = FRAC_PI_2 * f64::from(k);
                    let along = if *ccw { axis - a0 } else { a0 - axis }.rem_euclid(TAU);
                    if along <= total {
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "the extreme lies within the arc's own coordinates"
                        )]
                        let extreme = PtUm::new(
                            center.x + (radius * libm::cos(axis)).round() as i64,
                            center.y + (radius * libm::sin(axis)).round() as i64,
                        );
                        add(extreme);
                    }
                }
            }
            if let Some(p) = step.end_point() {
                add(p);
                at = Some(p);
            }
        }
        b
    }

    /// Number of moves (travel, lines and arcs).
    #[must_use]
    pub fn move_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| s.end_point().is_some())
            .count()
    }

    /// Number of drawn moves (lines and arcs).
    #[must_use]
    pub fn drawn_count(&self) -> usize {
        self.steps.iter().filter(|s| s.is_drawn()).count()
    }

    /// Number of tool changes: tool loads after the first.
    #[must_use]
    pub fn tool_change_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| matches!(s, Step::LoadTool { .. }))
            .count()
            .saturating_sub(1)
    }
}

#[cfg(test)]
mod tests;
