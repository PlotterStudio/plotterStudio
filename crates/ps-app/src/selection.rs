//! The selection: a set of `(object, optional path, optional node)` kept in app state,
//! not in the document. Selection changes are not undo steps.

use std::collections::BTreeSet;

use ps_core::{ObjectId, PathKey};

/// One selected thing: a whole object, one of its paths, or a node of a path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Item {
    /// The object.
    pub object: ObjectId,
    /// A path of the object, when the selection is narrower than the object.
    pub path: Option<PathKey>,
    /// A node of that path, when the selection is a single node.
    pub node: Option<u32>,
}

impl Item {
    /// A whole object.
    #[must_use]
    pub fn object(object: ObjectId) -> Self {
        Self {
            object,
            path: None,
            node: None,
        }
    }

    /// One shape (path) of an object.
    #[must_use]
    pub fn shape(object: ObjectId, path: PathKey) -> Self {
        Self {
            object,
            path: Some(path),
            node: None,
        }
    }

    /// Whether this is a shape selected on its own. A node counts as its whole object.
    #[must_use]
    pub fn is_shape(&self) -> bool {
        self.path.is_some() && self.node.is_none()
    }
}

/// What is selected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    items: BTreeSet<Item>,
}

impl Selection {
    /// Whether nothing is selected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// How many items are selected.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// The selected items in a stable order.
    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.items.iter()
    }

    /// The objects with anything selected in them, once each.
    #[must_use]
    pub fn objects(&self) -> Vec<ObjectId> {
        let mut out: Vec<ObjectId> = self.items.iter().map(|i| i.object).collect();
        out.dedup();
        out
    }

    /// Whether anything of this object is selected.
    #[must_use]
    pub fn contains_object(&self, object: ObjectId) -> bool {
        self.items.iter().any(|i| i.object == object)
    }

    /// Makes `item` the whole selection; true if the selection changed.
    pub fn select(&mut self, item: Item) -> bool {
        if self.items.len() == 1 && self.items.contains(&item) {
            return false;
        }
        self.items.clear();
        self.items.insert(item);
        true
    }

    /// Makes one whole object the selection; true if the selection changed.
    pub fn select_object(&mut self, object: ObjectId) -> bool {
        self.select(Item::object(object))
    }

    /// Adds an item; true if it was not already selected.
    pub fn add(&mut self, item: Item) -> bool {
        self.items.insert(item)
    }

    /// Adds a whole object, which takes the place of any of its shapes; true if it was not
    /// already selected whole.
    pub fn add_object(&mut self, object: ObjectId) -> bool {
        self.items.retain(|i| i.object != object || !i.is_shape());
        self.add(Item::object(object))
    }

    /// The shapes selected on their own, as opposed to whole objects.
    pub fn shapes(&self) -> impl Iterator<Item = (ObjectId, &PathKey)> {
        self.items
            .iter()
            .filter(|i| i.is_shape())
            .filter_map(|i| Some((i.object, i.path.as_ref()?)))
    }

    /// Whether any shape is selected on its own.
    #[must_use]
    pub fn has_shapes(&self) -> bool {
        self.items.iter().any(Item::is_shape)
    }

    /// Whether a path is selected, on its own or as part of its whole object.
    #[must_use]
    pub fn includes_path(&self, object: ObjectId, path: &PathKey) -> bool {
        self.items
            .iter()
            .filter(|i| i.object == object)
            .any(|i| !i.is_shape() || i.path.as_ref() == Some(path))
    }

    /// Selects exactly these shapes.
    pub fn set_shapes(&mut self, shapes: &[(ObjectId, PathKey)]) {
        self.items = shapes
            .iter()
            .map(|(o, p)| Item::shape(*o, p.clone()))
            .collect();
    }

    /// Adds a shape, which takes the place of its whole object.
    pub fn add_shape(&mut self, object: ObjectId, path: PathKey) {
        self.items.retain(|i| i.object != object || i.is_shape());
        self.items.insert(Item::shape(object, path));
    }

    /// Adds the shape if it is not selected on its own, removes it if it is.
    pub fn toggle_shape(&mut self, object: ObjectId, path: PathKey) {
        if !self.items.remove(&Item::shape(object, path.clone())) {
            self.add_shape(object, path);
        }
    }

    /// Drops the shapes and nodes whose path no longer exists (after a cut, a delete or an undo).
    pub fn retain_paths(&mut self, exists: impl Fn(ObjectId, &PathKey) -> bool) {
        self.items
            .retain(|i| i.path.as_ref().is_none_or(|p| exists(i.object, p)));
    }

    /// Adds the object if it is not selected, removes it if it is.
    pub fn toggle_object(&mut self, object: ObjectId) {
        if self.contains_object(object) {
            self.items.retain(|i| i.object != object);
        } else {
            self.items.insert(Item::object(object));
        }
    }

    /// Selects exactly these whole objects.
    pub fn set_objects(&mut self, objects: &[ObjectId]) {
        self.items = objects.iter().map(|o| Item::object(*o)).collect();
    }

    /// Empties the selection.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Drops items whose object no longer exists (after a delete or an undo).
    pub fn retain_existing(&mut self, exists: impl Fn(ObjectId) -> bool) {
        self.items.retain(|i| exists(i.object));
    }
}

#[cfg(test)]
mod tests;
