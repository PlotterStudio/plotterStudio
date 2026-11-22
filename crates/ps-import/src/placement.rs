//! Placement.
//!
//! Creates the document objects for an imported drawing: one per layer, one per path, or one for
//! everything (the user's choice), linked to the stored original asset, each path carrying the
//! operation the mapping rules assigned. Operations that share a tool and kind share an operation
//! row, so downstream they form one block. Unmapped paths get no operation and are reported by
//! the compiler's validation instead of being given a silent default.

use std::sync::Arc;

use ps_core::{AssetId, Diagnostic, Diagnostics, L10nKey, LibRef, ObjectId, OpId, OpRowId};
use ps_geometry::{Affine, FillRule};
use ps_model::{
    OpKind, Operation, ParamOverrides, PlacedObject, ShapePath, SourceLink, ToolSlotRef,
};

use crate::colour::to_hex;
use crate::drawing::{ImportedDrawing, ImportedPath};
use crate::rules::Mapping;

/// How imported paths are grouped into objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceMode {
    /// One object per layer (paths without a layer share one object, placed last).
    PerLayer,
    /// One object per path.
    PerPath,
    /// One object for the whole drawing.
    Single,
}

/// Everything placement reads.
#[derive(Clone, Copy)]
pub struct PlaceInput<'a> {
    /// The repaired drawing.
    pub drawing: &'a ImportedDrawing,
    /// One mapping per item of the drawing, in order; a missing one counts as unmapped.
    pub mappings: &'a [Mapping],
    /// The stored original the objects link to.
    pub asset: AssetId,
    /// How to group paths.
    pub mode: PlaceMode,
    /// Finds the library reference for a tool id (the version to use).
    pub resolve_tool: &'a dyn Fn(&str) -> Option<LibRef>,
}

impl std::fmt::Debug for PlaceInput<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlaceInput")
            .field("items", &self.drawing.items.len())
            .field("mappings", &self.mappings.len())
            .field("asset", &self.asset)
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}

/// An operation row created by placement, for the mat's row order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowInfo {
    /// The row.
    pub row: OpRowId,
    /// What the row does.
    pub kind: OpKind,
    /// The tool it uses.
    pub tool: LibRef,
}

/// The objects to add to a mat.
#[derive(Debug)]
pub struct Placement {
    /// The new objects, in placement order.
    pub objects: Vec<PlacedObject>,
    /// The operation rows created, in order of first use.
    pub rows: Vec<RowInfo>,
    /// Problems found while placing.
    pub diagnostics: Diagnostics,
}

struct Group {
    element: String,
    visible: bool,
    paths: Vec<Arc<ShapePath>>,
}

fn operation(
    kind: OpKind,
    tool: LibRef,
    pen_colour: Option<[u8; 3]>,
    rows: &mut Vec<RowInfo>,
) -> Operation {
    let existing = rows
        .iter()
        .find(|r| r.kind == kind && r.tool == tool)
        .map(|r| r.row);
    let row = existing.unwrap_or_else(|| {
        let row = OpRowId::new();
        rows.push(RowInfo {
            row,
            kind,
            tool: tool.clone(),
        });
        row
    });
    Operation {
        id: OpId::new(),
        kind,
        tool: ToolSlotRef {
            tool,
            pen_colour: pen_colour.map(to_hex),
        },
        overrides: ParamOverrides::default(),
        fill: None,
        enabled: true,
        row,
    }
}

fn shape(
    item: &ImportedPath,
    mapping: Option<&Mapping>,
    input: &PlaceInput<'_>,
    rows: &mut Vec<RowInfo>,
    diagnostics: &mut Diagnostics,
) -> Arc<ShapePath> {
    let mut ops = Vec::new();
    if let Some(Mapping::Assigned {
        op,
        tool,
        pen_colour,
        ..
    }) = mapping
    {
        match (input.resolve_tool)(tool) {
            Some(tool) => ops.push(operation(*op, tool, *pen_colour, rows)),
            None => diagnostics.push(
                Diagnostic::warning(L10nKey::new("diag-import-unknown-tool"))
                    .with_arg("tool", tool.clone()),
            ),
        }
    }
    Arc::new(ShapePath {
        key: item.key.clone(),
        geom: item.geom.clone(),
        fill_rule: FillRule::NonZero,
        style: item.style.clone(),
        ops,
        start_locked: false,
    })
}

/// Builds the objects for an imported drawing.
#[must_use]
pub fn place(input: &PlaceInput<'_>) -> Placement {
    let drawing = input.drawing;
    let mut rows = Vec::new();
    let mut diagnostics = Diagnostics::new();
    let shapes: Vec<Arc<ShapePath>> = drawing
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            shape(
                item,
                input.mappings.get(i),
                input,
                &mut rows,
                &mut diagnostics,
            )
        })
        .collect();

    let mut groups: Vec<Group> = Vec::new();
    match input.mode {
        PlaceMode::Single => groups.push(Group {
            element: String::new(),
            visible: true,
            paths: shapes,
        }),
        PlaceMode::PerPath => {
            for (item, s) in drawing.items.iter().zip(shapes) {
                groups.push(Group {
                    element: item.key.to_string(),
                    visible: true,
                    paths: vec![s],
                });
            }
        }
        PlaceMode::PerLayer => {
            // Listed layers come first, in their stacking order; paths without a layer last.
            let mut ordered: Vec<(Option<&str>, bool)> = Vec::new();
            let mut layers: Vec<_> = drawing.layers.iter().collect();
            layers.sort_by_key(|l| l.order);
            for l in layers {
                ordered.push((Some(l.name.as_str()), l.visible));
            }
            for item in &drawing.items {
                if let Some(name) = item.layer.as_deref()
                    && !ordered.iter().any(|(n, _)| *n == Some(name))
                {
                    ordered.push((Some(name), true));
                }
            }
            ordered.push((None, true));
            for (name, visible) in ordered {
                let paths: Vec<Arc<ShapePath>> = drawing
                    .items
                    .iter()
                    .zip(&shapes)
                    .filter(|(item, _)| item.layer.as_deref() == name)
                    .map(|(_, s)| Arc::clone(s))
                    .collect();
                if !paths.is_empty() {
                    groups.push(Group {
                        element: name.unwrap_or_default().to_owned(),
                        visible,
                        paths,
                    });
                }
            }
        }
    }

    let objects = groups
        .into_iter()
        .map(|g| PlacedObject {
            id: ObjectId::new(),
            source: Some(SourceLink {
                asset: input.asset,
                element: g.element,
            }),
            transform: Affine::IDENTITY,
            group: None,
            attach: None,
            paths: Arc::new(g.paths),
            locked: false,
            hidden: !g.visible,
        })
        .collect();
    Placement {
        objects,
        rows,
        diagnostics,
    }
}

#[cfg(test)]
mod tests;
