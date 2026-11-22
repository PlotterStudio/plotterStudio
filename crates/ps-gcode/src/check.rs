//! Validation before writing.
//!
//! The validator reads the IR, not the text, and checks every move against the profile: work
//! area, keep-outs, force range, speed limits, arc support, A-axis support and heater support.
//! Any violation blocks export; each lists the offending step and, for drawn moves, the source
//! reference so the canvas can link to it.

use ps_core::PtUm;
use ps_job::{MatJob, SrcRef, Step};
use ps_library::MachineProfile;

use crate::linearise::linearise;

/// Tolerance for checking an arc against the work area, in mm.
const ARC_CHECK_TOL_MM: f64 = 0.05;

/// What is wrong with a step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViolationKind {
    /// The move leaves the work area.
    OutsideWorkArea,
    /// The move enters a keep-out region.
    InKeepOut,
    /// The force is outside the profile's range.
    ForceOutOfRange {
        /// The force asked for.
        gf: u32,
        /// The lowest allowed.
        min_gf: u32,
        /// The highest allowed.
        max_gf: u32,
    },
    /// The feed is above the machine's maximum speed.
    SpeedTooHigh {
        /// The feed asked for, in mm/min.
        feed_mm_min: u32,
        /// The limit, in mm/min.
        max_mm_min: u32,
    },
    /// The machine cannot cut arcs.
    ArcsUnsupported,
    /// The machine has no tangential axis.
    TangentUnsupported,
    /// The machine has no heater.
    HeaterUnsupported,
    /// A tool is loaded that the mat's tool table does not list.
    UnknownTool {
        /// The slot.
        slot: u8,
    },
}

/// One problem found in a program.
#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    /// Index of the step in the mat's program.
    pub step: usize,
    /// What is wrong.
    pub kind: ViolationKind,
    /// Where a drawn move came from.
    pub src: Option<SrcRef>,
}

/// Export is blocked: the program breaks the machine's limits.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{} problems stop the export", .violations.len())]
pub struct ValidationError {
    /// Every violation, in step order.
    pub violations: Vec<Violation>,
}

struct Limits {
    width: i64,
    height: i64,
    keep_outs: Vec<[i64; 4]>,
}

fn um(mm: f64) -> i64 {
    ps_core::um_from_mm(mm).unwrap_or(i64::MAX)
}

fn orient(a: PtUm, b: PtUm, c: PtUm) -> i128 {
    i128::from(b.x - a.x) * i128::from(c.y - a.y) - i128::from(b.y - a.y) * i128::from(c.x - a.x)
}

fn inside(p: PtUm, r: &[i64; 4]) -> bool {
    p.x > r[0] && p.x < r[2] && p.y > r[1] && p.y < r[3]
}

/// True when the segment passes through the interior of the rectangle: an end lies strictly
/// inside, or the segment properly crosses two of its sides.
fn passes_through(start: PtUm, end: PtUm, rect: &[i64; 4]) -> bool {
    if inside(start, rect) || inside(end, rect) {
        return true;
    }
    let corners = [
        PtUm::new(rect[0], rect[1]),
        PtUm::new(rect[2], rect[1]),
        PtUm::new(rect[2], rect[3]),
        PtUm::new(rect[0], rect[3]),
    ];
    let crossings = (0..4)
        .filter(|&i| {
            let (from, to) = (corners[i], corners[(i + 1) % 4]);
            orient(start, end, from).signum() * orient(start, end, to).signum() < 0
                && orient(from, to, start).signum() * orient(from, to, end).signum() < 0
        })
        .count();
    crossings >= 2
}

impl Limits {
    fn point(&self, p: PtUm) -> Option<ViolationKind> {
        if p.x < 0 || p.y < 0 || p.x > self.width || p.y > self.height {
            return Some(ViolationKind::OutsideWorkArea);
        }
        self.keep_outs
            .iter()
            .any(|r| inside(p, r))
            .then_some(ViolationKind::InKeepOut)
    }

    fn segment(&self, a: PtUm, b: PtUm) -> Option<ViolationKind> {
        self.point(b).or_else(|| {
            self.keep_outs
                .iter()
                .any(|r| passes_through(a, b, r))
                .then_some(ViolationKind::InKeepOut)
        })
    }
}

fn limits(profile: &MachineProfile) -> Limits {
    Limits {
        width: um(profile.work_area.width_mm),
        height: um(profile.work_area.height_mm),
        keep_outs: profile
            .keep_outs
            .iter()
            .map(|k| {
                [
                    um(k.x_mm),
                    um(k.y_mm),
                    um(k.x_mm + k.width_mm),
                    um(k.y_mm + k.height_mm),
                ]
            })
            .collect(),
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "profile limits are small non-negative values"
)]
fn round_u32(v: f64) -> u32 {
    libm::round(v.max(0.0)) as u32
}

/// Checks a mat's program against the profile, returning every violation in step order.
#[must_use]
pub fn check(mat: &MatJob, profile: &MachineProfile) -> Vec<Violation> {
    let lim = limits(profile);
    let (min_gf, max_gf) = (
        round_u32(profile.force.min_gf),
        round_u32(profile.force.max_gf),
    );
    let max_feed = round_u32(profile.max_speed_mm_s * 60.0);
    let mut out = Vec::new();
    let mut at = PtUm::new(0, 0);

    for (i, step) in mat.steps.iter().enumerate() {
        let mut found: Vec<ViolationKind> = Vec::new();
        let mut src: Option<&SrcRef> = None;
        match step {
            Step::Travel { to } => {
                found.extend(lim.segment(at, *to));
                at = *to;
            }
            Step::Line {
                to,
                feed_mm_min,
                a_mdeg,
                src: s,
            } => {
                src = Some(s);
                found.extend(lim.segment(at, *to));
                if *feed_mm_min > max_feed {
                    found.push(ViolationKind::SpeedTooHigh {
                        feed_mm_min: *feed_mm_min,
                        max_mm_min: max_feed,
                    });
                }
                if a_mdeg.is_some() && !profile.tangential_axis {
                    found.push(ViolationKind::TangentUnsupported);
                }
                at = *to;
            }
            Step::Arc {
                to,
                center,
                ccw,
                feed_mm_min,
                src: s,
            } => {
                src = Some(s);
                if profile.arcs {
                    let mut prev = at;
                    for chord in linearise(at, *to, *center, *ccw, ARC_CHECK_TOL_MM) {
                        if let Some(kind) = lim.segment(prev, chord) {
                            found.push(kind);
                            break;
                        }
                        prev = chord;
                    }
                } else {
                    found.push(ViolationKind::ArcsUnsupported);
                }
                if *feed_mm_min > max_feed {
                    found.push(ViolationKind::SpeedTooHigh {
                        feed_mm_min: *feed_mm_min,
                        max_mm_min: max_feed,
                    });
                }
                at = *to;
            }
            Step::ToolDown { force_gf } => {
                if *force_gf < min_gf || *force_gf > max_gf {
                    found.push(ViolationKind::ForceOutOfRange {
                        gf: *force_gf,
                        min_gf,
                        max_gf,
                    });
                }
            }
            Step::Temperature { .. } => {
                if !profile.heated {
                    found.push(ViolationKind::HeaterUnsupported);
                }
            }
            Step::LoadTool { slot, .. } => {
                if !mat.tool_table.iter().any(|t| t.slot == *slot) {
                    found.push(ViolationKind::UnknownTool { slot: *slot });
                }
            }
            Step::Pause(_) | Step::Dwell { .. } | Step::ToolUp => {}
        }
        out.extend(found.into_iter().map(|kind| Violation {
            step: i,
            kind,
            src: src.cloned(),
        }));
    }
    out
}

/// Like [`check`], but returns an error listing the violations when there are any.
///
/// # Errors
///
/// [`ValidationError`] when the program breaks a limit.
pub fn ensure_valid(mat: &MatJob, profile: &MachineProfile) -> Result<(), ValidationError> {
    let violations = check(mat, profile);
    if violations.is_empty() {
        Ok(())
    } else {
        Err(ValidationError { violations })
    }
}

#[cfg(test)]
mod tests;
