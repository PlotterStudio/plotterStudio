//! What the automation API needs from the state that the window never did: acting on objects by
//! id, drawing a shape from numbers, and compiling on demand.

use std::sync::Arc;

use ps_core::{MatId, ObjectId, PathKey};
use ps_geometry::{Affine, Path, Point};
use ps_model::{AddObject, Command as ModelCommand, PlacedObject, ShapePath, SplitPath};

use crate::selection::Item;

use super::AppState;

impl AppState {
    /// The mat being edited.
    pub(crate) fn current_mat_id(&self) -> MatId {
        self.mat
    }

    /// The mat that holds an object, if any does.
    pub(crate) fn mat_of_object(&self, object: ObjectId) -> Option<MatId> {
        self.store
            .document()
            .mats
            .values()
            .find(|m| m.objects.contains_key(&object))
            .map(|m| m.id)
    }

    /// Adds one object whose single path `geom` is relative to `origin`, and selects it.
    pub(crate) fn add_shape(&mut self, geom: Path, origin: Point, now_ms: u64) -> Option<ObjectId> {
        let id = ObjectId::new();
        let object = PlacedObject {
            id,
            source: None,
            transform: Affine::translate((origin.x, origin.y)),
            group: None,
            attach: None,
            paths: Arc::new(vec![Arc::new(ShapePath {
                key: PathKey::source(format!("api-{id}")),
                geom,
                fill_rule: ps_geometry::FillRule::NonZero,
                style: ps_model::SourceStyle::default(),
                ops: Vec::new(),
                start_locked: false,
            })]),
            locked: false,
            hidden: false,
        };
        let command: Box<dyn ModelCommand> = Box::new(AddObject {
            mat: self.mat,
            object,
        });
        if self.commit(&[command], now_ms) {
            self.selection.set_objects(&[id]);
            Some(id)
        } else {
            None
        }
    }

    /// Selects one node of one path of an object, so the node commands act on it.
    pub(crate) fn select_node(&mut self, object: ObjectId, path: usize, node: usize) -> bool {
        let mat = self.current_mat();
        let Some(key) = mat
            .objects
            .get(&object)
            .and_then(|o| o.paths.get(path))
            .map(|p| p.key.clone())
        else {
            return false;
        };
        let Ok(node) = u32::try_from(node) else {
            return false;
        };
        self.selection.clear();
        self.selection.select(Item {
            object,
            path: Some(key),
            node: Some(node),
        })
    }

    /// Cuts every path of these objects that the line from `a` to `b` crosses.
    pub(crate) fn split_objects(
        &mut self,
        objects: &[ObjectId],
        a: Point,
        b: Point,
        now_ms: u64,
    ) -> bool {
        let mat = self.current_mat();
        let mut commands: Vec<Box<dyn ModelCommand>> = Vec::new();
        for id in objects {
            let Some(object) = mat.objects.get(id) else {
                continue;
            };
            // The cut is made in the object's own space, so the pieces need no transform.
            let inverse = object.transform.inverse();
            let (la, lb) = (inverse * a, inverse * b);
            for path in object.paths.iter() {
                let Some(pieces) = ps_geometry::knife(&path.geom, la, lb) else {
                    continue;
                };
                let pieces = pieces
                    .into_iter()
                    .enumerate()
                    .map(|(i, geom)| (PathKey::source(format!("{}-{id}-cut{i}", path.key)), geom))
                    .collect();
                commands.push(Box::new(SplitPath {
                    mat: self.mat,
                    object: *id,
                    path: path.key.clone(),
                    pieces,
                }));
            }
        }
        !commands.is_empty() && self.commit(&commands, now_ms)
    }

    /// Starts compiling now, without waiting for the edit delay. False when nothing can compile
    /// (no machine profile) or a compile is already running.
    pub(crate) fn compile_now(&mut self) -> bool {
        if self.compiling || self.profile.is_none() {
            return false;
        }
        self.start_compile();
        true
    }

    /// Whether the compiled job matches the current document.
    pub(crate) fn compile_is_current(&self) -> bool {
        self.compiled
            .as_ref()
            .is_some_and(|c| c.doc_generation == self.store.generation())
    }
}
