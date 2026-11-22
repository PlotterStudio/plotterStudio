//! Refitting polylines with lines and cubic Béziers.
//!
//! Used after the grid boolean fallback so a result that came back as thousands of short chords
//! is editable again. The polyline is split at *corners* (turning angle above [`CORNER_ANGLE`])
//! so sharp features stay sharp; every smooth run between corners becomes a line when it is
//! straight within the tolerance, otherwise a few cubics from Schneider's least-squares fit
//! ("An Algorithm for Automatically Fitting Digitized Curves", Graphics Gems I), with
//! Newton–Raphson reparameterisation and splitting at the worst point.
//!
//! Only `+ - * /`, `sqrt` and `libm` are used, so the result is identical on every platform.

use kurbo::{CubicBez, Point, Vec2};

use crate::arc;
use crate::cubic;
use crate::path::{Path, Seg};

/// Turning angle (radians) above which a polyline vertex is a corner, about 28.6°.
///
/// Flattening a circle at 0.002 mm turns the direction by 2·acos(1 − 0.002/r) per vertex, which
/// is below this for any radius above 0.03 mm; real design corners are far above it.
pub const CORNER_ANGLE: f64 = 0.5;

/// Newton–Raphson reparameterisation rounds before a run is split.
const REPARAM_ROUNDS: u32 = 4;
/// A fit within this multiple of the tolerance is worth reparameterising instead of splitting.
const REPARAM_RATIO: f64 = 4.0;
/// Recursion cap for splitting; deeper runs fall back to straight chords.
const MAX_DEPTH: u32 = 48;

fn dedupe(points: &[Point], closed: bool) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(points.len());
    for &p in points {
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    if closed && out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

fn unit(v: Vec2) -> Vec2 {
    let len = arc::length(v);
    if len == 0.0 { Vec2::ZERO } else { v / len }
}

/// Turning angle at `here` when travelling prev → here → next.
fn turn(prev: Point, here: Point, next: Point) -> f64 {
    let inbound = here - prev;
    let outbound = next - here;
    libm::atan2(inbound.cross(outbound).abs(), inbound.dot(outbound))
}

/// Refits a closed ring of vertices (the closing edge is implied).
#[must_use]
pub fn refit_ring(points: &[Point], tol: f64) -> Path {
    refit_polyline(points, true, tol)
}

/// Refits a polyline, open or closed, with lines and cubics.
///
/// Corners and the end points of an open polyline are kept exactly.
#[must_use]
pub fn refit_polyline(points: &[Point], closed: bool, tol: f64) -> Path {
    let pts = dedupe(points, closed);
    match pts.len() {
        0 => return Path::new(Point::ORIGIN),
        1 => return Path::new(pts[0]),
        2 => return Path::polyline(&pts, closed),
        _ => {}
    }
    let n = pts.len();

    // Indices of the vertices that split the polyline into smooth runs.
    let corners: Vec<usize> = if closed {
        (0..n)
            .filter(|&i| turn(pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]) > CORNER_ANGLE)
            .collect()
    } else {
        std::iter::once(0)
            .chain((1..n - 1).filter(|&i| turn(pts[i - 1], pts[i], pts[i + 1]) > CORNER_ANGLE))
            .chain(std::iter::once(n - 1))
            .collect()
    };

    let mut path = Path::new(pts[0]);
    if closed {
        if corners.is_empty() {
            // One smooth ring: cut it at vertex 0, with tangents continuing through the seam.
            let mut run = pts.clone();
            run.push(pts[0]);
            let tangent = unit(pts[1] - pts[n - 1]);
            fit_run(&run, Some((tangent, tangent)), tol, &mut path.segs);
        } else {
            let first = corners[0];
            path = Path::new(pts[first]);
            for (k, &a) in corners.iter().enumerate() {
                let b = corners[(k + 1) % corners.len()];
                let mut run = Vec::new();
                let mut i = a;
                loop {
                    run.push(pts[i]);
                    if i == b && run.len() > 1 {
                        break;
                    }
                    i = (i + 1) % n;
                }
                fit_run(&run, None, tol, &mut path.segs);
            }
        }
        path.closed = true;
    } else {
        for w in corners.windows(2) {
            fit_run(&pts[w[0]..=w[1]], None, tol, &mut path.segs);
        }
    }
    path
}

/// Fits one smooth run (corner to corner) and appends its segments.
///
/// `ends` gives explicit unit tangents (forward at the start, forward at the end) for a seamless
/// closed ring; otherwise they are estimated from the end chords.
fn fit_run(run: &[Point], ends: Option<(Vec2, Vec2)>, tol: f64, out: &mut Vec<Seg>) {
    let (Some(&first), Some(&last)) = (run.first(), run.last()) else {
        return;
    };
    if run.len() == 2 || is_straight(run, tol) {
        out.push(Seg::Line { to: last });
        return;
    }
    let (t_start, t_end_forward) =
        ends.unwrap_or_else(|| (unit(run[1] - first), unit(last - run[run.len() - 2])));
    // Schneider's end tangent at the far end points back into the curve.
    fit_cubic(run, t_start, -t_end_forward, tol, 0, out);
}

/// True when every point lies within `tol` of the chord.
fn is_straight(run: &[Point], tol: f64) -> bool {
    let (Some(&first), Some(&last)) = (run.first(), run.last()) else {
        return true;
    };
    let chord = last - first;
    let len = arc::length(chord);
    if len == 0.0 {
        return false;
    }
    run.iter()
        .all(|&p| (p - first).cross(chord).abs() / len <= tol)
}

fn bernstein(t: f64) -> [f64; 4] {
    let mt = 1.0 - t;
    [mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t]
}

fn chord_length_parameters(pts: &[Point]) -> Vec<f64> {
    let mut u = vec![0.0];
    for w in pts.windows(2) {
        let last = u.last().copied().unwrap_or(0.0);
        u.push(last + arc::dist(w[0], w[1]));
    }
    let total = u.last().copied().unwrap_or(0.0);
    if total > 0.0 {
        for v in &mut u {
            *v /= total;
        }
    }
    u
}

/// Least-squares cubic with fixed end points and tangent directions.
fn generate_bezier(pts: &[Point], u: &[f64], t1: Vec2, t2: Vec2) -> CubicBez {
    let (first, last) = (pts[0], pts[pts.len() - 1]);
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (&p, &t) in pts.iter().zip(u) {
        let [b0, b1, b2, b3] = bernstein(t);
        let a1 = t1 * b1;
        let a2 = t2 * b2;
        c00 += a1.dot(a1);
        c01 += a1.dot(a2);
        c11 += a2.dot(a2);
        let tmp = p - (first.to_vec2() * (b0 + b1) + last.to_vec2() * (b2 + b3)).to_point();
        x0 += a1.dot(tmp);
        x1 += a2.dot(tmp);
    }
    let det = c00 * c11 - c01 * c01;
    let (mut alpha_l, mut alpha_r) = if det == 0.0 {
        (0.0, 0.0)
    } else {
        ((x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det)
    };
    let seg_len = arc::dist(first, last);
    let eps = 1e-6 * seg_len;
    if alpha_l < eps || alpha_r < eps {
        alpha_l = seg_len / 3.0;
        alpha_r = seg_len / 3.0;
    }
    CubicBez::new(first, first + t1 * alpha_l, last + t2 * alpha_r, last)
}

/// Largest distance from the points to the curve, and where to split if it is too large.
///
/// The split index is the interior point nearest the middle among those whose error is within
/// 5% of the worst, so a symmetric error does not produce a lopsided split.
fn max_error(pts: &[Point], c: &CubicBez, u: &[f64]) -> (f64, usize) {
    let errors: Vec<f64> = pts
        .iter()
        .zip(u)
        .map(|(&p, &t)| arc::dist(cubic::eval(c, t), p))
        .collect();
    let interior = 1..pts.len().saturating_sub(1);
    let worst = interior.clone().map(|i| errors[i]).fold(0.0, f64::max);
    let middle = pts.len() / 2;
    let split = interior
        .filter(|&i| errors[i] >= 0.95 * worst)
        .min_by_key(|&i| i.abs_diff(middle))
        .unwrap_or(middle);
    (worst, split)
}

/// One Newton–Raphson step toward the closest curve parameter for each point.
fn reparameterize(pts: &[Point], u: &[f64], c: &CubicBez) -> Vec<f64> {
    pts.iter()
        .zip(u)
        .map(|(&p, &t)| {
            let d = cubic::eval(c, t) - p;
            let d1 = cubic::velocity(c, t);
            let q0 = c.p2.to_vec2() - c.p1.to_vec2() * 2.0 + c.p0.to_vec2();
            let q1 = c.p3.to_vec2() - c.p2.to_vec2() * 2.0 + c.p1.to_vec2();
            let d2 = (q0 * (1.0 - t) + q1 * t) * 6.0;
            let num = d.dot(d1);
            let den = d1.dot(d1) + d.dot(d2);
            if den == 0.0 {
                t
            } else {
                (t - num / den).clamp(0.0, 1.0)
            }
        })
        .collect()
}

fn fit_cubic(pts: &[Point], t1: Vec2, t2: Vec2, tol: f64, depth: u32, out: &mut Vec<Seg>) {
    let (first, last) = (pts[0], pts[pts.len() - 1]);
    if pts.len() == 2 {
        let third = arc::dist(first, last) / 3.0;
        out.push(Seg::Cubic {
            c1: first + t1 * third,
            c2: last + t2 * third,
            to: last,
        });
        return;
    }
    let mut u = chord_length_parameters(pts);
    let mut bez = generate_bezier(pts, &u, t1, t2);
    let (mut err, mut split) = max_error(pts, &bez, &u);
    if err <= tol {
        out.push(seg_of(&bez));
        return;
    }
    if err <= tol * REPARAM_RATIO {
        for _ in 0..REPARAM_ROUNDS {
            u = reparameterize(pts, &u, &bez);
            bez = generate_bezier(pts, &u, t1, t2);
            (err, split) = max_error(pts, &bez, &u);
            if err <= tol {
                out.push(seg_of(&bez));
                return;
            }
        }
    }
    if depth >= MAX_DEPTH || pts.len() < 4 {
        // Cannot split further: keep the original chords, which are within the input's own error.
        out.extend(pts.windows(2).map(|w| Seg::Line { to: w[1] }));
        return;
    }
    let split = split.clamp(1, pts.len() - 2);
    let centre = unit(pts[split - 1] - pts[split + 1]);
    fit_cubic(&pts[..=split], t1, centre, tol, depth + 1, out);
    fit_cubic(&pts[split..], -centre, t2, tol, depth + 1, out);
}

fn seg_of(c: &CubicBez) -> Seg {
    Seg::Cubic {
        c1: c.p1,
        c2: c.p2,
        to: c.p3,
    }
}

#[cfg(test)]
mod tests;
