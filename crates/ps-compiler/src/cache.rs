//! Incremental compiles.
//!
//! Live preview recompiles only what changed. The output of geometry preparation and drive-type
//! processing (stages 3 and 4) is memoised per (object, operation) under a BLAKE3 key of
//! everything [`prepare`] reads: path geometry, the composed transform (object transform, mirror,
//! origin, holder offset), resolved parameters, the tool's drive type, tip and blade offset, and
//! the operation. Ordering, tool planning and estimation always rerun for the mat.
//!
//! The cache is an LRU capped by bytes (256 MiB by default). Misses are prepared in parallel with
//! rayon and collected in input order, so the result never depends on thread scheduling.
//!
//! Deviation from the original design: the key does not name the tool version or the calibration
//! directly. It covers the *values* derived from them that preparation reads (drive type, tip
//! width, the calibrated blade offset, the holder-offset-adjusted transform), which is strictly
//! more precise: an unrelated tool or calibration edit cannot invalidate an entry.

use std::collections::BTreeMap;

use lru::LruCache;
use ps_core::{ContentHash, StableHasher};
use ps_geometry::{Path, Seg};
use rayon::prelude::*;

use crate::prepare::{Note, PrepareInput, Prepared, Stroke, prepare};

/// The default cache size, in bytes.
pub const DEFAULT_CAPACITY_BYTES: usize = 256 * 1024 * 1024;

/// Hit, miss and size counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Lookups answered from the cache.
    pub hits: u64,
    /// Lookups that had to prepare.
    pub misses: u64,
    /// Entries held now.
    pub entries: usize,
    /// Approximate bytes held now.
    pub bytes: usize,
}

struct Entry {
    prepared: Prepared,
    bytes: usize,
}

/// A memo of prepared geometry, shared across compiles of the same project.
pub struct CompileCache {
    capacity: usize,
    bytes: usize,
    hits: u64,
    misses: u64,
    entries: LruCache<ContentHash, Entry>,
}

impl std::fmt::Debug for CompileCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompileCache")
            .field("stats", &self.stats())
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

impl Default for CompileCache {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY_BYTES)
    }
}

fn hash_path(h: &mut StableHasher, path: &Path) {
    h.write_f64(path.start.x);
    h.write_f64(path.start.y);
    h.write_bool(path.closed);
    h.write_u64(u64::try_from(path.segs.len()).unwrap_or(u64::MAX));
    for seg in &path.segs {
        match *seg {
            Seg::Line { to } => {
                h.write_u8(0);
                h.write_f64(to.x);
                h.write_f64(to.y);
            }
            Seg::Arc { to, center, ccw } => {
                h.write_u8(1);
                for v in [to.x, to.y, center.x, center.y] {
                    h.write_f64(v);
                }
                h.write_bool(ccw);
            }
            Seg::Cubic { c1, c2, to } => {
                h.write_u8(2);
                for v in [c1.x, c1.y, c2.x, c2.y, to.x, to.y] {
                    h.write_f64(v);
                }
            }
        }
    }
}

/// The memo key of one preparation: a digest of everything [`prepare`] reads.
#[must_use]
pub fn memo_key(input: &PrepareInput<'_>) -> ContentHash {
    let mut h = StableHasher::new();
    h.write_str("ps-compiler/prepare/1");
    hash_path(&mut h, &input.path.geom);
    h.write_u8(match input.path.fill_rule {
        ps_geometry::FillRule::NonZero => 0,
        ps_geometry::FillRule::EvenOdd => 1,
    });
    for c in input.to_machine.as_coeffs() {
        h.write_f64(c);
    }
    h.write_str(input.kind.as_str());
    h.write_str(&format!("{:?}", input.drive));
    h.write_bool(input.tip_width_mm.is_some());
    h.write_f64(input.tip_width_mm.unwrap_or(0.0));
    h.write_f64(input.blade_offset_mm);
    h.write_f64(input.wheel_radius_mm.unwrap_or(0.0));
    match input.fill {
        None => h.write_u8(0),
        Some(f) => {
            h.write_u8(1);
            h.write_str(&format!("{:?}", f.pattern));
            h.write_f64(f.angle_deg);
            h.write_f64(f.overlap);
            h.write_bool(f.outline_pass);
        }
    }
    h.write_bool(input.turn_step_deg.is_some());
    h.write_f64(input.turn_step_deg.unwrap_or(0.0));
    for (name, value) in input.params {
        h.write_str(name);
        h.write_f64(*value);
    }
    h.finish()
}

fn size_of_prepared(p: &Prepared) -> usize {
    let strokes: usize = p
        .strokes
        .iter()
        .map(|s| {
            size_of::<Stroke>()
                + s.path.segs.len() * size_of::<Seg>()
                + s.tip_ring.as_ref().map_or(0, |r| r.len() * 16)
        })
        .sum();
    strokes + p.notes.len() * size_of::<Note>() + 64
}

impl CompileCache {
    /// An empty cache that holds at most `capacity_bytes` (approximate) bytes.
    #[must_use]
    pub fn new(capacity_bytes: usize) -> Self {
        Self {
            capacity: capacity_bytes,
            bytes: 0,
            hits: 0,
            misses: 0,
            entries: LruCache::unbounded(),
        }
    }

    /// The byte cap.
    #[must_use]
    pub fn capacity_bytes(&self) -> usize {
        self.capacity
    }

    /// Counters and current size.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits,
            misses: self.misses,
            entries: self.entries.len(),
            bytes: self.bytes,
        }
    }

    /// Drops every entry (the counters are kept).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    fn insert(&mut self, key: ContentHash, prepared: Prepared) {
        let bytes = size_of_prepared(&prepared);
        if bytes > self.capacity {
            return;
        }
        if let Some(old) = self.entries.put(key, Entry { prepared, bytes }) {
            self.bytes -= old.bytes;
        }
        self.bytes += bytes;
        while self.bytes > self.capacity {
            let Some((_, evicted)) = self.entries.pop_lru() else {
                break;
            };
            self.bytes -= evicted.bytes;
        }
    }

    /// Prepares every input, answering from the cache where possible and preparing the rest in
    /// parallel. The result is in input order.
    pub fn prepare_all(&mut self, inputs: &[PrepareInput<'_>]) -> Vec<Prepared> {
        let keys: Vec<ContentHash> = inputs.iter().map(memo_key).collect();
        let mut out: Vec<Option<Prepared>> = Vec::with_capacity(inputs.len());
        let mut missing: Vec<usize> = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            if let Some(entry) = self.entries.get(key) {
                self.hits += 1;
                out.push(Some(entry.prepared.clone()));
            } else {
                self.misses += 1;
                missing.push(i);
                out.push(None);
            }
        }
        // Equal keys within one call share work: prepare each distinct key once.
        let mut distinct: BTreeMap<ContentHash, usize> = BTreeMap::new();
        for &i in &missing {
            distinct.entry(keys[i]).or_insert(i);
        }
        let jobs: Vec<usize> = distinct.values().copied().collect();
        let done: Vec<Prepared> = jobs.par_iter().map(|&i| prepare(&inputs[i])).collect();
        let by_key: BTreeMap<ContentHash, Prepared> = distinct.keys().copied().zip(done).collect();
        for &i in &missing {
            if let Some(p) = by_key.get(&keys[i]) {
                out[i] = Some(p.clone());
            }
        }
        for (key, p) in by_key {
            self.insert(key, p);
        }
        out.into_iter().map(Option::unwrap_or_default).collect()
    }
}

#[cfg(test)]
mod tests;
