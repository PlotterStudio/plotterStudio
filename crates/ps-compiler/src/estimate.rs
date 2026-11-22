//! Estimation.
//!
//! Time uses the profile's motion model: a trapezoidal velocity profile per move with a
//! look-ahead planner. Tool-up and tool-down times, dwells and heating time are added. User
//! pauses are counted separately so the UI can show "machine time" and "with tool changes". A
//! per-machine `time_scale` (fitted in the calibration wizard) scales machine time. The result
//! also reports drawn length per tool slot and the number of tool changes.

use std::collections::BTreeMap;

use ps_core::PtUm;
use ps_job::{Estimate, Step};

use crate::motion::{Dir, MotionLimits, Move, accel_at, plan, vector_ms_at};

/// Longest chord used to approximate an arc, in mm.
const ARC_CHORD_MM: f64 = 0.5;
const PPM: u128 = 1_000_000;

/// Everything the estimator reads.
#[derive(Debug, Clone, Copy)]
pub struct EstimateInput<'a> {
    /// The program.
    pub steps: &'a [Step],
    /// The machine's motion limits.
    pub limits: MotionLimits,
    /// Time to raise the tool.
    pub tool_up_ms: u32,
    /// Time to lower the tool.
    pub tool_down_ms: u32,
    /// Heater rate in °C/s, when the machine has one.
    pub heating_rate_c_s: Option<f64>,
    /// Temperature the heater starts from.
    pub ambient_c: i32,
    /// The machine's measured time factor (1.0 when uncalibrated).
    pub time_scale: f64,
    /// Time allowed for each user pause, in ms.
    pub pause_allowance_ms: u32,
    /// How the machine's acceleration follows its speed, as `(mm/s, mm/s²)` points; empty for the
    /// one acceleration of the limits.
    pub accel_by_speed: &'a [(f64, f64)],
    /// What each drawn vector costs the machine, as `(mm/s, ms)` points; empty for none.
    pub vector_ms_by_speed: &'a [(f64, f64)],
}

#[expect(
    clippy::cast_precision_loss,
    reason = "grid coordinates are far below 2^53 µm, where i64 -> f64 is exact"
)]
fn mm(v: i64) -> f64 {
    v as f64 / 1_000.0
}

fn unit(dx: f64, dy: f64) -> (f64, Dir) {
    let len = libm::hypot(dx, dy);
    if len > 0.0 {
        (
            len,
            Dir {
                x: dx / len,
                y: dy / len,
            },
        )
    } else {
        (0.0, Dir { x: 0.0, y: 0.0 })
    }
}

/// Splits an arc into chords as moves, returning them and the arc's length in mm.
fn arc_moves(from: PtUm, to: PtUm, center: PtUm, ccw: bool, feed: f64) -> (Vec<Move>, f64) {
    let (cx, cy) = (mm(center.x), mm(center.y));
    let (fx, fy) = (mm(from.x) - cx, mm(from.y) - cy);
    let radius = libm::hypot(fx, fy);
    let start = libm::atan2(fy, fx);
    let end = libm::atan2(mm(to.y) - cy, mm(to.x) - cx);
    let tau = std::f64::consts::TAU;
    let mut sweep = if ccw { end - start } else { start - end }.rem_euclid(tau);
    if sweep == 0.0 && from != to {
        sweep = tau;
    }
    let length = radius * sweep;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a small positive count of chords"
    )]
    let chords = ((length / ARC_CHORD_MM).ceil() as usize).max(1);
    let mut moves = Vec::with_capacity(chords);
    let mut prev = (fx, fy);
    for i in 1..=chords {
        #[expect(clippy::cast_precision_loss, reason = "the chord count is small")]
        let frac = i as f64 / chords as f64;
        let angle = if ccw {
            start + sweep * frac
        } else {
            start - sweep * frac
        };
        let next = (radius * libm::cos(angle), radius * libm::sin(angle));
        let (len, dir) = unit(next.0 - prev.0, next.1 - prev.1);
        moves.push(Move {
            length_mm: len,
            dir,
            feed_mm_s: feed,
        });
        prev = next;
    }
    (moves, length)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "times are non-negative and far below u64::MAX"
)]
fn to_ms(seconds: f64) -> u64 {
    libm::round(seconds.max(0.0) * 1_000.0) as u64
}

/// What the walk over the steps accumulates apart from the motion runs.
struct Walk {
    slot: Option<u8>,
    temp: f64,
    fixed_ms: u64,
    tool_changes: u32,
    pauses: u64,
    cut: BTreeMap<u8, f64>,
}

impl Walk {
    /// Handles every step that is not a move; the caller has already flushed the motion run.
    fn fixed(&mut self, step: &Step, input: &EstimateInput<'_>) {
        match step {
            Step::ToolDown { .. } => self.fixed_ms += u64::from(input.tool_down_ms),
            Step::ToolUp => self.fixed_ms += u64::from(input.tool_up_ms),
            Step::Dwell { ms } => self.fixed_ms += u64::from(*ms),
            Step::Temperature { celsius, wait } => {
                let target = f64::from(*celsius);
                if *wait
                    && let Some(rate) = input.heating_rate_c_s
                    && rate > 0.0
                {
                    self.fixed_ms += to_ms((target - self.temp).abs() / rate);
                }
                self.temp = target;
            }
            Step::LoadTool { slot, pause, .. } => {
                self.slot = Some(*slot);
                if pause.is_some() {
                    self.tool_changes += 1;
                    self.pauses += 1;
                }
            }
            Step::Pause(_) => self.pauses += 1,
            Step::Travel { .. } | Step::Line { .. } | Step::Arc { .. } => {}
        }
    }

    fn drew(&mut self, length_mm: f64) {
        if let Some(slot) = self.slot {
            *self.cut.entry(slot).or_default() += length_mm;
        }
    }
}

/// Estimates a mat's program.
#[must_use]
pub fn estimate(input: &EstimateInput<'_>) -> Estimate {
    let mut at = PtUm::new(0, 0);
    let mut run: Vec<Move> = Vec::new();
    let mut seconds = 0.0_f64;
    let mut vector_ms = 0.0_f64;
    let mut walk = Walk {
        slot: None,
        temp: f64::from(input.ambient_c),
        fixed_ms: 0,
        tool_changes: 0,
        pauses: 0,
        cut: BTreeMap::new(),
    };
    let flush = |run: &mut Vec<Move>, seconds: &mut f64| {
        if !run.is_empty() {
            // A run is planned with the acceleration of its fastest move.
            let top = run.iter().map(|m| m.feed_mm_s).fold(0.0, f64::max);
            let limits = MotionLimits {
                accel_mm_s2: accel_at(input.accel_by_speed, input.limits.accel_mm_s2, top),
                ..input.limits
            };
            *seconds += plan(run, &limits).total_s;
            run.clear();
        }
    };

    for step in input.steps {
        match step {
            Step::Travel { to } => {
                let (length_mm, dir) = unit(mm(to.x - at.x), mm(to.y - at.y));
                run.push(Move {
                    length_mm,
                    dir,
                    feed_mm_s: input.limits.max_travel_speed_mm_s,
                });
                at = *to;
            }
            Step::Line {
                to, feed_mm_min, ..
            } => {
                let (length_mm, dir) = unit(mm(to.x - at.x), mm(to.y - at.y));
                run.push(Move {
                    length_mm,
                    dir,
                    feed_mm_s: f64::from(*feed_mm_min) / 60.0,
                });
                walk.drew(length_mm);
                vector_ms += vector_ms_at(input.vector_ms_by_speed, f64::from(*feed_mm_min) / 60.0);
                at = *to;
            }
            Step::Arc {
                to,
                center,
                ccw,
                feed_mm_min,
                ..
            } => {
                let (moves, length_mm) =
                    arc_moves(at, *to, *center, *ccw, f64::from(*feed_mm_min) / 60.0);
                vector_ms += vector_ms_at(input.vector_ms_by_speed, f64::from(*feed_mm_min) / 60.0)
                    * f64::from(u32::try_from(moves.len()).unwrap_or(u32::MAX));
                run.extend(moves);
                walk.drew(length_mm);
                at = *to;
            }
            other => {
                flush(&mut run, &mut seconds);
                walk.fixed(other, input);
            }
        }
    }
    flush(&mut run, &mut seconds);

    let fixed_s = f64::from(u32::try_from(walk.fixed_ms).unwrap_or(u32::MAX)) / 1_000.0;
    let machine = to_ms((seconds + fixed_s + vector_ms / 1_000.0) * input.time_scale);
    Estimate {
        machine_time_ms: machine,
        with_changes_ms: machine + walk.pauses * u64::from(input.pause_allowance_ms),
        cut_length_um: walk
            .cut
            .into_iter()
            .map(|(slot, length_mm)| (slot, to_ms(length_mm)))
            .filter(|(_, um)| *um > 0)
            .collect(),
        tool_changes: walk.tool_changes,
        utilisation_ppm: 0,
    }
}

/// The share of the mat covered, in parts per million, capped at the whole mat.
#[must_use]
pub fn utilisation_ppm(covered: u128, total: u128) -> u32 {
    if total == 0 {
        return 0;
    }
    u32::try_from((covered.saturating_mul(PPM) / total).min(PPM)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;
