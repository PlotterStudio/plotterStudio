//! The select tool: click and marquee selection, and the usual handles on the selection box. A
//! drag from the inside moves (with snapping), a drag from a square scales, and a drag from just
//! outside a corner rotates. While a gesture runs the tool reports a live transform, so the real
//! objects move on the canvas instead of a ghost of them.

use super::{
    Affine, BoxMode, CLICK_PX, CanvasEvent, Cursor, ModelCommand, Mods, ObjectId, Overlay, Point,
    Rect, SetTransform, Tool, ToolCtx, ToolId, ToolOutcome, Vec2, pick, pick_in_box,
};

fn screen_distance(a: Point, b: Point, zoom: f64) -> f64 {
    (a.x - b.x).hypot(a.y - b.y) * zoom
}

/// The distance the selection box stands off from the selection, in screen pixels.
pub(crate) const BOX_MARGIN_PX: f64 = 6.0;
/// How near a handle the pointer must be to take it, in screen pixels.
const HANDLE_HIT_PX: f64 = 7.0;
/// How far outside a corner handle a drag still rotates, in screen pixels.
const ROTATE_ZONE_PX: f64 = 24.0;
/// A box narrower than this (in screen pixels) has no side handles, so corners stay reachable.
const SIDE_HANDLES_MIN_PX: f64 = 36.0;
/// How near an edge or centre must be to a target to snap to it, in screen pixels.
const SNAP_PX: f64 = 8.0;
/// The grid the cursor snaps to, in millimetres, when it is wide enough to see.
const GRID_MM: f64 = 10.0;
/// The grid is only snapped to when its lines are at least this far apart on screen.
const MIN_GRID_PX: f64 = 8.0;
/// Rotation with Shift held moves in steps of this many radians (15 degrees).
const ROTATE_STEP: f64 = std::f64::consts::PI / 12.0;

/// The eight scale handles of a selection box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    /// Top left.
    NorthWest,
    /// Top middle.
    North,
    /// Top right.
    NorthEast,
    /// Right middle.
    East,
    /// Bottom right.
    SouthEast,
    /// Bottom middle.
    South,
    /// Bottom left.
    SouthWest,
    /// Left middle.
    West,
}

impl Handle {
    /// Every handle.
    pub const ALL: [Self; 8] = [
        Self::NorthWest,
        Self::North,
        Self::NorthEast,
        Self::East,
        Self::SouthEast,
        Self::South,
        Self::SouthWest,
        Self::West,
    ];

    /// Whether this is a corner.
    #[must_use]
    pub fn is_corner(self) -> bool {
        matches!(
            self,
            Self::NorthWest | Self::NorthEast | Self::SouthEast | Self::SouthWest
        )
    }

    /// Where the handle sits on `r`.
    #[must_use]
    pub fn point(self, r: Rect) -> Point {
        let (cx, cy) = (r.x0.midpoint(r.x1), r.y0.midpoint(r.y1));
        match self {
            Self::NorthWest => Point::new(r.x0, r.y0),
            Self::North => Point::new(cx, r.y0),
            Self::NorthEast => Point::new(r.x1, r.y0),
            Self::East => Point::new(r.x1, cy),
            Self::SouthEast => Point::new(r.x1, r.y1),
            Self::South => Point::new(cx, r.y1),
            Self::SouthWest => Point::new(r.x0, r.y1),
            Self::West => Point::new(r.x0, cy),
        }
    }

    /// Whether dragging this handle moves the left (-1), right (1) or neither (0) edge.
    fn x_side(self) -> i8 {
        match self {
            Self::NorthWest | Self::West | Self::SouthWest => -1,
            Self::NorthEast | Self::East | Self::SouthEast => 1,
            Self::North | Self::South => 0,
        }
    }

    /// Whether dragging this handle moves the top (-1), bottom (1) or neither (0) edge.
    fn y_side(self) -> i8 {
        match self {
            Self::NorthWest | Self::North | Self::NorthEast => -1,
            Self::SouthWest | Self::South | Self::SouthEast => 1,
            Self::East | Self::West => 0,
        }
    }
}

/// What a press on the selection box grabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grab {
    /// A scale handle.
    Scale(Handle),
    /// The rotation zone beside a corner.
    Rotate(Handle),
}

/// The selection box: the selection's bounds standing off by [`BOX_MARGIN_PX`].
#[must_use]
pub(crate) fn selection_box(bounds: Rect, zoom: f64) -> Rect {
    let m = BOX_MARGIN_PX / zoom;
    Rect::new(bounds.x0 - m, bounds.y0 - m, bounds.x1 + m, bounds.y1 + m)
}

/// The handles that are shown on `bounds` at `zoom`.
#[must_use]
pub(crate) fn visible_handles(bounds: Rect, zoom: f64) -> Vec<Handle> {
    let narrow =
        bounds.width() * zoom < SIDE_HANDLES_MIN_PX || bounds.height() * zoom < SIDE_HANDLES_MIN_PX;
    Handle::ALL
        .into_iter()
        .filter(|h| h.is_corner() || !narrow)
        .collect()
}

/// What the pointer at `p` would grab on the selection box of `bounds`.
#[must_use]
pub(crate) fn grab_at(bounds: Rect, p: Point, zoom: f64) -> Option<Grab> {
    let outer = selection_box(bounds, zoom);
    let dist = |h: Handle| {
        let q = h.point(outer);
        (q.x - p.x).hypot(q.y - p.y) * zoom
    };
    for h in visible_handles(bounds, zoom) {
        if dist(h) <= HANDLE_HIT_PX {
            return Some(Grab::Scale(h));
        }
    }
    let outside = p.x < outer.x0 || p.x > outer.x1 || p.y < outer.y0 || p.y > outer.y1;
    if outside {
        for h in Handle::ALL.into_iter().filter(|h| h.is_corner()) {
            if dist(h) <= ROTATE_ZONE_PX {
                return Some(Grab::Rotate(h));
            }
        }
    }
    None
}

#[derive(Debug, Default)]
enum SelectState {
    #[default]
    Idle,
    Pressed {
        start: Point,
        hit: ObjectId,
        shift: bool,
        moving: Vec<ObjectId>,
        bounds: Rect,
        delta: Vec2,
    },
    Scale {
        handle: Handle,
        start: Point,
        bounds: Rect,
        moving: Vec<ObjectId>,
        transform: Affine,
    },
    Rotate {
        start: Point,
        bounds: Rect,
        moving: Vec<ObjectId>,
        transform: Affine,
    },
    Marquee {
        start: Point,
        current: Point,
    },
}

/// Selects objects by click or marquee, moves them by dragging, and scales and rotates them from
/// the handles of the selection box.
#[derive(Debug, Default)]
pub struct SelectTool {
    state: SelectState,
    pickables: Vec<ps_render::Pickable<ObjectId>>,
    /// Lines the moving box may snap to: x positions, then y positions.
    targets: (Vec<f64>, Vec<f64>),
    /// The snap guides to draw for the current drag.
    guides: Vec<Overlay>,
    hover: Option<Grab>,
}

impl SelectTool {
    /// The bounds of the selected objects, from the pickables.
    fn selection_bounds(ctx: &ToolCtx) -> Option<Rect> {
        ctx.pickables
            .iter()
            .filter(|i| ctx.selected.iter().any(|(id, _)| *id == i.id))
            .map(|i| i.path.bounds())
            .reduce(|a, b| a.union(b))
    }

    /// Where a moving box may snap: the mat's edges, guides, the grid and other objects.
    fn collect_targets(&mut self, ctx: &ToolCtx) {
        let (mut xs, mut ys) = (vec![0.0, ctx.mat_size.0], vec![0.0, ctx.mat_size.1]);
        for g in &ctx.guides {
            match g.axis {
                ps_model::GuideAxis::Vertical => xs.push(g.at_mm),
                ps_model::GuideAxis::Horizontal => ys.push(g.at_mm),
            }
        }
        let mut seen: Vec<ObjectId> = Vec::new();
        for item in &self.pickables {
            if ctx.selected.iter().any(|(id, _)| *id == item.id) || seen.contains(&item.id) {
                continue;
            }
            seen.push(item.id);
            let b = self
                .pickables
                .iter()
                .filter(|i| i.id == item.id)
                .map(|i| i.path.bounds())
                .reduce(|a, b| a.union(b));
            if let Some(b) = b {
                xs.extend([b.x0, b.x0.midpoint(b.x1), b.x1]);
                ys.extend([b.y0, b.y0.midpoint(b.y1), b.y1]);
            }
        }
        self.targets = (xs, ys);
    }

    /// The nearest target within the snap distance of any of `values`: the shift that lands that
    /// value on it, where, and whether it is another object, the mat or a guide (which get a line
    /// drawn) rather than the grid (which snaps quietly).
    fn snap_axis(
        targets: &[f64],
        values: &[f64],
        zoom: f64,
        grid: bool,
    ) -> Option<(f64, f64, bool)> {
        let tol = SNAP_PX / zoom;
        let mut best: Option<(f64, f64, bool)> = None;
        for v in values {
            let mut consider = |t: f64, shown: bool| {
                let d = t - v;
                // A shape's edge wins a tie with the grid: that is the one worth showing.
                let better = best.is_none_or(|(b, _, was_shown)| {
                    d.abs() < b.abs() - 1e-9 || (shown && !was_shown && d.abs() <= b.abs() + 1e-9)
                });
                if d.abs() <= tol && better {
                    best = Some((d, t, shown));
                }
            };
            for t in targets {
                consider(*t, true);
            }
            if grid {
                consider((v / GRID_MM).round() * GRID_MM, false);
            }
        }
        best
    }

    fn snapping(mods: Mods, ctx: &ToolCtx) -> bool {
        ctx.snapping && !mods.command && !mods.alt
    }

    /// The move that carries `bounds` from a press at `start` to the pointer at `p`: constrained
    /// to an axis with Shift, snapped to the targets, and the guides that show what it snapped to.
    fn move_delta(
        &mut self,
        start: Point,
        p: Point,
        bounds: Rect,
        mods: Mods,
        ctx: &ToolCtx,
    ) -> Vec2 {
        let mut d = Vec2::new(p.x - start.x, p.y - start.y);
        if mods.shift {
            if d.x.abs() >= d.y.abs() {
                d.y = 0.0;
            } else {
                d.x = 0.0;
            }
        }
        self.guides.clear();
        if !Self::snapping(mods, ctx) {
            return d;
        }
        let grid = GRID_MM * ctx.zoom >= MIN_GRID_PX;
        let moved = bounds + d;
        let xs = [moved.x0, moved.x0.midpoint(moved.x1), moved.x1];
        let ys = [moved.y0, moved.y0.midpoint(moved.y1), moved.y1];
        if (d.x != 0.0 || !mods.shift)
            && let Some((shift, at, shown)) = Self::snap_axis(&self.targets.0, &xs, ctx.zoom, grid)
        {
            d.x += shift;
            if shown {
                self.guides.push(Overlay::Polyline {
                    points: vec![
                        Point::new(at, moved.y0.min(bounds.y0) - 6.0 / ctx.zoom),
                        Point::new(at, moved.y1.max(bounds.y1) + 6.0 / ctx.zoom),
                    ],
                    closed: false,
                });
            }
        }
        if (d.y != 0.0 || !mods.shift)
            && let Some((shift, at, shown)) = Self::snap_axis(&self.targets.1, &ys, ctx.zoom, grid)
        {
            d.y += shift;
            if shown {
                self.guides.push(Overlay::Polyline {
                    points: vec![
                        Point::new(moved.x0.min(bounds.x0) - 6.0 / ctx.zoom, at),
                        Point::new(moved.x1.max(bounds.x1) + 6.0 / ctx.zoom, at),
                    ],
                    closed: false,
                });
            }
        }
        d
    }

    /// The scale that drags `handle` of `bounds` by the pointer's move from `start` to `p`.
    fn scale_transform(
        &mut self,
        handle: Handle,
        start: Point,
        p: Point,
        bounds: Rect,
        mods: Mods,
        ctx: &ToolCtx,
    ) -> Affine {
        let centre = Point::new(bounds.x0.midpoint(bounds.x1), bounds.y0.midpoint(bounds.y1));
        let from_centre = mods.alt;
        let grid = GRID_MM * ctx.zoom >= MIN_GRID_PX;
        let snapping = Self::snapping(mods, ctx);
        let mut guides: Vec<Overlay> = Vec::new();
        let mut axis =
            |side: i8, lo: f64, hi: f64, c: f64, delta: f64, targets: &[f64], vertical: bool| {
                if side == 0 {
                    return 1.0;
                }
                let (moving, anchor) = if side > 0 {
                    (hi, if from_centre { c } else { lo })
                } else {
                    (lo, if from_centre { c } else { hi })
                };
                let mut at = moving + delta;
                if snapping
                    && let Some((shift, line, shown)) =
                        Self::snap_axis(targets, &[at], ctx.zoom, grid)
                {
                    {
                        at += shift;
                        if shown {
                            let (a, b) = if vertical {
                                (Point::new(line, bounds.y0), Point::new(line, bounds.y1))
                            } else {
                                (Point::new(bounds.x0, line), Point::new(bounds.x1, line))
                            };
                            guides.push(Overlay::Polyline {
                                points: vec![a, b],
                                closed: false,
                            });
                        }
                    }
                }
                let span = moving - anchor;
                if span.abs() < 1e-9 {
                    1.0
                } else {
                    let s = (at - anchor) / span;
                    if s.abs() < 1e-3 {
                        1e-3_f64.copysign(s)
                    } else {
                        s
                    }
                }
            };
        let targets = self.targets.clone();
        let mut sx = axis(
            handle.x_side(),
            bounds.x0,
            bounds.x1,
            centre.x,
            p.x - start.x,
            &targets.0,
            true,
        );
        let mut sy = axis(
            handle.y_side(),
            bounds.y0,
            bounds.y1,
            centre.y,
            p.y - start.y,
            &targets.1,
            false,
        );
        self.guides = guides;
        if mods.shift && handle.is_corner() {
            // Proportional: the axis that moved further decides.
            let s = if (sx - 1.0).abs() >= (sy - 1.0).abs() {
                sx
            } else {
                sy
            };
            (sx, sy) = (s, s);
        }
        let anchor = if from_centre {
            centre
        } else {
            let opposite = |side: i8, lo: f64, hi: f64| if side > 0 { lo } else { hi };
            Point::new(
                if handle.x_side() == 0 {
                    centre.x
                } else {
                    opposite(handle.x_side(), bounds.x0, bounds.x1)
                },
                if handle.y_side() == 0 {
                    centre.y
                } else {
                    opposite(handle.y_side(), bounds.y0, bounds.y1)
                },
            )
        };
        let to = Affine::translate(anchor.to_vec2());
        to * Affine::scale_non_uniform(sx, sy) * Affine::translate(-anchor.to_vec2())
    }

    fn rotate_transform(start: Point, p: Point, bounds: Rect, mods: Mods) -> Affine {
        let c = Point::new(bounds.x0.midpoint(bounds.x1), bounds.y0.midpoint(bounds.y1));
        let a0 = (start.y - c.y).atan2(start.x - c.x);
        let a1 = (p.y - c.y).atan2(p.x - c.x);
        let mut angle = a1 - a0;
        if mods.shift {
            angle = (angle / ROTATE_STEP).round() * ROTATE_STEP;
        }
        Affine::translate(c.to_vec2()) * Affine::rotate(angle) * Affine::translate(-c.to_vec2())
    }

    fn commit(ctx: &ToolCtx, moving: &[ObjectId], delta: Affine) -> ToolOutcome {
        let cmds: Vec<Box<dyn ModelCommand>> = ctx
            .selected
            .iter()
            .filter(|(id, _)| moving.contains(id))
            .map(|(id, t)| {
                Box::new(SetTransform {
                    mat: ctx.mat,
                    object: *id,
                    transform: delta * *t,
                }) as Box<dyn ModelCommand>
            })
            .collect();
        if cmds.is_empty() {
            ToolOutcome::Cancel
        } else {
            ToolOutcome::Commit(cmds)
        }
    }

    fn press(&mut self, p: Point, mods: Mods, ctx: &ToolCtx) -> ToolOutcome {
        self.pickables.clone_from(&ctx.pickables);
        let selection = Self::selection_bounds(ctx);
        let moving_all: Vec<ObjectId> = ctx.selected.iter().map(|(id, _)| *id).collect();
        if let Some(bounds) = selection
            && let Some(grab) = grab_at(bounds, p, ctx.zoom)
        {
            self.collect_targets(ctx);
            self.state = match grab {
                Grab::Scale(handle) => SelectState::Scale {
                    handle,
                    start: p,
                    bounds,
                    moving: moving_all,
                    transform: Affine::IDENTITY,
                },
                Grab::Rotate(_) => SelectState::Rotate {
                    start: p,
                    bounds,
                    moving: moving_all,
                    transform: Affine::IDENTITY,
                },
            };
            return ToolOutcome::Idle;
        }
        let Some(hit) = pick(&ctx.pickables, p, ctx.zoom) else {
            self.state = SelectState::Marquee {
                start: p,
                current: p,
            };
            return ToolOutcome::Idle;
        };
        let already = ctx.selected.iter().any(|(id, _)| *id == hit);
        let moving = if already { moving_all } else { vec![hit] };
        let bounds = ctx
            .pickables
            .iter()
            .filter(|i| moving.contains(&i.id))
            .map(|i| i.path.bounds())
            .reduce(|a, b| a.union(b))
            .unwrap_or_else(|| Rect::from_origin_size(p, (0.0, 0.0)));
        let ids = moving.clone();
        self.collect_targets_for(&ids, ctx);
        self.state = SelectState::Pressed {
            start: p,
            hit,
            shift: mods.shift,
            moving,
            bounds,
            delta: Vec2::ZERO,
        };
        if already || mods.shift {
            ToolOutcome::Idle
        } else {
            ToolOutcome::Select(vec![hit])
        }
    }

    /// Like [`Self::collect_targets`] when the moving objects are known by id.
    fn collect_targets_for(&mut self, moving: &[ObjectId], ctx: &ToolCtx) {
        let mut inner = ctx.clone();
        inner.selected = moving.iter().map(|id| (*id, Affine::IDENTITY)).collect();
        self.collect_targets(&inner);
    }
}

impl Tool for SelectTool {
    fn id(&self) -> ToolId {
        ToolId::Select
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one state machine over press, drag, release and escape reads best together"
    )]
    fn on_event(&mut self, event: &CanvasEvent, mods: Mods, ctx: &ToolCtx) -> ToolOutcome {
        match *event {
            CanvasEvent::Press(p) => self.press(p, mods, ctx),
            CanvasEvent::Drag(p) => {
                let mut state = std::mem::take(&mut self.state);
                let outcome = match &mut state {
                    SelectState::Pressed {
                        start,
                        bounds,
                        delta,
                        ..
                    } => {
                        if screen_distance(*start, p, ctx.zoom) > CLICK_PX || *delta != Vec2::ZERO {
                            *delta = self.move_delta(*start, p, *bounds, mods, ctx);
                            ToolOutcome::Preview
                        } else {
                            ToolOutcome::Idle
                        }
                    }
                    SelectState::Scale {
                        handle,
                        start,
                        bounds,
                        transform,
                        ..
                    } => {
                        *transform = self.scale_transform(*handle, *start, p, *bounds, mods, ctx);
                        ToolOutcome::Preview
                    }
                    SelectState::Rotate {
                        start,
                        bounds,
                        transform,
                        ..
                    } => {
                        *transform = Self::rotate_transform(*start, p, *bounds, mods);
                        ToolOutcome::Preview
                    }
                    SelectState::Marquee { current, .. } => {
                        *current = p;
                        ToolOutcome::Preview
                    }
                    SelectState::Idle => ToolOutcome::Idle,
                };
                self.state = state;
                outcome
            }
            CanvasEvent::Release(p) => {
                self.guides.clear();
                match std::mem::take(&mut self.state) {
                    SelectState::Idle => ToolOutcome::Idle,
                    SelectState::Pressed {
                        start,
                        hit,
                        shift,
                        moving,
                        bounds,
                        ..
                    } => {
                        if screen_distance(start, p, ctx.zoom) <= CLICK_PX {
                            return if shift {
                                ToolOutcome::Toggle(hit)
                            } else {
                                ToolOutcome::Select(vec![hit])
                            };
                        }
                        let d = self.move_delta(start, p, bounds, mods, ctx);
                        self.guides.clear();
                        Self::commit(ctx, &moving, Affine::translate(d))
                    }
                    SelectState::Scale {
                        handle,
                        start,
                        bounds,
                        moving,
                        ..
                    } => {
                        let t = self.scale_transform(handle, start, p, bounds, mods, ctx);
                        self.guides.clear();
                        Self::commit(ctx, &moving, t)
                    }
                    SelectState::Rotate {
                        start,
                        bounds,
                        moving,
                        ..
                    } => Self::commit(ctx, &moving, Self::rotate_transform(start, p, bounds, mods)),
                    SelectState::Marquee { start, .. } => {
                        if screen_distance(start, p, ctx.zoom) <= CLICK_PX {
                            return if mods.shift {
                                ToolOutcome::Idle
                            } else {
                                ToolOutcome::Select(Vec::new())
                            };
                        }
                        let enclosed = p.x >= start.x;
                        let rect = Rect::new(
                            start.x.min(p.x),
                            start.y.min(p.y),
                            start.x.max(p.x),
                            start.y.max(p.y),
                        );
                        let box_mode = if enclosed {
                            BoxMode::Enclosed
                        } else {
                            BoxMode::Crossing
                        };
                        let ids = pick_in_box(&ctx.pickables, rect, box_mode);
                        if mods.shift {
                            ToolOutcome::Add(ids)
                        } else {
                            ToolOutcome::Select(ids)
                        }
                    }
                }
            }
            CanvasEvent::Hover(p) => {
                self.hover = Self::selection_bounds(ctx).and_then(|b| grab_at(b, p, ctx.zoom));
                ToolOutcome::Idle
            }
            CanvasEvent::Enter | CanvasEvent::DoubleClick(_) => ToolOutcome::Idle,
            CanvasEvent::Escape => {
                let was_active = !matches!(self.state, SelectState::Idle);
                self.state = SelectState::Idle;
                self.guides.clear();
                if was_active {
                    ToolOutcome::Cancel
                } else {
                    ToolOutcome::Idle
                }
            }
        }
    }

    fn overlay(&self, out: &mut Vec<Overlay>) {
        if let SelectState::Marquee { start, current } = &self.state {
            out.push(Overlay::Marquee {
                rect: Rect::new(
                    start.x.min(current.x),
                    start.y.min(current.y),
                    start.x.max(current.x),
                    start.y.max(current.y),
                ),
                enclosed: current.x >= start.x,
            });
        }
        out.extend(self.guides.iter().cloned());
    }

    fn live(&self) -> Option<(Vec<ObjectId>, Affine)> {
        match &self.state {
            SelectState::Pressed { moving, delta, .. } if *delta != Vec2::ZERO => {
                Some((moving.clone(), Affine::translate(*delta)))
            }
            SelectState::Scale {
                moving, transform, ..
            }
            | SelectState::Rotate {
                moving, transform, ..
            } if *transform != Affine::IDENTITY => Some((moving.clone(), *transform)),
            _ => None,
        }
    }

    fn live_bounds(&self) -> Option<Rect> {
        match &self.state {
            SelectState::Pressed { bounds, .. }
            | SelectState::Scale { bounds, .. }
            | SelectState::Rotate { bounds, .. } => Some(*bounds),
            _ => None,
        }
    }

    fn pointer(&self) -> Cursor {
        let grab = match &self.state {
            SelectState::Scale { handle, .. } => Some(Grab::Scale(*handle)),
            SelectState::Rotate { .. } => Some(Grab::Rotate(Handle::NorthEast)),
            SelectState::Pressed { delta, .. } if *delta != Vec2::ZERO => return Cursor::Move,
            SelectState::Idle => self.hover,
            _ => None,
        };
        match grab {
            Some(Grab::Scale(h)) => Cursor::Resize(h),
            Some(Grab::Rotate(_)) => Cursor::Rotate,
            None => Cursor::Arrow,
        }
    }
}
