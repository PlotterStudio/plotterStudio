//! Snapping: pulling the cursor onto something meaningful.
//!
//! Candidates within [`SNAP_RADIUS_PX`] are considered. The nearest wins; when two are about as
//! near, the priority node > intersection > anchor > guide > grid breaks the tie, and the result
//! says what was hit so the canvas can explain the snap.

use ps_geometry::{Point, Rect};

/// How close, in screen pixels, a candidate must be to snap.
pub const SNAP_RADIUS_PX: f64 = 8.0;
/// Candidates closer than this many pixels to the nearest count as tied.
const TIE_PX: f64 = 0.5;

/// What a snap candidate is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapKind {
    /// A path node.
    Node,
    /// Where two paths cross.
    Intersection,
    /// A bounding-box corner, edge midpoint or centre.
    Anchor,
    /// The middle of a segment.
    Midpoint,
    /// A guide line.
    Guide,
    /// A mat or usable-area edge.
    Edge,
    /// A grid line crossing.
    Grid,
}

impl SnapKind {
    /// Higher wins a tie.
    #[must_use]
    pub fn priority(self) -> u8 {
        match self {
            Self::Node => 6,
            Self::Intersection => 5,
            Self::Anchor => 4,
            Self::Midpoint => 3,
            Self::Guide => 2,
            Self::Edge | Self::Grid => 1,
        }
    }

    /// The words the snap indicator shows.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Intersection => "intersection",
            Self::Anchor => "bounding-box anchor",
            Self::Midpoint => "midpoint",
            Self::Guide => "guide",
            Self::Edge => "edge",
            Self::Grid => "grid",
        }
    }
}

/// A place the cursor could snap to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    /// Where, in world millimetres.
    pub point: Point,
    /// What it is.
    pub kind: SnapKind,
}

/// The snap radius in millimetres at `zoom` pixels per millimetre.
#[must_use]
pub fn snap_radius_mm(zoom: f64) -> f64 {
    SNAP_RADIUS_PX / zoom
}

fn dist(a: Point, b: Point) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// The best candidate for `cursor`, if any is within the snap radius.
#[must_use]
pub fn snap(cursor: Point, candidates: &[Candidate], zoom: f64) -> Option<Candidate> {
    let radius = snap_radius_mm(zoom);
    let tie = TIE_PX / zoom;
    let near: Vec<(f64, Candidate)> = candidates
        .iter()
        .map(|c| (dist(cursor, c.point), *c))
        .filter(|(d, _)| *d <= radius)
        .collect();
    let nearest = near.iter().map(|(d, _)| *d).fold(f64::INFINITY, f64::min);
    near.into_iter()
        .filter(|(d, _)| *d <= nearest + tie)
        .max_by(|(da, a), (db, b)| {
            a.kind
                .priority()
                .cmp(&b.kind.priority())
                .then(db.total_cmp(da))
        })
        .map(|(_, c)| c)
}

/// The nearest grid line crossing to `p`; unchanged when `spacing` is not positive.
#[must_use]
pub fn grid_point(p: Point, spacing: f64) -> Point {
    if spacing <= 0.0 {
        return p;
    }
    Point::new(
        (p.x / spacing).round() * spacing,
        (p.y / spacing).round() * spacing,
    )
}

/// The nine anchors of a bounding box: corners, edge midpoints and the centre.
#[must_use]
pub fn bbox_anchors(r: Rect) -> Vec<Point> {
    let xs = [r.x0, r.x0.midpoint(r.x1), r.x1];
    let ys = [r.y0, r.y0.midpoint(r.y1), r.y1];
    ys.iter()
        .flat_map(|&y| xs.iter().map(move |&x| Point::new(x, y)))
        .collect()
}

/// The midpoints of a polyline's segments, including the closing segment when `closed`.
#[must_use]
pub fn segment_midpoints(points: &[Point], closed: bool) -> Vec<Point> {
    let mut out: Vec<Point> = points
        .windows(2)
        .map(|w| Point::new(w[0].x.midpoint(w[1].x), w[0].y.midpoint(w[1].y)))
        .collect();
    if closed && points.len() > 2 {
        let (a, b) = (points[points.len() - 1], points[0]);
        out.push(Point::new(a.x.midpoint(b.x), a.y.midpoint(b.y)));
    }
    out
}

/// Where segment `a`-`b` meets segment `c`-`d`; `None` when they miss or are parallel.
#[must_use]
pub fn segment_intersection(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    let (rx, ry) = (b.x - a.x, b.y - a.y);
    let (sx, sy) = (d.x - c.x, d.y - c.y);
    let denom = rx * sy - ry * sx;
    if denom.abs() < 1e-12 {
        return None;
    }
    let along_first = ((c.x - a.x) * sy - (c.y - a.y) * sx) / denom;
    let along_second = ((c.x - a.x) * ry - (c.y - a.y) * rx) / denom;
    let eps = 1e-9;
    let range = -eps..=1.0 + eps;
    (range.contains(&along_first) && range.contains(&along_second))
        .then(|| Point::new(a.x + along_first * rx, a.y + along_first * ry))
}

fn segments(points: &[Point], closed: bool) -> Vec<(Point, Point)> {
    let mut out: Vec<(Point, Point)> = points.windows(2).map(|w| (w[0], w[1])).collect();
    if closed && points.len() > 2 {
        out.push((points[points.len() - 1], points[0]));
    }
    out
}

/// Every point where two polylines cross.
#[must_use]
pub fn polyline_intersections(
    a: &[Point],
    a_closed: bool,
    b: &[Point],
    b_closed: bool,
) -> Vec<Point> {
    let (sa, sb) = (segments(a, a_closed), segments(b, b_closed));
    let mut out = Vec::new();
    for &(p, q) in &sa {
        for &(r, s) in &sb {
            if let Some(x) = segment_intersection(p, q, r, s) {
                out.push(x);
            }
        }
    }
    out
}

/// The point on the boundary of `rect` nearest to `p`.
#[must_use]
pub fn edge_point(p: Point, rect: Rect) -> Point {
    let clamped = Point::new(p.x.clamp(rect.x0, rect.x1), p.y.clamp(rect.y0, rect.y1));
    if clamped != p {
        return clamped;
    }
    // Inside: move to the nearest side.
    let gaps = [
        (p.x - rect.x0, Point::new(rect.x0, p.y)),
        (rect.x1 - p.x, Point::new(rect.x1, p.y)),
        (p.y - rect.y0, Point::new(p.x, rect.y0)),
        (rect.y1 - p.y, Point::new(p.x, rect.y1)),
    ];
    gaps.into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(p, |(_, pt)| pt)
}

#[cfg(test)]
mod tests;
