//! The path type: lines, true circular arcs and cubic Béziers.
//!
//! Arcs are kept as arcs (not cubics) so G2/G3-capable machines and arc-preserving offsets can
//! use them. A closed path always ends where it starts: [`Path::closed`] appends the closing
//! line when needed.

use std::f64::consts::{FRAC_PI_2, TAU};

use kurbo::{Affine, CubicBez, ParamCurveExtrema};
use kurbo::{Point, Rect, Vec2};
use serde::{Deserialize, Serialize};

use crate::{arc, cubic};

/// Distances closer than this to a path end are not treated as cuts, in mm.
const CUT_EPS: f64 = 1e-9;

/// One segment of a [`Path`], running from the previous end point to `to`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Seg {
    /// A straight line.
    Line {
        /// End point.
        to: Point,
    },
    /// A true circular arc, kept for G2/G3.
    Arc {
        /// End point.
        to: Point,
        /// Centre of the circle.
        center: Point,
        /// Direction of travel.
        ccw: bool,
    },
    /// A cubic Bézier.
    Cubic {
        /// First control point.
        c1: Point,
        /// Second control point.
        c2: Point,
        /// End point.
        to: Point,
    },
}

impl Seg {
    /// Returns the end point.
    #[must_use]
    pub fn to(&self) -> Point {
        match *self {
            Self::Line { to } | Self::Arc { to, .. } | Self::Cubic { to, .. } => to,
        }
    }

    /// Returns the length of the segment when it starts at `from`.
    #[must_use]
    pub fn length(&self, from: Point) -> f64 {
        match *self {
            Self::Line { to } => arc::dist(from, to),
            Self::Arc { to, center, ccw } => {
                arc::dist(from, center) * arc::sweep(from, to, center, ccw).abs()
            }
            Self::Cubic { c1, c2, to } => cubic::arclen(&CubicBez::new(from, c1, c2, to)),
        }
    }
}

/// A connected run of segments, open or closed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Path {
    /// Where the path starts.
    pub start: Point,
    /// The segments, in order.
    pub segs: Vec<Seg>,
    /// Whether the path is a closed ring (its last segment ends at `start`).
    pub closed: bool,
}

impl Path {
    /// Starts an empty open path at `start`.
    #[must_use]
    pub fn new(start: Point) -> Self {
        Self {
            start,
            segs: Vec::new(),
            closed: false,
        }
    }

    /// Appends a line.
    #[must_use]
    pub fn line_to(mut self, to: Point) -> Self {
        self.segs.push(Seg::Line { to });
        self
    }

    /// Appends a circular arc about `center`.
    #[must_use]
    pub fn arc_to(mut self, to: Point, center: Point, ccw: bool) -> Self {
        self.segs.push(Seg::Arc { to, center, ccw });
        self
    }

    /// Appends a cubic Bézier.
    #[must_use]
    pub fn cubic_to(mut self, c1: Point, c2: Point, to: Point) -> Self {
        self.segs.push(Seg::Cubic { c1, c2, to });
        self
    }

    /// Closes the path, adding a closing line if the end is not already the start.
    #[must_use]
    pub fn closed(mut self) -> Self {
        if self.end() != self.start {
            self.segs.push(Seg::Line { to: self.start });
        }
        self.closed = true;
        self
    }

    /// Builds a path through `points`, optionally closed. An empty slice gives an empty path at
    /// the origin.
    #[must_use]
    pub fn polyline(points: &[Point], closed: bool) -> Self {
        let Some((&first, rest)) = points.split_first() else {
            return Self::new(Point::ORIGIN);
        };
        let mut p = Self::new(first);
        p.segs = rest.iter().map(|&to| Seg::Line { to }).collect();
        if closed { p.closed() } else { p }
    }

    /// Builds a closed counter-clockwise rectangle (y-up) with its corner at `(x, y)`.
    #[must_use]
    pub fn rect(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self::polyline(
            &[
                Point::new(x, y),
                Point::new(x + w, y),
                Point::new(x + w, y + h),
                Point::new(x, y + h),
            ],
            true,
        )
    }

    /// Builds a closed counter-clockwise circle from two half-turn arcs.
    #[must_use]
    pub fn circle(center: Point, radius: f64) -> Self {
        let east = Point::new(center.x + radius, center.y);
        let west = Point::new(center.x - radius, center.y);
        Self::new(east)
            .arc_to(west, center, true)
            .arc_to(east, center, true)
            .closed()
    }

    /// Returns the end point (the start point for an empty path).
    #[must_use]
    pub fn end(&self) -> Point {
        self.segs.last().map_or(self.start, Seg::to)
    }

    /// Returns true when there are no segments.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }

    /// Iterates over `(start point, segment)` pairs.
    pub fn segments(&self) -> impl Iterator<Item = (Point, &Seg)> {
        let mut from = self.start;
        self.segs.iter().map(move |seg| {
            let item = (from, seg);
            from = seg.to();
            item
        })
    }

    /// Returns the total length in mm.
    #[must_use]
    pub fn length(&self) -> f64 {
        self.segments().map(|(from, seg)| seg.length(from)).sum()
    }

    /// Returns the signed area enclosed, treating an open path as closed by a straight line.
    ///
    /// Positive for counter-clockwise paths in the y-up convention.
    #[must_use]
    pub fn signed_area(&self) -> f64 {
        let mut twice = 0.0;
        for (from, seg) in self.segments() {
            twice += match *seg {
                Seg::Line { to } => from.x * to.y - to.x * from.y,
                Seg::Arc { to, center, ccw } => {
                    let r = arc::dist(from, center);
                    center.x * (to.y - from.y) - center.y * (to.x - from.x)
                        + r * r * arc::sweep(from, to, center, ccw)
                }
                Seg::Cubic { c1, c2, to } => cubic_twice_area(from, c1, c2, to),
            };
        }
        // Chord that closes an open path.
        let end = self.end();
        twice += end.x * self.start.y - self.start.x * end.y;
        twice / 2.0
    }

    /// Returns the tight bounding box. An empty path gives a degenerate box at its start.
    #[must_use]
    pub fn bounds(&self) -> Rect {
        let mut r = Rect::from_points(self.start, self.start);
        for (from, seg) in self.segments() {
            match *seg {
                Seg::Line { to } => r = r.union_pt(to),
                Seg::Arc { to, center, ccw } => {
                    r = r.union_pt(to);
                    r = union_arc_extremes(r, from, to, center, ccw);
                }
                Seg::Cubic { c1, c2, to } => {
                    r = r.union(CubicBez::new(from, c1, c2, to).bounding_box());
                }
            }
        }
        r
    }

    /// Returns the path travelled in the opposite direction.
    #[must_use]
    pub fn reversed(&self) -> Self {
        let mut segs = Vec::with_capacity(self.segs.len());
        let froms: Vec<Point> = self.segments().map(|(f, _)| f).collect();
        for (seg, &from) in self.segs.iter().zip(&froms).rev() {
            segs.push(match *seg {
                Seg::Line { .. } => Seg::Line { to: from },
                Seg::Arc { center, ccw, .. } => Seg::Arc {
                    to: from,
                    center,
                    ccw: !ccw,
                },
                Seg::Cubic { c1, c2, .. } => Seg::Cubic {
                    c1: c2,
                    c2: c1,
                    to: from,
                },
            });
        }
        Self {
            start: self.end(),
            segs,
            closed: self.closed,
        }
    }

    /// Applies an affine transform.
    ///
    /// Arcs stay arcs under translation, rotation, mirroring and uniform scale. Under any other
    /// transform (non-uniform scale, skew) a circle is no longer a circle, so arcs become cubics.
    #[must_use]
    pub fn transformed(&self, t: Affine) -> Self {
        let keep_arcs = is_similarity(t);
        let mirrored = t.determinant() < 0.0;
        let mut segs = Vec::with_capacity(self.segs.len());
        for (from, seg) in self.segments() {
            match *seg {
                Seg::Line { to } => segs.push(Seg::Line { to: t * to }),
                Seg::Cubic { c1, c2, to } => segs.push(Seg::Cubic {
                    c1: t * c1,
                    c2: t * c2,
                    to: t * to,
                }),
                Seg::Arc { to, center, ccw } if keep_arcs => segs.push(Seg::Arc {
                    to: t * to,
                    center: t * center,
                    ccw: ccw != mirrored,
                }),
                Seg::Arc { to, center, ccw } => {
                    segs.extend(arc::to_cubics(from, to, center, ccw).into_iter().map(
                        |[c1, c2, to]| Seg::Cubic {
                            c1: t * c1,
                            c2: t * c2,
                            to: t * to,
                        },
                    ));
                }
            }
        }
        Self {
            start: t * self.start,
            segs,
            closed: self.closed,
        }
    }

    /// Returns the point and unit tangent at arc-length `distance` from the start.
    ///
    /// `None` when the distance is outside the path or the path has no length.
    #[must_use]
    pub fn sample_at(&self, distance: f64) -> Option<(Point, Vec2)> {
        if distance < 0.0 {
            return None;
        }
        let mut walked = 0.0;
        for (from, seg) in self.segments() {
            let len = seg.length(from);
            if distance <= walked + len + CUT_EPS && len > 0.0 {
                let local = (distance - walked).clamp(0.0, len);
                return Some(sample_segment(from, seg, local, len));
            }
            walked += len;
        }
        None
    }

    /// The arc-length distance from the start of the outline point nearest `p`, within about
    /// `tol` mm (curves are measured through a polyline of that accuracy).
    ///
    /// `None` for a path with no length.
    #[must_use]
    pub fn distance_of_nearest(&self, p: Point, tol: f64) -> Option<f64> {
        let mut points = crate::flatten::flatten(self, tol);
        if self.closed {
            points.push(self.start);
        }
        let mut walked = 0.0;
        let mut best: Option<(f64, f64)> = None; // (distance to p, distance along the path)
        for pair in points.windows(2) {
            let len = arc::dist(pair[0], pair[1]);
            if len <= 0.0 {
                continue;
            }
            let (t, d) = arc::nearest_on_line(pair[0], pair[1], p);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, walked + t * len));
            }
            walked += len;
        }
        best.map(|(_, along)| along)
    }

    /// A closed path rotated to start `distance` mm along itself; an open path comes back as is.
    #[must_use]
    pub fn rotated_to_distance(&self, distance: f64) -> Self {
        if !self.closed {
            return self.clone();
        }
        let pieces = self.split_at_lengths(&[distance]);
        let [first, second] = pieces.as_slice() else {
            return self.clone();
        };
        let mut rotated = Self::new(second.start);
        rotated.segs.extend(second.segs.iter().copied());
        rotated.segs.extend(first.segs.iter().copied());
        rotated.closed = true;
        rotated
    }

    /// Splits the path at the given arc-length distances into open pieces.
    ///
    /// Distances are sorted and de-duplicated; those at or beyond either end are ignored, so no
    /// empty piece is produced. With no valid cut the path is returned whole.
    #[must_use]
    pub fn split_at_lengths(&self, distances: &[f64]) -> Vec<Self> {
        let total = self.length();
        let mut cuts: Vec<f64> = distances
            .iter()
            .copied()
            .filter(|d| d.is_finite() && *d > CUT_EPS && *d < total - CUT_EPS)
            .collect();
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() <= CUT_EPS);
        if cuts.is_empty() {
            return vec![self.clone()];
        }

        let mut bounds = vec![0.0];
        bounds.extend(cuts);
        bounds.push(total);
        bounds
            .windows(2)
            .map(|w| self.sub_path(w[0], w[1]))
            .collect()
    }

    /// Extracts the open piece between two arc-length distances.
    fn sub_path(&self, d0: f64, d1: f64) -> Self {
        let mut piece: Option<Self> = None;
        let mut walked = 0.0;
        for (from, seg) in self.segments() {
            let len = seg.length(from);
            let (s0, s1) = (walked, walked + len);
            walked = s1;
            if len <= 0.0 || s1 <= d0 + CUT_EPS || s0 >= d1 - CUT_EPS {
                continue;
            }
            let a = (d0 - s0).max(0.0);
            let b = (d1 - s0).min(len);
            let (start, sub) = if a <= CUT_EPS && b >= len - CUT_EPS {
                (from, *seg) // the whole segment: keep exact coordinates
            } else {
                cut_segment(from, seg, a, b, len)
            };
            piece.get_or_insert_with(|| Self::new(start)).segs.push(sub);
        }
        piece.unwrap_or_else(|| Self::new(self.start))
    }
}

/// Twice the signed area swept by a cubic about the origin, ∫(x dy − y dx), by exact Gauss
/// quadrature (the integrand is a quintic polynomial in the parameter).
fn cubic_twice_area(p0: Point, p1: Point, p2: Point, p3: Point) -> f64 {
    let curve = CubicBez::new(p0, p1, p2, p3);
    let spread = (0.6_f64).sqrt() / 2.0;
    let nodes = [
        (0.5 - spread, 5.0 / 18.0),
        (0.5, 8.0 / 18.0),
        (0.5 + spread, 5.0 / 18.0),
    ];
    nodes
        .iter()
        .map(|&(t, weight)| {
            let pos = cubic::eval(&curve, t);
            let vel = cubic::velocity(&curve, t);
            weight * (pos.x * vel.y - pos.y * vel.x)
        })
        .sum()
}

/// Returns true when `t` maps circles to circles (uniform scale, rotation, mirroring).
fn is_similarity(t: Affine) -> bool {
    let [xx, xy, yx, yy, _, _] = t.as_coeffs();
    let col0 = xx * xx + xy * xy;
    let col1 = yx * yx + yy * yy;
    let scale = col0.max(col1).max(f64::MIN_POSITIVE);
    (col0 - col1).abs() <= 1e-12 * scale && (xx * yx + xy * yy).abs() <= 1e-12 * scale
}

/// Extends `r` by the axis-aligned extreme points that lie on the arc.
fn union_arc_extremes(mut r: Rect, from: Point, to: Point, center: Point, ccw: bool) -> Rect {
    let radius = arc::dist(from, center);
    let total = arc::sweep(from, to, center, ccw).abs();
    let a0 = arc::angle_of(from, center);
    for k in 0..4 {
        let axis = FRAC_PI_2 * f64::from(k);
        let along = if ccw { axis - a0 } else { a0 - axis }.rem_euclid(TAU);
        if along <= total {
            r = r.union_pt(arc::point_at(center, radius, axis));
        }
    }
    r
}

/// Returns the sub-segment covering local lengths `a..b` of a segment of length `len`,
/// together with its start point.
fn cut_segment(from: Point, seg: &Seg, a: f64, b: f64, len: f64) -> (Point, Seg) {
    match *seg {
        Seg::Line { to } => {
            let p0 = from.lerp(to, a / len);
            let p1 = from.lerp(to, b / len);
            (p0, Seg::Line { to: p1 })
        }
        Seg::Arc { to, center, ccw } => {
            let radius = arc::dist(from, center);
            let total = arc::sweep(from, to, center, ccw);
            let a0 = arc::angle_of(from, center);
            let p0 = arc::point_at(center, radius, a0 + total * (a / len));
            let p1 = arc::point_at(center, radius, a0 + total * (b / len));
            (
                p0,
                Seg::Arc {
                    to: p1,
                    center,
                    ccw,
                },
            )
        }
        Seg::Cubic { c1, c2, to } => {
            let c = CubicBez::new(from, c1, c2, to);
            let t0 = cubic::inv_arclen(&c, a);
            let t1 = cubic::inv_arclen(&c, b);
            let s = cubic::subsegment(&c, t0, t1);
            (
                s.p0,
                Seg::Cubic {
                    c1: s.p1,
                    c2: s.p2,
                    to: s.p3,
                },
            )
        }
    }
}

/// Returns the point and unit tangent at local length `local` of a segment of length `len`.
fn sample_segment(from: Point, seg: &Seg, local: f64, len: f64) -> (Point, Vec2) {
    match *seg {
        Seg::Line { to } => {
            let dir = to - from;
            (from.lerp(to, local / len), dir / arc::length(dir))
        }
        Seg::Arc { to, center, ccw } => {
            let radius = arc::dist(from, center);
            let total = arc::sweep(from, to, center, ccw);
            let angle = arc::angle_of(from, center) + total * (local / len);
            let sign = if ccw { 1.0 } else { -1.0 };
            let tangent = Vec2::new(-libm::sin(angle), libm::cos(angle)) * sign;
            (arc::point_at(center, radius, angle), tangent)
        }
        Seg::Cubic { c1, c2, to } => {
            let c = CubicBez::new(from, c1, c2, to);
            let t = cubic::inv_arclen(&c, local);
            let v = cubic::velocity(&c, t);
            (cubic::eval(&c, t), v / arc::length(v))
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "tests are the independent oracle and may use platform maths"
)]
mod tests;
