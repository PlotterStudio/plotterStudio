//! Plotter Studio importers.
//!
//! Every importer produces the same intermediate form, so repair, mapping rules and re-import are
//! written once.

mod colour;
mod drawing;
mod dxf;
mod pdf;
mod placement;
mod reimport;
mod repair;
mod rules;
mod svg;
mod svgfont;
mod text;
mod units;

pub use colour::{delta_e2000, delta_e2000_lab, nearest, parse_hex, to_hex, within};
pub use drawing::{ImportedDrawing, ImportedPath, LayerInfo};
pub use dxf::{DxfError, aci_rgb, import_dxf};
pub use pdf::{MAX_PDF_BYTES, PdfError, PdfOptions, import_pdf};
pub use placement::{PlaceInput, PlaceMode, Placement, RowInfo, place};
pub use reimport::{Counts, MatchPass, Matched, PROXIMITY_MM, Reimport, hausdorff, match_paths};
pub use repair::{RepairAction, RepairOptions, Repaired, repair};
pub use rules::{
    Mapping, OwnedPen, RuleIssue, check_rules, colour_distance, map_all, map_style, matches,
};
pub use svg::{ImportError, SvgOptions, import_svg, import_svg_auto};
pub use svgfont::{SvgFontError, SvgStrokeFont};
pub use text::{Align, StrokeFont, TextError, TextOutline, TextStyle, outline_text, stroke_text};
pub use units::{
    Generator, SizeCandidate, UnitGuess, detect_generator, dxf_is_unitless, dxf_unit_guess,
    mm_per_unit, paper_name, svg_size_candidates,
};
