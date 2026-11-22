//! Deterministic cubic Bézier helpers.
//!
//! Arc length, inverse arc length, sub-segments and flattening are built from `+ - * /`, `sqrt`
//! (exact in IEEE 754) and `libm::hypot`, so results are bit-identical on every operating
//! system. kurbo's own versions route through the platform's maths library.

use kurbo::{CubicBez, Point, Vec2};

use crate::arc;

/// Composite Gauss–Legendre panels used for arc length.
const PANELS: u32 = 16;
/// Bisection steps for the inverse arc length (resolves the parameter to about 1e-19).
const BISECTIONS: u32 = 64;

/// Five-point Gauss–Legendre abscissae on [-1, 1] and their weights.
const GAUSS: [(f64, f64); 5] = [
    (0.0, 0.568_888_888_888_888_9),
    (-0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
    (0.906_179_845_938_664, 0.236_926_885_056_189_1),
];

/// Evaluates the curve at parameter `t` using only multiplication and addition.
#[must_use]
pub fn eval(c: &CubicBez, t: f64) -> Point {
    let mt = 1.0 - t;
    let w0 = mt * mt * mt;
    let w1 = 3.0 * mt * mt * t;
    let w2 = 3.0 * mt * t * t;
    let w3 = t * t * t;
    Point::new(
        w0 * c.p0.x + w1 * c.p1.x + w2 * c.p2.x + w3 * c.p3.x,
        w0 * c.p0.y + w1 * c.p1.y + w2 * c.p2.y + w3 * c.p3.y,
    )
}

/// Evaluates the derivative (velocity) at `t`.
#[must_use]
pub fn velocity(c: &CubicBez, t: f64) -> Vec2 {
    let mt = 1.0 - t;
    let d0 = c.p1 - c.p0;
    let d1 = c.p2 - c.p1;
    let d2 = c.p3 - c.p2;
    (d0 * (mt * mt) + d1 * (2.0 * mt * t) + d2 * (t * t)) * 3.0
}

/// Returns the arc length between parameters `t0` and `t1`.
#[must_use]
pub fn arclen_range(c: &CubicBez, t0: f64, t1: f64) -> f64 {
    let panel = (t1 - t0) / f64::from(PANELS);
    let mut total = 0.0;
    for i in 0..PANELS {
        let lo = t0 + panel * f64::from(i);
        let mid = lo + panel / 2.0;
        let half = panel / 2.0;
        let mut sum = 0.0;
        for (x, w) in GAUSS {
            let v = velocity(c, mid + half * x);
            sum += w * libm::hypot(v.x, v.y);
        }
        total += sum * half;
    }
    total
}

/// Returns the total arc length.
#[must_use]
pub fn arclen(c: &CubicBez) -> f64 {
    arclen_range(c, 0.0, 1.0)
}

/// Returns the parameter at which the arc length from the start reaches `len`.
///
/// Lengths at or below zero give 0 and lengths at or beyond the total give 1.
#[must_use]
pub fn inv_arclen(c: &CubicBez, len: f64) -> f64 {
    if len <= 0.0 {
        return 0.0;
    }
    if len >= arclen(c) {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for _ in 0..BISECTIONS {
        let mid = lo.midpoint(hi);
        if arclen_range(c, 0.0, mid) < len {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo.midpoint(hi)
}

/// Returns the part of the curve between `t0` and `t1` as its own cubic.
#[must_use]
pub fn subsegment(c: &CubicBez, t0: f64, t1: f64) -> CubicBez {
    let third = (t1 - t0) / 3.0;
    let p0 = eval(c, t0);
    let p3 = eval(c, t1);
    CubicBez::new(
        p0,
        p0 + velocity(c, t0) * third,
        p3 - velocity(c, t1) * third,
        p3,
    )
}

/// Splits the curve at its parameter midpoint.
#[must_use]
pub fn split_half(c: &CubicBez) -> (CubicBez, CubicBez) {
    (subsegment(c, 0.0, 0.5), subsegment(c, 0.5, 1.0))
}

/// True when both control points lie within `tol` of the chord, so the whole curve does.
#[must_use]
pub fn is_straight(c: &CubicBez, tol: f64) -> bool {
    let chord = c.p3 - c.p0;
    let len = arc::length(chord);
    let off = |p: Point| {
        if len == 0.0 {
            arc::dist(p, c.p0)
        } else {
            (p - c.p0).cross(chord).abs() / len
        }
    };
    off(c.p1) <= tol && off(c.p2) <= tol
}

/// Flattens the curve to chords no further than `tol` from it.
///
/// Returns the vertices after the start point; the last one is exactly the curve's end.
/// The number of chords comes from Wang's bound `n = ceil(sqrt(3·2/8 · M / tol))`, where `M` is
/// the larger second difference of the control polygon.
#[must_use]
pub fn flatten(c: &CubicBez, tol: f64) -> Vec<Point> {
    if is_straight(c, tol) {
        return vec![c.p3];
    }
    let m = arc::length(c.p0.to_vec2() - c.p1.to_vec2() * 2.0 + c.p2.to_vec2()).max(arc::length(
        c.p1.to_vec2() - c.p2.to_vec2() * 2.0 + c.p3.to_vec2(),
    ));
    let ratio = (0.75 * m / tol).sqrt().ceil();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "ratio is a finite, non-negative ceil; saturating conversion is the intent"
    )]
    let chords = (ratio as u32).clamp(1, 1_000_000);
    let mut out = Vec::with_capacity(chords as usize);
    for i in 1..chords {
        out.push(eval(c, f64::from(i) / f64::from(chords)));
    }
    out.push(c.p3);
    out
}

#[cfg(test)]
mod tests;
