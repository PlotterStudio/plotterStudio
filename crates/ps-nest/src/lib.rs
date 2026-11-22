//! Plotter Studio auto-nesting.
//!
//! Items are object outlines grown by half the spacing; bins are mats (usable area minus
//! keep-outs and scrap regions). [`nest`] places items with a bottom-left-fill heuristic using
//! the `jagua-rs` collision engine, improved by a search over item orders. It is deterministic:
//! the search budget is an iteration count, the seed comes from the job's content hash (see
//! [`seed_from_hash`]) and the clock is never read; only cancellation can shorten a run.
//!
//! This crate is the one place `jagua-rs` (MPL-2.0, used unchanged) is linked, so the rest of
//! Plotter Studio stays MIT. Its collision queries work in `f32`, so every placement it accepts
//! is checked again by integer geometry in this crate's tests.

mod engine;
mod layout;
mod rotation;

use ps_core::PtUm;
use serde::{Deserialize, Serialize};

pub use engine::nest;
pub use layout::{grid_layout, placed_outline, utilisation};
pub use rotation::{Bounds, Rotations, angles, bbox, convex_hull, rotate_ring, seed_from_hash};

/// A mat to nest onto: a usable outline with regions that must stay free.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestBin {
    /// The usable area, counter-clockwise, in µm.
    pub outline: Vec<PtUm>,
    /// Regions that must stay clear: keep-outs and scrap regions already used.
    pub keep_outs: Vec<Vec<PtUm>>,
}

impl NestBin {
    /// The outline as a list of rings, for boolean operations.
    #[must_use]
    pub fn outline_rings(&self) -> Vec<Vec<PtUm>> {
        vec![self.outline.clone()]
    }

    /// The keep-out rings.
    #[must_use]
    pub fn keep_out_rings(&self) -> Vec<Vec<PtUm>> {
        self.keep_outs.clone()
    }
}

/// An object to place. Several rings form an attached set that moves as one rigid item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestItem {
    /// Caller's identifier, returned in [`Placement::item`].
    pub id: u32,
    /// The outline rings in µm, in the item's own frame (counter-clockwise).
    pub outline: Vec<Vec<PtUm>>,
    /// Which rotations are allowed.
    pub rotations: Rotations,
}

/// Where an item goes: rotate its outline about the origin, then move it by `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    /// The item's identifier.
    pub item: u32,
    /// Index of the bin it was placed in.
    pub bin: usize,
    /// Counter-clockwise rotation in degrees.
    pub rotation_deg: f64,
    /// Translation applied after the rotation, in µm.
    pub offset: PtUm,
}

/// Settings for one nesting run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NestParams {
    /// Minimum gap between items, in µm.
    pub spacing_um: i64,
    /// Seed of the order search (derive it with [`seed_from_hash`]).
    pub seed: u64,
    /// How many alternative item orders to try after the greedy first one. Zero is greedy only.
    pub iterations: u32,
}

/// The result of a nesting run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Nesting {
    /// Where each placed item goes.
    pub placements: Vec<Placement>,
    /// Identifiers of items that did not fit anywhere, ascending.
    pub unplaced: Vec<u32>,
    /// For each bin, the fraction of its usable area covered by placed items.
    pub utilisation: Vec<f64>,
}
