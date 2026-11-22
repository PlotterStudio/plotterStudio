//! Spatial index over bounding boxes for hit testing, rubber-band selection,
//! snapping candidates and nearest-neighbour ordering.
//!
//! Query results are always sorted by id, so callers see the same order on every run.

use std::collections::BTreeMap;

use kurbo::{Point, Rect};
use rstar::primitives::{GeomWithData, Rectangle};
use rstar::{AABB, RTree};

type Entry<T> = GeomWithData<Rectangle<[f64; 2]>, T>;

/// An R*-tree of rectangles keyed by id.
#[derive(Debug, Clone)]
pub struct SpatialIndex<T> {
    tree: RTree<Entry<T>>,
    rects: BTreeMap<T, Rect>,
}

impl<T: Ord + Clone> Default for SpatialIndex<T> {
    fn default() -> Self {
        Self::new()
    }
}

fn entry<T>(id: T, r: Rect) -> Entry<T> {
    GeomWithData::new(Rectangle::from_corners([r.x0, r.y0], [r.x1, r.y1]), id)
}

/// Distance from a point to a rectangle (zero inside).
fn rect_distance(p: Point, r: Rect) -> f64 {
    let dx = (r.x0 - p.x).max(0.0).max(p.x - r.x1);
    let dy = (r.y0 - p.y).max(0.0).max(p.y - r.y1);
    libm::hypot(dx, dy)
}

impl<T: Ord + Clone> SpatialIndex<T> {
    /// Creates an empty index.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tree: RTree::new(),
            rects: BTreeMap::new(),
        }
    }

    /// Builds an index in one pass. A later entry with the same id replaces an earlier one.
    #[must_use]
    pub fn from_entries(items: impl IntoIterator<Item = (T, Rect)>) -> Self {
        let rects: BTreeMap<T, Rect> = items.into_iter().map(|(id, r)| (id, r.abs())).collect();
        let entries = rects.iter().map(|(id, r)| entry(id.clone(), *r)).collect();
        Self {
            tree: RTree::bulk_load(entries),
            rects,
        }
    }

    /// Returns the number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rects.len()
    }

    /// Returns true when the index is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// Inserts a rectangle, replacing any entry with the same id.
    pub fn insert(&mut self, id: T, rect: Rect) {
        self.remove(&id);
        let rect = rect.abs();
        self.tree.insert(entry(id.clone(), rect));
        self.rects.insert(id, rect);
    }

    /// Removes an entry, returning whether it existed.
    pub fn remove(&mut self, id: &T) -> bool {
        let Some(rect) = self.rects.remove(id) else {
            return false;
        };
        self.tree.remove(&entry(id.clone(), rect)).is_some()
    }

    /// Returns the ids of rectangles intersecting `rect` (touching edges count), sorted.
    #[must_use]
    pub fn query_rect(&self, rect: Rect) -> Vec<T> {
        let r = rect.abs();
        let env = AABB::from_corners([r.x0, r.y0], [r.x1, r.y1]);
        let mut ids: Vec<T> = self
            .tree
            .locate_in_envelope_intersecting(&env)
            .map(|e| e.data.clone())
            .collect();
        ids.sort();
        ids
    }

    /// Returns the ids of rectangles within `radius` of `p`, sorted.
    #[must_use]
    pub fn query_point(&self, p: Point, radius: f64) -> Vec<T> {
        let probe = Rect::new(p.x - radius, p.y - radius, p.x + radius, p.y + radius);
        let mut ids: Vec<T> = self
            .query_rect(probe)
            .into_iter()
            .filter(|id| {
                self.rects
                    .get(id)
                    .is_some_and(|r| rect_distance(p, *r) <= radius)
            })
            .collect();
        ids.sort();
        ids
    }

    /// Returns the id of the rectangle nearest to `p`; ties go to the smaller id.
    #[must_use]
    pub fn nearest(&self, p: Point) -> Option<T> {
        let mut best: Option<(f64, &T)> = None;
        for e in self.tree.nearest_neighbor_iter(&[p.x, p.y]) {
            let d = self
                .rects
                .get(&e.data)
                .map_or(f64::INFINITY, |r| rect_distance(p, *r));
            match best {
                None => best = Some((d, &e.data)),
                Some((bd, bid)) => match d.total_cmp(&bd) {
                    std::cmp::Ordering::Greater => break,
                    std::cmp::Ordering::Equal if e.data < *bid => best = Some((d, &e.data)),
                    _ => {}
                },
            }
        }
        best.map(|(_, id)| id.clone())
    }
}

#[cfg(test)]
mod tests;
