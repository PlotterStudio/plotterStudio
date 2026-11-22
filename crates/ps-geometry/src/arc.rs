//! Pure helpers for true circular arcs.
//!
//! Angles are radians, counter-clockwise positive, in the y-up maths convention the rest of the
//! crate uses. All trigonometry goes through `libm` so results are identical on every operating
//! system.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use kurbo::Point;

/// Relative tolerance below which a triple of points counts as collinear.
const COLLINEAR_EPS: f64 = 1e-12;

/// Returns the length of a vector, computed with `libm` so it is identical on every platform.
#[must_use]
pub fn length(v: kurbo::Vec2) -> f64 {
    libm::hypot(v.x, v.y)
}

/// Returns the distance between two points, computed with `libm`.
#[must_use]
pub fn dist(a: Point, b: Point) -> f64 {
    length(b - a)
}

/// Returns how far along the segment `from`–`to` (0 to 1) the point nearest `p` lies, and its
/// distance from `p`.
#[must_use]
pub fn nearest_on_line(from: Point, to: Point, p: Point) -> (f64, f64) {
    let edge = to - from;
    let len2 = edge.dot(edge);
    let t = if len2 > 0.0 {
        ((p - from).dot(edge) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (t, dist(from + edge * t, p))
}

/// Returns the bulge (tangent of a quarter of the sweep) for a signed sweep angle.
///
/// A bulge is positive for counter-clockwise arcs; a semicircle has bulge 1.
#[must_use]
pub fn bulge_from_sweep(sweep: f64) -> f64 {
    libm::tan(sweep / 4.0)
}

/// Returns the signed sweep angle for a bulge.
#[must_use]
pub fn sweep_from_bulge(bulge: f64) -> f64 {
    4.0 * libm::atan(bulge)
}

/// Returns the radius of the arc joining `p0` to `p1` with the given bulge.
///
/// A zero bulge is a straight line, whose radius is infinite.
#[must_use]
pub fn radius_from_bulge(p0: Point, p1: Point, bulge: f64) -> f64 {
    if bulge == 0.0 {
        return f64::INFINITY;
    }
    let half_chord = dist(p0, p1) / 2.0;
    half_chord * (1.0 + bulge * bulge) / (2.0 * bulge.abs())
}

/// Returns the centre of the arc joining `p0` to `p1` with the given bulge, or `None` for a
/// zero bulge (a straight line) or coincident end points.
#[must_use]
pub fn center_from_bulge(p0: Point, p1: Point, bulge: f64) -> Option<Point> {
    let chord = p1 - p0;
    let len = length(chord);
    if bulge == 0.0 || len == 0.0 {
        return None;
    }
    let mid = p0.midpoint(p1);
    // Unit normal to the left of the chord direction.
    let left = kurbo::Vec2::new(-chord.y, chord.x) / len;
    // Signed distance from the chord midpoint to the centre: a / tan(sweep / 2), written in the
    // bulge b = tan(sweep / 4) as a (1 - b^2) / (2 b).
    let dist = (len / 2.0) * (1.0 - bulge * bulge) / (2.0 * bulge);
    Some(mid + left * dist)
}

/// Returns the bulge of the arc from `p0` to `p1` about `center` in the given direction.
#[must_use]
pub fn bulge_from_arc(p0: Point, p1: Point, center: Point, ccw: bool) -> f64 {
    bulge_from_sweep(sweep(p0, p1, center, ccw))
}

/// Returns the signed sweep of the arc from `p0` to `p1` about `center`.
///
/// Counter-clockwise sweeps are in (0, 2π], clockwise sweeps in [-2π, 0). Coincident end points
/// mean a full turn.
#[must_use]
pub fn sweep(p0: Point, p1: Point, center: Point, ccw: bool) -> f64 {
    if p0 == p1 {
        return if ccw { TAU } else { -TAU };
    }
    let a0 = angle_of(p0, center);
    let a1 = angle_of(p1, center);
    if ccw {
        let d = (a1 - a0).rem_euclid(TAU);
        if d == 0.0 { TAU } else { d }
    } else {
        let d = (a0 - a1).rem_euclid(TAU);
        if d == 0.0 { -TAU } else { -d }
    }
}

/// Returns the polar angle of `p` about `center`.
#[must_use]
pub fn angle_of(p: Point, center: Point) -> f64 {
    libm::atan2(p.y - center.y, p.x - center.x)
}

/// Returns the point at `angle` on the circle with the given centre and radius.
#[must_use]
pub fn point_at(center: Point, radius: f64, angle: f64) -> Point {
    Point::new(
        center.x + radius * libm::cos(angle),
        center.y + radius * libm::sin(angle),
    )
}

/// Converts an arc into cubic Béziers, one per quarter turn or part of one.
///
/// Each element is `[control 1, control 2, end]`; the last end point is exactly `p1`.
#[must_use]
pub fn to_cubics(p0: Point, p1: Point, center: Point, ccw: bool) -> Vec<[Point; 3]> {
    let total = sweep(p0, p1, center, ccw);
    let radius = dist(p0, center);
    let a0 = angle_of(p0, center);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "ceil of a value in [0, 4] fits a u32 and is non-negative"
    )]
    let n = ((total.abs() / FRAC_PI_2 - 1e-9).ceil().max(1.0)) as u32;
    let step = total / f64::from(n);
    // 4/3 tan(step/4) is signed, so clockwise arcs get handles pointing the right way.
    let k = 4.0 / 3.0 * libm::tan(step / 4.0);

    let mut out = Vec::with_capacity(n as usize);
    let mut from = p0;
    for i in 0..n {
        let ang0 = a0 + step * f64::from(i);
        let ang1 = a0 + step * f64::from(i + 1);
        let to = if i + 1 == n {
            p1
        } else {
            point_at(center, radius, ang1)
        };
        let t0 = kurbo::Vec2::new(-libm::sin(ang0), libm::cos(ang0));
        let t1 = kurbo::Vec2::new(-libm::sin(ang1), libm::cos(ang1));
        out.push([from + t0 * (k * radius), to - t1 * (k * radius), to]);
        from = to;
    }
    out
}

/// Returns the centre and direction of the circle through three points, travelled from `p0`
/// through `p1` to `p2`, or `None` if they are collinear or coincide.
#[must_use]
pub fn from_three_points(p0: Point, p1: Point, p2: Point) -> Option<(Point, bool)> {
    let cross = (p1 - p0).cross(p2 - p0);
    let scale = (p1 - p0).hypot2().max((p2 - p0).hypot2());
    if cross.abs() <= COLLINEAR_EPS * scale {
        return None;
    }
    let d = 2.0 * (p0.x * (p1.y - p2.y) + p1.x * (p2.y - p0.y) + p2.x * (p0.y - p1.y));
    let s0 = p0.to_vec2().hypot2();
    let s1 = p1.to_vec2().hypot2();
    let s2 = p2.to_vec2().hypot2();
    let ux = (s0 * (p1.y - p2.y) + s1 * (p2.y - p0.y) + s2 * (p0.y - p1.y)) / d;
    let uy = (s0 * (p2.x - p1.x) + s1 * (p0.x - p2.x) + s2 * (p1.x - p0.x)) / d;
    Some((Point::new(ux, uy), cross > 0.0))
}

/// Returns the largest angular step (radians) for which a chord of an arc of `radius` deviates
/// from the arc by at most `tol`.
///
/// The step is capped at a half turn, which also covers degenerate radii.
#[must_use]
pub fn max_step(radius: f64, tol: f64) -> f64 {
    if radius <= 0.0 || tol >= radius {
        return PI;
    }
    (2.0 * libm::acos(1.0 - tol / radius)).min(PI)
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "tests are the independent oracle and may use platform maths"
)]
mod tests;
