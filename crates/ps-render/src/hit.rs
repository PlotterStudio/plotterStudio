//! Hit testing: which object is under the cursor.
//!
//! The app asks the R*-tree for candidates in a box of [`PICK_RADIUS_PX`] around the cursor and
//! passes them here to be tested exactly: distance to the curve for strokes and open paths,
//! winding number for filled shapes. When several are hit, the topmost (last drawn) wins.

use ps_geometry::{Path, Point, Rect, flatten};

use crate::lod::flatten_tolerance_mm;

/// The pick box around the cursor, in screen pixels.
pub const PICK_RADIUS_PX: f64 = 4.0;

/// The pick radius in millimetres at `zoom` pixels per millimetre.
#[must_use]
pub fn pick_radius_mm(zoom: f64) -> f64 {
    PICK_RADIUS_PX / zoom
}

/// Something that can be picked: a path in world coordinates, filled or just outlined.
#[derive(Debug, Clone, PartialEq)]
pub struct Pickable<Id> {
    /// What to report when it is hit.
    pub id: Id,
    /// Its geometry in world millimetres.
    pub path: Path,
    /// Whether the inside counts as a hit.
    pub filled: bool,
}

/// How a box selection treats things it only touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxMode {
    /// Everything the box touches.
    Crossing,
    /// Only what lies wholly inside the box.
    Enclosed,
}

/// The distance from `p` to the segment `a`-`b`.
#[must_use]
pub fn distance_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 <= f64::EPSILON {
        return (p.x - a.x).hypot(p.y - a.y);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    (p.x - (a.x + t * dx)).hypot(p.y - (a.y + t * dy))
}

/// The distance from `p` to the nearest point of `path`, flattening within `tol` mm.
#[must_use]
pub fn distance_to_path(p: Point, path: &Path, tol: f64) -> f64 {
    let points = flatten(path, tol);
    let mut best = f64::INFINITY;
    for pair in points.windows(2) {
        best = best.min(distance_to_segment(p, pair[0], pair[1]));
    }
    if let (true, Some(first), Some(last)) = (path.closed, points.first(), points.last()) {
        best = best.min(distance_to_segment(p, *last, *first));
    }
    if points.len() == 1 {
        best = (p.x - points[0].x).hypot(p.y - points[0].y);
    }
    best
}

/// The winding number of `path` (treated as closed) around `p`: zero means outside.
#[must_use]
pub fn winding_number(p: Point, path: &Path, tol: f64) -> i32 {
    let points = flatten(path, tol);
    let n = points.len();
    if n < 3 {
        return 0;
    }
    let mut winding = 0;
    for i in 0..n {
        let a = points[i];
        let b = points[(i + 1) % n];
        let side = (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y);
        if a.y <= p.y {
            if b.y > p.y && side > 0.0 {
                winding += 1;
            }
        } else if b.y <= p.y && side < 0.0 {
            winding -= 1;
        }
    }
    winding
}

fn is_hit<Id>(item: &Pickable<Id>, cursor: Point, radius: f64, tol: f64) -> bool {
    if distance_to_path(cursor, &item.path, tol) <= radius {
        return true;
    }
    item.filled && winding_number(cursor, &item.path, tol) != 0
}

/// The topmost item under `cursor` (world mm) at `zoom` pixels per millimetre.
#[must_use]
pub fn pick<Id: Clone>(items: &[Pickable<Id>], cursor: Point, zoom: f64) -> Option<Id> {
    let radius = pick_radius_mm(zoom);
    let tol = flatten_tolerance_mm(zoom);
    items
        .iter()
        .rev()
        .find(|item| is_hit(item, cursor, radius, tol))
        .map(|item| item.id.clone())
}

/// The item under `cursor` a person most likely means when shapes nest: an outline within reach
/// (the nearest, the topmost on a tie), otherwise the smallest filled item around the cursor.
#[must_use]
pub fn pick_innermost<Id: Clone>(items: &[Pickable<Id>], cursor: Point, zoom: f64) -> Option<Id> {
    let radius = pick_radius_mm(zoom);
    let tol = flatten_tolerance_mm(zoom);
    let outline = items
        .iter()
        .rev()
        .map(|item| (distance_to_path(cursor, &item.path, tol), item))
        .filter(|(distance, _)| *distance <= radius)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, item)| item);
    let area = |item: &Pickable<Id>| item.path.bounds().area();
    outline
        .or_else(|| {
            items
                .iter()
                .rev()
                .filter(|item| item.filled && winding_number(cursor, &item.path, tol) != 0)
                .min_by(|a, b| area(a).total_cmp(&area(b)))
        })
        .map(|item| item.id.clone())
}

fn inside(r: Rect, p: Point) -> bool {
    p.x >= r.x0 && p.x <= r.x1 && p.y >= r.y0 && p.y <= r.y1
}

fn segments_cross(a: Point, b: Point, c: Point, d: Point) -> bool {
    crate::snap::segment_intersection(a, b, c, d).is_some()
}

fn touches(rect: Rect, points: &[Point], closed: bool) -> bool {
    if points.iter().any(|p| inside(rect, *p)) {
        return true;
    }
    let corners = [
        Point::new(rect.x0, rect.y0),
        Point::new(rect.x1, rect.y0),
        Point::new(rect.x1, rect.y1),
        Point::new(rect.x0, rect.y1),
    ];
    let mut segments: Vec<(Point, Point)> = points.windows(2).map(|w| (w[0], w[1])).collect();
    if closed && points.len() > 2 {
        segments.push((points[points.len() - 1], points[0]));
    }
    segments
        .iter()
        .any(|&(a, b)| (0..4).any(|i| segments_cross(a, b, corners[i], corners[(i + 1) % 4])))
}

/// The ids of the items a box selection takes.
#[must_use]
pub fn pick_in_box<Id: Clone>(items: &[Pickable<Id>], rect: Rect, mode: BoxMode) -> Vec<Id> {
    let centre = Point::new(rect.x0.midpoint(rect.x1), rect.y0.midpoint(rect.y1));
    items
        .iter()
        .filter(|item| {
            let tol = 0.01;
            let points = flatten(&item.path, tol);
            match mode {
                BoxMode::Enclosed => !points.is_empty() && points.iter().all(|p| inside(rect, *p)),
                BoxMode::Crossing => {
                    touches(rect, &points, item.path.closed)
                        || (item.filled && winding_number(centre, &item.path, tol) != 0)
                }
            }
        })
        .map(|item| item.id.clone())
        .collect()
}

#[cfg(test)]
mod tests;
