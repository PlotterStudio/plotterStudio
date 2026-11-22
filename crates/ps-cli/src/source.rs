//! Turning an input file into a document: a project is opened as it is; a drawing goes through
//! the importer, repair, the mapping rules and placement, exactly as the app's import does.

use std::path::Path;
use std::sync::Arc;

use ps_core::{AssetId, Diagnostics, MatId};
use ps_import::{
    ImportedDrawing, OwnedPen, PdfOptions, PlaceInput, PlaceMode, RepairOptions, SvgOptions,
    check_rules, import_dxf, import_pdf, import_svg_auto, map_all, parse_hex, place, repair,
};
use ps_library::LibrarySnapshot;
use ps_model::{Asset, Document, MappingRule, Mat, MatSize, Project, ProjectMeta};
use semver::Version;

use crate::Failure;

/// A document ready to compile, with what the import noticed on the way.
pub(crate) struct Loaded {
    pub document: Document,
    pub diagnostics: Diagnostics,
    pub name: String,
}

/// The kind of input, from its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Project,
    Svg,
    Dxf,
    Pdf,
}

pub(crate) fn kind_of(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "pstudio" => Some(Kind::Project),
        "svg" | "svgz" => Some(Kind::Svg),
        "dxf" => Some(Kind::Dxf),
        "pdf" | "ai" => Some(Kind::Pdf),
        _ => None,
    }
}

fn read(path: &Path) -> Result<Vec<u8>, Failure> {
    std::fs::read(path).map_err(|e| Failure::usage(format!("cannot read {}: {e}", path.display())))
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| "job".to_owned(), |s| s.to_string_lossy().into_owned())
}

fn load_rules(path: Option<&Path>) -> Result<Vec<MappingRule>, Failure> {
    let path = path.ok_or_else(|| {
        Failure::usage("importing a drawing needs mapping rules; pass --rules <file.json>")
    })?;
    let text = std::fs::read_to_string(path)
        .map_err(|e| Failure::usage(format!("cannot read {}: {e}", path.display())))?;
    let rules: Vec<MappingRule> = serde_json::from_str(&text)
        .map_err(|e| Failure::usage(format!("{} is not a rule list: {e}", path.display())))?;
    if let Some(issue) = check_rules(&rules).into_iter().next() {
        return Err(Failure::usage(format!(
            "{}: rule {}: {}",
            path.display(),
            issue.rule + 1,
            issue.message
        )));
    }
    Ok(rules)
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

fn imported(
    drawing: ImportedDrawing,
    path: &Path,
    bytes: Vec<u8>,
    rules: &[MappingRule],
    libs: &LibrarySnapshot,
    created: &str,
) -> Loaded {
    let name = stem(path);
    let mut diagnostics = drawing.diagnostics.clone();
    let mut drawing = drawing;
    let repaired = repair(
        std::mem::take(&mut drawing.items),
        &RepairOptions::default(),
    );
    drawing.items = repaired.items;

    let styles: Vec<_> = drawing.items.iter().map(|i| i.style.clone()).collect();
    let (mappings, mapping_diags) = map_all(rules, &styles, &owned_pens(libs));
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

    let mut mat = Mat::new(MatId::new(), "Mat 1", MatSize::inches(12.0, 12.0));
    for object in placement.objects {
        Arc::make_mut(&mut mat.objects).insert(object.id, Arc::new(object));
    }
    mat.op_order = Arc::new(placement.rows.iter().map(|r| r.row).collect());

    let mut document = Document::new(ProjectMeta::new(&name, created, env!("CARGO_PKG_VERSION")));
    let file_name = path
        .file_name()
        .map_or_else(|| name.clone(), |f| f.to_string_lossy().into_owned());
    let asset = Asset::new(asset_id, &file_name, bytes);
    Arc::make_mut(&mut document.assets).insert(asset.id, Arc::new(asset));
    Arc::make_mut(&mut document.mats).insert(mat.id, Arc::new(mat));
    document.rules = Arc::new(rules.to_vec());
    Loaded {
        document,
        diagnostics,
        name,
    }
}

/// Loads a project, or imports a drawing with the mapping rules.
pub(crate) fn load(
    path: &Path,
    rules: Option<&Path>,
    libs: &LibrarySnapshot,
    created: &str,
) -> Result<Loaded, Failure> {
    let kind = kind_of(path).ok_or_else(|| {
        Failure::usage(format!(
            "{}: unsupported input; expected .pstudio, .svg, .svgz, .dxf, .pdf or .ai",
            path.display()
        ))
    })?;
    let bytes = read(path)?;
    match kind {
        Kind::Project => {
            let app = Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version::new(0, 0, 0));
            let loaded = Project::from_bytes(&bytes, &app)
                .map_err(|e| Failure::refused(format!("{}: {e}", path.display())))?;
            Ok(Loaded {
                document: loaded.project.document,
                diagnostics: Diagnostics::new(),
                name: stem(path),
            })
        }
        Kind::Svg | Kind::Dxf | Kind::Pdf => {
            let rules = load_rules(rules)?;
            let refuse =
                |e: &dyn std::fmt::Display| Failure::refused(format!("{}: {e}", path.display()));
            let drawing = match kind {
                Kind::Svg => {
                    import_svg_auto(&bytes, &SvgOptions::default()).map_err(|e| refuse(&e))?
                }
                Kind::Pdf => import_pdf(&bytes, &PdfOptions::default()).map_err(|e| refuse(&e))?,
                _ => import_dxf(&bytes).map_err(|e| refuse(&e))?,
            };
            Ok(imported(drawing, path, bytes, &rules, libs, created))
        }
    }
}
