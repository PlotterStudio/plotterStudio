//! Motion planner for time estimation.
//!
//! Each move gets a trapezoidal velocity profile. A look-ahead planner (forward and backward
//! passes, like GRBL) limits the speed at each junction by the junction-deviation formula and by
//! what the acceleration allows over the neighbouring moves. The plan starts and ends at rest.

/// The slowest a junction is ever taken, in mm/s (a reversal does not stop dead in the model).
pub const MIN_JUNCTION_SPEED: f64 = 0.1;

/// A unit direction of travel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dir {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
}

/// One straight move to plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Move {
    /// Length in mm.
    pub length_mm: f64,
    /// Unit direction of travel.
    pub dir: Dir,
    /// Requested feed in mm/s.
    pub feed_mm_s: f64,
}

/// Machine motion limits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionLimits {
    /// Highest speed the machine reaches while drawing, in mm/s.
    pub max_speed_mm_s: f64,
    /// Highest speed it reaches moving with the tool up, in mm/s.
    pub max_travel_speed_mm_s: f64,
    /// Acceleration in mm/s².
    pub accel_mm_s2: f64,
    /// Junction deviation in mm.
    pub junction_deviation_mm: f64,
}

/// The result of planning a run of moves.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Total time in seconds.
    pub total_s: f64,
    /// Speed at each junction: one more than the number of moves, first and last zero.
    pub junction_speeds: Vec<f64>,
}

/// The acceleration at `speed_mm_s` from `(speed, acceleration)` points in ascending speed:
/// straight lines between the points, the last value above them, and in proportion to the speed
/// below the first. `fallback` when there are no points.
#[must_use]
pub(crate) fn accel_at(table: &[(f64, f64)], fallback: f64, speed_mm_s: f64) -> f64 {
    let (Some(first), Some(last)) = (table.first(), table.last()) else {
        return fallback;
    };
    if speed_mm_s <= first.0 {
        return first.1 * speed_mm_s / first.0;
    }
    if speed_mm_s >= last.0 {
        return last.1;
    }
    table
        .windows(2)
        .find(|w| speed_mm_s <= w[1].0)
        .map_or(last.1, |w| {
            let t = (speed_mm_s - w[0].0) / (w[1].0 - w[0].0);
            w[0].1 + (w[1].1 - w[0].1) * t
        })
}

/// The milliseconds the machine spends on one vector drawn at `speed_mm_s`, from
/// `(speed, milliseconds)` points in ascending speed: straight lines between them and the end
/// values held beyond. Nothing without points.
#[must_use]
pub(crate) fn vector_ms_at(table: &[(f64, f64)], speed_mm_s: f64) -> f64 {
    let (Some(first), Some(last)) = (table.first(), table.last()) else {
        return 0.0;
    };
    if speed_mm_s <= first.0 {
        return first.1;
    }
    if speed_mm_s >= last.0 {
        return last.1;
    }
    table
        .windows(2)
        .find(|w| speed_mm_s <= w[1].0)
        .map_or(last.1, |w| {
            let t = (speed_mm_s - w[0].0) / (w[1].0 - w[0].0);
            w[0].1 + (w[1].1 - w[0].1) * t
        })
}

/// Time to cover `length` mm entering at `v_in`, leaving at `v_out`, cruising at up to `v_cruise`
/// with constant acceleration `accel`: a trapezoid, or a triangle when the move is too short.
#[must_use]
pub fn segment_time(length: f64, v_in: f64, v_cruise: f64, v_out: f64, accel: f64) -> f64 {
    if length <= 0.0 || accel <= 0.0 {
        return 0.0;
    }
    let cruise = v_cruise.max(v_in).max(v_out);
    let up = (cruise * cruise - v_in * v_in) / (2.0 * accel);
    let down = (cruise * cruise - v_out * v_out) / (2.0 * accel);
    if up + down > length {
        let peak = (f64::midpoint(2.0 * accel * length, v_in * v_in + v_out * v_out)).sqrt();
        (peak - v_in) / accel + (peak - v_out) / accel
    } else {
        (cruise - v_in) / accel + (cruise - v_out) / accel + (length - up - down) / cruise
    }
}

/// The fastest a junction between directions `a` and `b` may be taken (junction deviation).
///
/// `v² = accel · deviation · s / (1 − s)` with `s = sin(θ/2) = sqrt((1 + a·b) / 2)`, capped at
/// `max` and floored at [`MIN_JUNCTION_SPEED`].
#[must_use]
pub fn junction_speed(a: Dir, b: Dir, accel: f64, deviation: f64, max: f64) -> f64 {
    let cos = (a.x * b.x + a.y * b.y).clamp(-1.0, 1.0);
    let s = f64::midpoint(1.0, cos).sqrt();
    if s >= 1.0 - 1e-12 {
        return max;
    }
    let v = (accel * deviation * s / (1.0 - s)).sqrt();
    v.min(max).max(MIN_JUNCTION_SPEED.min(max))
}

/// Plans a run of moves from rest to rest.
#[must_use]
pub fn plan(moves: &[Move], limits: &MotionLimits) -> Plan {
    let n = moves.len();
    let a = limits.accel_mm_s2;
    let cruise: Vec<f64> = moves
        .iter()
        .map(|m| {
            m.feed_mm_s
                .min(limits.max_speed_mm_s.max(limits.max_travel_speed_mm_s))
        })
        .collect();
    let mut v = vec![0.0_f64; n + 1];
    for i in 1..n {
        v[i] = junction_speed(
            moves[i - 1].dir,
            moves[i].dir,
            a,
            limits.junction_deviation_mm,
            cruise[i - 1].min(cruise[i]),
        );
    }
    // Forward pass: speed reachable by accelerating over the previous move.
    for i in 0..n {
        let reach = (v[i] * v[i] + 2.0 * a * moves[i].length_mm).sqrt();
        v[i + 1] = v[i + 1].min(reach);
    }
    v[n] = 0.0;
    // Backward pass: speed from which the next move can still brake to its exit speed.
    for i in (0..n).rev() {
        let reach = (v[i + 1] * v[i + 1] + 2.0 * a * moves[i].length_mm).sqrt();
        v[i] = v[i].min(reach);
    }
    let total_s = (0..n)
        .map(|i| segment_time(moves[i].length_mm, v[i], cruise[i], v[i + 1], a))
        .sum();
    Plan {
        total_s,
        junction_speeds: v,
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "tests are the independent oracle and may use platform maths"
)]
mod tests;
