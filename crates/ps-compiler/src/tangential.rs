//! Tangential-tool helpers.
//!
//! The tool angle `A` follows the path heading. It is unwrapped to a continuous value so the tool
//! never spins a full turn to cross ±180°, and strokes are split at corners sharper than the
//! corner-lift angle (the tool lifts, rotates, and goes down again). All angles are integer
//! milli-degrees, so they hash and compare exactly.

use ps_core::PtUm;

const FULL_TURN: i64 = 360_000;
const HALF_TURN: i64 = 180_000;

/// Heading of the vector `from -> to`, in milli-degrees counter-clockwise from +x, in
/// `-180_000..=180_000`.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "grid coordinates are far below 2^53 µm, where i64 -> f64 is exact"
)]
#[expect(
    clippy::cast_possible_truncation,
    reason = "the rounded angle is within ±180_000, far inside i64"
)]
pub fn heading_mdeg(from: PtUm, to: PtUm) -> i64 {
    let a = libm::atan2((to.y - from.y) as f64, (to.x - from.x) as f64);
    libm::round(a.to_degrees() * 1_000.0) as i64
}

/// The representation of `angle` (mod a full turn) nearest to `prev`; an exact tie (half a turn
/// away) goes to the higher angle.
#[must_use]
pub fn unwrap_near_mdeg(angle: i64, prev: i64) -> i64 {
    let turns = (prev - angle + HALF_TURN).div_euclid(FULL_TURN);
    angle + turns * FULL_TURN
}

/// The unwrapped heading of every edge of a polyline, continuing from `prev`.
///
/// A zero-length edge keeps the previous heading.
#[must_use]
pub fn unwrapped_angles(points: &[PtUm], prev: i64) -> Vec<i64> {
    let mut out = Vec::with_capacity(points.len().saturating_sub(1));
    let mut last = prev;
    for w in points.windows(2) {
        if w[0] != w[1] {
            last = unwrap_near_mdeg(heading_mdeg(w[0], w[1]), last);
        }
        out.push(last);
    }
    out
}

/// The smaller angle between two headings, in `0..=180_000` milli-degrees.
#[must_use]
pub fn turn_mdeg(a: i64, b: i64) -> i64 {
    let d = (b - a).rem_euclid(FULL_TURN);
    d.min(FULL_TURN - d)
}

/// Splits a polyline into strokes at corners whose turn is at or above `lift_deg`.
///
/// Consecutive strokes share the corner point. Zero-length edges never form a corner.
#[must_use]
pub fn split_at_corners(points: &[PtUm], lift_deg: f64) -> Vec<Vec<PtUm>> {
    if points.is_empty() {
        return Vec::new();
    }
    if points.len() == 1 {
        return vec![points.to_vec()];
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a finite, small angle in milli-degrees"
    )]
    let threshold = libm::round(lift_deg * 1_000.0) as i64;
    let mut strokes = Vec::new();
    let mut current = vec![points[0]];
    let mut prev_heading: Option<i64> = None;
    for w in points.windows(2) {
        if w[0] == w[1] {
            current.push(w[1]);
            continue;
        }
        let h = heading_mdeg(w[0], w[1]);
        if let Some(p) = prev_heading
            && turn_mdeg(p, h) >= threshold
        {
            strokes.push(std::mem::replace(&mut current, vec![w[0]]));
        }
        current.push(w[1]);
        prev_heading = Some(h);
    }
    strokes.push(current);
    strokes
}

#[cfg(test)]
mod tests;
