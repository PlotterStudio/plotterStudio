//! The toolpath view: compiled steps as line segments in millimetres, travel apart from
//! drawn moves, up to the step the timeline has reached, plus timeline markers and playback maths.

use ps_core::PtUm;
use ps_geometry::{Path, Point, flatten};
use ps_job::Step;

/// Chord tolerance when flattening arcs for display, in millimetres.
const ARC_TOLERANCE_MM: f64 = 0.02;
/// Steps shown per second of playback at 1x.
pub const STEPS_PER_SECOND: f64 = 20.0;

/// The playback speeds the timeline offers.
pub const PLAYBACK_SPEEDS: [f64; 7] = [1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0];

/// What a segment is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    /// A move with the tool up.
    Travel,
    /// A drawn move, with the tool slot that made it when known.
    Draw {
        /// The tool slot in the holder.
        slot: Option<u8>,
    },
}

/// One straight piece of the toolpath.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// Where it starts, in millimetres.
    pub from: Point,
    /// Where it ends, in millimetres.
    pub to: Point,
    /// Travel or drawn.
    pub kind: SegmentKind,
}

fn mm(p: PtUm) -> Point {
    let (x, y) = p.to_mm();
    Point::new(x, y)
}

/// The segments of the first `up_to` steps. The first move of a program has no segment because
/// the start position is not known.
#[must_use]
pub fn segments(steps: &[Step], up_to: usize) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut at: Option<Point> = None;
    let mut slot: Option<u8> = None;
    for step in steps.iter().take(up_to) {
        match step {
            Step::LoadTool { slot: s, .. } => slot = Some(*s),
            Step::Travel { to } => {
                let to = mm(*to);
                if let Some(from) = at {
                    out.push(Segment {
                        from,
                        to,
                        kind: SegmentKind::Travel,
                    });
                }
                at = Some(to);
            }
            Step::Line { to, .. } => {
                let to = mm(*to);
                if let Some(from) = at {
                    out.push(Segment {
                        from,
                        to,
                        kind: SegmentKind::Draw { slot },
                    });
                }
                at = Some(to);
            }
            Step::Arc {
                to, center, ccw, ..
            } => {
                let to = mm(*to);
                if let Some(from) = at {
                    let path = Path::new(from).arc_to(to, mm(*center), *ccw);
                    let points = flatten(&path, ARC_TOLERANCE_MM);
                    for pair in points.windows(2) {
                        out.push(Segment {
                            from: pair[0],
                            to: pair[1],
                            kind: SegmentKind::Draw { slot },
                        });
                    }
                }
                at = Some(to);
            }
            Step::Pause(_)
            | Step::Temperature { .. }
            | Step::Dwell { .. }
            | Step::ToolDown { .. }
            | Step::ToolUp => {}
        }
    }
    out
}

/// Where the tool is after the first `up_to` steps; `None` before the first move.
#[must_use]
pub fn tool_position(steps: &[Step], up_to: usize) -> Option<Point> {
    steps[..up_to.min(steps.len())]
        .iter()
        .rev()
        .find_map(Step::end_point)
        .map(mm)
}

/// What a timeline marker marks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerKind {
    /// A tool is put in a holder.
    ToolChange {
        /// The slot.
        slot: u8,
    },
    /// The job stops for the user.
    Pause {
        /// What the user is told.
        message: String,
    },
}

/// A tool change or pause at a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// The step index.
    pub step: usize,
    /// What it is.
    pub kind: MarkerKind,
}

/// The tool changes and pauses of a program, in order.
#[must_use]
pub fn markers(steps: &[Step]) -> Vec<Marker> {
    let mut out = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        match step {
            Step::LoadTool { slot, pause, .. } => {
                out.push(Marker {
                    step: index,
                    kind: MarkerKind::ToolChange { slot: *slot },
                });
                if let Some(message) = pause {
                    out.push(Marker {
                        step: index,
                        kind: MarkerKind::Pause {
                            message: message.text().to_owned(),
                        },
                    });
                }
            }
            Step::Pause(message) => out.push(Marker {
                step: index,
                kind: MarkerKind::Pause {
                    message: message.text().to_owned(),
                },
            }),
            _ => {}
        }
    }
    out
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a rounded non-negative step count, capped by the program length"
)]
fn to_step(value: f64, total: usize) -> usize {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    (value.round() as usize).min(total)
}

/// The step playback has reached after `elapsed_s` seconds at `speed` times normal.
#[must_use]
pub fn playback_step(total: usize, elapsed_s: f64, speed: f64) -> usize {
    to_step(elapsed_s * speed * STEPS_PER_SECOND, total)
}

/// The step a timeline position from 0 to 1 corresponds to.
#[must_use]
pub fn step_at_fraction(total: usize, fraction: f64) -> usize {
    #[expect(
        clippy::cast_precision_loss,
        reason = "program lengths are far below 2^52"
    )]
    let total_f = total as f64;
    to_step(fraction.clamp(0.0, 1.0) * total_f, total)
}

#[cfg(test)]
mod tests;
