//! The app state.
//!
//! Panels read this each frame and send commands; the state applies them to the document store,
//! keeps the selection, tool, viewport and mode, and schedules a debounced compile when the
//! document changes. It holds no window or GPU types, so all of it is tested without a UI.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ps_compiler::{
    CompileCache, CompileInput, DEFAULT_CAPACITY_BYTES, MatSelection, compile_cached,
};
use ps_core::{LibRef, MatId, ObjectId, OpId, OpRowId, PathKey, Severity};
use ps_geometry::{Affine, FillRule, Point, Rect};
use ps_job::Job;
use ps_library::{Calibration, LibrarySnapshot, MachineProfile, Settings, builtin_entries};
use ps_model::{
    AddMat, AddObject, AssignOperation, Command as ModelCommand, Document, DocumentStore, Guide,
    GuideAxis, Mat, MatSize, OpKind, Operation, ParamOverrides, PlacedObject, Project, ProjectMeta,
    ReadOnly, RemoveMat, RemoveObject, RemovePath, SetGuides, SetMatMaterial, SetOpEnabled,
    SetOpOrder, SetRunAfter, SetTransform, ShapePath, SourceStyle, ToolSlotRef, validate,
};
use ps_render::{Candidate, Pickable, SnapKind, Viewport};

use crate::arrange::{self, AlignKind, Anchor, Axis, Copy as ArrangeCopy, Item as ArrangeItem};
use crate::commands::{Context, Mods, Registry, Shortcut, nudge_distance_mm};
use crate::jobs::{COMPILE_DEBOUNCE_MS, Debounce, JobManager, Outcome};
use crate::selection::Selection;
use crate::tools::{CanvasEvent, EditPath, Overlay, Tool, ToolCtx, ToolId, ToolOutcome};

const DEFAULT_MACHINE: &str = "generic.cutter-small";
const DUPLICATE_OFFSET_MM: f64 = 5.0;
const NUDGE_STEP_MM: f64 = 1.0;
const ZOOM_STEP: f64 = 1.25;
/// The grid the cursor snaps to, in millimetres, when it is wide enough to see.
const GRID_SNAP_MM: f64 = 10.0;
const MIN_GRID_SNAP_PX: f64 = 8.0;
/// The most objects looked at for snap points, so a huge drawing stays responsive.
const MAX_SNAP_OBJECTS: usize = 2_000;
/// The most paths near the cursor that are checked against each other for crossings.
const MAX_CROSSING_PATHS: usize = 40;
const FIT_MARGIN_PX: f64 = 40.0;
/// How many roll widths of a long roll the fit view shows from the top.
const ROLL_VIEW_ASPECT: f64 = 2.0;

/// The three modes that share one canvas and one selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Arrange and assign operations.
    Prepare,
    /// Watch the job.
    Preview,
    /// Export the job.
    Export,
}

/// Work a command asks the window layer to do (dialogs, files, the machine).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Nothing more to do.
    None,
    /// Start a new project.
    NewProject,
    /// Choose a material package and install it.
    InstallPackage,
    /// Choose a device plugin and install it. Only the settings window asks for this: no command
    /// or automation tool can, because a plugin is code that drives a machine.
    InstallPlugin,
    /// Choose and open a project.
    OpenProject,
    /// Save the project.
    Save,
    /// Save under a new name.
    SaveAs,
    /// Open the export step.
    Export,
    /// Choose and import a drawing.
    Import,
    /// Send the compiled job to the machine.
    SendToMachine,
}

/// One row of the operations table: operations of one kind with one tool, run together.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationRow {
    /// The row's identity.
    pub id: OpRowId,
    /// What the operations do.
    pub kind: OpKind,
    /// The library reference of the tool.
    pub tool: String,
    /// The tool's display name.
    pub tool_name: String,
    /// The holder the tool goes in, for display; empty when the tool lists none.
    pub holder: String,
    /// The ink of a pen row as `#RRGGBB`; `None` when the tool is not a pen.
    pub ink: Option<String>,
    /// How many paths carry the row.
    pub paths: usize,
    /// The total line length of those paths, in millimetres.
    pub length_mm: f64,
    /// True when every operation in the row runs.
    pub enabled: bool,
    /// True when some but not all of them run.
    pub partly: bool,
    /// True when the row is locked to run after the row above it.
    pub locked_after_previous: bool,
    /// The name of the mat's material when it has a preset for this row's tool and operation.
    pub preset: Option<String>,
    /// The row's share of the compiled job's machine time, by drawn length; `None` until compiled.
    pub time_ms: Option<u64>,
}

/// What the realistic preview shows: the job's effects composited over the mat's material.
#[derive(Debug, Clone)]
pub struct Preview {
    /// The composited picture of the mat.
    pub image: ps_render::Image,
    /// One look per tool in the job.
    pub looks: Vec<ps_render::ToolLook>,
    /// The material the effects are laid over.
    pub base: ps_render::BaseMaterial,
    /// The effect maps the picture was composited from.
    pub maps: ps_render::EffectMaps,
    /// The pieces the through cuts free, for the weeded view.
    pub pieces: ps_render::Pieces,
    /// Whether the material is vinyl, whose waste is removed in the weeded view.
    pub vinyl: bool,
}

/// How the material preview is seen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PreviewView {
    /// Straight down.
    Flat,
    /// A tilted plane that can be orbited, with parallax from the height map.
    Tilted {
        /// Degrees from looking straight down.
        tilt_deg: f64,
        /// Degrees clockwise the view comes from.
        azimuth_deg: f64,
    },
    /// Cut pieces lifted off the liner; for vinyl the waste is gone.
    Weeded {
        /// How far the pieces are lifted, in millimetres.
        lift_mm: f64,
    },
}

/// How tall a full height step is shown in the tilted view, in millimetres.
const RELIEF_MM: f64 = 1.5;
/// The liner under vinyl.
const LINER: [u8; 3] = [244, 244, 247];

impl Preview {
    /// The picture for a view of the mat.
    #[must_use]
    pub fn render(&self, view: &PreviewView) -> ps_render::Image {
        let light = ps_render::Light::raking(315.0);
        match *view {
            PreviewView::Flat => self.image.clone(),
            PreviewView::Tilted {
                tilt_deg,
                azimuth_deg,
            } => ps_render::orbit_view(
                &self.base,
                &self.maps,
                &light,
                &ps_render::Orbit {
                    tilt_deg,
                    azimuth_deg,
                    relief_mm: RELIEF_MM,
                },
            ),
            PreviewView::Weeded { lift_mm } => ps_render::weeded(
                &self.base,
                &self.maps,
                &self.pieces,
                &light,
                &ps_render::Weeding {
                    lift_mm,
                    direction: (1.0, -1.0),
                    remove_waste: self.vinyl,
                    liner: LINER,
                },
            ),
        }
    }
}

/// The colour a material of this category is drawn in on a library card.
pub(crate) fn material_swatch(category: &str) -> [u8; 3] {
    match category {
        "paper" | "cardstock" | "card" => [0xDC, 0xC0, 0x8F],
        "vinyl" | "htv" => [0x1B, 0xB5, 0xA2],
        "fabric" | "felt" => [0xE2, 0xDC, 0xD2],
        "foil" | "metal" => [0xE4, 0xB4, 0x43],
        _ => [0xEE, 0xE8, 0xDA],
    }
}

/// How a material category looks.
fn base_for(category: &str) -> ps_render::BaseMaterial {
    let (colour, roughness, metallic) = match category {
        "vinyl" | "htv" => ([236, 236, 240], 0.3, 0.0),
        "paper" | "cardstock" | "card" => ([246, 241, 229], 0.85, 0.0),
        "fabric" | "felt" => ([226, 220, 210], 0.95, 0.0),
        "foil" | "metal" => ([205, 205, 210], 0.25, 0.8),
        _ => ([238, 232, 218], 0.8, 0.0),
    };
    ps_render::BaseMaterial {
        colour,
        roughness,
        metallic,
    }
}

/// The preview effect a tool's `render.effect` names.
fn effect_named(name: &str) -> ps_render::Effect {
    use ps_render::Effect;
    match name {
        "cut" => Effect::Cut,
        "kiss_cut" | "kiss-cut" => Effect::KissCut,
        "score" => Effect::Score,
        "emboss" => Effect::Emboss,
        "deboss" => Effect::Deboss,
        "engrave" => Effect::Engrave,
        "foil" => Effect::Foil,
        _ => Effect::Ink,
    }
}

/// `#RRGGBB` as bytes; black for anything else.
fn hex_colour(hex: &str) -> [u8; 3] {
    let h = hex.trim_start_matches('#');
    let byte = |i: usize| h.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok());
    match (byte(0), byte(2), byte(4)) {
        (Some(r), Some(g), Some(b)) => [r, g, b],
        _ => [0, 0, 0],
    }
}

/// A mat in the project, for the project tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatSummary {
    /// Its id.
    pub id: MatId,
    /// Its name.
    pub name: String,
    /// How many objects are on it.
    pub objects: usize,
    /// Whether it is the mat being edited.
    pub current: bool,
}

/// Where the job stands, in the words the status bar, the preview and the export panel share.
///
/// "Out of date" is only for an edit whose compile has not finished; every other reason a job is
/// missing has its own value, so the user is told what to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStatus {
    /// No machine is chosen, so nothing can be compiled.
    NoMachine,
    /// A compile is running.
    Compiling,
    /// The last compile could not run at all.
    Failed(String),
    /// The compile finished but the job has errors, so it cannot be previewed or exported.
    Blocked {
        /// How many errors block it.
        errors: usize,
    },
    /// The compile finished and there is nothing to do: no shape has an operation yet.
    NothingToDo,
    /// The document changed since the last compile, which is yet to come.
    OutOfDate,
    /// The job matches the document and can be previewed and exported.
    Ready,
}

/// A material the library offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialChoice {
    /// The library reference, such as `community.vinyl@1.0.0`.
    pub reference: String,
    /// The display name.
    pub name: String,
}

/// A machine profile the library offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineChoice {
    /// The profile id.
    pub id: String,
    /// The display name.
    pub name: String,
}

/// The panels keyboard focus can move between with F6, in focus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPanel {
    /// The top bar.
    TopBar,
    /// The project tree.
    Tree,
    /// The canvas.
    Canvas,
    /// The inspector.
    Inspector,
    /// The bottom panel.
    Bottom,
}

const FOCUS_ORDER: [FocusPanel; 5] = [
    FocusPanel::TopBar,
    FocusPanel::Tree,
    FocusPanel::Canvas,
    FocusPanel::Inspector,
    FocusPanel::Bottom,
];

/// A tool the library offers for an operation kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolChoice {
    /// The library reference, such as `generic.drag-blade@1.0.0`.
    pub reference: String,
    /// The display name.
    pub name: String,
}

/// A finished compile and the document version it was made from.
#[derive(Debug)]
pub struct Compiled {
    /// The compiled job.
    pub job: Job,
    /// The store generation of the document that was compiled.
    pub doc_generation: u64,
}

/// What the status bar and export panel show about a compiled job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompileSummary {
    /// How many mats have a program.
    pub mats: usize,
    /// Machine time over all mats, excluding user pauses.
    pub machine_time_ms: u64,
    /// Tool changes over all mats.
    pub tool_changes: u32,
}

struct CompileResult {
    doc_generation: u64,
    result: Result<Job, String>,
}

/// Everything the panels read and the commands change.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts reported by the window layer"
)]
pub struct AppState {
    store: DocumentStore,
    mat: MatId,
    selection: Selection,
    tool: Box<dyn Tool>,
    tool_id: ToolId,
    viewport: Viewport,
    mode: Mode,
    registry: Registry,
    settings: Settings,
    palette_open: bool,
    debounce: Debounce,
    saved_generation: u64,
    compile_ready: bool,
    device_ready: bool,
    dpi: f64,
    pending: Effect,
    libs: LibrarySnapshot,
    profile: Option<Arc<MachineProfile>>,
    jobs: JobManager<CompileResult>,
    cache: Arc<Mutex<CompileCache>>,
    compiled: Option<Arc<Compiled>>,
    compiling: bool,
    /// Why the last compile produced nothing, when it failed.
    compile_error: Option<String>,
    path: Option<PathBuf>,
    shell: Option<Project>,
    read_only: Option<ReadOnly>,
    settings_changed: bool,
    settings_open: bool,
    text_open: bool,
    snapping: bool,
    highlight_row: Option<OpRowId>,
    highlight_layer: Option<(ObjectId, Option<PathKey>)>,
    last_snap: Option<(Point, SnapKind)>,
    /// Where the pointer last was over the canvas, for tools that act on what is under it.
    hover_world: Option<Point>,
    clock_ms: u64,
    tool_sides: u32,
    star_ratio: f64,
    focus_index: Option<usize>,
    focus_request: Option<FocusPanel>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("mode", &self.mode)
            .field("tool", &self.tool_id)
            .field("selection", &self.selection.len())
            .finish_non_exhaustive()
    }
}

fn indexmap_with_capacity(n: usize) -> indexmap::IndexMap<ObjectId, Arc<PlacedObject>> {
    indexmap::IndexMap::with_capacity(n)
}

fn default_mat() -> Mat {
    Mat::new(MatId::new(), "Mat 1", MatSize::inches(12.0, 12.0))
}

/// The document with a default mat added if it has none (not an undo step).
fn ensure_mat(doc: Document) -> (Document, MatId) {
    if let Some(id) = doc.mats.keys().next() {
        let id = *id;
        return (doc, id);
    }
    let mat = default_mat();
    let id = mat.id;
    let with = AddMat { mat }.apply(&doc).unwrap_or(doc);
    (with, id)
}

/// The newest version of the machine profile with this id, if the library has one.
fn newest_profile(libs: &LibrarySnapshot, id: &str) -> Option<Arc<MachineProfile>> {
    libs.profiles()
        .into_iter()
        .filter(|p| p.id == id)
        .max_by(|a, b| a.version.cmp(&b.version))
}

impl AppState {
    /// A new untitled project with one 12 x 12 inch mat.
    #[must_use]
    pub fn new(settings: Settings) -> Self {
        let libs = builtin_entries().map_or_else(
            |_| LibrarySnapshot::from_entries(Vec::new()),
            LibrarySnapshot::from_entries,
        );
        let profile = newest_profile(
            &libs,
            settings
                .default_machine
                .as_deref()
                .unwrap_or(DEFAULT_MACHINE),
        );
        let registry = Registry::new(&settings.shortcuts);
        let meta = ProjectMeta::new(
            "Untitled",
            "1970-01-01T00:00:00Z",
            env!("CARGO_PKG_VERSION"),
        );
        let (doc, mat) = ensure_mat(Document::new(meta));
        let store = DocumentStore::new(doc);
        let saved_generation = store.generation();
        Self {
            store,
            mat,
            selection: Selection::default(),
            tool: ToolId::Select.new_tool(),
            tool_id: ToolId::Select,
            viewport: Viewport::new(800.0, 600.0),
            mode: Mode::Prepare,
            registry,
            settings,
            palette_open: false,
            debounce: Debounce::new(COMPILE_DEBOUNCE_MS),
            saved_generation,
            compile_ready: false,
            device_ready: false,
            dpi: 96.0,
            pending: Effect::None,
            libs,
            profile,
            jobs: JobManager::new(2),
            cache: Arc::new(Mutex::new(CompileCache::new(DEFAULT_CAPACITY_BYTES))),
            compiled: None,
            compiling: false,
            compile_error: None,
            path: None,
            shell: None,
            read_only: None,
            settings_changed: false,
            settings_open: false,
            text_open: false,
            snapping: true,
            highlight_row: None,
            highlight_layer: None,
            last_snap: None,
            hover_world: None,
            clock_ms: 0,
            tool_sides: 5,
            star_ratio: 0.5,
            focus_index: None,
            focus_request: None,
        }
    }

    // ---- reading ----

    /// The current document.
    #[must_use]
    pub fn document(&self) -> Arc<Document> {
        self.store.document()
    }

    /// The mat being edited.
    ///
    /// # Panics
    ///
    /// Never in practice: the state always keeps a mat in its document.
    #[must_use]
    pub fn current_mat(&self) -> Arc<Mat> {
        Arc::clone(
            self.store
                .document()
                .mat(self.mat)
                .expect("the state keeps its current mat in the document"),
        )
    }

    /// The selection.
    #[must_use]
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// The active tool.
    #[must_use]
    pub fn tool_id(&self) -> ToolId {
        self.tool_id
    }

    /// The mode.
    #[must_use]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The canvas viewport.
    #[must_use]
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    /// The world rectangle the canvas shows.
    #[must_use]
    pub fn visible_rect(&self) -> Rect {
        self.viewport.visible_rect()
    }

    /// The command registry (shortcuts, labels, palette search).
    #[must_use]
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The settings.
    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Changes the settings; they count as changed (to be saved) only if the edit differs.
    pub fn update_settings(&mut self, edit: impl FnOnce(&mut Settings)) {
        let before = self.settings.clone();
        edit(&mut self.settings);
        if self.settings != before {
            self.settings_changed = true;
        }
    }

    /// Sets the screen calibration from the length a 100 mm bar measured on the screen. False,
    /// with nothing changed, when the measurement is not usable.
    pub fn calibrate_screen(&mut self, measured_mm: f64) -> bool {
        let Some(next) =
            ps_library::calibration_from_measurement(self.settings.screen_calibration, measured_mm)
        else {
            return false;
        };
        self.update_settings(|s| s.screen_calibration = next);
        true
    }

    /// Whether the settings changed since the last call; reading clears it.
    pub fn take_settings_changed(&mut self) -> bool {
        std::mem::take(&mut self.settings_changed)
    }

    /// Whether the settings window is showing.
    #[must_use]
    pub fn settings_open(&self) -> bool {
        self.settings_open
    }

    /// Whether the text window is showing.
    #[must_use]
    pub fn text_open(&self) -> bool {
        self.text_open
    }

    /// Closes the text window.
    pub fn close_text(&mut self) {
        self.text_open = false;
    }

    /// Closes the settings window.
    pub fn close_settings(&mut self) {
        self.settings_open = false;
    }

    /// Binds a shortcut to a command (or unbinds it with `None`) and records the override.
    ///
    /// # Errors
    ///
    /// [`crate::commands::AssignError`] when the command is unknown or another command has the
    /// shortcut; nothing changes then.
    pub fn assign_shortcut(
        &mut self,
        id: &str,
        shortcut: Option<Shortcut>,
    ) -> Result<(), crate::commands::AssignError> {
        self.registry.assign(id, shortcut)?;
        self.sync_shortcuts();
        Ok(())
    }

    /// Restores a command's default shortcut.
    pub fn reset_shortcut(&mut self, id: &str) {
        self.registry.reset(id);
        self.sync_shortcuts();
    }

    fn sync_shortcuts(&mut self) {
        let overrides = self.registry.overrides();
        self.update_settings(|s| s.shortcuts = overrides);
    }

    /// The file the project was opened from or last saved to.
    #[must_use]
    pub fn project_path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Set when the project was opened read-only and must not be saved over.
    #[must_use]
    pub fn read_only(&self) -> Option<&ReadOnly> {
        self.read_only.as_ref()
    }

    pub(crate) fn shell(&self) -> Option<&Project> {
        self.shell.as_ref()
    }

    /// The window title: the project name, `*` when it has unsaved changes.
    #[must_use]
    pub fn window_title(&self) -> String {
        let name = self.path.as_ref().and_then(|p| p.file_stem()).map_or_else(
            || "Untitled".to_owned(),
            |s| s.to_string_lossy().into_owned(),
        );
        let dirty = if self.is_dirty() { "*" } else { "" };
        format!("{name}{dirty} — Plotter Studio")
    }

    /// Loads recovered work: the document is current but counts as unsaved and has no file yet.
    pub fn adopt_recovered(&mut self, opened: crate::project_io::OpenProject) {
        self.load_document(opened.document);
        self.shell = Some(opened.shell);
        self.saved_generation = u64::MAX;
    }

    /// Makes an opened project the current one and records its file in the recent list.
    pub fn adopt_project(&mut self, opened: crate::project_io::OpenProject, path: &Path) {
        self.load_document(opened.document);
        self.shell = Some(opened.shell);
        self.read_only = opened.read_only;
        self.path = Some(path.to_path_buf());
        self.settings.push_recent(&path.to_string_lossy());
    }

    pub(crate) fn finish_save(&mut self, path: &Path, shell: Project) {
        self.shell = Some(shell);
        self.path = Some(path.to_path_buf());
        self.mark_saved();
        self.settings.push_recent(&path.to_string_lossy());
    }

    /// Whether the command palette is showing.
    #[must_use]
    pub fn palette_open(&self) -> bool {
        self.palette_open
    }

    /// Whether there is something to undo.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.store.can_undo()
    }

    /// Whether there is something to redo.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.store.can_redo()
    }

    /// A number that changes with every edit, undo and redo.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.store.generation()
    }

    /// Whether the project differs from what was last saved.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.store.generation() != self.saved_generation
    }

    /// The facts command availability depends on.
    #[must_use]
    pub fn context(&self) -> Context {
        Context {
            has_selection: !self.selection.is_empty(),
            has_whole_objects: !self.selection.is_empty() && !self.selection.has_shapes(),
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
            has_project: true,
            has_compile: self.compile_ready || self.fresh_compile().is_some(),
            device_ready: self.device_ready,
        }
    }

    /// Where the job stands; see [`JobStatus`].
    #[must_use]
    pub fn job_status(&self) -> JobStatus {
        if self.profile.is_none() {
            return JobStatus::NoMachine;
        }
        if self.compiling {
            return JobStatus::Compiling;
        }
        if let Some(why) = &self.compile_error {
            return JobStatus::Failed(why.clone());
        }
        let Some(c) = self.compiled.as_ref() else {
            return JobStatus::OutOfDate;
        };
        if c.doc_generation != self.store.generation() {
            return JobStatus::OutOfDate;
        }
        if c.job.is_blocked() {
            return JobStatus::Blocked {
                errors: c.job.diagnostics.count(Severity::Error),
            };
        }
        if c.job.mats.is_empty() {
            JobStatus::NothingToDo
        } else {
            JobStatus::Ready
        }
    }

    /// The compiled job if it still matches the document and has something to run.
    pub(crate) fn fresh_compile(&self) -> Option<&Arc<Compiled>> {
        self.compiled.as_ref().filter(|c| {
            c.doc_generation == self.store.generation()
                && !c.job.mats.is_empty()
                && !c.job.is_blocked()
        })
    }

    /// The machine profile jobs are compiled for.
    pub(crate) fn profile(&self) -> Option<&Arc<MachineProfile>> {
        self.profile.as_ref()
    }

    /// The latest compile, which may be out of date; see [`AppState::context`] for readiness.
    #[must_use]
    pub fn compiled(&self) -> Option<&Arc<Compiled>> {
        self.compiled.as_ref()
    }

    /// Totals over the latest compile, if there is one.
    #[must_use]
    pub fn compile_summary(&self) -> Option<CompileSummary> {
        let c = self.compiled.as_ref()?;
        Some(CompileSummary {
            mats: c.job.mats.len(),
            machine_time_ms: c.job.mats.iter().map(|m| m.estimate.machine_time_ms).sum(),
            tool_changes: c.job.mats.iter().map(|m| m.estimate.tool_changes).sum(),
        })
    }

    /// Why the last compile failed, if it did.
    #[must_use]
    pub fn compile_error(&self) -> Option<&str> {
        self.compile_error.as_deref()
    }

    /// Whether a compile is running now.
    #[must_use]
    pub fn compile_running(&self) -> bool {
        self.compiling
    }

    /// How many paths are selected; `None` when nothing is selected.
    #[must_use]
    pub fn selected_path_count(&self) -> Option<usize> {
        (!self.selection.is_empty()).then(|| self.selected_paths().len())
    }

    /// How many paths the objects with something selected have in all.
    #[must_use]
    pub fn selected_object_path_count(&self) -> usize {
        self.selected_objects().iter().map(|o| o.paths.len()).sum()
    }

    /// Whether shapes inside an object are selected, rather than only whole objects.
    #[must_use]
    pub fn has_shape_selection(&self) -> bool {
        self.selection.has_shapes()
    }

    /// Every selected path with its object: all the paths of a whole object, or the shapes
    /// chosen inside it. This is what operations are assigned to.
    fn selected_paths(&self) -> Vec<(ObjectId, Arc<ShapePath>)> {
        self.selected_objects()
            .iter()
            .flat_map(|object| {
                object
                    .paths
                    .iter()
                    .filter(|path| self.selection.includes_path(object.id, &path.key))
                    .map(|path| (object.id, Arc::clone(path)))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// How many layout warnings and errors the cheap validation pass finds.
    #[must_use]
    pub fn warning_count(&self) -> usize {
        self.problems()
            .iter()
            .filter(|d| d.severity != Severity::Info)
            .count()
    }

    /// Everything worth telling the user about the job: what checking the document finds, and
    /// what compiling it reported, each once. The Export tab's count and its list both come from
    /// here, so they always agree.
    #[must_use]
    pub fn problems(&self) -> Vec<ps_core::Diagnostic> {
        let mut out: Vec<ps_core::Diagnostic> = Vec::new();
        if let Some(profile) = &self.profile {
            out.extend(
                validate(&self.store.document(), &self.libs, profile)
                    .iter()
                    .cloned(),
            );
        }
        if let Some(c) = self
            .compiled
            .as_ref()
            .filter(|c| c.doc_generation == self.store.generation())
        {
            for d in &c.job.diagnostics {
                if !out.contains(d) {
                    out.push(d.clone());
                }
            }
        }
        out
    }

    /// How many sides a polygon has and how many points a star has.
    #[must_use]
    pub fn tool_sides(&self) -> u32 {
        self.tool_sides
    }

    /// Sets the polygon sides and star points, kept between 3 and 32.
    pub fn set_tool_sides(&mut self, sides: u32) {
        self.tool_sides = sides.clamp(3, 32);
    }

    /// A star's inner radius as a fraction of its outer radius.
    #[must_use]
    pub fn star_ratio(&self) -> f64 {
        self.star_ratio
    }

    /// Sets the star's inner radius ratio, kept between 0.05 and 0.95.
    pub fn set_star_ratio(&mut self, ratio: f64) {
        self.star_ratio = ratio.clamp(0.05, 0.95);
    }

    /// The tool's current preview shapes.
    #[must_use]
    pub fn overlay(&self) -> Vec<Overlay> {
        let mut out = Vec::new();
        self.tool.overlay(&mut out);
        out
    }

    // ---- inputs from the window layer ----

    /// Tells the state how big the canvas is.
    pub fn resize_canvas(&mut self, width_px: f64, height_px: f64) {
        self.viewport.resize(width_px, height_px);
    }

    /// Tells the state the display's pixel density, for true scale.
    pub fn set_display_dpi(&mut self, dpi: f64) {
        self.dpi = dpi;
    }

    /// Records whether a compiled job is ready to export.
    pub fn set_compile_ready(&mut self, ready: bool) {
        self.compile_ready = ready;
    }

    /// Records whether a machine is connected and idle.
    pub fn set_device_ready(&mut self, ready: bool) {
        self.device_ready = ready;
    }

    /// Records that the project was just saved.
    pub fn mark_saved(&mut self) {
        self.saved_generation = self.store.generation();
    }

    /// Closes the command palette.
    pub fn close_palette(&mut self) {
        self.palette_open = false;
    }

    /// Replaces the document (open or new), clearing history and selection.
    pub fn load_document(&mut self, doc: Document) {
        let (doc, mat) = ensure_mat(doc);
        self.store = DocumentStore::new(doc);
        self.shell = None;
        self.read_only = None;
        self.path = None;
        self.mat = mat;
        self.selection.clear();
        self.tool = self.tool_id.new_tool();
        self.saved_generation = self.store.generation();
        self.compile_ready = false;
        self.debounce.touch(0);
    }

    /// Per-frame housekeeping: starts a compile when the debounce has passed and collects
    /// finished ones. Never blocks.
    pub fn tick(&mut self, now_ms: u64) {
        self.clock_ms = now_ms;
        if self.take_compile_due(now_ms) {
            self.start_compile();
        }
        while let Some(outcome) = self.jobs.try_recv() {
            self.apply_compile(outcome);
        }
    }

    /// Waits for the running compile to deliver; true when a compiled job is available
    /// afterwards (including one that [`AppState::tick`] already collected), false if there is
    /// none or the wait timed out.
    pub fn wait_for_compile(&mut self, timeout: Duration) -> bool {
        if self.compiling
            && let Some(outcome) = self.jobs.recv(timeout)
        {
            self.apply_compile(outcome);
        }
        self.compiled.is_some()
    }

    fn start_compile(&mut self) {
        let Some(profile) = self.profile.clone() else {
            tracing::warn!("no machine is chosen, so the job is not prepared");
            return;
        };
        let generation = self.jobs.next_generation();
        let doc = self.store.document();
        let doc_generation = self.store.generation();
        let libs = self.libs.clone();
        let cache = Arc::clone(&self.cache);
        self.compiling = true;
        self.jobs.submit(generation, move |_| {
            let calibration = Calibration::default();
            let mut cache = cache.lock().unwrap_or_else(PoisonError::into_inner);
            let result = compile_cached(
                &CompileInput {
                    doc: &doc,
                    libs: &libs,
                    profile: &profile,
                    calibration: &calibration,
                    mats: MatSelection::All,
                },
                &mut cache,
            )
            .map_err(|e| e.to_string());
            CompileResult {
                doc_generation,
                result,
            }
        });
    }

    fn apply_compile(&mut self, outcome: Outcome<CompileResult>) {
        self.compiling = false;
        match outcome {
            Outcome::Done { value, .. } => match value.result {
                Ok(job) => {
                    if job.is_blocked() {
                        tracing::warn!(
                            errors = job.diagnostics.count(Severity::Error),
                            "the job is blocked"
                        );
                        for d in job
                            .diagnostics
                            .iter()
                            .filter(|d| d.severity == Severity::Error)
                            .take(10)
                        {
                            tracing::warn!(key = d.key.as_str(), args = ?d.args, "blocking error");
                        }
                    }
                    self.compile_error = None;
                    self.compiled = Some(Arc::new(Compiled {
                        job,
                        doc_generation: value.doc_generation,
                    }));
                }
                Err(why) => {
                    tracing::error!(%why, "the job could not be prepared");
                    self.compile_error = Some(why);
                    self.compiled = None;
                }
            },
            Outcome::Panicked { message, .. } => {
                tracing::error!(%message, "preparing the job panicked");
                self.compile_error = Some(message);
                self.compiled = None;
            }
            Outcome::Cancelled { .. } => {}
        }
    }

    /// Whether a debounced compile should start now; true once per burst of edits.
    pub fn take_compile_due(&mut self, now_ms: u64) -> bool {
        self.debounce.take_if_due(now_ms)
    }

    /// The effect left by the last key press, if any.
    pub fn take_effect(&mut self) -> Effect {
        std::mem::replace(&mut self.pending, Effect::None)
    }

    /// Runs the command bound to a key press; false when nothing is bound to it or the palette
    /// has the keyboard. The command's [`Effect`] is left for [`AppState::take_effect`].
    pub fn handle_key(&mut self, mods: Mods, key: &str, now_ms: u64) -> bool {
        if self.palette_open {
            return false;
        }
        let Some(mut shortcut) = Shortcut::parse(key) else {
            return false;
        };
        shortcut.command = mods.command;
        shortcut.shift = mods.shift;
        shortcut.alt = mods.alt;
        let Some(command) = self.registry.find_by_shortcut(&shortcut) else {
            return false;
        };
        self.pending = self.run_command(command.id, now_ms);
        true
    }

    // ---- commands ----

    /// Commands that edit the selection, its nodes and the mats; false when `id` is not one.
    fn run_editing_command(&mut self, id: &str, now_ms: u64) -> bool {
        match id {
            "arrange.align-left" => {
                self.align_selection(AlignKind::Left, now_ms);
            }
            "arrange.align-right" => {
                self.align_selection(AlignKind::Right, now_ms);
            }
            "arrange.align-top" => {
                self.align_selection(AlignKind::Top, now_ms);
            }
            "arrange.align-bottom" => {
                self.align_selection(AlignKind::Bottom, now_ms);
            }
            "arrange.align-centre-h" => {
                self.align_selection(AlignKind::CentreHorizontal, now_ms);
            }
            "arrange.align-centre-v" => {
                self.align_selection(AlignKind::CentreVertical, now_ms);
            }
            "arrange.distribute-h" => {
                self.distribute_selection(Axis::Horizontal, now_ms);
            }
            "arrange.distribute-v" => {
                self.distribute_selection(Axis::Vertical, now_ms);
            }
            "arrange.mirror-h" => {
                self.mirror_selection(Axis::Horizontal, now_ms);
            }
            "arrange.mirror-v" => {
                self.mirror_selection(Axis::Vertical, now_ms);
            }
            "arrange.rotate-cw" => {
                self.rotate_selection(90.0, now_ms);
            }
            "arrange.rotate-ccw" => {
                self.rotate_selection(-90.0, now_ms);
            }
            "node.delete" => {
                self.edit_selected_node(ps_geometry::delete_node, true, now_ms);
            }
            "node.to-curve" => {
                self.edit_selected_node(ps_geometry::segment_to_curve, false, now_ms);
            }
            "node.to-line" => {
                self.edit_selected_node(ps_geometry::segment_to_line, false, now_ms);
            }
            "node.smooth" => {
                self.edit_selected_node(ps_geometry::smooth_node, false, now_ms);
            }
            "node.corner" => {
                self.edit_selected_node(ps_geometry::corner_node, false, now_ms);
            }
            "mat.add" => {
                self.add_mat(now_ms);
            }
            "mat.duplicate" => {
                self.duplicate_mat(now_ms);
            }
            "mat.remove" => {
                self.remove_current_mat(now_ms);
            }
            _ => return false,
        }
        true
    }

    /// Runs a command by id if it can run now.
    pub fn run_command(&mut self, id: &str, now_ms: u64) -> Effect {
        if !self.registry.is_enabled(id, &self.context()) {
            return Effect::None;
        }
        if let Some(tool) = ToolId::from_command(id) {
            self.tool_id = tool;
            self.tool = tool.new_tool();
            return Effect::None;
        }
        if let Some((dx, dy)) = nudge_distance_mm(id, NUDGE_STEP_MM) {
            self.move_selection(dx, dy, now_ms);
            return Effect::None;
        }
        if self.run_editing_command(id, now_ms) {
            return Effect::None;
        }
        match id {
            "file.new" => return Effect::NewProject,
            "file.open" => return Effect::OpenProject,
            "file.save" => return Effect::Save,
            "file.save-as" => return Effect::SaveAs,
            "file.export" => return Effect::Export,
            "file.import" => return Effect::Import,
            "machine.send" => return Effect::SendToMachine,
            "edit.undo" => {
                if self.store.undo() {
                    self.after_history_change(now_ms);
                }
            }
            "edit.redo" => {
                if self.store.redo() {
                    self.after_history_change(now_ms);
                }
            }
            "edit.delete" => {
                if self.selected_node().is_some() {
                    self.edit_selected_node(ps_geometry::delete_node, true, now_ms);
                } else {
                    self.delete_selection(now_ms);
                }
            }
            "edit.duplicate" => self.duplicate_selection(now_ms),
            "edit.select-all" => {
                let ids: Vec<ObjectId> = self.current_mat().objects.keys().copied().collect();
                self.selection.set_objects(&ids);
            }
            "mode.prepare" => self.mode = Mode::Prepare,
            "mode.preview" => self.mode = Mode::Preview,
            "mode.export" => self.mode = Mode::Export,
            "view.zoom-in" => self.zoom_about_centre(ZOOM_STEP),
            "view.zoom-out" => self.zoom_about_centre(1.0 / ZOOM_STEP),
            "view.zoom-fit" => self.fit_mat(),
            "mat.add-roll" => {
                self.add_matless_mat(now_ms);
            }
            "view.true-scale" => self
                .viewport
                .set_true_scale(self.dpi, self.settings.screen_calibration),
            "focus.next-panel" => self.move_focus(true),
            "focus.previous-panel" => self.move_focus(false),
            "palette.open" => self.palette_open = !self.palette_open,
            "settings.open" => self.settings_open = !self.settings_open,
            "text.open" => self.text_open = !self.text_open,
            "view.snap" => {
                self.snapping = !self.snapping;
                self.last_snap = None;
            }
            _ => {}
        }
        Effect::None
    }

    /// The library tools that can perform operations of `kind`, by name.
    #[must_use]
    pub fn tools_for(&self, kind: OpKind) -> Vec<ToolChoice> {
        let mut tools: Vec<ToolChoice> = self
            .libs
            .tools()
            .iter()
            .filter(|t| t.operations.iter().any(|o| o == kind.as_str()))
            .map(|t| ToolChoice {
                reference: format!("{}@{}", t.id, t.version),
                name: t.name.clone(),
            })
            .collect();
        tools.sort_by(|a, b| a.name.cmp(&b.name).then(a.reference.cmp(&b.reference)));
        tools
    }

    // ---- arranging the selection ----

    fn arrange_items(&self) -> Vec<ArrangeItem> {
        // Shapes share their object's transform, so there is nothing to arrange on its own.
        if self.selection.has_shapes() {
            return Vec::new();
        }
        self.selected_objects()
            .iter()
            .filter_map(|o| {
                let bounds = o
                    .paths
                    .iter()
                    .map(|p| p.geom.transformed(o.transform).bounds())
                    .reduce(|a, b| a.union(b))?;
                Some(ArrangeItem {
                    id: o.id,
                    bounds,
                    transform: o.transform,
                })
            })
            .collect()
    }

    /// The bounds of everything selected, in millimetres on the mat.
    #[must_use]
    pub fn selection_bounds(&self) -> Option<Rect> {
        arrange::union_bounds(&self.arrange_items())
    }

    fn apply_transforms(&mut self, changes: &[(ObjectId, Affine)], now_ms: u64) -> bool {
        if changes.is_empty() {
            return false;
        }
        let cmds: Vec<Box<dyn ModelCommand>> = changes
            .iter()
            .map(|(object, transform)| {
                Box::new(SetTransform {
                    mat: self.mat,
                    object: *object,
                    transform: *transform,
                }) as Box<dyn ModelCommand>
            })
            .collect();
        self.commit(&cmds, now_ms)
    }

    /// Moves the selection so `anchor` of its bounds is at `(x, y)` millimetres.
    pub fn move_selection_to(&mut self, anchor: Anchor, x: f64, y: f64, now_ms: u64) -> bool {
        let changes = arrange::move_to(&self.arrange_items(), anchor, Point::new(x, y));
        self.apply_transforms(&changes, now_ms)
    }

    /// Scales the selection to `width` by `height` about `anchor`.
    pub fn resize_selection(
        &mut self,
        anchor: Anchor,
        width: f64,
        height: f64,
        lock_aspect: bool,
        now_ms: u64,
    ) -> bool {
        let changes = arrange::resize(&self.arrange_items(), anchor, width, height, lock_aspect);
        self.apply_transforms(&changes, now_ms)
    }

    /// Rotates the selection about its centre.
    pub fn rotate_selection(&mut self, degrees: f64, now_ms: u64) -> bool {
        let changes = arrange::rotate(&self.arrange_items(), degrees);
        self.apply_transforms(&changes, now_ms)
    }

    /// Mirrors the selection about its centre.
    pub fn mirror_selection(&mut self, axis: Axis, now_ms: u64) -> bool {
        let changes = arrange::mirror(&self.arrange_items(), axis);
        self.apply_transforms(&changes, now_ms)
    }

    /// Lines the selection up.
    pub fn align_selection(&mut self, kind: AlignKind, now_ms: u64) -> bool {
        let changes = arrange::align(&self.arrange_items(), kind);
        self.apply_transforms(&changes, now_ms)
    }

    /// Spaces the selection evenly; needs three or more objects.
    pub fn distribute_selection(&mut self, axis: Axis, now_ms: u64) -> bool {
        let changes = arrange::distribute(&self.arrange_items(), axis);
        self.apply_transforms(&changes, now_ms)
    }

    fn add_copies(&mut self, copies: &[ArrangeCopy], now_ms: u64) -> bool {
        if copies.is_empty() {
            return false;
        }
        let mat = self.current_mat();
        let mut ids: Vec<ObjectId> = self.selection.objects();
        let cmds: Vec<Box<dyn ModelCommand>> = copies
            .iter()
            .filter_map(|c| {
                let mut object = (**mat.objects.get(&c.source)?).clone();
                object.id = ObjectId::new();
                object.transform = c.transform;
                ids.push(object.id);
                Some(Box::new(AddObject {
                    mat: self.mat,
                    object,
                }) as Box<dyn ModelCommand>)
            })
            .collect();
        if self.commit(&cmds, now_ms) {
            self.selection.set_objects(&ids);
            true
        } else {
            false
        }
    }

    /// Adds a `columns` by `rows` grid of copies of the selection.
    pub fn grid_array_selection(
        &mut self,
        columns: u32,
        rows: u32,
        dx: f64,
        dy: f64,
        now_ms: u64,
    ) -> bool {
        let copies = arrange::grid_array(&self.arrange_items(), columns, rows, dx, dy);
        self.add_copies(&copies, now_ms)
    }

    /// Adds copies of the selection spaced around its centre: `count` objects in total over
    /// `total_degrees`.
    pub fn circular_array_selection(
        &mut self,
        count: u32,
        total_degrees: f64,
        now_ms: u64,
    ) -> bool {
        let items = self.arrange_items();
        let Some(bounds) = arrange::union_bounds(&items) else {
            return false;
        };
        let centre = arrange::anchor_point(bounds, Anchor::Centre);
        let copies = arrange::circular_array(&items, count, centre, total_degrees);
        self.add_copies(&copies, now_ms)
    }

    // ---- mats, materials and the machine ----

    /// The mats of the project, in order.
    #[must_use]
    pub fn mat_summaries(&self) -> Vec<MatSummary> {
        self.store
            .document()
            .mats
            .values()
            .map(|m| MatSummary {
                id: m.id,
                name: m.name.clone(),
                objects: m.objects.len(),
                current: m.id == self.mat,
            })
            .collect()
    }

    /// Makes another mat the one being edited.
    pub fn set_current_mat(&mut self, id: MatId) {
        if self.store.document().mat(id).is_some() && id != self.mat {
            self.mat = id;
            self.selection.clear();
            self.tool = self.tool_id.new_tool();
        }
    }

    fn switch_to_a_mat_that_exists(&mut self) {
        let doc = self.store.document();
        if doc.mat(self.mat).is_none()
            && let Some(first) = doc.mats.keys().next()
        {
            self.mat = *first;
        }
    }

    /// Adds an empty 12 x 12 inch mat named `Mat N` and switches to it.
    pub fn add_mat(&mut self, now_ms: u64) -> bool {
        let mut n = self.store.document().mats.len() + 1;
        let name = loop {
            let candidate = format!("Mat {n}");
            if !self
                .store
                .document()
                .mats
                .values()
                .any(|m| m.name == candidate)
            {
                break candidate;
            }
            n += 1;
        };
        let mat = Mat::new(MatId::new(), &name, MatSize::inches(12.0, 12.0));
        let id = mat.id;
        let cmds: Vec<Box<dyn ModelCommand>> = vec![Box::new(AddMat { mat })];
        if !self.commit(&cmds, now_ms) {
            return false;
        }
        self.set_current_mat(id);
        true
    }

    /// The roll the chosen machine cuts from, when it can cut without a mat.
    #[must_use]
    pub fn roll(&self) -> Option<ps_library::Roll> {
        self.profile.as_ref().and_then(|p| p.roll)
    }

    /// Adds a matless "roll" mat as wide as the machine's roll and as long as its longest cut, and
    /// switches to it. False when the machine has no roll.
    pub fn add_matless_mat(&mut self, now_ms: u64) -> bool {
        let Some(roll) = self.roll() else {
            return false;
        };
        let mut n = self.store.document().mats.len() + 1;
        let name = loop {
            let candidate = format!("Roll {n}");
            if !self
                .store
                .document()
                .mats
                .values()
                .any(|m| m.name == candidate)
            {
                break candidate;
            }
            n += 1;
        };
        let mut mat = Mat::new(
            MatId::new(),
            &name,
            MatSize {
                width: ps_core::Mm(roll.width_mm),
                height: ps_core::Mm(roll.max_length_mm),
            },
        );
        mat.mat_type = ps_model::MatType::Matless;
        let id = mat.id;
        let cmds: Vec<Box<dyn ModelCommand>> = vec![Box::new(AddMat { mat })];
        if !self.commit(&cmds, now_ms) {
            return false;
        }
        self.set_current_mat(id);
        true
    }

    /// How far down a roll the content reaches, in millimetres; `None` for a mat with a fixed
    /// size or one with nothing on it.
    #[must_use]
    pub fn used_length_mm(&self) -> Option<f64> {
        let mat = self.current_mat();
        if mat.mat_type != ps_model::MatType::Matless {
            return None;
        }
        mat.objects
            .values()
            .flat_map(|o| {
                o.paths
                    .iter()
                    .map(|p| p.geom.transformed(o.transform).bounds().y1)
                    .collect::<Vec<_>>()
            })
            .fold(None, |acc: Option<f64>, y| {
                Some(acc.map_or(y, |a| a.max(y)))
            })
    }

    /// Copies the current mat, its objects under new ids, and switches to the copy.
    pub fn duplicate_mat(&mut self, now_ms: u64) -> bool {
        let mut copy = (*self.current_mat()).clone();
        copy.id = MatId::new();
        copy.name = format!("{} (copy)", copy.name);
        let mut objects = indexmap_with_capacity(copy.objects.len());
        for object in copy.objects.values() {
            let mut fresh = (**object).clone();
            fresh.id = ObjectId::new();
            objects.insert(fresh.id, Arc::new(fresh));
        }
        copy.objects = Arc::new(objects);
        let id = copy.id;
        let cmds: Vec<Box<dyn ModelCommand>> = vec![Box::new(AddMat { mat: copy })];
        if !self.commit(&cmds, now_ms) {
            return false;
        }
        self.set_current_mat(id);
        true
    }

    /// Removes the current mat; the last mat cannot be removed.
    pub fn remove_current_mat(&mut self, now_ms: u64) -> bool {
        if self.store.document().mats.len() <= 1 {
            return false;
        }
        let cmds: Vec<Box<dyn ModelCommand>> = vec![Box::new(RemoveMat { mat: self.mat })];
        if !self.commit(&cmds, now_ms) {
            return false;
        }
        self.switch_to_a_mat_that_exists();
        self.selection.clear();
        true
    }

    /// The materials in the library, by name.
    #[must_use]
    pub fn material_choices(&self) -> Vec<MaterialChoice> {
        let mut out: Vec<MaterialChoice> = self
            .libs
            .materials()
            .iter()
            .map(|m| MaterialChoice {
                reference: format!("{}@{}", m.id, m.version),
                name: m.name.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name).then(a.reference.cmp(&b.reference)));
        out
    }

    /// The reference of the material on the current mat.
    #[must_use]
    pub fn current_material_reference(&self) -> Option<String> {
        self.current_mat()
            .material
            .as_ref()
            .map(ToString::to_string)
    }

    /// Sets (or clears) the material on the current mat.
    pub fn set_mat_material(&mut self, reference: Option<&str>, now_ms: u64) -> bool {
        let material = match reference {
            None => None,
            Some(text) => match text.parse::<LibRef>() {
                Ok(r) => Some(r),
                Err(_) => return false,
            },
        };
        let cmds: Vec<Box<dyn ModelCommand>> = vec![Box::new(SetMatMaterial {
            mat: self.mat,
            material,
        })];
        self.commit(&cmds, now_ms)
    }

    /// The machine profiles in the library (the newest version of each), by name.
    #[must_use]
    pub fn machine_choices(&self) -> Vec<MachineChoice> {
        let mut newest: std::collections::BTreeMap<String, Arc<MachineProfile>> =
            std::collections::BTreeMap::new();
        for p in self.libs.profiles() {
            match newest.get(&p.id) {
                Some(have) if have.version >= p.version => {}
                _ => {
                    newest.insert(p.id.clone(), p);
                }
            }
        }
        let mut out: Vec<MachineChoice> = newest
            .into_values()
            .map(|p| MachineChoice {
                id: p.id.clone(),
                name: p.name.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// The id of the machine profile jobs are compiled for.
    #[must_use]
    pub fn current_machine_id(&self) -> Option<String> {
        self.profile.as_ref().map(|p| p.id.clone())
    }

    /// Chooses the machine profile, remembers it in the settings and recompiles.
    pub fn set_machine(&mut self, id: &str) -> bool {
        let Some(profile) = self
            .libs
            .profiles()
            .into_iter()
            .filter(|p| p.id == id)
            .max_by(|a, b| a.version.cmp(&b.version))
        else {
            return false;
        };
        tracing::info!(machine = %profile.id, version = %profile.version, "machine chosen");
        self.profile = Some(profile);
        self.compiled = None;
        self.debounce.touch(self.clock_ms);
        let wanted = id.to_owned();
        self.update_settings(|s| s.default_machine = Some(wanted));
        true
    }

    /// Like [`Self::assign_operation_row`], for callers that do not need the new row.
    pub fn assign_operation(&mut self, kind: OpKind, tool: &str, now_ms: u64) -> bool {
        self.assign_operation_row(kind, tool, now_ms).is_some()
    }

    /// Adds an operation of `kind` using the tool `tool` (a library reference such as
    /// `generic.drag-blade@1.0.0`) to every selected path, as one undo step, and
    /// returns its row. Every call makes a new row that runs last, so the same operation can be
    /// repeated with its own colour and settings. `None` when nothing is selected or the
    /// reference is not valid.
    pub fn assign_operation_row(
        &mut self,
        kind: OpKind,
        tool: &str,
        now_ms: u64,
    ) -> Option<OpRowId> {
        let tool = tool.parse::<LibRef>().ok()?;
        let row = OpRowId::new();
        let mut cmds: Vec<Box<dyn ModelCommand>> = Vec::new();
        for (object, path) in self.selected_paths() {
            cmds.push(Box::new(AssignOperation {
                mat: self.mat,
                object,
                path: path.key.clone(),
                op: Operation {
                    id: OpId::new(),
                    kind,
                    tool: ToolSlotRef {
                        tool: tool.clone(),
                        pen_colour: None,
                    },
                    overrides: ParamOverrides::default(),
                    fill: None,
                    enabled: true,
                    row,
                },
            }));
        }
        if cmds.is_empty() {
            return None;
        }
        // Spell out the whole order: rows missing from it would otherwise sort after this one.
        let mut order: Vec<OpRowId> = self.operation_rows().iter().map(|r| r.id).collect();
        order.push(row);
        cmds.push(Box::new(SetOpOrder {
            mat: self.mat,
            order,
        }));
        self.commit(&cmds, now_ms).then_some(row)
    }

    /// The selected paths that do not carry `row` yet.
    fn paths_not_on_row(&self, row: OpRowId) -> Vec<(ObjectId, PathKey)> {
        self.selected_paths()
            .into_iter()
            .filter(|(_, path)| path.ops.iter().all(|op| op.row != row))
            .map(|(object, path)| (object, path.key.clone()))
            .collect()
    }

    /// The operations of `row` on the selected paths.
    fn selected_ops_on_row(&self, row: OpRowId) -> Vec<(ObjectId, PathKey, OpId)> {
        self.selected_paths()
            .into_iter()
            .flat_map(|(object, path)| {
                path.ops
                    .iter()
                    .filter(|op| op.row == row)
                    .map(|op| (object, path.key.clone(), op.id))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Whether [`Self::remove_selection_from_row`] has anything to do.
    #[must_use]
    pub fn can_remove_selection_from_row(&self, row: OpRowId) -> bool {
        !self.selected_ops_on_row(row).is_empty()
    }

    /// Takes the row's operation off the selected paths only, as one undo step. False when none
    /// of them has it.
    pub fn remove_selection_from_row(&mut self, row: OpRowId, now_ms: u64) -> bool {
        let cmds: Vec<Box<dyn ModelCommand>> = self
            .selected_ops_on_row(row)
            .into_iter()
            .map(|(object, path, op)| {
                Box::new(ps_model::RemoveOperation {
                    mat: self.mat,
                    object,
                    path,
                    op,
                }) as Box<dyn ModelCommand>
            })
            .collect();
        !cmds.is_empty() && self.commit(&cmds, now_ms)
    }

    /// Whether [`Self::add_selection_to_row`] has anything to do.
    #[must_use]
    pub fn can_add_selection_to_row(&self, row: OpRowId) -> bool {
        !self.row_operations(row).is_empty() && !self.paths_not_on_row(row).is_empty()
    }

    /// Puts an existing row's operation, with its tool, ink, fill and settings, on every selected
    /// path that does not have it, as one undo step. False when there is no such row or nothing
    /// to add.
    pub fn add_selection_to_row(&mut self, row: OpRowId, now_ms: u64) -> bool {
        let ops = self.row_operations(row);
        let Some((_, _, template)) = ops.first() else {
            return false;
        };
        // The new paths run unless the whole row is switched off.
        let enabled = ops.iter().any(|(_, _, op)| op.enabled);
        let cmds: Vec<Box<dyn ModelCommand>> = self
            .paths_not_on_row(row)
            .into_iter()
            .map(|(object, path)| {
                let mut op = (**template).clone();
                op.id = OpId::new();
                op.enabled = enabled;
                Box::new(AssignOperation {
                    mat: self.mat,
                    object,
                    path,
                    op,
                }) as Box<dyn ModelCommand>
            })
            .collect();
        !cmds.is_empty() && self.commit(&cmds, now_ms)
    }

    /// The realistic preview of the current mat's compiled job at `px_per_mm`, drawing only the
    /// first `up_to` steps; `None` until a job is compiled.
    #[must_use]
    pub fn preview(&self, px_per_mm: f64, up_to: usize) -> Option<Preview> {
        self.preview_inputs()?.build(px_per_mm, up_to)
    }

    /// Whether there is a compiled job for this mat to draw a preview of; false when none has
    /// compiled yet or the job is blocked by errors.
    #[must_use]
    pub(crate) fn can_preview(&self) -> bool {
        self.compiled()
            .is_some_and(|c| c.job.mat(self.mat).is_some())
    }

    /// What building the preview needs, owned so the work can run on another thread.
    #[must_use]
    pub(crate) fn preview_inputs(&self) -> Option<PreviewInputs> {
        let compiled = Arc::clone(self.compiled()?);
        compiled.job.mat(self.mat)?;
        Some(PreviewInputs {
            compiled,
            libs: self.libs.clone(),
            mat: self.current_mat(),
        })
    }

    /// The operation rows of the current mat in run order: kind, tool, how many paths and how
    /// much line they have, and whether they are enabled.
    #[must_use]
    pub fn operation_rows(&self) -> Vec<OperationRow> {
        let mat = self.current_mat();
        let mut rows: Vec<OperationRow> = Vec::new();
        for object in mat.objects.values() {
            for path in object.paths.iter() {
                let length = path.geom.transformed(object.transform).length();
                for op in &path.ops {
                    let at = rows.iter().position(|r| r.id == op.row);
                    let row = if let Some(i) = at {
                        &mut rows[i]
                    } else {
                        rows.push(OperationRow {
                            id: op.row,
                            kind: op.kind,
                            tool: op.tool.tool.to_string(),
                            tool_name: String::new(),
                            holder: String::new(),
                            // The chosen ink; the pen's own is filled in from the library.
                            ink: op.tool.pen_colour.clone(),
                            paths: 0,
                            length_mm: 0.0,
                            enabled: true,
                            partly: false,
                            locked_after_previous: false,
                            preset: None,
                            time_ms: None,
                        });
                        let last = rows.len() - 1;
                        &mut rows[last]
                    };
                    row.paths += 1;
                    row.length_mm += length;
                    row.enabled &= op.enabled;
                }
            }
        }
        // A row is partly enabled when some, but not all, of its operations run.
        for row in &mut rows {
            let ops: Vec<bool> = mat
                .objects
                .values()
                .flat_map(|o| o.paths.iter())
                .flat_map(|p| p.ops.iter())
                .filter(|op| op.row == row.id)
                .map(|op| op.enabled)
                .collect();
            row.partly = ops.iter().any(|e| *e) && ops.iter().any(|e| !*e);
        }
        let order = mat.op_order.clone();
        rows.sort_by_key(|r| {
            order
                .iter()
                .position(|id| *id == r.id)
                .unwrap_or(usize::MAX)
        });
        let locks = mat.run_after.clone();
        for i in 1..rows.len() {
            let (above, this) = (rows[i - 1].id, rows[i].id);
            rows[i].locked_after_previous = locks.iter().any(|(a, b)| *a == above && *b == this);
        }
        self.add_presets(&mat, &mut rows);
        self.add_times(&mat, &mut rows);
        self.add_tool_names(&mut rows);
        rows
    }

    /// The libraries the app compiles against.
    #[must_use]
    pub(crate) fn libraries(&self) -> &LibrarySnapshot {
        &self.libs
    }

    /// Fills in each row's tool name and holder from the library.
    fn add_tool_names(&self, rows: &mut [OperationRow]) {
        for row in rows {
            let Ok(reference) = row.tool.parse::<LibRef>() else {
                continue;
            };
            let Ok(found) = self.libs.resolve_tool(&reference) else {
                continue;
            };
            row.tool_name.clone_from(&found.item.name);
            row.ink = found
                .item
                .pen
                .as_ref()
                .map(|pen| row.ink.take().unwrap_or_else(|| pen.colour.hex.clone()));
            row.holder = found
                .item
                .holders
                .first()
                .map(|h| holder_name(h))
                .unwrap_or_default();
        }
    }

    /// Fills in each row's preset from the mat's material.
    fn add_presets(&self, mat: &Mat, rows: &mut [OperationRow]) {
        let Some(reference) = &mat.material else {
            return;
        };
        let Ok(found) = self.libs.resolve_material(reference) else {
            return;
        };
        let material = found.item;
        for row in rows {
            let Ok(tool) = row.tool.parse::<LibRef>() else {
                continue;
            };
            let has = material
                .presets
                .iter()
                .any(|p| p.tool == tool.id() && p.operation == row.kind.as_str());
            if has {
                row.preset = Some(material.name.clone());
            }
        }
    }

    /// Splits the compiled job's machine time between enabled rows in proportion to length.
    fn add_times(&self, mat: &Mat, rows: &mut [OperationRow]) {
        let Some(compiled) = self.compiled() else {
            return;
        };
        let Some(job) = compiled.job.mat(mat.id) else {
            return;
        };
        let total: f64 = rows.iter().filter(|r| r.enabled).map(|r| r.length_mm).sum();
        if total <= 0.0 {
            return;
        }
        let time = job.estimate.machine_time_ms;
        for row in rows.iter_mut().filter(|r| r.enabled) {
            let share = row.length_mm / total;
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss,
                reason = "a share of a millisecond count, rounded to whole milliseconds"
            )]
            let part = (time as f64 * share).round() as u64;
            row.time_ms = Some(part);
        }
    }

    /// Moves a row up (`-1`) or down (`1`) in run order. False when it cannot move that way.
    pub fn move_operation_row(&mut self, row: OpRowId, delta: i32, now_ms: u64) -> bool {
        let mut order: Vec<OpRowId> = self.operation_rows().iter().map(|r| r.id).collect();
        let Some(at) = order.iter().position(|id| *id == row) else {
            return false;
        };
        let to = i64::try_from(at).unwrap_or(0) + i64::from(delta);
        let Ok(to) = usize::try_from(to) else {
            return false;
        };
        if to >= order.len() || to == at {
            return false;
        }
        order.swap(at, to);
        let command: Box<dyn ModelCommand> = Box::new(SetOpOrder {
            mat: self.mat,
            order,
        });
        self.commit(&[command], now_ms)
    }

    /// The project's tool ordering mode.
    #[must_use]
    pub fn ordering(&self) -> ps_model::OrderingMode {
        self.store.document().settings.ordering
    }

    /// Whether a check mark is drawn before each pen.
    #[must_use]
    pub fn pen_check(&self) -> bool {
        self.store.document().settings.pen_check
    }

    /// Sets the ordering mode. False when it is already that.
    pub fn set_ordering(&mut self, mode: ps_model::OrderingMode, now_ms: u64) -> bool {
        let mut settings = (*self.store.document().settings).clone();
        if settings.ordering == mode {
            return false;
        }
        settings.ordering = mode;
        self.set_job_settings(settings, now_ms)
    }

    /// Turns the pen check on or off. False when it is already that.
    pub fn set_pen_check(&mut self, on: bool, now_ms: u64) -> bool {
        let mut settings = (*self.store.document().settings).clone();
        if settings.pen_check == on {
            return false;
        }
        settings.pen_check = on;
        self.set_job_settings(settings, now_ms)
    }

    fn set_job_settings(&mut self, settings: ps_model::JobSettings, now_ms: u64) -> bool {
        let command: Box<dyn ModelCommand> = Box::new(ps_model::SetJobSettings { settings });
        self.commit(&[command], now_ms)
    }

    /// Moves a row to a position in the run order (as when it is dropped there). False when it is
    /// already there or the position does not exist.
    pub fn move_operation_row_to(&mut self, row: OpRowId, to: usize, now_ms: u64) -> bool {
        let mut order: Vec<OpRowId> = self.operation_rows().iter().map(|r| r.id).collect();
        let Some(at) = order.iter().position(|id| *id == row) else {
            return false;
        };
        if to >= order.len() || to == at {
            return false;
        }
        let moved = order.remove(at);
        order.insert(to, moved);
        let command: Box<dyn ModelCommand> = Box::new(SetOpOrder {
            mat: self.mat,
            order,
        });
        self.commit(&[command], now_ms)
    }

    /// Locks a row to run after the row above it, or removes that lock. False when the row is
    /// first or unknown, or nothing changes.
    pub fn set_row_locked_after_previous(
        &mut self,
        row: OpRowId,
        locked: bool,
        now_ms: u64,
    ) -> bool {
        let rows = self.operation_rows();
        let Some(at) = rows.iter().position(|r| r.id == row) else {
            return false;
        };
        if at == 0 {
            return false;
        }
        let above = rows[at - 1].id;
        let mut locks: Vec<(OpRowId, OpRowId)> = (*self.current_mat().run_after).clone();
        let had = locks.contains(&(above, row));
        if had == locked {
            return false;
        }
        if locked {
            locks.push((above, row));
        } else {
            locks.retain(|l| *l != (above, row));
        }
        let command: Box<dyn ModelCommand> = Box::new(SetRunAfter {
            mat: self.mat,
            locks,
        });
        self.commit(&[command], now_ms)
    }

    /// Turns every operation of a row on or off as one undo step.
    pub fn set_operation_row_enabled(&mut self, row: OpRowId, enabled: bool, now_ms: u64) -> bool {
        let mat = self.current_mat();
        let mat_id = self.mat;
        let cmds: Vec<Box<dyn ModelCommand>> = mat
            .objects
            .values()
            .flat_map(|o| o.paths.iter().map(move |p| (o.id, p)))
            .flat_map(|(object, p)| {
                p.ops
                    .iter()
                    .filter(|op| op.row == row && op.enabled != enabled)
                    .map(move |op| {
                        Box::new(SetOpEnabled {
                            mat: mat_id,
                            object,
                            path: p.key.clone(),
                            op: op.id,
                            enabled,
                        }) as Box<dyn ModelCommand>
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        !cmds.is_empty() && self.commit(&cmds, now_ms)
    }

    fn move_focus(&mut self, forward: bool) {
        let count = FOCUS_ORDER.len();
        let next = match (self.focus_index, forward) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(i), true) => (i + 1) % count,
            (Some(i), false) => (i + count - 1) % count,
        };
        self.focus_index = Some(next);
        self.focus_request = Some(FOCUS_ORDER[next]);
    }

    /// The panel that should take the keyboard focus now; reading clears it.
    pub fn take_focus_request(&mut self) -> Option<FocusPanel> {
        self.focus_request.take()
    }

    /// Whether a node is selected, for the node commands.
    #[must_use]
    pub fn has_selected_node(&self) -> bool {
        self.selected_node().is_some()
    }

    /// The selected node, if one is: its object, path and index.
    fn selected_node(&self) -> Option<(ObjectId, PathKey, usize)> {
        self.selection
            .items()
            .find_map(|i| Some((i.object, i.path.clone()?, usize::try_from(i.node?).ok()?)))
    }

    /// Applies a node edit to the selected node's path as one undo step. After a deletion the
    /// node is gone, so the selection falls back to the whole object.
    fn edit_selected_node(
        &mut self,
        edit: impl FnOnce(&ps_geometry::Path, usize) -> Option<ps_geometry::Path>,
        removes_node: bool,
        now_ms: u64,
    ) -> bool {
        let Some((object, key, node)) = self.selected_node() else {
            return false;
        };
        let Some(geom) = self
            .current_mat()
            .objects
            .get(&object)
            .and_then(|o| o.paths.iter().find(|p| p.key == key))
            .and_then(|p| edit(&p.geom, node))
        else {
            return false;
        };
        let cmds: Vec<Box<dyn ModelCommand>> = vec![Box::new(ps_model::SetPathGeometry {
            mat: self.mat,
            object,
            path: key,
            geom,
        })];
        let done = self.commit(&cmds, now_ms);
        if done && removes_node {
            self.selection.select_object(object);
        }
        done
    }

    fn zoom_about_centre(&mut self, factor: f64) {
        let c = self.viewport.world_to_screen(self.viewport.center());
        self.viewport.zoom_at(c, factor);
    }

    fn fit_mat(&mut self) {
        let mat = self.current_mat();
        let width = mat.size.width.get();
        // A roll is metres long: fit its width and the first stretch of it, not the whole length.
        let depth = if mat.mat_type == ps_model::MatType::Matless {
            mat.size.height.get().min(width * ROLL_VIEW_ASPECT)
        } else {
            mat.size.height.get()
        };
        self.viewport
            .fit(Rect::new(0.0, 0.0, width, depth), FIT_MARGIN_PX);
    }

    fn after_history_change(&mut self, now_ms: u64) {
        self.switch_to_a_mat_that_exists();
        self.drop_stale_selection();
        self.debounce.touch(now_ms);
    }

    /// Forgets whatever is selected that the document no longer has.
    fn drop_stale_selection(&mut self) {
        let mat = self.current_mat();
        self.selection
            .retain_existing(|o| mat.objects.contains_key(&o));
        self.selection.retain_paths(|o, key| {
            mat.objects
                .get(&o)
                .is_some_and(|o| o.paths.iter().any(|p| p.key == *key))
        });
    }

    fn commit(&mut self, cmds: &[Box<dyn ModelCommand>], now_ms: u64) -> bool {
        self.clock_ms = now_ms;
        let label = ps_core::L10nKey::new("cmd-edit");
        if self.store.transaction(label, cmds, now_ms).is_err() {
            return false;
        }
        self.switch_to_a_mat_that_exists();
        self.drop_stale_selection();
        self.debounce.touch(now_ms);
        true
    }

    fn selected_objects(&self) -> Vec<Arc<PlacedObject>> {
        let mat = self.current_mat();
        self.selection
            .objects()
            .into_iter()
            .filter_map(|id| mat.objects.get(&id).cloned())
            .collect()
    }

    fn move_selection(&mut self, dx: f64, dy: f64, now_ms: u64) {
        if self.selection.has_shapes() {
            return;
        }
        let cmds: Vec<Box<dyn ModelCommand>> = self
            .selected_objects()
            .iter()
            .map(|o| {
                Box::new(SetTransform {
                    mat: self.mat,
                    object: o.id,
                    transform: Affine::translate((dx, dy)) * o.transform,
                }) as Box<dyn ModelCommand>
            })
            .collect();
        self.commit(&cmds, now_ms);
    }

    fn delete_selection(&mut self, now_ms: u64) {
        let mut cmds: Vec<Box<dyn ModelCommand>> = Vec::new();
        for object in self.selected_objects() {
            let chosen: Vec<&PathKey> = object
                .paths
                .iter()
                .map(|p| &p.key)
                .filter(|key| self.selection.includes_path(object.id, key))
                .collect();
            // Taking every shape takes the object: one with no paths would be nothing.
            if chosen.len() == object.paths.len() {
                cmds.push(Box::new(RemoveObject {
                    mat: self.mat,
                    object: object.id,
                }));
                continue;
            }
            for path in chosen {
                cmds.push(Box::new(RemovePath {
                    mat: self.mat,
                    object: object.id,
                    path: path.clone(),
                }));
            }
        }
        self.commit(&cmds, now_ms);
    }

    /// Places text as one object whose top-left corner is `at`, and selects it.
    ///
    /// # Errors
    ///
    /// A message when the text is blank or cannot be made; the document is left alone.
    pub fn add_text(
        &mut self,
        params: &crate::text_object::TextParams,
        fonts: &crate::fonts::FontLibrary,
        at: Point,
        now_ms: u64,
    ) -> Result<(), String> {
        let paths = crate::text_object::text_paths(params, fonts)?;
        let id = ObjectId::new();
        let shapes: Vec<Arc<ShapePath>> = paths
            .into_iter()
            .enumerate()
            .map(|(i, geom)| {
                Arc::new(ShapePath {
                    key: PathKey::source(format!("text-{id}-{i}")),
                    geom,
                    fill_rule: FillRule::NonZero,
                    style: SourceStyle::default(),
                    ops: Vec::new(),
                    start_locked: false,
                })
            })
            .collect();
        let object = PlacedObject {
            id,
            source: None,
            transform: Affine::translate((at.x, at.y)),
            group: None,
            attach: None,
            paths: Arc::new(shapes),
            locked: false,
            hidden: false,
        };
        let command: Box<dyn ModelCommand> = Box::new(AddObject {
            mat: self.mat,
            object,
        });
        if self.commit(&[command], now_ms) {
            self.selection.set_objects(&[id]);
        }
        Ok(())
    }

    fn duplicate_selection(&mut self, now_ms: u64) {
        if self.selection.has_shapes() {
            return;
        }
        let mut copies = Vec::new();
        let cmds: Vec<Box<dyn ModelCommand>> = self
            .selected_objects()
            .iter()
            .map(|o| {
                let mut copy = (**o).clone();
                copy.id = ObjectId::new();
                copy.transform =
                    Affine::translate((DUPLICATE_OFFSET_MM, DUPLICATE_OFFSET_MM)) * o.transform;
                copies.push(copy.id);
                Box::new(AddObject {
                    mat: self.mat,
                    object: copy,
                }) as Box<dyn ModelCommand>
            })
            .collect();
        if self.commit(&cmds, now_ms) {
            self.selection.set_objects(&copies);
        }
    }

    // ---- the canvas ----

    fn tool_ctx(&self) -> ToolCtx {
        let mat = self.current_mat();
        let (pickables, pickable_keys): (Vec<Pickable<ObjectId>>, Vec<PathKey>) = mat
            .objects
            .values()
            .filter(|o| !o.hidden)
            .flat_map(|o| {
                o.paths.iter().map(|p| {
                    let pickable = Pickable {
                        id: o.id,
                        path: p.geom.transformed(o.transform),
                        filled: p.geom.closed,
                    };
                    (pickable, p.key.clone())
                })
            })
            .unzip();
        let shapes: Vec<(ObjectId, PathKey)> = self
            .selection
            .shapes()
            .map(|(o, p)| (o, p.clone()))
            .collect();
        // Nothing moves while shapes are being picked, so no object counts as selected then.
        let selected = if shapes.is_empty() {
            self.selection
                .objects()
                .into_iter()
                .filter_map(|id| mat.objects.get(&id).map(|o| (id, o.transform)))
                .collect()
        } else {
            Vec::new()
        };
        let edit_paths = if matches!(
            self.tool_id,
            ToolId::Node | ToolId::Knife | ToolId::StartPoint
        ) {
            // The object under the pointer can be worked on too, without selecting it first.
            let mut ids = self.selection.objects();
            if self.tool_id != ToolId::Knife
                && let Some(under) = self
                    .hover_world
                    .and_then(|p| ps_render::pick(&pickables, p, self.viewport.zoom()))
                && !ids.contains(&under)
            {
                ids.push(under);
            }
            ids.into_iter()
                .filter_map(|id| mat.objects.get(&id))
                .flat_map(|o| {
                    // The knife cuts only the chosen shapes of an object that has some chosen.
                    let wanted = o.paths.iter().filter(|p| {
                        self.tool_id != ToolId::Knife || self.selection.includes_path(o.id, &p.key)
                    });
                    wanted.map(|p| EditPath {
                        object: o.id,
                        path: p.key.clone(),
                        geom: p.geom.clone(),
                        transform: o.transform,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        ToolCtx {
            mat: self.mat,
            new_object: ObjectId::new(),
            new_path: PathKey::source(format!("p-{}", ObjectId::new())),
            zoom: self.viewport.zoom(),
            pickables,
            pickable_keys,
            shapes,
            selected,
            sides: self.tool_sides,
            star_ratio: self.star_ratio,
            edit_paths,
            guides: mat.guides.to_vec(),
            snapping: self.snapping,
            mat_size: (mat.size.width.get(), mat.size.height.get()),
        }
    }

    /// The operations row whose paths are shown highlighted on the canvas, if any.
    #[must_use]
    pub fn highlight_row(&self) -> Option<OpRowId> {
        self.highlight_row
    }

    /// Highlights the paths of one operations row (while the pointer is over it), or none.
    pub fn set_highlight_row(&mut self, row: Option<OpRowId>) {
        self.highlight_row = row;
    }

    /// The object, or the one shape of it, highlighted from the layers panel, if any.
    #[must_use]
    pub fn highlight_layer(&self) -> Option<&(ObjectId, Option<PathKey>)> {
        self.highlight_layer.as_ref()
    }

    /// Highlights an object (no path) or one of its shapes while the pointer is on its row in
    /// the layers panel, or none.
    pub fn set_highlight_layer(&mut self, layer: Option<(ObjectId, Option<PathKey>)>) {
        self.highlight_layer = layer;
    }

    /// The objects a gesture in progress is moving, and the transform applied to them so far.
    #[must_use]
    pub fn live_transform(&self) -> Option<(Vec<ObjectId>, Affine)> {
        self.tool.live()
    }

    /// The bounds the gesture in progress started from.
    #[must_use]
    pub fn live_bounds(&self) -> Option<Rect> {
        self.tool.live_bounds()
    }

    /// The pointer shape the active tool wants right now.
    #[must_use]
    pub fn tool_pointer(&self) -> crate::tools::Cursor {
        self.tool.pointer()
    }

    // ---- snapping and guides ----

    /// Whether the cursor snaps to nearby points.
    #[must_use]
    pub fn snapping(&self) -> bool {
        self.snapping
    }

    /// The last thing the cursor snapped to, for the indicator; cleared when it stops snapping.
    #[must_use]
    pub fn last_snap(&self) -> Option<(Point, SnapKind)> {
        self.last_snap
    }

    /// Where a cursor at `cursor` lands, and what it snapped to, if anything.
    #[must_use]
    pub fn snap_point(&self, cursor: Point) -> (Point, Option<SnapKind>) {
        let zoom = self.viewport.zoom();
        let mat = self.current_mat();
        let mut candidates: Vec<Candidate> = Vec::new();
        if GRID_SNAP_MM * zoom >= MIN_GRID_SNAP_PX {
            candidates.push(Candidate {
                point: ps_render::grid_point(cursor, GRID_SNAP_MM),
                kind: SnapKind::Grid,
            });
        }
        let outline = Rect::new(0.0, 0.0, mat.size.width.get(), mat.size.height.get());
        candidates.push(Candidate {
            point: ps_render::edge_point(cursor, outline),
            kind: SnapKind::Edge,
        });
        for guide in mat.guides.iter() {
            let point = match guide.axis {
                GuideAxis::Vertical => Point::new(guide.at_mm, cursor.y),
                GuideAxis::Horizontal => Point::new(cursor.x, guide.at_mm),
            };
            candidates.push(Candidate {
                point,
                kind: SnapKind::Guide,
            });
        }
        let mut budget = MAX_SNAP_OBJECTS;
        for object in mat.objects.values().filter(|o| !o.hidden) {
            if budget == 0 {
                break;
            }
            budget -= 1;
            if let Some(b) = object.bounds() {
                for point in ps_render::bbox_anchors(b) {
                    candidates.push(Candidate {
                        point,
                        kind: SnapKind::Anchor,
                    });
                }
            }
            for path in object.paths.iter() {
                let world = path.geom.transformed(object.transform);
                let mut nodes = vec![world.start];
                nodes.extend(world.segments().map(|(_, seg)| seg.to()));
                for point in ps_render::segment_midpoints(&nodes, world.closed) {
                    candidates.push(Candidate {
                        point,
                        kind: SnapKind::Midpoint,
                    });
                }
                for point in nodes {
                    candidates.push(Candidate {
                        point,
                        kind: SnapKind::Node,
                    });
                }
            }
        }
        candidates.extend(
            Self::crossing_candidates(&mat, cursor, zoom)
                .into_iter()
                .map(|point| Candidate {
                    point,
                    kind: SnapKind::Intersection,
                }),
        );
        match ps_render::snap(cursor, &candidates, zoom) {
            Some(hit) => (hit.point, Some(hit.kind)),
            None => (cursor, None),
        }
    }

    /// Where paths near the cursor cross each other. Only paths whose box reaches the snap
    /// radius around the cursor are looked at, and only a bounded number of them.
    fn crossing_candidates(mat: &Mat, cursor: Point, zoom: f64) -> Vec<Point> {
        let reach = ps_render::snap_radius_mm(zoom) * 2.0;
        let mut lines: Vec<(Vec<Point>, bool)> = Vec::new();
        for object in mat.objects.values().filter(|o| !o.hidden) {
            for path in object.paths.iter() {
                let world = path.geom.transformed(object.transform);
                let b = world.bounds();
                if cursor.x < b.x0 - reach
                    || cursor.x > b.x1 + reach
                    || cursor.y < b.y0 - reach
                    || cursor.y > b.y1 + reach
                {
                    continue;
                }
                lines.push((
                    ps_geometry::flatten(&world, ps_render::flatten_tolerance_mm(zoom)),
                    world.closed,
                ));
                if lines.len() >= MAX_CROSSING_PATHS {
                    break;
                }
            }
        }
        let mut out = Vec::new();
        for (i, (a, a_closed)) in lines.iter().enumerate() {
            for (b, b_closed) in lines.iter().skip(i + 1) {
                out.extend(ps_render::polyline_intersections(
                    a, *a_closed, b, *b_closed,
                ));
            }
        }
        out
    }

    /// Adds a guide line. False when the same guide is already there.
    pub fn add_guide(&mut self, axis: GuideAxis, at_mm: f64, now_ms: u64) -> bool {
        let mut guides: Vec<Guide> = (*self.current_mat().guides).clone();
        if guides
            .iter()
            .any(|g| g.axis == axis && (g.at_mm - at_mm).abs() < 1e-9)
        {
            return false;
        }
        guides.push(Guide { axis, at_mm });
        self.set_guides(guides, now_ms)
    }

    /// A pointer event from the canvas widget: like [`canvas_event`](Self::canvas_event), but the
    /// point first snaps to what is near it (unless the command key is held or snapping is off).
    pub fn canvas_pointer_event(&mut self, event: CanvasEvent, mods: Mods, now_ms: u64) {
        if let CanvasEvent::Hover(p) | CanvasEvent::Press(p) = event {
            self.hover_world = Some(p);
        }
        let event = self.snapped_event(event, mods);
        self.canvas_event(event, mods, now_ms);
    }

    /// Removes the guide of `axis` within `tolerance_mm` of `at_mm`. False when there is none.
    pub fn remove_guide_near(
        &mut self,
        axis: GuideAxis,
        at_mm: f64,
        tolerance_mm: f64,
        now_ms: u64,
    ) -> bool {
        let mut guides: Vec<Guide> = (*self.current_mat().guides).clone();
        let before = guides.len();
        guides.retain(|g| !(g.axis == axis && (g.at_mm - at_mm).abs() <= tolerance_mm));
        guides.len() != before && self.set_guides(guides, now_ms)
    }

    fn set_guides(&mut self, guides: Vec<Guide>, now_ms: u64) -> bool {
        let command: Box<dyn ModelCommand> = Box::new(SetGuides {
            mat: self.mat,
            guides,
        });
        self.commit(&[command], now_ms)
    }

    fn snapped_event(&mut self, event: CanvasEvent, mods: Mods) -> CanvasEvent {
        if !self.snapping
            || mods.command
            || matches!(self.tool_id, ToolId::Select | ToolId::StartPoint)
        {
            self.last_snap = None;
            return event;
        }
        let mut hit = None;
        let mut at = |p: Point| {
            let (snapped, kind) = self.snap_point(p);
            hit = kind.map(|k| (snapped, k));
            snapped
        };
        let out = match event {
            CanvasEvent::Press(p) => CanvasEvent::Press(at(p)),
            CanvasEvent::Drag(p) => CanvasEvent::Drag(at(p)),
            CanvasEvent::Release(p) => CanvasEvent::Release(at(p)),
            CanvasEvent::Hover(p) => CanvasEvent::Hover(at(p)),
            CanvasEvent::DoubleClick(p) => CanvasEvent::DoubleClick(at(p)),
            other => other,
        };
        self.last_snap = hit;
        out
    }

    /// Feeds a canvas event to the active tool and applies what it asks for.
    pub fn canvas_event(&mut self, event: CanvasEvent, mods: Mods, now_ms: u64) {
        let ctx = self.tool_ctx();
        let outcome = self.tool.on_event(&event, mods, &ctx);
        match outcome {
            ToolOutcome::Commit(cmds) => {
                if self.commit(&cmds, now_ms)
                    && self.current_mat().objects.contains_key(&ctx.new_object)
                {
                    self.selection.select_object(ctx.new_object);
                }
            }
            ToolOutcome::Select(ids) => self.selection.set_objects(&ids),
            ToolOutcome::Add(ids) => {
                for id in ids {
                    self.selection.add_object(id);
                }
            }
            ToolOutcome::Toggle(id) => self.selection.toggle_object(id),
            ToolOutcome::SelectNode(item) => {
                self.selection.select(item);
            }
            ToolOutcome::SelectShapes(shapes) => self.selection.set_shapes(&shapes),
            ToolOutcome::AddShapes(shapes) => {
                for (object, path) in shapes {
                    self.selection.add_shape(object, path);
                }
            }
            ToolOutcome::ToggleShape(object, path) => self.selection.toggle_shape(object, path),
            ToolOutcome::Idle | ToolOutcome::Preview | ToolOutcome::Cancel => {}
        }
    }

    /// The point on the mat under a screen position, for the canvas widget.
    #[must_use]
    pub fn screen_to_world(&self, screen: Point) -> Point {
        self.viewport.screen_to_world(screen)
    }

    /// Pans the canvas by a screen offset.
    pub fn pan_by_screen(&mut self, across_px: f64, down_px: f64) {
        self.viewport.pan_by_screen(across_px, down_px);
    }

    /// Replaces the library the app compiles against (after the user installs or removes
    /// something) and compiles again.
    ///
    /// The machine is chosen again from the new library: the one in use if it is still there (at
    /// its newest version), else the one the settings name, else the built-in default. A machine
    /// that comes from an installed plugin's library is not in the built-in one the app starts
    /// with, so this is what selects it at start-up.
    pub fn replace_libraries(&mut self, libs: LibrarySnapshot) {
        self.libs = libs;
        let current = self.profile.as_ref().map(|p| p.id.clone());
        let saved = self.settings.default_machine.clone();
        self.profile = [current.as_deref(), saved.as_deref(), Some(DEFAULT_MACHINE)]
            .into_iter()
            .flatten()
            .find_map(|id| newest_profile(&self.libs, id));
        if let Some(p) = &self.profile {
            tracing::info!(machine = %p.id, version = %p.version, "library loaded");
        } else {
            tracing::warn!(
                wanted = ?saved,
                "library loaded, but it has no machine to prepare jobs for"
            );
        }
        self.start_compile();
    }

    /// The library the app compiles against.
    pub(crate) fn libs(&self) -> &LibrarySnapshot {
        &self.libs
    }

    /// The id of the mat being edited.
    pub(crate) fn mat_id(&self) -> MatId {
        self.mat
    }

    /// Applies commands as one undo step; false if any of them did not apply.
    pub(crate) fn commit_all(&mut self, cmds: &[Box<dyn ModelCommand>], now_ms: u64) -> bool {
        self.commit(cmds, now_ms)
    }

    /// Makes exactly these objects the selection.
    pub(crate) fn select_objects(&mut self, ids: &[ObjectId]) {
        self.selection.set_objects(ids);
    }

    /// Replaces the mapping rules as one undo step.
    ///
    /// # Errors
    ///
    /// The command error when the rules cannot be set.
    pub fn set_rules(
        &mut self,
        rules: Vec<ps_model::MappingRule>,
        now_ms: u64,
    ) -> Result<(), ps_model::CommandError> {
        let cmds: Vec<Box<dyn ModelCommand>> = vec![Box::new(ps_model::SetRules { rules })];
        let label = ps_core::L10nKey::new("cmd-set-rules");
        self.store.transaction(label, &cmds, now_ms)?;
        self.debounce.touch(now_ms);
        Ok(())
    }

    /// Makes exactly these shapes (object and path) the selection.
    pub(crate) fn select_shapes(&mut self, shapes: &[(ObjectId, PathKey)]) {
        self.selection.set_shapes(shapes);
    }

    /// Adds a shape to the selection or takes it out (Shift-click in the project tree).
    pub(crate) fn toggle_shape(&mut self, object: ObjectId, path: PathKey) {
        self.selection.toggle_shape(object, path);
    }

    /// Makes one object the whole selection (a click in the project tree).
    pub fn select_object(&mut self, object: ObjectId) {
        self.selection.select_object(object);
    }

    /// Zooms the canvas about a screen position.
    pub fn zoom_at(&mut self, cursor: Point, factor: f64) {
        self.viewport.zoom_at(cursor, factor);
    }
}

mod automation;
mod params;
mod rows;
pub(crate) use params::{ParamOrigin, ParamView};

#[cfg(test)]
mod tests;

/// A holder id such as `generic.holder-b` as a name such as "Holder B".
fn holder_name(id: &str) -> String {
    let last = id.rsplit('.').next().unwrap_or(id);
    let mut out = String::new();
    for (i, word) in last.split('-').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            if word.len() == 1 {
                out.extend(first.to_uppercase());
            } else if i == 0 {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            } else {
                out.push(first);
                out.push_str(chars.as_str());
            }
        }
    }
    out
}

/// Everything the realistic preview is built from, owned so it can be built off the UI thread.
#[derive(Debug, Clone)]
pub(crate) struct PreviewInputs {
    compiled: Arc<Compiled>,
    libs: LibrarySnapshot,
    mat: Arc<Mat>,
}

impl PreviewInputs {
    /// Builds the preview at `px_per_mm`, drawing only the first `up_to` steps.
    pub(crate) fn build(&self, px_per_mm: f64, up_to: usize) -> Option<Preview> {
        let compiled = &self.compiled;
        let job = compiled.job.mat(self.mat.id)?;
        let mat = Arc::clone(&self.mat);
        let looks: Vec<ps_render::ToolLook> = job
            .tool_table
            .iter()
            .filter_map(|entry| {
                let tool = self.libs.resolve_tool(&entry.tool).ok()?.item;
                let effect = effect_named(&tool.render.effect);
                let colour = entry
                    .pen
                    .as_ref()
                    .and_then(|p| p.colour.as_deref())
                    .map(hex_colour)
                    .or_else(|| tool.pen.as_ref().map(|p| hex_colour(&p.colour.hex)))
                    .unwrap_or([40, 40, 40]);
                let variant_width = entry
                    .pen
                    .as_ref()
                    .and_then(|p| p.tip_width_um)
                    .map(|um| f64::from(um) / 1000.0);
                let width_mm = variant_width.or(tool.geometry.tip_width_mm).unwrap_or(
                    if matches!(effect, ps_render::Effect::Ink) {
                        0.4
                    } else {
                        0.3
                    },
                );
                Some(ps_render::ToolLook {
                    slot: entry.slot,
                    effect,
                    colour,
                    width_mm,
                })
            })
            .collect();
        let category = mat
            .material
            .as_ref()
            .and_then(|r| self.libs.resolve_material(r).ok())
            .map(|m| m.item.category.clone())
            .unwrap_or_default();
        let base = base_for(&category);
        let maps = ps_render::EffectMaps::rasterise(
            &job.steps,
            &looks,
            mat.size.width.get(),
            mat.size.height.get(),
            px_per_mm,
            up_to,
        );
        let image = ps_render::composite(&base, &maps, &ps_render::Light::raking(315.0));
        let pieces = ps_render::Pieces::from_steps(
            &job.steps,
            &looks,
            mat.size.width.get(),
            mat.size.height.get(),
            px_per_mm,
            up_to,
        );
        let vinyl = matches!(category.as_str(), "vinyl" | "htv");
        Some(Preview {
            image,
            looks,
            base,
            maps,
            pieces,
            vinyl,
        })
    }
}
