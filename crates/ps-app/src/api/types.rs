//! The request types of the automation API: twelve tools, each an enum of operations.
//!
//! A tool call is `{"op": "...", ...fields}`. Objects, mats and operation rows are named by the
//! ids earlier replies returned, so nothing depends on what is selected in a window.

use schemars::JsonSchema;
use serde::Deserialize;

/// An id as text, as every reply prints it.
pub type Id = String;

/// A point in millimetres on the mat.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, JsonSchema)]
pub struct Pt {
    /// Distance from the left edge.
    pub x: f64,
    /// Distance from the top edge.
    pub y: f64,
}

/// Where on a selection's bounding box a position or size is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    /// Top-left corner.
    #[default]
    TopLeft,
    /// Middle of the top edge.
    Top,
    /// Top-right corner.
    TopRight,
    /// Middle of the left edge.
    Left,
    /// The centre.
    Centre,
    /// Middle of the right edge.
    Right,
    /// Bottom-left corner.
    BottomLeft,
    /// Middle of the bottom edge.
    Bottom,
    /// Bottom-right corner.
    BottomRight,
}

/// A direction along the mat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Left to right.
    Horizontal,
    /// Top to bottom.
    Vertical,
}

/// Which edges or centres to line objects up on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    /// Left edges.
    Left,
    /// Right edges.
    Right,
    /// Top edges.
    Top,
    /// Bottom edges.
    Bottom,
    /// Centres, left to right.
    CentreHorizontal,
    /// Centres, top to bottom.
    CentreVertical,
}

/// What an operation does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Cut through.
    Cut,
    /// Cut the top layer only.
    KissCut,
    /// Draw a line with a pen.
    Draw,
    /// Fill a region with a pen.
    Fill,
    /// Score a fold line.
    Score,
    /// Emboss.
    Emboss,
}

/// A shape to draw, in mat millimetres.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Shape {
    /// An axis-aligned rectangle.
    Rectangle {
        /// Left edge.
        x: f64,
        /// Top edge.
        y: f64,
        /// Width.
        width: f64,
        /// Height.
        height: f64,
    },
    /// An ellipse in a bounding box.
    Ellipse {
        /// Left edge of the box.
        x: f64,
        /// Top edge of the box.
        y: f64,
        /// Width.
        width: f64,
        /// Height.
        height: f64,
    },
    /// A straight line.
    Line {
        /// Start.
        from: Pt,
        /// End.
        to: Pt,
    },
    /// A regular polygon.
    Polygon {
        /// Centre.
        centre: Pt,
        /// Distance from the centre to a corner.
        radius: f64,
        /// Number of sides, at least 3.
        sides: u32,
    },
    /// A star.
    Star {
        /// Centre.
        centre: Pt,
        /// Distance from the centre to a tip.
        radius: f64,
        /// Number of tips, at least 3.
        points: u32,
        /// Inner radius as a fraction of the outer one.
        ratio: f64,
    },
    /// An arc through three points.
    Arc {
        /// Start.
        from: Pt,
        /// A point the arc passes through.
        through: Pt,
        /// End.
        to: Pt,
    },
    /// A polyline through points; closed when `closed` is true.
    Path {
        /// The corners, at least two.
        points: Vec<Pt>,
        /// Whether the last point joins back to the first.
        #[serde(default)]
        closed: bool,
    },
}

/// `project`: the project file and what is in it.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Project {
    /// Starts an empty project, discarding the current one.
    New,
    /// Opens a project file.
    Open {
        /// The `.pstudio` file.
        path: String,
    },
    /// Saves the project. Without `path`, saves where it was opened from.
    Save {
        /// Where to save; required for a project that has no file yet.
        path: Option<String>,
    },
    /// Describes the project: name, file, mats, whether it has unsaved changes.
    Info,
}

/// `import`: puts drawings, text and shapes onto the current mat.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Import {
    /// Imports an SVG, SVGZ, DXF, PDF or AI file.
    File {
        /// The drawing's path.
        path: String,
    },
    /// Adds text as outlines or single-stroke lines.
    Text {
        /// What to write.
        text: String,
        /// The font family; the bundled one when omitted.
        font: Option<String>,
        /// Letter height in millimetres.
        size_mm: f64,
        /// Where the text's top-left corner goes.
        at: Pt,
    },
    /// Adds a shape.
    Shape {
        /// The shape.
        #[serde(flatten)]
        shape: Shape,
    },
}

/// A change to node (corner) type on a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeAction {
    /// Deletes the node.
    Delete,
    /// Turns the segment after the node into a curve.
    ToCurve,
    /// Turns the segment after the node into a straight line.
    ToLine,
    /// Makes the node smooth.
    Smooth,
    /// Makes the node a corner.
    Corner,
}

/// `object`: select, delete, duplicate, move, size, arrange and edit objects.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Object {
    /// Selects objects, or shapes inside one, in the window; `objects` empty clears the
    /// selection.
    Select {
        /// The objects.
        objects: Vec<Id>,
        /// Only these shapes, by the `key` that `query` `objects` lists for each path; needs
        /// exactly one object. Without it every path of the objects.
        #[serde(default)]
        paths: Option<Vec<String>>,
    },
    /// Deletes objects, or shapes inside one.
    Delete {
        /// The objects.
        objects: Vec<Id>,
        /// Only these shapes, by the `key` that `query` `objects` lists for each path; needs
        /// exactly one object. Without it every path of the objects.
        #[serde(default)]
        paths: Option<Vec<String>>,
    },
    /// Duplicates objects, offset a little; the reply lists the copies.
    Duplicate {
        /// The objects.
        objects: Vec<Id>,
    },
    /// Moves objects by a distance.
    Move {
        /// The objects.
        objects: Vec<Id>,
        /// Distance to the right.
        dx: f64,
        /// Distance down.
        dy: f64,
    },
    /// Moves the objects' bounding box so its anchor is at a point.
    MoveTo {
        /// The objects.
        objects: Vec<Id>,
        /// Which part of the bounding box lands on the point.
        #[serde(default)]
        anchor: Anchor,
        /// Target x.
        x: f64,
        /// Target y.
        y: f64,
    },
    /// Resizes the objects' bounding box.
    Resize {
        /// The objects.
        objects: Vec<Id>,
        /// The corner or edge that stays put.
        #[serde(default)]
        anchor: Anchor,
        /// New width.
        width: f64,
        /// New height.
        height: f64,
        /// Keep the aspect ratio, fitting inside the width and height.
        #[serde(default)]
        lock_aspect: bool,
    },
    /// Rotates objects about their centre.
    Rotate {
        /// The objects.
        objects: Vec<Id>,
        /// Clockwise degrees.
        degrees: f64,
    },
    /// Mirrors objects about the centre of their bounds.
    Mirror {
        /// The objects.
        objects: Vec<Id>,
        /// `horizontal` flips left-right, `vertical` flips top-bottom.
        axis: Axis,
    },
    /// Lines objects up.
    Align {
        /// The objects.
        objects: Vec<Id>,
        /// What to line up.
        to: Align,
    },
    /// Spaces objects evenly.
    Distribute {
        /// The objects.
        objects: Vec<Id>,
        /// The direction to spread over.
        axis: Axis,
    },
    /// Makes a grid of copies.
    GridArray {
        /// The objects.
        objects: Vec<Id>,
        /// Copies across, including the original.
        columns: u32,
        /// Copies down, including the original.
        rows: u32,
        /// Spacing across.
        dx: f64,
        /// Spacing down.
        dy: f64,
    },
    /// Makes copies around the centre.
    CircularArray {
        /// The objects.
        objects: Vec<Id>,
        /// Total objects including the original.
        count: u32,
        /// Degrees to spread the copies over.
        total_degrees: f64,
    },
    /// Edits one node of a path.
    Node {
        /// The object.
        object: Id,
        /// Index of the path within the object, from 0.
        #[serde(default)]
        path: usize,
        /// Index of the node within the path, from 0.
        node: usize,
        /// What to do.
        action: NodeAction,
    },
    /// Chooses where a closed path starts, nearest to a point.
    SetStartPoint {
        /// The object.
        object: Id,
        /// Index of the path within the object, from 0.
        #[serde(default)]
        path: usize,
        /// The point the start moves nearest to.
        near: Pt,
    },
    /// Cuts the objects along a line.
    Split {
        /// The objects.
        objects: Vec<Id>,
        /// Start of the cutting line.
        from: Pt,
        /// End of the cutting line.
        to: Pt,
    },
}

/// `mat`: the mats of the project, their material and guides.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Mat {
    /// Adds a mat and makes it current.
    Add,
    /// Adds a roll (matless) mat and makes it current.
    AddRoll,
    /// Duplicates a mat.
    Duplicate {
        /// The mat; the current one when omitted.
        mat: Option<Id>,
    },
    /// Removes a mat.
    Remove {
        /// The mat; the current one when omitted.
        mat: Option<Id>,
    },
    /// Makes a mat the one later calls without `mat` act on.
    SetCurrent {
        /// The mat.
        mat: Id,
    },
    /// Sets the material loaded on a mat.
    SetMaterial {
        /// The mat; the current one when omitted.
        mat: Option<Id>,
        /// A library reference such as `community.cardstock-80lb@1.0.0`; `null` clears it.
        material: Option<String>,
    },
    /// Turns the mat's mirror flag on or off, for jobs cut on the back of the material.
    SetMirror {
        /// The mat; the current one when omitted.
        mat: Option<Id>,
        /// Whether to mirror the job at compile time.
        mirror: bool,
    },
    /// Adds a guide line.
    AddGuide {
        /// `horizontal` is a line of constant y.
        axis: Axis,
        /// Its position in millimetres.
        at_mm: f64,
    },
    /// Removes the guide nearest a position, within a tolerance.
    RemoveGuide {
        /// The axis.
        axis: Axis,
        /// The position in millimetres.
        at_mm: f64,
        /// How far away a guide may be, default 1 mm.
        tolerance_mm: Option<f64>,
    },
}

/// The pattern a fill is drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    /// Parallel hatch lines.
    Parallel,
    /// Hatch plus a second hatch at 90 degrees.
    Cross,
    /// Hatch lines linked end to end.
    ZigZag,
    /// Rings offset inward until nothing is left.
    Concentric,
}

/// How a fill is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, JsonSchema)]
pub struct Fill {
    /// The pattern.
    pub pattern: Pattern,
    /// Hatch angle in degrees, counter-clockwise from the x axis.
    #[serde(default)]
    pub angle_deg: f64,
    /// Overlap between adjacent passes from 0 to 1; 0.15 when omitted.
    pub overlap: Option<f64>,
    /// Also draw the boundary of the inset region.
    #[serde(default)]
    pub outline_pass: bool,
}

/// Whether paths are ordered to save tool changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Ordering {
    /// Merge adjacent groups with the same tool.
    MinimiseToolChanges,
    /// Keep the row order.
    StrictOrder,
}

/// `operation`: what is done to the paths, in which order, with which tool.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Operation {
    /// Puts an existing row's operation, with its settings, on more objects.
    AddObjects {
        /// The row.
        row: Id,
        /// The objects.
        objects: Vec<Id>,
        /// Only these shapes, by the `key` that `query` `objects` lists for each path; needs
        /// exactly one object. Without it every path of the objects.
        #[serde(default)]
        paths: Option<Vec<String>>,
    },
    /// Takes an existing row's operation off these objects only.
    RemoveObjects {
        /// The row.
        row: Id,
        /// The objects.
        objects: Vec<Id>,
        /// Only these shapes, by the `key` that `query` `objects` lists for each path; needs
        /// exactly one object. Without it every path of the objects.
        #[serde(default)]
        paths: Option<Vec<String>>,
    },
    /// Gives objects a new operation with a tool, in a row of its own even when an equal one
    /// exists. The reply holds the row.
    Assign {
        /// The objects.
        objects: Vec<Id>,
        /// What it does.
        kind: Kind,
        /// A tool reference such as `generic.drag-blade@1.0.0`; see `query` `tools`.
        tool: String,
        /// Only these shapes, by the `key` that `query` `objects` lists for each path; needs
        /// exactly one object. Without it every path of the objects.
        #[serde(default)]
        paths: Option<Vec<String>>,
    },
    /// Removes an operations row.
    Remove {
        /// The row.
        row: Id,
    },
    /// Turns a row on or off.
    Enable {
        /// The row.
        row: Id,
        /// Whether it runs.
        enabled: bool,
    },
    /// Moves a row to a position in the order.
    Reorder {
        /// The row.
        row: Id,
        /// Its new index, from 0.
        to: usize,
    },
    /// Locks a row to run straight after the row above it.
    LockAfterPrevious {
        /// The row.
        row: Id,
        /// Whether it is locked.
        locked: bool,
    },
    /// Sets a tool parameter of a row (pressure, speed, passes, ...).
    SetParam {
        /// The row.
        row: Id,
        /// The parameter name; see `query` `row_params`.
        name: String,
        /// The value in the parameter's unit.
        value: f64,
    },
    /// Resets one parameter, or all of them, to the preset's value.
    ResetParam {
        /// The row.
        row: Id,
        /// The parameter; all when omitted.
        name: Option<String>,
    },
    /// Sets the pen colour of a row, as a `#rrggbb` hex string; `null` clears it.
    SetPenColour {
        /// The row.
        row: Id,
        /// The colour.
        colour: Option<String>,
    },
    /// Sets how a fill row is drawn.
    SetFill {
        /// The row.
        row: Id,
        /// The fill.
        fill: Fill,
    },
    /// Chooses the ordering mode and the pen check mark for the job.
    SetJob {
        /// The ordering mode.
        ordering: Option<Ordering>,
        /// Whether to draw a pen check mark first.
        pen_check: Option<bool>,
    },
    /// Replaces the rules that map imported colours and layers to operations. The rules are
    /// JSON as `query` `rules` returns them.
    SetRules {
        /// The rules, each `{"match": ..., "assign": ...}`.
        rules: Vec<serde_json::Value>,
    },
    /// Chooses the machine profile the job is compiled for.
    SetMachine {
        /// A profile id such as `generic.cutter-medium`; see `query` `machines`.
        machine: String,
    },
}

/// `edit`: undo and redo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Edit {
    /// Undoes the last change.
    Undo,
    /// Redoes the last undone change.
    Redo,
    /// Says whether undo and redo are available.
    History,
}

/// `compile`: turns the design into machine programs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Compile {
    /// Starts compiling now instead of after the usual short delay.
    Start,
    /// Says whether a compile is running, and whether the result is current.
    Status,
    /// Waits for the running compile to finish.
    Wait {
        /// How long to wait at most, default 30000.
        timeout_ms: Option<u64>,
    },
    /// The compiled job: time, tool changes, warnings and errors.
    Result,
}

/// A picture of the preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Look {
    /// Straight down on the material.
    #[default]
    Flat,
}

/// `export`: the compiled job as G-code, files or a picture.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Export {
    /// The G-code of one mat.
    Gcode {
        /// Index of the mat in the job, from 0.
        #[serde(default)]
        mat: usize,
    },
    /// Writes the G-code and a report into a folder.
    Files {
        /// The folder, which must exist.
        folder: String,
    },
    /// Renders the preview to a PNG file.
    Preview {
        /// Where to write the PNG.
        path: String,
        /// Pixels per millimetre, default 4.
        px_per_mm: Option<f64>,
        /// How it is seen.
        #[serde(default)]
        look: Look,
    },
}

/// `machine`: the connected cutter. Real hardware moves, so a job is only sent with `armed`.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Machine {
    /// Lists known devices and whether each is approved.
    List,
    /// Allows the app to talk to a device.
    Approve {
        /// The device id from `list`.
        device: String,
    },
    /// Connects to an approved device.
    Connect {
        /// The device id from `list`.
        device: String,
    },
    /// Disconnects.
    Disconnect,
    /// Connection state, progress and any prompt the machine is waiting on.
    Status,
    /// Sends the compiled job. Refused on real hardware unless the user armed the API.
    Send {
        /// Index of the mat to send, from 0.
        #[serde(default)]
        mat: usize,
    },
    /// Pauses the job.
    Pause,
    /// Resumes a paused job.
    Resume,
    /// Cancels the job.
    Cancel,
    /// Moves the carriage by a distance.
    Jog {
        /// Distance along x in millimetres.
        x_mm: f64,
        /// Distance along y in millimetres.
        y_mm: f64,
    },
    /// Answers the machine's prompt (load the mat, change the tool).
    AnswerPrompt,
}

/// What to do when an installed library entry has the same id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    /// Replace the installed entry.
    Replace,
    /// Keep both.
    KeepBoth,
    /// Skip the entry in the package.
    #[default]
    Skip,
}

/// The kinds of library entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LibraryKind {
    /// Materials.
    Material,
    /// Tools.
    Tool,
    /// Machine profiles.
    Machine,
}

/// `library`: materials, tools and machine profiles.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Library {
    /// Lists the entries of one kind.
    List {
        /// The kind.
        kind: LibraryKind,
    },
    /// Installs a `.psmaterials` package.
    Install {
        /// The package file.
        path: String,
        /// What to do with entries that already exist.
        #[serde(default)]
        conflict: Conflict,
    },
    /// Removes an installed entry.
    Uninstall {
        /// The kind.
        kind: LibraryKind,
        /// The entry id.
        id: String,
    },
}

/// What `query` reads.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(tag = "what", rename_all = "snake_case")]
pub enum Query {
    /// The project: name, mats, dirty flag.
    Document,
    /// The mats, with the current one marked.
    Mats,
    /// The objects of a mat with bounds and operations.
    Objects {
        /// The mat; the current one when omitted.
        mat: Option<Id>,
    },
    /// The selected objects.
    Selection,
    /// The operations rows, in running order.
    Rows,
    /// The tunable parameters of one row, with their current values and limits.
    RowParams {
        /// The row.
        row: Id,
    },
    /// The tools the library offers for an operation kind.
    Tools {
        /// The kind.
        kind: Kind,
    },
    /// The materials the library offers.
    Materials,
    /// The machine profiles the library offers.
    Machines,
    /// The rules that map imported colours and layers to operations.
    Rules,
    /// The problems found in the design and in the compile.
    Problems,
    /// How much of the mat the design uses.
    Bounds {
        /// The objects; the whole mat when omitted.
        objects: Option<Vec<Id>>,
    },
}

/// `settings`: application preferences.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Settings {
    /// Reads all settings as JSON.
    Get,
    /// Merges the given fields into the settings.
    Set {
        /// A JSON object holding the fields to change.
        values: serde_json::Value,
    },
    /// Calibrates true scale from a measured length of the 100 mm bar.
    CalibrateScreen {
        /// The length measured, in millimetres.
        measured_mm: f64,
    },
}
