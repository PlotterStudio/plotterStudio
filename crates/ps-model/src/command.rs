//! Commands and undo.
//!
//! Every change to a [`Document`] is a [`Command`] applied by a [`DocumentStore`], which keeps
//! the whole previous document roots for undo. Because unchanged nodes are shared (`Arc`), a step
//! costs only the changed nodes plus a clone of the affected map of `Arc`s. Commands with the
//! same [`MergeKey`] within 500 ms merge into one undo step, so a drag or a slider move is a
//! single undo. Transactions group commands. History is capped by memory, oldest steps first.

use std::collections::BTreeSet;
use std::sync::Arc;

use ps_core::{L10nKey, LibRef, MatId, ObjectId, OpId, OpRowId, PathKey};
use ps_geometry::{Affine, Path};

use crate::{
    Asset, Document, Guide, JobSettings, MappingRule, Mat, Operation, ParamKey, ParamOverrides,
    ParamValue, PlacedObject, ShapePath,
};

/// Commands with the same key within this many milliseconds merge into one undo step.
pub const MERGE_WINDOW_MS: u64 = 500;

/// Why a command could not be applied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    /// A mat, object, path or operation it refers to does not exist.
    #[error("{0} not found")]
    NotFound(String),
    /// Something with this id already exists.
    #[error("{0} already exists")]
    Duplicate(String),
}

/// Identifies a stream of edits that should merge (a drag, a slider).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeKey(String);

impl MergeKey {
    /// Creates a key.
    #[must_use]
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }
}

/// A change to the document.
pub trait Command: Send + Sync {
    /// The undo-menu label (a localisation key).
    fn label(&self) -> L10nKey;
    /// Produces the new document; the old one is left untouched.
    ///
    /// # Errors
    /// [`CommandError`] when the command does not apply.
    fn apply(&self, doc: &Document) -> Result<Document, CommandError>;
    /// Commands with equal keys applied in quick succession merge into one undo step.
    fn merge_key(&self) -> Option<MergeKey> {
        None
    }
}

// ---- copy-on-write helpers ----

fn with_mat(
    doc: &Document,
    id: MatId,
    edit: impl FnOnce(&mut Mat) -> Result<(), CommandError>,
) -> Result<Document, CommandError> {
    let old = doc
        .mat(id)
        .ok_or_else(|| CommandError::NotFound(format!("mat {id}")))?;
    let mut mat = (**old).clone();
    edit(&mut mat)?;
    let mut out = doc.clone();
    let mut mats = (*doc.mats).clone();
    mats.insert(id, Arc::new(mat));
    out.mats = Arc::new(mats);
    Ok(out)
}

fn with_object(
    doc: &Document,
    mat: MatId,
    object: ObjectId,
    edit: impl FnOnce(&mut PlacedObject) -> Result<(), CommandError>,
) -> Result<Document, CommandError> {
    with_mat(doc, mat, |m| {
        let old = m
            .objects
            .get(&object)
            .ok_or_else(|| CommandError::NotFound(format!("object {object}")))?;
        let mut obj = (**old).clone();
        edit(&mut obj)?;
        Arc::make_mut(&mut m.objects).insert(object, Arc::new(obj));
        Ok(())
    })
}

fn with_path(
    doc: &Document,
    mat: MatId,
    object: ObjectId,
    path: &PathKey,
    edit: impl FnOnce(&mut ShapePath) -> Result<(), CommandError>,
) -> Result<Document, CommandError> {
    with_object(doc, mat, object, |o| {
        let index = o
            .paths
            .iter()
            .position(|p| p.key == *path)
            .ok_or_else(|| CommandError::NotFound(format!("path {path}")))?;
        let mut sp = (*o.paths[index]).clone();
        edit(&mut sp)?;
        Arc::make_mut(&mut o.paths)[index] = Arc::new(sp);
        Ok(())
    })
}

// ---- commands ----

/// Adds a mat.
#[derive(Debug, Clone)]
pub struct AddMat {
    /// The mat.
    pub mat: Mat,
}

impl Command for AddMat {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-add-mat")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        if doc.mats.contains_key(&self.mat.id) {
            return Err(CommandError::Duplicate(format!("mat {}", self.mat.id)));
        }
        let mut out = doc.clone();
        Arc::make_mut(&mut out.mats).insert(self.mat.id, Arc::new(self.mat.clone()));
        Ok(out)
    }
}

/// Removes a mat.
#[derive(Debug, Clone)]
pub struct RemoveMat {
    /// The mat to remove.
    pub mat: MatId,
}

impl Command for RemoveMat {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-remove-mat")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        if !doc.mats.contains_key(&self.mat) {
            return Err(CommandError::NotFound(format!("mat {}", self.mat)));
        }
        let mut out = doc.clone();
        Arc::make_mut(&mut out.mats).shift_remove(&self.mat);
        Ok(out)
    }
}

/// Places an object on a mat.
#[derive(Debug, Clone)]
pub struct AddObject {
    /// The mat.
    pub mat: MatId,
    /// The object.
    pub object: PlacedObject,
}

impl Command for AddObject {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-add-object")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        with_mat(doc, self.mat, |m| {
            if m.objects.contains_key(&self.object.id) {
                return Err(CommandError::Duplicate(format!(
                    "object {}",
                    self.object.id
                )));
            }
            Arc::make_mut(&mut m.objects).insert(self.object.id, Arc::new(self.object.clone()));
            Ok(())
        })
    }
}

/// Removes an object from a mat.
#[derive(Debug, Clone)]
pub struct RemoveObject {
    /// The mat.
    pub mat: MatId,
    /// The object.
    pub object: ObjectId,
}

impl Command for RemoveObject {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-remove-object")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        with_mat(doc, self.mat, |m| {
            if !m.objects.contains_key(&self.object) {
                return Err(CommandError::NotFound(format!("object {}", self.object)));
            }
            Arc::make_mut(&mut m.objects).shift_remove(&self.object);
            Ok(())
        })
    }
}

/// Sets an object's transform (a move, rotation or scale).
#[derive(Debug, Clone)]
pub struct SetTransform {
    /// The mat.
    pub mat: MatId,
    /// The object.
    pub object: ObjectId,
    /// The new object-to-mat transform.
    pub transform: Affine,
}

impl Command for SetTransform {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-transform")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        with_object(doc, self.mat, self.object, |o| {
            o.transform = self.transform;
            Ok(())
        })
    }
    fn merge_key(&self) -> Option<MergeKey> {
        Some(MergeKey::new(format!("transform:{}", self.object)))
    }
}

/// Where a parameter override lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverrideTarget {
    /// Project level.
    Project,
    /// Mat level.
    Mat(MatId),
    /// Operation level.
    Operation {
        /// The mat.
        mat: MatId,
        /// The object.
        object: ObjectId,
        /// The path.
        path: PathKey,
        /// The operation.
        op: OpId,
    },
}

impl OverrideTarget {
    fn tag(&self) -> String {
        match self {
            Self::Project => "project".to_owned(),
            Self::Mat(m) => format!("mat:{m}"),
            Self::Operation { op, .. } => format!("op:{op}"),
        }
    }
}

/// Sets or removes one parameter override.
#[derive(Debug, Clone)]
pub struct SetOverride {
    /// Which layer of the chain.
    pub target: OverrideTarget,
    /// The parameter.
    pub key: ParamKey,
    /// The value, or `None` to remove the override.
    pub value: Option<ParamValue>,
}

impl SetOverride {
    fn edit(&self, overrides: &mut ParamOverrides) {
        match &self.value {
            Some(v) => overrides.set(self.key.clone(), v.clone()),
            None => {
                overrides.remove(&self.key);
            }
        }
    }
}

impl Command for SetOverride {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-set-parameter")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        match &self.target {
            OverrideTarget::Project => {
                let mut out = doc.clone();
                let mut settings = (*doc.settings).clone();
                self.edit(&mut settings.overrides);
                out.settings = Arc::new(settings);
                Ok(out)
            }
            OverrideTarget::Mat(id) => with_mat(doc, *id, |m| {
                self.edit(&mut m.overrides);
                Ok(())
            }),
            OverrideTarget::Operation {
                mat,
                object,
                path,
                op,
            } => with_path(doc, *mat, *object, path, |p| {
                let operation = p
                    .ops
                    .iter_mut()
                    .find(|o| o.id == *op)
                    .ok_or_else(|| CommandError::NotFound(format!("operation {op}")))?;
                self.edit(&mut operation.overrides);
                Ok(())
            }),
        }
    }
    fn merge_key(&self) -> Option<MergeKey> {
        Some(MergeKey::new(format!(
            "override:{}:{}",
            self.target.tag(),
            self.key
        )))
    }
}

/// Assigns an operation to a path.
#[derive(Debug, Clone)]
pub struct AssignOperation {
    /// The mat.
    pub mat: MatId,
    /// The object.
    pub object: ObjectId,
    /// The path.
    pub path: PathKey,
    /// The operation.
    pub op: Operation,
}

impl Command for AssignOperation {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-assign-operation")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        with_path(doc, self.mat, self.object, &self.path, |p| {
            if p.ops.iter().any(|o| o.id == self.op.id) {
                return Err(CommandError::Duplicate(format!("operation {}", self.op.id)));
            }
            p.ops.push(self.op.clone());
            Ok(())
        })
    }
}

/// Takes an operation off a path.
#[derive(Debug, Clone)]
pub struct RemoveOperation {
    /// The mat.
    pub mat: MatId,
    /// The object.
    pub object: ObjectId,
    /// The path.
    pub path: PathKey,
    /// The operation.
    pub op: OpId,
}

impl Command for RemoveOperation {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-remove-operation")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        with_path(doc, self.mat, self.object, &self.path, |p| {
            let before = p.ops.len();
            p.ops.retain(|o| o.id != self.op);
            if p.ops.len() == before {
                return Err(CommandError::NotFound(format!("operation {}", self.op)));
            }
            Ok(())
        })
    }
}

/// Sets or clears the colour of the pen an operation draws with.
#[derive(Debug, Clone)]
pub struct SetPenColour {
    /// The mat.
    pub mat: MatId,
    /// The object.
    pub object: ObjectId,
    /// The path.
    pub path: PathKey,
    /// The operation.
    pub op: OpId,
    /// `#RRGGBB`, or `None` for the pen's own colour.
    pub colour: Option<String>,
}

impl Command for SetPenColour {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-set-pen-colour")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        with_path(doc, self.mat, self.object, &self.path, |p| {
            let op = p
                .ops
                .iter_mut()
                .find(|o| o.id == self.op)
                .ok_or_else(|| CommandError::NotFound(format!("operation {}", self.op)))?;
            op.tool.pen_colour.clone_from(&self.colour);
            Ok(())
        })
    }
}

/// Sets or clears the fill settings of an operation.
#[derive(Debug, Clone)]
pub struct SetOpFill {
    /// The mat.
    pub mat: MatId,
    /// The object.
    pub object: ObjectId,
    /// The path.
    pub path: PathKey,
    /// The operation.
    pub op: OpId,
    /// The pattern, angle and overlap, or `None` for the defaults.
    pub fill: Option<ps_geometry::FillSpec>,
}

impl Command for SetOpFill {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-set-fill")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        with_path(doc, self.mat, self.object, &self.path, |p| {
            let op = p
                .ops
                .iter_mut()
                .find(|o| o.id == self.op)
                .ok_or_else(|| CommandError::NotFound(format!("operation {}", self.op)))?;
            op.fill = self.fill;
            Ok(())
        })
    }
    fn merge_key(&self) -> Option<MergeKey> {
        Some(MergeKey::new(format!("fill:{}", self.op)))
    }
}

/// Turns an operation on or off.
#[derive(Debug, Clone)]
pub struct SetOpEnabled {
    /// The mat.
    pub mat: MatId,
    /// The object.
    pub object: ObjectId,
    /// The path.
    pub path: PathKey,
    /// The operation.
    pub op: OpId,
    /// Whether it runs.
    pub enabled: bool,
}

impl Command for SetOpEnabled {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-toggle-operation")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        with_path(doc, self.mat, self.object, &self.path, |p| {
            let op = p
                .ops
                .iter_mut()
                .find(|o| o.id == self.op)
                .ok_or_else(|| CommandError::NotFound(format!("operation {}", self.op)))?;
            op.enabled = self.enabled;
            Ok(())
        })
    }
}

/// Sets the material on a mat.
#[derive(Debug, Clone)]
pub struct SetMatMaterial {
    /// The mat.
    pub mat: MatId,
    /// The material, or `None` for none.
    pub material: Option<LibRef>,
}

impl Command for SetMatMaterial {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-set-material")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        with_mat(doc, self.mat, |m| {
            m.material.clone_from(&self.material);
            Ok(())
        })
    }
}

/// Turns the mat's mirror flag on or off.
#[derive(Debug, Clone)]
pub struct SetMirror {
    /// The mat.
    pub mat: MatId,
    /// Mirror the job at compile time.
    pub mirror: bool,
}

impl Command for SetMirror {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-mirror")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        with_mat(doc, self.mat, |m| {
            m.mirror = self.mirror;
            Ok(())
        })
    }
}

/// Stores an imported original file.
#[derive(Debug, Clone)]
pub struct AddAsset {
    /// The asset.
    pub asset: Asset,
}

impl Command for AddAsset {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-add-asset")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        if doc.assets.contains_key(&self.asset.id) {
            return Err(CommandError::Duplicate(format!("asset {}", self.asset.id)));
        }
        let mut out = doc.clone();
        Arc::make_mut(&mut out.assets).insert(self.asset.id, Arc::new(self.asset.clone()));
        Ok(out)
    }
}

/// Replaces the geometry of one path (a node drag, a convert, an inserted or deleted node).
#[derive(Debug, Clone)]
pub struct SetPathGeometry {
    /// The mat.
    pub mat: MatId,
    /// The object.
    pub object: ObjectId,
    /// The path.
    pub path: PathKey,
    /// The new geometry in the object's own space.
    pub geom: Path,
}

impl Command for SetPathGeometry {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-edit-path")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        with_path(doc, self.mat, self.object, &self.path, |p| {
            p.geom = self.geom.clone();
            Ok(())
        })
    }
    fn merge_key(&self) -> Option<MergeKey> {
        Some(MergeKey::new(format!(
            "path-geometry:{}:{}",
            self.object, self.path
        )))
    }
}

/// Sets the order operation rows run in on a mat.
#[derive(Debug, Clone)]
pub struct SetOpOrder {
    /// The mat.
    pub mat: MatId,
    /// The rows, in run order.
    pub order: Vec<OpRowId>,
}

impl Command for SetOpOrder {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-order-operations")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        let mut seen = BTreeSet::new();
        if let Some(row) = self.order.iter().find(|r| !seen.insert(**r)) {
            return Err(CommandError::Duplicate(format!("operation row {row}")));
        }
        with_mat(doc, self.mat, |m| {
            m.op_order = Arc::new(self.order.clone());
            Ok(())
        })
    }
}

/// Replaces one path with the pieces a cut made of it (the knife). Every piece keeps the path's
/// style and a copy of its operations, with fresh operation ids on the same rows.
#[derive(Debug, Clone)]
pub struct SplitPath {
    /// The mat.
    pub mat: MatId,
    /// The object.
    pub object: ObjectId,
    /// The path being cut.
    pub path: PathKey,
    /// The pieces, with their new keys and geometry in the object's own space.
    pub pieces: Vec<(PathKey, Path)>,
}

impl Command for SplitPath {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-split-path")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        if self.pieces.is_empty() {
            return Err(CommandError::NotFound("pieces to split into".to_owned()));
        }
        let mut seen = BTreeSet::new();
        if let Some((key, _)) = self.pieces.iter().find(|(k, _)| !seen.insert(k.clone())) {
            return Err(CommandError::Duplicate(format!("path {key}")));
        }
        with_object(doc, self.mat, self.object, |o| {
            let index = o
                .paths
                .iter()
                .position(|p| p.key == self.path)
                .ok_or_else(|| CommandError::NotFound(format!("path {}", self.path)))?;
            let original = (*o.paths[index]).clone();
            let others: BTreeSet<&PathKey> = o
                .paths
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != index)
                .map(|(_, p)| &p.key)
                .collect();
            if let Some((key, _)) = self.pieces.iter().find(|(k, _)| others.contains(k)) {
                return Err(CommandError::Duplicate(format!("path {key}")));
            }
            let replacements: Vec<Arc<ShapePath>> = self
                .pieces
                .iter()
                .map(|(key, geom)| {
                    let mut piece = original.clone();
                    piece.key = key.clone();
                    piece.geom = geom.clone();
                    for op in &mut piece.ops {
                        op.id = OpId::new();
                    }
                    Arc::new(piece)
                })
                .collect();
            Arc::make_mut(&mut o.paths).splice(index..=index, replacements);
            Ok(())
        })
    }
}

/// Chooses where a closed path starts: the path is rotated to the outline point nearest `at` and
/// locked there, so the compiler keeps that start. `None` unlocks it (the geometry stays as it is).
#[derive(Debug, Clone)]
pub struct SetStartPoint {
    /// The mat.
    pub mat: MatId,
    /// The object.
    pub object: ObjectId,
    /// The path.
    pub path: PathKey,
    /// The wanted start in the object's own space, or `None` to unlock.
    pub at: Option<ps_geometry::Point>,
}

impl Command for SetStartPoint {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-start-point")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        with_object(doc, self.mat, self.object, |o| {
            let index = o
                .paths
                .iter()
                .position(|p| p.key == self.path)
                .ok_or_else(|| CommandError::NotFound(format!("path {}", self.path)))?;
            let mut path = (*o.paths[index]).clone();
            match self.at {
                Some(at) => {
                    if !path.geom.closed {
                        return Err(CommandError::NotFound("a closed path".to_owned()));
                    }
                    path.geom = rotate_to_point(&path.geom, at);
                    path.start_locked = true;
                }
                None => path.start_locked = false,
            }
            Arc::make_mut(&mut o.paths)[index] = Arc::new(path);
            Ok(())
        })
    }
}

/// A closed path rotated to start at the outline point nearest `at`.
fn rotate_to_point(path: &Path, at: ps_geometry::Point) -> Path {
    path.distance_of_nearest(at, START_TOL_MM)
        .map_or_else(|| path.clone(), |d| path.rotated_to_distance(d))
}

/// Accuracy, in mm, of locating a start point on a curved outline.
const START_TOL_MM: f64 = 0.005;

/// Sets the run-after locks of a mat: each pair is (runs first, runs later).
#[derive(Debug, Clone)]
pub struct SetRunAfter {
    /// The mat.
    pub mat: MatId,
    /// The locks.
    pub locks: Vec<(OpRowId, OpRowId)>,
}

impl Command for SetRunAfter {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-run-after")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        if let Some((row, _)) = self.locks.iter().find(|(a, b)| a == b) {
            return Err(CommandError::Duplicate(format!(
                "operation row {row} locked after itself"
            )));
        }
        with_mat(doc, self.mat, |m| {
            m.run_after = Arc::new(self.locks.clone());
            Ok(())
        })
    }
}

/// Replaces the guide lines of a mat.
#[derive(Debug, Clone)]
pub struct SetGuides {
    /// The mat.
    pub mat: MatId,
    /// The guides.
    pub guides: Vec<Guide>,
}

impl Command for SetGuides {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-guides")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        if self.guides.iter().any(|g| !g.at_mm.is_finite()) {
            return Err(CommandError::NotFound("a finite guide position".to_owned()));
        }
        with_mat(doc, self.mat, |m| {
            m.guides = Arc::new(self.guides.clone());
            Ok(())
        })
    }
}

/// Replaces the project's job settings (ordering mode, pen check, project overrides).
#[derive(Debug, Clone)]
pub struct SetJobSettings {
    /// The new settings.
    pub settings: JobSettings,
}

impl Command for SetJobSettings {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-job-settings")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        let mut next = doc.clone();
        next.settings = Arc::new(self.settings.clone());
        Ok(next)
    }
    fn merge_key(&self) -> Option<MergeKey> {
        Some(MergeKey::new("job-settings"))
    }
}

/// Replaces the mapping rules used to assign operations on import.
#[derive(Debug, Clone)]
pub struct SetRules {
    /// The rules, in priority order.
    pub rules: Vec<MappingRule>,
}

impl Command for SetRules {
    fn label(&self) -> L10nKey {
        L10nKey::new("cmd-set-rules")
    }
    fn apply(&self, doc: &Document) -> Result<Document, CommandError> {
        let mut out = doc.clone();
        out.rules = Arc::new(self.rules.clone());
        Ok(out)
    }
}

// ---- the store ----

/// A past document root with the label of the step that left it.
#[derive(Debug, Clone)]
struct Step {
    doc: Arc<Document>,
    label: L10nKey,
}

/// The document and its undo history.
#[derive(Debug)]
pub struct DocumentStore {
    current: Arc<Document>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    generation: u64,
    last_merge: Option<(MergeKey, u64)>,
    limit_bytes: Option<usize>,
}

impl DocumentStore {
    /// A store holding `doc`, with no history.
    #[must_use]
    pub fn new(doc: Document) -> Self {
        Self {
            current: Arc::new(doc),
            undo: Vec::new(),
            redo: Vec::new(),
            generation: 0,
            last_merge: None,
            limit_bytes: None,
        }
    }

    /// The current document: a cheap clone of a shared root, safe to hand to background jobs.
    #[must_use]
    pub fn document(&self) -> Arc<Document> {
        Arc::clone(&self.current)
    }

    /// Counts every change (edit, undo, redo); background results are valid only for the
    /// generation they were computed from.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Caps the memory held by history. Nodes shared with the current document are free.
    pub fn set_history_limit_bytes(&mut self, limit: usize) {
        self.limit_bytes = Some(limit);
        self.trim();
    }

    /// Applies a command at time `now_ms`.
    ///
    /// # Errors
    /// The command's [`CommandError`]; nothing changes.
    pub fn apply(&mut self, cmd: &dyn Command, now_ms: u64) -> Result<(), CommandError> {
        let next = cmd.apply(&self.current)?;
        let key = cmd.merge_key();
        let merges = match (&key, &self.last_merge) {
            (Some(k), Some((last, at))) => {
                k == last && now_ms.saturating_sub(*at) <= MERGE_WINDOW_MS
            }
            _ => false,
        };
        if !merges {
            self.undo.push(Step {
                doc: Arc::clone(&self.current),
                label: cmd.label(),
            });
        }
        self.redo.clear();
        self.current = Arc::new(next);
        self.last_merge = key.map(|k| (k, now_ms));
        self.generation += 1;
        self.trim();
        Ok(())
    }

    /// Applies several commands as one undo step, all or nothing.
    ///
    /// # Errors
    /// The first failing command's [`CommandError`]; nothing changes.
    pub fn transaction(
        &mut self,
        label: L10nKey,
        cmds: &[Box<dyn Command>],
        _now_ms: u64,
    ) -> Result<(), CommandError> {
        if cmds.is_empty() {
            return Ok(());
        }
        let mut doc = (*self.current).clone();
        for cmd in cmds {
            doc = cmd.apply(&doc)?;
        }
        self.undo.push(Step {
            doc: Arc::clone(&self.current),
            label,
        });
        self.redo.clear();
        self.current = Arc::new(doc);
        self.last_merge = None;
        self.generation += 1;
        self.trim();
        Ok(())
    }

    /// True when there is a step to undo.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// True when there is a step to redo.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The label of the step undo would undo.
    #[must_use]
    pub fn undo_label(&self) -> Option<&L10nKey> {
        self.undo.last().map(|s| &s.label)
    }

    /// The label of the step redo would redo.
    #[must_use]
    pub fn redo_label(&self) -> Option<&L10nKey> {
        self.redo.last().map(|s| &s.label)
    }

    /// Undoes the last step; false when there is none.
    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else {
            return false;
        };
        self.redo.push(Step {
            doc: Arc::clone(&self.current),
            label: step.label,
        });
        self.current = step.doc;
        self.last_merge = None;
        self.generation += 1;
        true
    }

    /// Redoes the last undone step; false when there is none.
    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else {
            return false;
        };
        self.undo.push(Step {
            doc: Arc::clone(&self.current),
            label: step.label,
        });
        self.current = step.doc;
        self.last_merge = None;
        self.generation += 1;
        true
    }

    /// Drops the oldest steps while history holds more than the limit; one step always stays.
    fn trim(&mut self) {
        let Some(limit) = self.limit_bytes else {
            return;
        };
        while self.undo.len() > 1 && self.history_bytes() > limit {
            self.undo.remove(0);
        }
    }

    /// Bytes held by history alone: unique nodes of the undo and redo roots that the current
    /// document does not also hold.
    fn history_bytes(&self) -> usize {
        let mut seen = BTreeSet::new();
        let mut scratch = 0;
        walk(&self.current, &mut seen, &mut scratch);
        let mut bytes = 0;
        for step in self.undo.iter().chain(&self.redo) {
            walk(&step.doc, &mut seen, &mut bytes);
        }
        bytes
    }
}

/// Counts the unique nodes of a document into `bytes`, skipping nodes already in `seen`.
fn walk(doc: &Document, seen: &mut BTreeSet<usize>, bytes: &mut usize) {
    fn fresh<T>(arc: &Arc<T>, cost: usize, seen: &mut BTreeSet<usize>, bytes: &mut usize) -> bool {
        let new = seen.insert(Arc::as_ptr(arc).addr());
        if new {
            *bytes += cost;
        }
        new
    }
    fresh(&doc.meta, 128, seen, bytes);
    fresh(&doc.settings, 128, seen, bytes);
    fresh(&doc.rules, 64 + doc.rules.len() * 96, seen, bytes);
    if fresh(&doc.assets, 64 + doc.assets.len() * 64, seen, bytes) {
        for a in doc.assets.values() {
            fresh(a, 128 + a.bytes.len(), seen, bytes);
        }
    }
    if fresh(&doc.mats, 64 + doc.mats.len() * 64, seen, bytes) {
        for mat in doc.mats.values() {
            if !fresh(mat, 256, seen, bytes) {
                continue;
            }
            if !fresh(&mat.objects, 64 + mat.objects.len() * 64, seen, bytes) {
                continue;
            }
            for obj in mat.objects.values() {
                if !fresh(obj, 128, seen, bytes) {
                    continue;
                }
                if fresh(&obj.paths, 64 + obj.paths.len() * 8, seen, bytes) {
                    for p in obj.paths.iter() {
                        fresh(
                            p,
                            192 + p.geom.segs.len() * 48 + p.ops.len() * 160,
                            seen,
                            bytes,
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
