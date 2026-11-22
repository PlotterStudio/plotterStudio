//! Importing a drawing into the open project.
//!
//! The same importers, repair, mapping rules and placement as `pstudio export`, but the result is
//! added to the current mat as one undo step. The original file is kept in the project so a
//! re-import can match its paths again, and the rules used are saved with the project.

use std::path::Path;

use ps_core::{AssetId, Diagnostics, ObjectId};
use ps_import::{
    ImportedDrawing, OwnedPen, PdfOptions, PlaceInput, PlaceMode, RepairOptions, SvgOptions,
    import_dxf, import_pdf, import_svg_auto, map_all, parse_hex, place, repair,
};
use ps_library::LibrarySnapshot;
use ps_model::{
    AddAsset, AddObject, Asset, Assign, Command as ModelCommand, MappingRule, Match, OpKind,
    SetOpOrder, SetRules,
};

use crate::state::AppState;

/// The kind of drawing, from the file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    /// SVG or compressed SVG.
    Svg,
    /// DXF.
    Dxf,
    /// PDF, or an Illustrator file with a PDF-compatible stream.
    Pdf,
}

/// Why a file was not imported.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    /// The extension is not a drawing the app imports.
    #[error("{0}: not a drawing the app can import (SVG, SVGZ or DXF)")]
    Unsupported(String),
    /// The file could not be read.
    #[error("cannot read {0}")]
    Io(String),
    /// The importer refused the file.
    #[error("{0}")]
    Refused(String),
}

/// What an import did.
#[derive(Debug)]
pub struct ImportReport {
    /// The file's name.
    pub name: String,
    /// How many objects were added.
    pub objects: usize,
    /// What the importer, the rules and the placement noticed.
    pub diagnostics: Diagnostics,
}

/// The kind of drawing a file is, if the app imports it.
#[must_use]
pub fn kind_of(path: &Path) -> Option<ImportKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "svg" | "svgz" => Some(ImportKind::Svg),
        "dxf" => Some(ImportKind::Dxf),
        "pdf" | "ai" => Some(ImportKind::Pdf),
        _ => None,
    }
}

/// The rules a new project starts with: red lines cut, blue lines are drawn with the black
/// fineliner. They are saved with the project, where the user can change them.
#[must_use]
pub fn default_rules() -> Vec<MappingRule> {
    let rule = |colour: &str, op: OpKind, tool: &str| MappingRule {
        matcher: Match {
            stroke: Some(colour.to_owned()),
            ..Match::default()
        },
        assign: Assign {
            op,
            tool: tool.to_owned(),
        },
    };
    vec![
        rule("#FF0000", OpKind::Cut, "generic.drag-blade"),
        rule("#0000FF", OpKind::Draw, "pen.fineliner-black-0.4"),
    ]
}

fn owned_pens(libs: &LibrarySnapshot) -> Vec<OwnedPen> {
    libs.owned_pens()
        .iter()
        .filter_map(|tool| {
            let colour = tool.pen.as_ref()?.colour.hex.as_str();
            Some(OwnedPen {
                tool_id: tool.id.clone(),
                colour: parse_hex(colour)?,
            })
        })
        .collect()
}

fn newest_tool(libs: &LibrarySnapshot, id: &str) -> Option<ps_core::LibRef> {
    libs.tools()
        .into_iter()
        .filter(|t| t.id == id)
        .max_by(|a, b| a.version.cmp(&b.version))
        .map(|t| t.lib_ref())
}

fn read_drawing(
    path: &Path,
    kind: ImportKind,
    bytes: &[u8],
) -> Result<ImportedDrawing, ImportError> {
    let refuse =
        |e: &dyn std::fmt::Display| ImportError::Refused(format!("{}: {e}", path.display()));
    match kind {
        ImportKind::Svg => import_svg_auto(bytes, &SvgOptions::default()).map_err(|e| refuse(&e)),
        ImportKind::Dxf => import_dxf(bytes).map_err(|e| refuse(&e)),
        ImportKind::Pdf => import_pdf(bytes, &PdfOptions::default()).map_err(|e| refuse(&e)),
    }
}

/// Imports a drawing onto the current mat as one undo step and selects what it added.
///
/// # Errors
///
/// [`ImportError`] when the file is not a supported drawing, cannot be read, or is refused by
/// the importer; the project is then unchanged.
pub fn import_file(
    state: &mut AppState,
    path: &Path,
    now_ms: u64,
) -> Result<ImportReport, ImportError> {
    let name = path.display().to_string();
    let kind = kind_of(path).ok_or_else(|| ImportError::Unsupported(name.clone()))?;
    let bytes = std::fs::read(path).map_err(|_| ImportError::Io(name.clone()))?;
    let mut drawing = read_drawing(path, kind, &bytes)?;

    let mut diagnostics = drawing.diagnostics.clone();
    let repaired = repair(
        std::mem::take(&mut drawing.items),
        &RepairOptions::default(),
    );
    drawing.items = repaired.items;

    let existing = state.document().rules.as_ref().clone();
    let (rules, save_rules) = if existing.is_empty() {
        (default_rules(), true)
    } else {
        (existing, false)
    };
    let libs = state.libs();
    let styles: Vec<_> = drawing.items.iter().map(|i| i.style.clone()).collect();
    let (mappings, mapping_diags) = map_all(&rules, &styles, &owned_pens(libs));
    for d in &mapping_diags {
        diagnostics.push(d.clone());
    }

    let asset_id = AssetId::new();
    let placement = place(&PlaceInput {
        drawing: &drawing,
        mappings: &mappings,
        asset: asset_id,
        mode: PlaceMode::PerLayer,
        resolve_tool: &|id| newest_tool(libs, id),
    });
    for d in &placement.diagnostics {
        diagnostics.push(d.clone());
    }

    let file_name = path
        .file_name()
        .map_or_else(|| name.clone(), |f| f.to_string_lossy().into_owned());
    let mat = state.mat_id();
    let mut cmds: Vec<Box<dyn ModelCommand>> = vec![Box::new(AddAsset {
        asset: Asset::new(asset_id, &file_name, bytes),
    })];
    let mut ids: Vec<ObjectId> = Vec::new();
    for object in placement.objects {
        ids.push(object.id);
        cmds.push(Box::new(AddObject { mat, object }));
    }
    let mut order = state.current_mat().op_order.as_ref().clone();
    for row in &placement.rows {
        if !order.contains(&row.row) {
            order.push(row.row);
        }
    }
    cmds.push(Box::new(SetOpOrder { mat, order }));
    if save_rules {
        cmds.push(Box::new(SetRules { rules }));
    }
    if !state.commit_all(&cmds, now_ms) {
        return Err(ImportError::Refused(name));
    }
    state.select_objects(&ids);
    Ok(ImportReport {
        name: file_name,
        objects: ids.len(),
        diagnostics,
    })
}

#[cfg(test)]
mod tests;
