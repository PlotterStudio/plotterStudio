//! Plotter Studio application logic.
//!
//! Everything the UI needs that is not drawing: number formatting, the command registry with
//! shortcuts and the palette, localisation, and background jobs. The window code is a thin layer
//! over this, so the behaviour is tested without a window.

pub mod api;
mod arrange;
mod canvas_scene;
mod clock;
mod commands;
mod export;
mod fonts;
mod import;
mod jobs;
mod l10n;
mod library_manager;
pub mod logging;
mod machine;
pub mod mcp;
mod numfmt;
mod online;
mod plugin_install;
mod project_io;
mod selection;
mod session;
mod shell;
mod state;
mod text_object;
mod theme;
mod tools;
mod ui;

pub use arrange::{
    AlignKind, Anchor, Axis, Copy, Item as ArrangeItem, align, anchor_point, circular_array,
    distribute, grid_array, mirror, move_to, resize, rotate, union_bounds,
};
pub use canvas_scene::{ACCENT, GUIDE, MAT_COLOUR, build_scene, build_scene_at};
pub use clock::rfc3339_utc;
pub use commands::{
    AssignError, Command, Context, Hit, Mods, Platform, Registry, Shortcut, nudge_distance_mm,
};
pub use export::{
    ExportError, ExportFormat, LineClass, classify_gcode_line, export_job, gcode_for_mat,
    job_id_of, mat_label,
};
pub use fonts::{BUNDLED_FAMILY, FontLibrary};
pub use import::{ImportError, ImportKind, ImportReport, default_rules, import_file, kind_of};
pub use jobs::{COMPILE_DEBOUNCE_MS, Debounce, JobContext, JobId, JobManager, Outcome};
pub use l10n::{Arg, Localizer};
pub use library_manager::{InstallReport, LibraryManager, LibraryRow};
pub use machine::{MachineController, job_header};
pub use numfmt::{NumberFormat, format_duration_ms, format_percent};
pub use online::{
    MATERIAL_INDEX_URL, OnlineController, OnlineError, RELEASE_FEED_URL, trusted_keys,
    trusted_keys_from,
};
pub use project_io::{OpenProject, ProjectIoError, open_project, save_project};
pub use selection::{Item, Selection};
pub use session::{SessionGuard, Snapshot, Startup, project_id, write_panic_files};
pub use shell::{Dialogs, export_to_folder, handle_effect};
pub use state::{
    AppState, CompileSummary, Compiled, Effect, FocusPanel, JobStatus, MachineChoice, MatSummary,
    MaterialChoice, Mode, OperationRow, Preview, PreviewView, ToolChoice,
};
pub use text_object::{TextParams, text_paths};
pub use tools::{
    ArcTool, CanvasEvent, Cursor, EditPath, EllipseTool, Handle, LineTool, NodeTool, Overlay,
    PenTool, PolygonTool, RectangleTool, SelectTool, StarTool, Tool, ToolCtx, ToolId, ToolOutcome,
};
pub use ui::{App, LegendEntry, key_name, operation_legend, show};
