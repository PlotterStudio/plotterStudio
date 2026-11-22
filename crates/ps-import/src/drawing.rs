//! The intermediate form every importer produces, so repair, mapping rules and
//! re-import are written once.

use ps_core::{Diagnostics, PathKey, StableHasher};
use ps_geometry::{Path, flatten_to_grid};
use ps_model::SourceStyle;

use crate::units::{SizeCandidate, UnitGuess};

/// A layer of the source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerInfo {
    /// The layer's name.
    pub name: String,
    /// Whether it is visible in the source.
    pub visible: bool,
    /// Position in the source's stacking order, bottom first.
    pub order: u32,
}

/// One path of the source, already in millimetres with all transforms applied.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedPath {
    /// The source id when the file has one, else a structural fingerprint.
    pub key: PathKey,
    /// The layer the path belongs to.
    pub layer: Option<String>,
    /// The geometry.
    pub geom: Path,
    /// Stroke colour, width, dash array, fill colour and opacity.
    pub style: SourceStyle,
}

/// The result of reading a file, before repair and mapping.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedDrawing {
    /// How the file's units were interpreted.
    pub unit_guess: UnitGuess,
    /// The physical size under each plausible reading, when they disagree.
    pub size_candidates: Vec<SizeCandidate>,
    /// The file's layers.
    pub layers: Vec<LayerInfo>,
    /// The paths, bottom of the stacking order first.
    pub items: Vec<ImportedPath>,
    /// What the importer noticed.
    pub diagnostics: Diagnostics,
}

/// The structural fingerprint of a path that has no source id: its layer path, its
/// element index in the file, the index of the piece within the element, and its geometry
/// rounded to the µm grid. Identical files give identical fingerprints.
pub(crate) fn fingerprint(
    layer_path: &str,
    element: u64,
    sub: u64,
    piece: u64,
    path: &Path,
) -> PathKey {
    let mut h = StableHasher::new();
    h.write_str(layer_path);
    h.write_u64(element);
    h.write_u64(sub);
    h.write_u64(piece);
    for p in flatten_to_grid(path, 0.002).unwrap_or_default() {
        h.write_i64(p.x);
        h.write_i64(p.y);
    }
    PathKey::fingerprint(h.finish())
}
