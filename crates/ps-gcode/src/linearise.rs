//! Arc linearisation.
//!
//! A circular arc becomes chords whose sagitta (the gap between a chord and the arc) is at most
//! the tolerance. Vertices are computed with `libm` and rounded to the µm grid once; the last
//! vertex is the exact target.

use std::f64::consts::TAU;

use ps_core::PtUm;

#[expect(
    clippy::cast_precision_loss,
    reason = "grid coordinates are far below 2^53 µm, where i64 -> f64 is exact"
)]
fn as_f64(v: i64) -> f64 {
    v as f64
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "a rounded grid coordinate of a point on a circle that fits the machine"
)]
fn round_um(v: f64) -> i64 {
    libm::round(v) as i64
}

/// The chords that replace an arc, excluding the start and ending exactly at `to`.
///
/// `tolerance_mm` bounds the sagitta; a degenerate arc (zero radius) is one straight line.
#[must_use]
pub fn linearise(from: PtUm, to: PtUm, center: PtUm, ccw: bool, tolerance_mm: f64) -> Vec<PtUm> {
    let (cx, cy) = (as_f64(center.x), as_f64(center.y));
    let (fx, fy) = (as_f64(from.x) - cx, as_f64(from.y) - cy);
    let radius = libm::hypot(fx, fy);
    if radius < 1.0 {
        return vec![to];
    }
    let start = libm::atan2(fy, fx);
    let end = libm::atan2(as_f64(to.y) - cy, as_f64(to.x) - cx);
    let mut sweep = if ccw { end - start } else { start - end }.rem_euclid(TAU);
    if sweep == 0.0 && from != to {
        sweep = TAU;
    }
    if from == to {
        sweep = TAU;
    }

    // A chord subtending `step` has sagitta r (1 - cos(step / 2)).
    let max_gap_um = tolerance_mm * 1_000.0;
    let ratio = (1.0 - max_gap_um / radius).clamp(-1.0, 1.0);
    let chord_angle = 2.0 * libm::acos(ratio);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a positive, finite chord count"
    )]
    let chords = if chord_angle <= 0.0 {
        1
    } else {
        ((sweep / chord_angle).ceil() as usize).max(1)
    };

    let mut out = Vec::with_capacity(chords);
    for i in 1..chords {
        #[expect(clippy::cast_precision_loss, reason = "chord counts are small")]
        let t = i as f64 / chords as f64;
        let angle = if ccw {
            start + sweep * t
        } else {
            start - sweep * t
        };
        out.push(PtUm::new(
            round_um(cx + radius * libm::cos(angle)),
            round_um(cy + radius * libm::sin(angle)),
        ));
    }
    out.push(to);
    out
}

#[cfg(test)]
mod tests;
