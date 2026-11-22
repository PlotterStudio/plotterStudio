//! Canvas tools: each tool turns pointer events into commands.
//!
//! Tools never edit the document directly. During a drag a tool shows a preview overlay; on
//! release it commits one transaction, so a gesture is one undo step. The tools here are select
//! and move, rectangle and ellipse; node editing, the pen and the remaining shape tools build on
//! the same trait.

use std::sync::Arc;

use ps_core::{MatId, ObjectId, PathKey};
use ps_geometry::{Affine, FillRule, Path, Point, Rect, Vec2, flatten};
use ps_model::{
    AddObject, Command as ModelCommand, PlacedObject, SetPathGeometry, SetStartPoint, SetTransform,
    ShapePath, SourceStyle, SplitPath,
};
use ps_render::{BoxMode, Pickable, flatten_tolerance_mm, pick, pick_in_box};

use crate::commands::Mods;
use crate::selection::Item;

/// The smallest rectangle or ellipse a drag may create, in millimetres.
const MIN_SIZE_MM: f64 = 0.1;
/// A press and release closer than this many screen pixels is a click, not a drag.
const CLICK_PX: f64 = 3.0;
/// The control-point distance that makes four cubic arcs closest to a circle.
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// A pointer or key event on the canvas, in world millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CanvasEvent {
    /// The primary button went down.
    Press(Point),
    /// The pointer moved with the button down.
    Drag(Point),
    /// The primary button went up.
    Release(Point),
    /// The Escape key.
    Escape,
    /// The Enter key.
    Enter,
    /// The pointer moved with no button down.
    Hover(Point),
    /// A double click.
    DoubleClick(Point),
}

/// What a tool wants done after an event.
pub enum ToolOutcome {
    /// Nothing to do.
    Idle,
    /// The overlay changed; redraw.
    Preview,
    /// The gesture was abandoned.
    Cancel,
    /// Make exactly these objects the selection.
    Select(Vec<ObjectId>),
    /// Add these objects to the selection.
    Add(Vec<ObjectId>),
    /// Toggle one object in the selection.
    Toggle(ObjectId),
    /// Select one node of one path.
    SelectNode(Item),
    /// Apply these commands as one undo step.
    Commit(Vec<Box<dyn ModelCommand>>),
}

impl std::fmt::Debug for ToolOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => f.write_str("Idle"),
            Self::Preview => f.write_str("Preview"),
            Self::Cancel => f.write_str("Cancel"),
            Self::Select(ids) => f.debug_tuple("Select").field(ids).finish(),
            Self::Add(ids) => f.debug_tuple("Add").field(ids).finish(),
            Self::Toggle(id) => f.debug_tuple("Toggle").field(id).finish(),
            Self::SelectNode(item) => f.debug_tuple("SelectNode").field(item).finish(),
            Self::Commit(cmds) => f
                .debug_tuple("Commit")
                .field(&cmds.iter().map(|c| c.label()).collect::<Vec<_>>())
                .finish(),
        }
    }
}

/// A shape drawn over the canvas while a tool is active.
#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    /// A polyline in world millimetres.
    Polyline {
        /// The vertices.
        points: Vec<Point>,
        /// Whether the last vertex joins the first.
        closed: bool,
    },
    /// A selection marquee; `enclosed` means only whole objects are taken.
    Marquee {
        /// The box in world millimetres.
        rect: Rect,
        /// Whether objects must be wholly inside (a left-to-right drag).
        enclosed: bool,
    },
    /// A ring marking the one point a click would act on.
    Marker {
        /// Where, in world millimetres.
        at: Point,
    },
    /// Bezier handles: a line from a node to each of its curve handles, with a dot at the end.
    Arms {
        /// Each node and the handle that hangs from it, in world millimetres.
        arms: Vec<(Point, Point)>,
    },
    /// Node handles of the paths being edited, with the node being dragged.
    Handles {
        /// Where the nodes are, in world millimetres.
        points: Vec<Point>,
        /// The node being dragged, if any.
        selected: Option<usize>,
    },
}

/// The pointer shapes tools ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    /// The ordinary arrow.
    Arrow,
    /// A crosshair for drawing.
    Crosshair,
    /// Moving the selection.
    Move,
    /// Scaling the selection by this handle.
    Resize(Handle),
    /// Rotating the selection.
    Rotate,
}

/// What a tool may look at.
#[derive(Debug, Clone)]
pub struct ToolCtx {
    /// The mat being edited.
    pub mat: MatId,
    /// The id a new object gets.
    pub new_object: ObjectId,
    /// The key a new path gets.
    pub new_path: PathKey,
    /// Pixels per millimetre.
    pub zoom: f64,
    /// Everything that can be picked, bottom to top, in world coordinates.
    pub pickables: Vec<Pickable<ObjectId>>,
    /// The selected objects and their current transforms.
    pub selected: Vec<(ObjectId, Affine)>,
    /// How many sides a polygon has and how many points a star has.
    pub sides: u32,
    /// A star's inner radius as a fraction of its outer radius.
    pub star_ratio: f64,
    /// The paths of the selected objects, for node editing.
    pub edit_paths: Vec<EditPath>,
    /// The guides on the mat.
    pub guides: Vec<ps_model::Guide>,
    /// Whether moving and scaling snap to the grid, guides and other objects.
    pub snapping: bool,
    /// The mat's width and height in millimetres.
    pub mat_size: (f64, f64),
}

/// A path of a selected object, as node editing sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct EditPath {
    /// The object.
    pub object: ObjectId,
    /// The path.
    pub path: PathKey,
    /// Its geometry in the object's own space.
    pub geom: Path,
    /// The object-to-mat transform.
    pub transform: Affine,
}

/// One canvas tool.
pub trait Tool {
    /// Which tool this is.
    fn id(&self) -> ToolId;
    /// Handles an event.
    fn on_event(&mut self, event: &CanvasEvent, mods: Mods, ctx: &ToolCtx) -> ToolOutcome;
    /// Appends the handles, rubber bands and previews to draw.
    fn overlay(&self, out: &mut Vec<Overlay>);
    /// The objects a gesture in progress is moving, and the transform it applies to them so far;
    /// the canvas draws them transformed, as they will be once the gesture is committed.
    fn live(&self) -> Option<(Vec<ObjectId>, Affine)> {
        None
    }
    /// The bounds a gesture in progress started from.
    fn live_bounds(&self) -> Option<Rect> {
        None
    }
    /// The pointer shape for the tool's current state.
    fn pointer(&self) -> Cursor {
        self.id().cursor()
    }
}

/// The tools the registry can switch to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolId {
    /// Select and move.
    Select,
    /// Draw rectangles.
    Rectangle,
    /// Draw ellipses.
    Ellipse,
    /// Draw straight lines.
    Line,
    /// Draw regular polygons.
    Polygon,
    /// Draw stars.
    Star,
    /// Draw arcs through three points.
    Arc,
    /// Draw paths node by node.
    Pen,
    /// Edit the nodes of the selected paths.
    Node,
    /// Draw arcs from their centre.
    ArcCenter,
    /// Cut the selected shapes along a line.
    Knife,
    /// Place and remove guide lines.
    Guide,
    /// Choose where a closed path starts.
    StartPoint,
}

impl ToolId {
    /// Every tool.
    pub const ALL: [Self; 13] = [
        Self::Select,
        Self::Node,
        Self::Rectangle,
        Self::Ellipse,
        Self::Line,
        Self::Polygon,
        Self::Star,
        Self::Arc,
        Self::ArcCenter,
        Self::Pen,
        Self::Knife,
        Self::Guide,
        Self::StartPoint,
    ];

    /// The id of the command that switches to this tool.
    #[must_use]
    pub fn command_id(self) -> &'static str {
        match self {
            Self::Select => "tool.select",
            Self::Rectangle => "tool.rectangle",
            Self::Ellipse => "tool.ellipse",
            Self::Line => "tool.line",
            Self::Polygon => "tool.polygon",
            Self::Star => "tool.star",
            Self::Arc => "tool.arc",
            Self::Pen => "tool.pen",
            Self::Node => "tool.node",
            Self::ArcCenter => "tool.arc-center",
            Self::Knife => "tool.knife",
            Self::Guide => "tool.guide",
            Self::StartPoint => "tool.start-point",
        }
    }

    /// The tool a command id switches to.
    #[must_use]
    pub fn from_command(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.command_id() == id)
    }

    /// The pointer shape while this tool is active.
    #[must_use]
    pub fn cursor(self) -> Cursor {
        match self {
            Self::Select | Self::Node => Cursor::Arrow,
            Self::Rectangle
            | Self::Ellipse
            | Self::Line
            | Self::Polygon
            | Self::Star
            | Self::Arc
            | Self::ArcCenter
            | Self::Knife
            | Self::Guide
            | Self::StartPoint
            | Self::Pen => Cursor::Crosshair,
        }
    }

    /// A fresh tool.
    #[must_use]
    pub fn new_tool(self) -> Box<dyn Tool> {
        match self {
            Self::Select => Box::new(SelectTool::default()),
            Self::Rectangle => Box::new(RectangleTool::default()),
            Self::Ellipse => Box::new(EllipseTool::default()),
            Self::Line => Box::new(LineTool::default()),
            Self::Polygon => Box::new(PolygonTool::default()),
            Self::Star => Box::new(StarTool::default()),
            Self::Arc => Box::new(ArcTool::default()),
            Self::Pen => Box::new(PenTool::default()),
            Self::Node => Box::new(NodeTool::default()),
            Self::ArcCenter => Box::new(ArcCenterTool::default()),
            Self::Knife => Box::new(KnifeTool::default()),
            Self::Guide => Box::new(GuideTool::default()),
            Self::StartPoint => Box::new(StartPointTool::default()),
        }
    }
}

// ---- shape drawing ----

/// A drag box from `start` to `current`: Shift makes it square, Alt draws it from the centre.
fn drag_box(start: Point, current: Point, mods: Mods) -> Rect {
    let (mut dx, mut dy) = (current.x - start.x, current.y - start.y);
    if mods.shift {
        let side = dx.abs().max(dy.abs());
        dx = if dx < 0.0 { -side } else { side };
        dy = if dy < 0.0 { -side } else { side };
    }
    if mods.alt {
        Rect::new(
            start.x - dx.abs(),
            start.y - dy.abs(),
            start.x + dx.abs(),
            start.y + dy.abs(),
        )
    } else {
        Rect::new(
            start.x.min(start.x + dx),
            start.y.min(start.y + dy),
            start.x.max(start.x + dx),
            start.y.max(start.y + dy),
        )
    }
}

fn new_object(ctx: &ToolCtx, bounds: Rect, geom: Path) -> Box<dyn ModelCommand> {
    Box::new(AddObject {
        mat: ctx.mat,
        object: PlacedObject {
            id: ctx.new_object,
            source: None,
            transform: Affine::translate((bounds.x0, bounds.y0)),
            group: None,
            attach: None,
            paths: Arc::new(vec![Arc::new(ShapePath {
                key: ctx.new_path.clone(),
                geom,
                fill_rule: FillRule::NonZero,
                style: SourceStyle::default(),
                ops: Vec::new(),
                start_locked: false,
            })]),
            locked: false,
            hidden: false,
        },
    })
}

pub(crate) fn ellipse_path(w: f64, h: f64) -> Path {
    let (rx, ry) = (w / 2.0, h / 2.0);
    let (cx, cy) = (rx, ry);
    let (kx, ky) = (KAPPA * rx, KAPPA * ry);
    Path::new(Point::new(cx + rx, cy))
        .cubic_to(
            Point::new(cx + rx, cy + ky),
            Point::new(cx + kx, cy + ry),
            Point::new(cx, cy + ry),
        )
        .cubic_to(
            Point::new(cx - kx, cy + ry),
            Point::new(cx - rx, cy + ky),
            Point::new(cx - rx, cy),
        )
        .cubic_to(
            Point::new(cx - rx, cy - ky),
            Point::new(cx - kx, cy - ry),
            Point::new(cx, cy - ry),
        )
        .cubic_to(
            Point::new(cx + kx, cy - ry),
            Point::new(cx + rx, cy - ky),
            Point::new(cx + rx, cy),
        )
        .closed()
}

/// Which closed shape a box-drag tool makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Rectangle,
    Ellipse,
}

#[derive(Debug, Default)]
struct BoxDrag {
    anchor: Option<(Point, Point)>,
}

impl BoxDrag {
    fn on_event(
        &mut self,
        shape: Shape,
        event: &CanvasEvent,
        mods: Mods,
        ctx: &ToolCtx,
    ) -> ToolOutcome {
        match *event {
            CanvasEvent::Press(p) => {
                self.anchor = Some((p, p));
                ToolOutcome::Preview
            }
            CanvasEvent::Drag(p) => match &mut self.anchor {
                Some((_, current)) => {
                    *current = p;
                    ToolOutcome::Preview
                }
                None => ToolOutcome::Idle,
            },
            CanvasEvent::Release(p) => {
                let Some((start, _)) = self.anchor.take() else {
                    return ToolOutcome::Idle;
                };
                let b = drag_box(start, p, mods);
                if b.width() < MIN_SIZE_MM || b.height() < MIN_SIZE_MM {
                    return ToolOutcome::Cancel;
                }
                let geom = match shape {
                    Shape::Rectangle => Path::rect(0.0, 0.0, b.width(), b.height()),
                    Shape::Ellipse => ellipse_path(b.width(), b.height()),
                };
                ToolOutcome::Commit(vec![new_object(ctx, b, geom)])
            }
            CanvasEvent::Escape => {
                if self.anchor.take().is_some() {
                    ToolOutcome::Cancel
                } else {
                    ToolOutcome::Idle
                }
            }
            CanvasEvent::Enter | CanvasEvent::Hover(_) | CanvasEvent::DoubleClick(_) => {
                ToolOutcome::Idle
            }
        }
    }

    fn overlay(&self, shape: Shape, mods: Mods, out: &mut Vec<Overlay>) {
        let Some((start, current)) = self.anchor else {
            return;
        };
        let b = drag_box(start, current, mods);
        let points = match shape {
            Shape::Rectangle => vec![
                Point::new(b.x0, b.y0),
                Point::new(b.x1, b.y0),
                Point::new(b.x1, b.y1),
                Point::new(b.x0, b.y1),
            ],
            Shape::Ellipse => {
                let g = ellipse_path(b.width(), b.height());
                flatten(&g, 0.05)
                    .into_iter()
                    .map(|q| Point::new(q.x + b.x0, q.y + b.y0))
                    .collect()
            }
        };
        out.push(Overlay::Polyline {
            points,
            closed: true,
        });
    }
}

/// Draws rectangles by dragging a box.
#[derive(Debug, Default)]
pub struct RectangleTool {
    drag: BoxDrag,
    mods: Mods,
}

impl Tool for RectangleTool {
    fn id(&self) -> ToolId {
        ToolId::Rectangle
    }

    fn on_event(&mut self, event: &CanvasEvent, mods: Mods, ctx: &ToolCtx) -> ToolOutcome {
        self.mods = mods;
        self.drag.on_event(Shape::Rectangle, event, mods, ctx)
    }

    fn overlay(&self, out: &mut Vec<Overlay>) {
        self.drag.overlay(Shape::Rectangle, self.mods, out);
    }
}

/// Draws ellipses by dragging their bounding box.
#[derive(Debug, Default)]
pub struct EllipseTool {
    drag: BoxDrag,
    mods: Mods,
}

impl Tool for EllipseTool {
    fn id(&self) -> ToolId {
        ToolId::Ellipse
    }

    fn on_event(&mut self, event: &CanvasEvent, mods: Mods, ctx: &ToolCtx) -> ToolOutcome {
        self.mods = mods;
        self.drag.on_event(Shape::Ellipse, event, mods, ctx)
    }

    fn overlay(&self, out: &mut Vec<Overlay>) {
        self.drag.overlay(Shape::Ellipse, self.mods, out);
    }
}

// ---- line, polygon, star, arc and pen ----

/// Adds one object whose single path `geom` is in coordinates relative to `origin`.
fn object_at(ctx: &ToolCtx, origin: Point, geom: Path) -> Box<dyn ModelCommand> {
    new_object(ctx, Rect::new(origin.x, origin.y, origin.x, origin.y), geom)
}

/// The drag vector, with Shift snapping its angle to a multiple of 45 degrees.
fn constrained(delta: Vec2, mods: Mods) -> Vec2 {
    if !mods.shift {
        return delta;
    }
    let length = delta.hypot();
    if length <= f64::EPSILON {
        return delta;
    }
    let step = std::f64::consts::FRAC_PI_4;
    let angle = (delta.y.atan2(delta.x) / step).round() * step;
    Vec2::new(length * angle.cos(), length * angle.sin())
}

#[derive(Debug, Default)]
struct TwoPoint {
    anchor: Option<(Point, Point)>,
}

impl TwoPoint {
    fn press(&mut self, p: Point) -> ToolOutcome {
        self.anchor = Some((p, p));
        ToolOutcome::Preview
    }

    fn drag(&mut self, p: Point) -> ToolOutcome {
        match &mut self.anchor {
            Some((_, current)) => {
                *current = p;
                ToolOutcome::Preview
            }
            None => ToolOutcome::Idle,
        }
    }

    fn release(&mut self) -> Option<(Point, Point)> {
        self.anchor.take()
    }

    fn escape(&mut self) -> ToolOutcome {
        if self.anchor.take().is_some() {
            ToolOutcome::Cancel
        } else {
            ToolOutcome::Idle
        }
    }
}

/// Draws a straight line by dragging; Shift snaps the angle to 45 degrees.
#[derive(Debug, Default)]
pub struct LineTool {
    drag: TwoPoint,
    mods: Mods,
}

impl Tool for LineTool {
    fn id(&self) -> ToolId {
        ToolId::Line
    }

    fn on_event(&mut self, event: &CanvasEvent, mods: Mods, ctx: &ToolCtx) -> ToolOutcome {
        self.mods = mods;
        match *event {
            CanvasEvent::Press(p) => self.drag.press(p),
            CanvasEvent::Drag(p) => self.drag.drag(p),
            CanvasEvent::Release(p) => {
                let Some((start, _)) = self.drag.release() else {
                    return ToolOutcome::Idle;
                };
                let delta = constrained(p - start, mods);
                if delta.hypot() < MIN_SIZE_MM {
                    return ToolOutcome::Cancel;
                }
                let geom = Path::polyline(&[Point::ZERO, Point::new(delta.x, delta.y)], false);
                ToolOutcome::Commit(vec![object_at(ctx, start, geom)])
            }
            CanvasEvent::Escape => self.drag.escape(),
            CanvasEvent::Enter | CanvasEvent::Hover(_) | CanvasEvent::DoubleClick(_) => {
                ToolOutcome::Idle
            }
        }
    }

    fn overlay(&self, out: &mut Vec<Overlay>) {
        if let Some((start, current)) = self.drag.anchor {
            let end = start + constrained(current - start, self.mods);
            out.push(Overlay::Polyline {
                points: vec![start, end],
                closed: false,
            });
        }
    }
}

/// The corner points of a regular polygon or star about the origin.
pub(crate) fn radial_points(
    points: u32,
    outer: f64,
    inner: Option<f64>,
    first_angle: f64,
) -> Vec<Point> {
    let corners = if inner.is_some() { points * 2 } else { points };
    let step = std::f64::consts::TAU / f64::from(corners.max(1));
    (0..corners)
        .map(|k| {
            let radius = match inner {
                Some(r) if k % 2 == 1 => r,
                _ => outer,
            };
            let angle = first_angle + step * f64::from(k);
            Point::new(radius * angle.cos(), radius * angle.sin())
        })
        .collect()
}

/// A centre-drag tool: the drag sets the radius and where the first corner points.
#[derive(Debug, Default)]
struct RadialDrag {
    drag: TwoPoint,
    mods: Mods,
}

impl RadialDrag {
    fn event(
        &mut self,
        event: &CanvasEvent,
        mods: Mods,
        ctx: &ToolCtx,
        shape: impl Fn(f64, f64) -> Vec<Point>,
    ) -> ToolOutcome {
        self.mods = mods;
        match *event {
            CanvasEvent::Press(p) => self.drag.press(p),
            CanvasEvent::Drag(p) => self.drag.drag(p),
            CanvasEvent::Release(p) => {
                let Some((centre, _)) = self.drag.release() else {
                    return ToolOutcome::Idle;
                };
                let d = p - centre;
                let radius = d.hypot();
                if radius < MIN_SIZE_MM {
                    return ToolOutcome::Cancel;
                }
                let first = if mods.shift {
                    -std::f64::consts::FRAC_PI_2
                } else {
                    d.y.atan2(d.x)
                };
                ToolOutcome::Commit(vec![object_at(
                    ctx,
                    centre,
                    Path::polyline(&shape(radius, first), true),
                )])
            }
            CanvasEvent::Escape => self.drag.escape(),
            CanvasEvent::Enter | CanvasEvent::Hover(_) | CanvasEvent::DoubleClick(_) => {
                ToolOutcome::Idle
            }
        }
    }

    fn overlay(&self, out: &mut Vec<Overlay>, shape: impl Fn(f64, f64) -> Vec<Point>) {
        let Some((centre, current)) = self.drag.anchor else {
            return;
        };
        let d = current - centre;
        let radius = d.hypot();
        if radius <= f64::EPSILON {
            return;
        }
        let first = if self.mods.shift {
            -std::f64::consts::FRAC_PI_2
        } else {
            d.y.atan2(d.x)
        };
        out.push(Overlay::Polyline {
            points: shape(radius, first)
                .into_iter()
                .map(|q| Point::new(q.x + centre.x, q.y + centre.y))
                .collect(),
            closed: true,
        });
    }
}

/// Draws a regular polygon from its centre; the number of sides is a tool setting.
#[derive(Debug, Default)]
pub struct PolygonTool {
    inner: RadialDrag,
    sides: u32,
}

impl Tool for PolygonTool {
    fn id(&self) -> ToolId {
        ToolId::Polygon
    }

    fn on_event(&mut self, event: &CanvasEvent, mods: Mods, ctx: &ToolCtx) -> ToolOutcome {
        let sides = ctx.sides.max(3);
        self.sides = sides;
        self.inner
            .event(event, mods, ctx, |r, a| radial_points(sides, r, None, a))
    }

    fn overlay(&self, out: &mut Vec<Overlay>) {
        let sides = self.sides.max(3);
        self.inner
            .overlay(out, |r, a| radial_points(sides, r, None, a));
    }
}

/// Draws a star from its centre; the number of points and the inner radius are tool settings.
#[derive(Debug, Default)]
pub struct StarTool {
    inner: RadialDrag,
    points: u32,
    ratio: f64,
}

impl Tool for StarTool {
    fn id(&self) -> ToolId {
        ToolId::Star
    }

    fn on_event(&mut self, event: &CanvasEvent, mods: Mods, ctx: &ToolCtx) -> ToolOutcome {
        let points = ctx.sides.max(3);
        let ratio = ctx.star_ratio.clamp(0.05, 0.95);
        (self.points, self.ratio) = (points, ratio);
        self.inner.event(event, mods, ctx, |r, a| {
            radial_points(points, r, Some(r * ratio), a)
        })
    }

    fn overlay(&self, out: &mut Vec<Overlay>) {
        let (points, ratio) = (self.points.max(3), self.ratio.clamp(0.05, 0.95));
        self.inner
            .overlay(out, |r, a| radial_points(points, r, Some(r * ratio), a));
    }
}

/// Draws a circular arc by clicking its start, its end and a point it passes through.
#[derive(Debug, Default)]
pub struct ArcTool {
    points: Vec<Point>,
    hover: Option<Point>,
}

impl Tool for ArcTool {
    fn id(&self) -> ToolId {
        ToolId::Arc
    }

    fn on_event(&mut self, event: &CanvasEvent, _mods: Mods, ctx: &ToolCtx) -> ToolOutcome {
        match *event {
            CanvasEvent::Hover(p) | CanvasEvent::Drag(p) => {
                self.hover = Some(p);
                if self.points.is_empty() {
                    ToolOutcome::Idle
                } else {
                    ToolOutcome::Preview
                }
            }
            CanvasEvent::Release(p) => {
                self.points.push(p);
                if self.points.len() < 3 {
                    return ToolOutcome::Preview;
                }
                let (start, end, through) = (self.points[0], self.points[1], self.points[2]);
                self.points.clear();
                self.hover = None;
                let Some((centre, ccw)) = ps_geometry::arc::from_three_points(start, through, end)
                else {
                    return ToolOutcome::Cancel;
                };
                let rel = |q: Point| Point::new(q.x - start.x, q.y - start.y);
                let geom = Path::new(Point::ZERO).arc_to(rel(end), rel(centre), ccw);
                ToolOutcome::Commit(vec![object_at(ctx, start, geom)])
            }
            CanvasEvent::Escape => {
                self.hover = None;
                if std::mem::take(&mut self.points).is_empty() {
                    ToolOutcome::Idle
                } else {
                    ToolOutcome::Cancel
                }
            }
            CanvasEvent::Press(_) | CanvasEvent::Enter | CanvasEvent::DoubleClick(_) => {
                ToolOutcome::Idle
            }
        }
    }

    fn overlay(&self, out: &mut Vec<Overlay>) {
        if self.points.is_empty() {
            return;
        }
        let mut points = self.points.clone();
        if let Some(h) = self.hover {
            points.push(h);
        }
        out.push(Overlay::Polyline {
            points,
            closed: false,
        });
    }
}

/// Draws a circular arc from its centre: click the centre, click where the arc starts (which sets
/// the radius), then click towards where it ends; the arc takes the shorter way round.
#[derive(Debug, Default)]
pub(crate) struct ArcCenterTool {
    points: Vec<Point>,
    hover: Option<Point>,
}

impl Tool for ArcCenterTool {
    fn id(&self) -> ToolId {
        ToolId::ArcCenter
    }

    fn on_event(&mut self, event: &CanvasEvent, _mods: Mods, ctx: &ToolCtx) -> ToolOutcome {
        match *event {
            CanvasEvent::Hover(p) | CanvasEvent::Drag(p) => {
                self.hover = Some(p);
                if self.points.is_empty() {
                    ToolOutcome::Idle
                } else {
                    ToolOutcome::Preview
                }
            }
            CanvasEvent::Release(p) => {
                self.points.push(p);
                if self.points.len() < 3 {
                    return ToolOutcome::Preview;
                }
                let (centre, start, toward) = (self.points[0], self.points[1], self.points[2]);
                self.points.clear();
                self.hover = None;
                let radius = ps_geometry::arc::dist(centre, start);
                if radius < 1e-9 || ps_geometry::arc::dist(centre, toward) < 1e-9 {
                    return ToolOutcome::Cancel;
                }
                let a0 = ps_geometry::arc::angle_of(start, centre);
                let a1 = ps_geometry::arc::angle_of(toward, centre);
                let end = ps_geometry::arc::point_at(centre, radius, a1);
                let ccw = (a1 - a0).rem_euclid(std::f64::consts::TAU) <= std::f64::consts::PI;
                let rel = |q: Point| Point::new(q.x - start.x, q.y - start.y);
                let geom = Path::new(Point::ZERO).arc_to(rel(end), rel(centre), ccw);
                ToolOutcome::Commit(vec![object_at(ctx, start, geom)])
            }
            CanvasEvent::Escape => {
                self.hover = None;
                if std::mem::take(&mut self.points).is_empty() {
                    ToolOutcome::Idle
                } else {
                    ToolOutcome::Cancel
                }
            }
            CanvasEvent::Press(_) | CanvasEvent::Enter | CanvasEvent::DoubleClick(_) => {
                ToolOutcome::Idle
            }
        }
    }

    fn overlay(&self, out: &mut Vec<Overlay>) {
        if self.points.is_empty() {
            return;
        }
        let mut points = self.points.clone();
        if let Some(h) = self.hover {
            points.push(h);
        }
        out.push(Overlay::Polyline {
            points,
            closed: false,
        });
    }
}

/// Places guide lines: a click adds a horizontal guide at that height, Shift-click a vertical one
/// at that position, and a click on an existing guide removes it.
#[derive(Debug, Default)]
pub(crate) struct GuideTool {
    pressed: Option<Point>,
    shift: bool,
}

/// How close a click must be to a guide to remove it, in screen pixels.
const GUIDE_CLICK_PX: f64 = 4.0;

impl Tool for GuideTool {
    fn id(&self) -> ToolId {
        ToolId::Guide
    }

    fn on_event(&mut self, event: &CanvasEvent, mods: Mods, ctx: &ToolCtx) -> ToolOutcome {
        match *event {
            CanvasEvent::Press(p) => {
                self.pressed = Some(p);
                self.shift = mods.shift;
                ToolOutcome::Idle
            }
            CanvasEvent::Release(p) => {
                if self.pressed.take().is_none() {
                    return ToolOutcome::Idle;
                }
                let tolerance = GUIDE_CLICK_PX / ctx.zoom.max(1e-9);
                let mut guides = ctx.guides.clone();
                let near = |g: &ps_model::Guide| {
                    let at = match g.axis {
                        ps_model::GuideAxis::Horizontal => p.y,
                        ps_model::GuideAxis::Vertical => p.x,
                    };
                    (g.at_mm - at).abs() <= tolerance
                };
                if guides.iter().any(near) {
                    guides.retain(|g| !near(g));
                } else if self.shift {
                    guides.push(ps_model::Guide {
                        axis: ps_model::GuideAxis::Vertical,
                        at_mm: p.x,
                    });
                } else {
                    guides.push(ps_model::Guide {
                        axis: ps_model::GuideAxis::Horizontal,
                        at_mm: p.y,
                    });
                }
                ToolOutcome::Commit(vec![Box::new(ps_model::SetGuides {
                    mat: ctx.mat,
                    guides,
                })])
            }
            CanvasEvent::Escape => {
                self.pressed = None;
                ToolOutcome::Idle
            }
            CanvasEvent::Drag(_)
            | CanvasEvent::Hover(_)
            | CanvasEvent::Enter
            | CanvasEvent::DoubleClick(_) => ToolOutcome::Idle,
        }
    }

    fn overlay(&self, _out: &mut Vec<Overlay>) {}
}

/// Makes the point on a closed path's outline nearest a click the place its stroke starts.
#[derive(Debug, Default)]
pub(crate) struct StartPointTool {
    /// The outline point a click would choose, while the pointer is near an outline.
    hover: Option<Point>,
}

/// How near the outline, in screen pixels, a click must be to choose a start.
const START_PICK_PX: f64 = 24.0;
/// How near a corner, in screen pixels, a pick snaps to the corner itself.
const START_CORNER_SNAP_PX: f64 = 8.0;
/// Accuracy, in mm, of locating a point on a curved outline.
const START_TOL_MM: f64 = 0.005;

impl StartPointTool {
    /// The closed path whose outline passes nearest `at` (within reach), the start it would get
    /// in the path's own space, and where that start is on the mat: the nearest outline point,
    /// or a corner when one is close.
    fn target(ctx: &ToolCtx, at: Point) -> Option<(&EditPath, Point, Point)> {
        let (_, path, local) = ctx
            .edit_paths
            .iter()
            .filter(|t| t.geom.closed)
            .map(|t| {
                let local = t.transform.inverse() * at;
                let d = flatten(&t.geom, 0.05)
                    .iter()
                    .zip(flatten(&t.geom, 0.05).iter().cycle().skip(1))
                    .map(|(a, b)| ps_geometry::arc::nearest_on_line(*a, *b, local).1)
                    .fold(f64::INFINITY, f64::min);
                (d, t, local)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .filter(|(d, _, _)| d * ctx.zoom <= START_PICK_PX)?;
        let on_outline = path
            .geom
            .distance_of_nearest(local, START_TOL_MM)
            .and_then(|d| path.geom.sample_at(d))
            .map(|(point, _)| point)?;
        let corner = (0..ps_geometry::node_count(&path.geom))
            .filter_map(|i| ps_geometry::node_position(&path.geom, i))
            .min_by(|a, b| {
                ps_geometry::arc::dist(*a, local).total_cmp(&ps_geometry::arc::dist(*b, local))
            })
            .filter(|c| ps_geometry::arc::dist(*c, local) * ctx.zoom <= START_CORNER_SNAP_PX);
        let start = corner.unwrap_or(on_outline);
        Some((path, start, path.transform * start))
    }
}

impl Tool for StartPointTool {
    fn id(&self) -> ToolId {
        ToolId::StartPoint
    }

    fn on_event(&mut self, event: &CanvasEvent, _mods: Mods, ctx: &ToolCtx) -> ToolOutcome {
        match *event {
            CanvasEvent::Hover(at) => {
                self.hover = Self::target(ctx, at).map(|(_, _, corner)| corner);
                ToolOutcome::Preview
            }
            CanvasEvent::Press(at) => match Self::target(ctx, at) {
                Some((target, local, _)) => ToolOutcome::Commit(vec![Box::new(SetStartPoint {
                    mat: ctx.mat,
                    object: target.object,
                    path: target.path.clone(),
                    at: Some(local),
                })]),
                None => ToolOutcome::Idle,
            },
            _ => ToolOutcome::Idle,
        }
    }

    fn overlay(&self, out: &mut Vec<Overlay>) {
        if let Some(at) = self.hover {
            out.push(Overlay::Marker { at });
        }
    }
}

/// Cuts the selected shapes along a line dragged across them.
#[derive(Debug, Default)]
pub(crate) struct KnifeTool {
    from: Option<Point>,
    to: Option<Point>,
}

impl Tool for KnifeTool {
    fn id(&self) -> ToolId {
        ToolId::Knife
    }

    fn on_event(&mut self, event: &CanvasEvent, _mods: Mods, ctx: &ToolCtx) -> ToolOutcome {
        match *event {
            CanvasEvent::Press(p) => {
                self.from = Some(p);
                self.to = Some(p);
                ToolOutcome::Preview
            }
            CanvasEvent::Drag(p) => {
                if self.from.is_some() {
                    self.to = Some(p);
                    ToolOutcome::Preview
                } else {
                    ToolOutcome::Idle
                }
            }
            CanvasEvent::Release(p) => {
                let Some(from) = self.from.take() else {
                    return ToolOutcome::Idle;
                };
                self.to = None;
                let cuts = cut_commands(ctx, from, p);
                if cuts.is_empty() {
                    ToolOutcome::Cancel
                } else {
                    ToolOutcome::Commit(cuts)
                }
            }
            CanvasEvent::Escape => {
                self.to = None;
                if self.from.take().is_some() {
                    ToolOutcome::Cancel
                } else {
                    ToolOutcome::Idle
                }
            }
            CanvasEvent::Hover(_) | CanvasEvent::Enter | CanvasEvent::DoubleClick(_) => {
                ToolOutcome::Idle
            }
        }
    }

    fn overlay(&self, out: &mut Vec<Overlay>) {
        if let (Some(a), Some(b)) = (self.from, self.to) {
            out.push(Overlay::Polyline {
                points: vec![a, b],
                closed: false,
            });
        }
    }
}

/// The commands that cut every selected path the line from `a` to `b` crosses.
fn cut_commands(ctx: &ToolCtx, a: Point, b: Point) -> Vec<Box<dyn ModelCommand>> {
    let mut commands: Vec<Box<dyn ModelCommand>> = Vec::new();
    for target in &ctx.edit_paths {
        // The cut is made in the object's own space, so the pieces need no transform.
        let inverse = target.transform.inverse();
        let (la, lb) = (inverse * a, inverse * b);
        let Some(pieces) = ps_geometry::knife(&target.geom, la, lb) else {
            continue;
        };
        let pieces = pieces
            .into_iter()
            .enumerate()
            .map(|(i, geom)| {
                (
                    PathKey::source(format!("{}-{}-cut{i}", ctx.new_path, target.path)),
                    geom,
                )
            })
            .collect();
        commands.push(Box::new(SplitPath {
            mat: ctx.mat,
            object: target.object,
            path: target.path.clone(),
            pieces,
        }));
    }
    commands
}

#[derive(Debug, Clone, Copy)]
struct PenNode {
    pos: Point,
    out: Option<Vec2>,
}

/// Draws a path node by node: a click is a corner, a drag pulls out symmetric handles; Enter
/// finishes an open path, a click on the first node closes it, Escape discards it.
#[derive(Debug, Default)]
pub struct PenTool {
    nodes: Vec<PenNode>,
    pressed: bool,
    closing: bool,
    hover: Option<Point>,
}

/// How close to the first node a click closes the path, in screen pixels.
const CLOSE_PX: f64 = 6.0;

impl PenTool {
    fn segment(from: PenNode, to: PenNode) -> ps_geometry::Seg {
        if from.out.is_none() && to.out.is_none() {
            return ps_geometry::Seg::Line { to: to.pos };
        }
        let c1 = from.pos + from.out.unwrap_or(Vec2::ZERO);
        let c2 = to.pos - to.out.unwrap_or(Vec2::ZERO);
        ps_geometry::Seg::Cubic { c1, c2, to: to.pos }
    }

    fn path(&self, closed: bool) -> (Point, Path) {
        let origin = self.nodes[0].pos;
        let rel = |n: PenNode| PenNode {
            pos: Point::new(n.pos.x - origin.x, n.pos.y - origin.y),
            out: n.out,
        };
        let nodes: Vec<PenNode> = self.nodes.iter().copied().map(rel).collect();
        let shift = |s: ps_geometry::Seg| s;
        let mut path = Path::new(Point::ZERO);
        for pair in nodes.windows(2) {
            path.segs.push(shift(Self::segment(pair[0], pair[1])));
        }
        if closed && nodes.len() > 2 {
            let first = nodes[0];
            path.segs.push(Self::segment(nodes[nodes.len() - 1], first));
            path.closed = true;
        }
        (origin, path)
    }

    fn finish(&mut self, ctx: &ToolCtx, closed: bool) -> ToolOutcome {
        if self.nodes.len() < 2 {
            *self = Self::default();
            return ToolOutcome::Cancel;
        }
        let (origin, geom) = self.path(closed);
        *self = Self::default();
        ToolOutcome::Commit(vec![object_at(ctx, origin, geom)])
    }
}

impl Tool for PenTool {
    fn id(&self) -> ToolId {
        ToolId::Pen
    }

    fn on_event(&mut self, event: &CanvasEvent, _mods: Mods, ctx: &ToolCtx) -> ToolOutcome {
        match *event {
            CanvasEvent::Press(p) => {
                self.pressed = true;
                let near_first =
                    self.nodes.len() >= 3 && (p - self.nodes[0].pos).hypot() * ctx.zoom <= CLOSE_PX;
                if near_first {
                    self.closing = true;
                } else {
                    self.nodes.push(PenNode { pos: p, out: None });
                }
                ToolOutcome::Preview
            }
            CanvasEvent::Drag(p) => {
                self.hover = Some(p);
                if self.pressed
                    && !self.closing
                    && let Some(last) = self.nodes.last_mut()
                {
                    let handle = p - last.pos;
                    last.out = (handle.hypot() * ctx.zoom > 3.0).then_some(handle);
                }
                ToolOutcome::Preview
            }
            CanvasEvent::Release(_) => {
                self.pressed = false;
                if self.closing {
                    self.closing = false;
                    return self.finish(ctx, true);
                }
                ToolOutcome::Preview
            }
            CanvasEvent::Hover(p) => {
                self.hover = Some(p);
                if self.nodes.is_empty() {
                    ToolOutcome::Idle
                } else {
                    ToolOutcome::Preview
                }
            }
            CanvasEvent::Enter => {
                if self.nodes.is_empty() {
                    ToolOutcome::Idle
                } else {
                    self.finish(ctx, false)
                }
            }
            CanvasEvent::DoubleClick(_) => ToolOutcome::Idle,
            CanvasEvent::Escape => {
                let had = !self.nodes.is_empty();
                *self = Self::default();
                if had {
                    ToolOutcome::Cancel
                } else {
                    ToolOutcome::Idle
                }
            }
        }
    }

    fn overlay(&self, out: &mut Vec<Overlay>) {
        let Some(first) = self.nodes.first() else {
            return;
        };
        // The path as it will be, curves and all, with a rubber band to the pointer.
        let mut path = Path::new(first.pos);
        for pair in self.nodes.windows(2) {
            path.segs.push(Self::segment(pair[0], pair[1]));
        }
        if let (Some(last), Some(h)) = (self.nodes.last(), self.hover)
            && !self.pressed
        {
            path.segs
                .push(Self::segment(*last, PenNode { pos: h, out: None }));
        }
        out.push(Overlay::Polyline {
            points: flatten(&path, 0.05),
            closed: false,
        });
        // The handles pulled out of the nodes, both sides of each.
        let arms: Vec<(Point, Point)> = self
            .nodes
            .iter()
            .filter_map(|n| n.out.map(|h| (n.pos, h)))
            .flat_map(|(pos, h)| [(pos, pos + h), (pos, pos - h)])
            .collect();
        if !arms.is_empty() {
            out.push(Overlay::Arms { arms });
        }
    }
}

// ---- node editing ----

/// How close, in screen pixels, a press must be to a node to grab it.
const NODE_PX: f64 = 7.0;
/// How close, in screen pixels, a double click must be to a segment to add a node.
const SEGMENT_PX: f64 = 6.0;

#[derive(Debug, Clone)]
struct NodeDrag {
    path: usize,
    node: usize,
    /// `Some(true)` for a node's leaving curve handle, `Some(false)` for its arriving one; `None`
    /// when the node itself is being dragged.
    handle: Option<bool>,
    start: Point,
    current: Point,
    moved: bool,
    /// Alt held: the other side's handle stays put instead of staying in line.
    sharp: bool,
}

/// Edits the nodes of the selected paths: press a node to select and drag it, double-click a
/// segment to add a node. Conversion and deletion are commands on the selected node.
#[derive(Debug, Default)]
pub struct NodeTool {
    drag: Option<NodeDrag>,
    paths: Vec<EditPath>,
    zoom: f64,
    /// The node last pressed (path index, node), whose curve handles are shown.
    chosen: Option<(usize, usize)>,
}

impl NodeTool {
    fn world_nodes(path: &EditPath) -> Vec<Point> {
        (0..ps_geometry::node_count(&path.geom))
            .filter_map(|i| ps_geometry::node_position(&path.geom, i))
            .map(|q| path.transform * q)
            .collect()
    }

    fn to_local(path: &EditPath, world: Point) -> Point {
        path.transform.inverse() * world
    }

    /// Starts dragging a curve handle of the chosen node if `p` is on one; a handle is grabbed
    /// before any node under it.
    fn grab_handle(&mut self, p: Point, mods: Mods, ctx: &ToolCtx) -> bool {
        let radius = NODE_PX / ctx.zoom;
        let Some((pi, node)) = self.chosen else {
            return false;
        };
        let Some(path) = ctx.edit_paths.get(pi) else {
            return false;
        };
        let (incoming, outgoing) = ps_geometry::node_handles(&path.geom, node);
        for (handle, outward) in [(incoming, false), (outgoing, true)] {
            if let Some(h) = handle
                && (path.transform * h - p).hypot() <= radius
            {
                self.drag = Some(NodeDrag {
                    path: pi,
                    node,
                    handle: Some(outward),
                    start: p,
                    current: p,
                    moved: false,
                    sharp: mods.alt,
                });
                return true;
            }
        }
        false
    }

    fn command(ctx: &ToolCtx, path: &EditPath, geom: Path) -> Box<dyn ModelCommand> {
        Box::new(SetPathGeometry {
            mat: ctx.mat,
            object: path.object,
            path: path.path.clone(),
            geom,
        })
    }
}

impl Tool for NodeTool {
    fn id(&self) -> ToolId {
        ToolId::Node
    }

    fn on_event(&mut self, event: &CanvasEvent, mods: Mods, ctx: &ToolCtx) -> ToolOutcome {
        if self.paths.len() != ctx.edit_paths.len()
            || self
                .paths
                .iter()
                .zip(&ctx.edit_paths)
                .any(|(a, b)| a.object != b.object || a.path != b.path)
        {
            self.chosen = None;
        }
        self.paths.clone_from(&ctx.edit_paths);
        self.zoom = ctx.zoom;
        match *event {
            CanvasEvent::Press(p) => {
                let radius = NODE_PX / ctx.zoom;
                if self.grab_handle(p, mods, ctx) {
                    return ToolOutcome::Preview;
                }
                let hit = ctx.edit_paths.iter().enumerate().find_map(|(pi, path)| {
                    Self::world_nodes(path)
                        .iter()
                        .position(|q| (*q - p).hypot() <= radius)
                        .map(|node| (pi, node))
                });
                let Some((path, node)) = hit else {
                    return ToolOutcome::Idle;
                };
                self.drag = Some(NodeDrag {
                    path,
                    node,
                    handle: None,
                    start: p,
                    current: p,
                    moved: false,
                    sharp: false,
                });
                self.chosen = Some((path, node));
                let edit = &ctx.edit_paths[path];
                ToolOutcome::SelectNode(Item {
                    object: edit.object,
                    path: Some(edit.path.clone()),
                    node: u32::try_from(node).ok(),
                })
            }
            CanvasEvent::Drag(p) => match &mut self.drag {
                Some(d) => {
                    d.current = p;
                    d.moved |= (p - d.start).hypot() * ctx.zoom > CLICK_PX;
                    ToolOutcome::Preview
                }
                None => ToolOutcome::Idle,
            },
            CanvasEvent::Release(p) => {
                let Some(d) = self.drag.take() else {
                    return ToolOutcome::Idle;
                };
                if !(d.moved || (p - d.start).hypot() * ctx.zoom > CLICK_PX) {
                    return ToolOutcome::Idle;
                }
                let path = &ctx.edit_paths[d.path];
                let local = Self::to_local(path, p);
                let edited = match d.handle {
                    Some(outward) => {
                        ps_geometry::move_handle(&path.geom, d.node, outward, local, !d.sharp)
                    }
                    None => ps_geometry::move_node(&path.geom, d.node, local),
                };
                match edited {
                    Some(geom) => ToolOutcome::Commit(vec![Self::command(ctx, path, geom)]),
                    None => ToolOutcome::Cancel,
                }
            }
            CanvasEvent::DoubleClick(p) => {
                for path in &ctx.edit_paths {
                    let local = Self::to_local(path, p);
                    let Some(hit) = ps_geometry::nearest_on_path(&path.geom, local, 0.01) else {
                        continue;
                    };
                    if hit.distance * ctx.zoom <= SEGMENT_PX
                        && let Some(geom) = ps_geometry::insert_node(&path.geom, hit.segment, hit.t)
                    {
                        return ToolOutcome::Commit(vec![Self::command(ctx, path, geom)]);
                    }
                }
                ToolOutcome::Idle
            }
            CanvasEvent::Escape => {
                if self.drag.take().is_some() {
                    ToolOutcome::Cancel
                } else {
                    ToolOutcome::Idle
                }
            }
            CanvasEvent::Hover(_) | CanvasEvent::Enter => ToolOutcome::Idle,
        }
    }

    fn overlay(&self, out: &mut Vec<Overlay>) {
        for (index, path) in self.paths.iter().enumerate() {
            let dragged = self.drag.as_ref().filter(|d| d.path == index);
            let mut geom = path.geom.clone();
            if let Some(d) = dragged.filter(|d| d.moved) {
                let local = Self::to_local(path, d.current);
                let edited = match d.handle {
                    Some(outward) => {
                        ps_geometry::move_handle(&path.geom, d.node, outward, local, !d.sharp)
                    }
                    None => ps_geometry::move_node(&path.geom, d.node, local),
                };
                if let Some(edited) = edited {
                    geom = edited;
                    let tol = flatten_tolerance_mm(self.zoom.max(1e-6));
                    out.push(Overlay::Polyline {
                        points: flatten(&geom, tol)
                            .into_iter()
                            .map(|q| path.transform * q)
                            .collect(),
                        closed: geom.closed,
                    });
                }
            }
            let edited = EditPath {
                geom,
                ..path.clone()
            };
            if let Some((pi, node)) = self.chosen
                && pi == index
                && let Some(here) = ps_geometry::node_position(&edited.geom, node)
            {
                let (incoming, outgoing) = ps_geometry::node_handles(&edited.geom, node);
                let arms: Vec<(Point, Point)> = [incoming, outgoing]
                    .into_iter()
                    .flatten()
                    .map(|end| (path.transform * here, path.transform * end))
                    .collect();
                if !arms.is_empty() {
                    out.push(Overlay::Arms { arms });
                }
            }
            out.push(Overlay::Handles {
                points: Self::world_nodes(&edited),
                selected: dragged
                    .filter(|d| d.handle.is_none())
                    .map(|d| d.node)
                    .or_else(|| self.chosen.filter(|(pi, _)| *pi == index).map(|(_, n)| n)),
            });
        }
    }
}

// ---- select and move ----

mod select;
pub use select::{Handle, SelectTool};
pub(crate) use select::{selection_box, visible_handles};

#[cfg(test)]
mod tests;
