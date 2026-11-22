//! The `object`, `mat`, `operation` and `edit` tools: changing the design.
//!
//! Each call names its objects, mats and rows by id. Before acting, the call makes those objects
//! the window's selection (so the person watching sees what is being changed) and then runs the
//! same `AppState` method the window's buttons run.

use ps_core::ObjectId;
use ps_geometry::{Affine, FillSpec, Point};
use ps_model::{OpKind, SetStartPoint, SetTransform};
use serde_json::{Value, json};

use super::types::{
    Align, Anchor, Axis, Edit, Fill, Kind, Mat, NodeAction, Object, Operation, Ordering, Pattern,
};
use super::{ApiError, Reply, created, mat_id, object_id, row_id};
use crate::arrange::{AlignKind, Anchor as A, Axis as X};
use crate::ui::App;

fn anchor(a: Anchor) -> A {
    match a {
        Anchor::TopLeft => A::TopLeft,
        Anchor::Top => A::Top,
        Anchor::TopRight => A::TopRight,
        Anchor::Left => A::Left,
        Anchor::Centre => A::Centre,
        Anchor::Right => A::Right,
        Anchor::BottomLeft => A::BottomLeft,
        Anchor::Bottom => A::Bottom,
        Anchor::BottomRight => A::BottomRight,
    }
}

fn axis(a: Axis) -> X {
    match a {
        Axis::Horizontal => X::Horizontal,
        Axis::Vertical => X::Vertical,
    }
}

fn align(a: Align) -> AlignKind {
    match a {
        Align::Left => AlignKind::Left,
        Align::Right => AlignKind::Right,
        Align::Top => AlignKind::Top,
        Align::Bottom => AlignKind::Bottom,
        Align::CentreHorizontal => AlignKind::CentreHorizontal,
        Align::CentreVertical => AlignKind::CentreVertical,
    }
}

pub(super) fn op_kind(k: Kind) -> OpKind {
    match k {
        Kind::Cut => OpKind::Cut,
        Kind::KissCut => OpKind::KissCut,
        Kind::Draw => OpKind::Draw,
        Kind::Fill => OpKind::Fill,
        Kind::Score => OpKind::Score,
        Kind::Emboss => OpKind::Emboss,
    }
}

fn done(changed: bool, what: &str, hint: &str) -> Reply {
    if changed {
        Ok(json!({}))
    } else {
        Err(ApiError::refused(format!("{what} changed nothing"), hint))
    }
}

impl App {
    /// Resolves object ids, switches to their mat and selects them.
    pub(super) fn focus(&mut self, ids: &[String]) -> Result<Vec<ObjectId>, ApiError> {
        let mut objects = Vec::with_capacity(ids.len());
        let mut on = None;
        for text in ids {
            let id = object_id(text)?;
            let Some(mat) = self.state().mat_of_object(id) else {
                return Err(ApiError::new("unknown_object", format!("no object {text}"))
                    .hint("call `query` with what=objects to list the ids that exist"));
            };
            if on.is_some_and(|m| m != mat) {
                return Err(
                    ApiError::new("mixed_mats", "the objects are on different mats")
                        .hint("act on one mat's objects at a time"),
                );
            }
            on = Some(mat);
            objects.push(id);
        }
        if let Some(mat) = on {
            self.state_mut().set_current_mat(mat);
        }
        self.state_mut().select_objects(&objects);
        Ok(objects)
    }

    fn focus_some(&mut self, ids: &[String]) -> Result<Vec<ObjectId>, ApiError> {
        if ids.is_empty() {
            return Err(
                ApiError::new("bad_arguments", "`objects` must not be empty")
                    .hint("pass ids from `query` objects or from a `created` list"),
            );
        }
        self.focus(ids)
    }

    /// Switches to a mat by id; `None` keeps the current one.
    fn use_mat(&mut self, mat: Option<&str>) -> Result<(), ApiError> {
        let Some(text) = mat else {
            return Ok(());
        };
        let id = mat_id(text)?;
        if self.state().document().mat(id).is_none() {
            return Err(ApiError::new("unknown_mat", format!("no mat {text}"))
                .hint("call `query` with what=mats"));
        }
        self.state_mut().set_current_mat(id);
        Ok(())
    }

    /// The objects now selected that were not before: what an array or duplicate made.
    fn new_selection(&self, before: &[ObjectId]) -> Value {
        let fresh: Vec<String> = self
            .state()
            .selection()
            .objects()
            .into_iter()
            .filter(|o| !before.contains(o))
            .map(|o| o.to_string())
            .collect();
        created(&fresh)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one arm per operation of the tool; each arm is a few lines and splitting would only scatter the dispatch"
    )]
    pub(super) fn object_tool(&mut self, op: Object, now: u64) -> Reply {
        match op {
            Object::Select { objects } => {
                self.focus(&objects)?;
                Ok(json!({}))
            }
            Object::Delete { objects } => {
                let ids = self.focus_some(&objects)?;
                self.state_mut().run_command("edit.delete", now);
                let gone = ids
                    .iter()
                    .all(|id| self.state().mat_of_object(*id).is_none());
                done(gone, "delete", "the objects may be locked")
            }
            Object::Duplicate { objects } => {
                let before = self.focus_some(&objects)?;
                self.state_mut().run_command("edit.duplicate", now);
                Ok(self.new_selection(&before))
            }
            Object::Move { objects, dx, dy } => {
                let ids = self.focus_some(&objects)?;
                let mat = self.state().current_mat();
                let cmds: Vec<Box<dyn ps_model::Command>> = ids
                    .iter()
                    .filter_map(|id| mat.objects.get(id))
                    .map(|o| {
                        Box::new(SetTransform {
                            mat: mat.id,
                            object: o.id,
                            transform: Affine::translate((dx, dy)) * o.transform,
                        }) as Box<dyn ps_model::Command>
                    })
                    .collect();
                done(
                    self.state_mut().commit_all(&cmds, now),
                    "move",
                    "check the ids",
                )
            }
            Object::MoveTo {
                objects,
                anchor: a,
                x,
                y,
            } => {
                self.focus_some(&objects)?;
                let ok = self.state_mut().move_selection_to(anchor(a), x, y, now);
                done(ok, "move_to", "check the ids")
            }
            Object::Resize {
                objects,
                anchor: a,
                width,
                height,
                lock_aspect,
            } => {
                self.focus_some(&objects)?;
                let ok =
                    self.state_mut()
                        .resize_selection(anchor(a), width, height, lock_aspect, now);
                done(ok, "resize", "width and height must be positive")
            }
            Object::Rotate { objects, degrees } => {
                self.focus_some(&objects)?;
                let ok = self.state_mut().rotate_selection(degrees, now);
                done(ok, "rotate", "check the ids")
            }
            Object::Mirror { objects, axis: a } => {
                self.focus_some(&objects)?;
                let ok = self.state_mut().mirror_selection(axis(a), now);
                done(ok, "mirror", "check the ids")
            }
            Object::Align { objects, to } => {
                self.focus_some(&objects)?;
                let ok = self.state_mut().align_selection(align(to), now);
                done(
                    ok,
                    "align",
                    "align needs at least two objects that are not already aligned",
                )
            }
            Object::Distribute { objects, axis: a } => {
                self.focus_some(&objects)?;
                let ok = self.state_mut().distribute_selection(axis(a), now);
                done(ok, "distribute", "distribute needs at least three objects")
            }
            Object::GridArray {
                objects,
                columns,
                rows,
                dx,
                dy,
            } => {
                let before = self.focus_some(&objects)?;
                let ok = self
                    .state_mut()
                    .grid_array_selection(columns, rows, dx, dy, now);
                done(
                    ok,
                    "grid_array",
                    "columns and rows must be at least 1, and not both 1",
                )?;
                Ok(self.new_selection(&before))
            }
            Object::CircularArray {
                objects,
                count,
                total_degrees,
            } => {
                let before = self.focus_some(&objects)?;
                let ok = self
                    .state_mut()
                    .circular_array_selection(count, total_degrees, now);
                done(ok, "circular_array", "count must be at least 2")?;
                Ok(self.new_selection(&before))
            }
            Object::Node {
                object,
                path,
                node,
                action,
            } => {
                let id = object_id(&object)?;
                self.focus(&[object])?;
                if !self.state_mut().select_node(id, path, node) {
                    return Err(ApiError::new("unknown_node", "no such path of the object")
                        .hint("paths and nodes are numbered from 0"));
                }
                let command = match action {
                    NodeAction::Delete => "node.delete",
                    NodeAction::ToCurve => "node.to-curve",
                    NodeAction::ToLine => "node.to-line",
                    NodeAction::Smooth => "node.smooth",
                    NodeAction::Corner => "node.corner",
                };
                let before = self.state().generation();
                self.state_mut().run_command(command, now);
                let changed = self.state().generation() != before;
                // Leave the whole object selected, not a node that may be gone.
                self.state_mut().select_objects(&[id]);
                done(
                    changed,
                    "the node edit",
                    "check the node index and that the edit applies there",
                )
            }
            Object::SetStartPoint { object, path, near } => {
                let id = object_id(&object)?;
                self.focus(&[object])?;
                let mat = self.state().current_mat();
                let Some(placed) = mat.objects.get(&id) else {
                    return Err(ApiError::new("unknown_object", "the object is gone"));
                };
                let Some(shape) = placed.paths.get(path) else {
                    return Err(ApiError::new("unknown_path", "the object has no such path")
                        .hint("paths are numbered from 0"));
                };
                let local = placed.transform.inverse() * Point::new(near.x, near.y);
                let cmd: Box<dyn ps_model::Command> = Box::new(SetStartPoint {
                    mat: mat.id,
                    object: id,
                    path: shape.key.clone(),
                    at: Some(local),
                });
                done(
                    self.state_mut().commit_all(&[cmd], now),
                    "set_start_point",
                    "only closed paths have a start point",
                )
            }
            Object::Split { objects, from, to } => {
                let ids = self.focus_some(&objects)?;
                let ok = self.state_mut().split_objects(
                    &ids,
                    Point::new(from.x, from.y),
                    Point::new(to.x, to.y),
                    now,
                );
                done(ok, "split", "the line must cross the objects")
            }
        }
    }

    pub(super) fn mat_tool(&mut self, op: Mat, now: u64) -> Reply {
        match op {
            Mat::Add => {
                done(self.state_mut().add_mat(now), "add", "")?;
                Ok(created(&[self.state().current_mat_id().to_string()]))
            }
            Mat::AddRoll => {
                let ok = self.state_mut().add_matless_mat(now);
                done(ok, "add_roll", "the machine profile has no roll")?;
                Ok(created(&[self.state().current_mat_id().to_string()]))
            }
            Mat::Duplicate { mat } => {
                self.use_mat(mat.as_deref())?;
                done(self.state_mut().duplicate_mat(now), "duplicate", "")?;
                Ok(created(&[self.state().current_mat_id().to_string()]))
            }
            Mat::Remove { mat } => {
                self.use_mat(mat.as_deref())?;
                done(
                    self.state_mut().remove_current_mat(now),
                    "remove",
                    "the last mat cannot be removed",
                )
            }
            Mat::SetCurrent { mat } => self.use_mat(Some(&mat)).map(|()| json!({})),
            Mat::SetMaterial { mat, material } => {
                self.use_mat(mat.as_deref())?;
                let ok = self.state_mut().set_mat_material(material.as_deref(), now);
                done(
                    ok,
                    "set_material",
                    "call `query` with what=materials for references",
                )
            }
            Mat::SetMirror { mat, mirror } => {
                self.use_mat(mat.as_deref())?;
                let id = self.state().current_mat_id();
                let cmd: Box<dyn ps_model::Command> =
                    Box::new(ps_model::SetMirror { mat: id, mirror });
                done(
                    self.state_mut().commit_all(&[cmd], now),
                    "set_mirror",
                    "the mirror flag already had that value",
                )
            }
            Mat::AddGuide { axis: a, at_mm } => {
                let ok = self.state_mut().add_guide(guide_axis(a), at_mm, now);
                done(ok, "add_guide", "")
            }
            Mat::RemoveGuide {
                axis: a,
                at_mm,
                tolerance_mm,
            } => {
                let ok = self.state_mut().remove_guide_near(
                    guide_axis(a),
                    at_mm,
                    tolerance_mm.unwrap_or(1.0),
                    now,
                );
                done(ok, "remove_guide", "no guide within the tolerance")
            }
        }
    }

    fn row(&self, text: &str) -> Result<ps_core::OpRowId, ApiError> {
        let id = row_id(text)?;
        if self.state().operation_rows().iter().any(|r| r.id == id) {
            Ok(id)
        } else {
            Err(
                ApiError::new("unknown_row", format!("no operations row {text}"))
                    .hint("call `query` with what=rows"),
            )
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one arm per operation of the tool; each arm is a few lines and splitting would only scatter the dispatch"
    )]
    pub(super) fn operation_tool(&mut self, op: Operation, now: u64) -> Reply {
        match op {
            Operation::Assign {
                objects,
                kind,
                tool,
            } => {
                self.focus_some(&objects)?;
                let kind = op_kind(kind);
                if !self
                    .state()
                    .tools_for(kind)
                    .iter()
                    .any(|t| t.reference == tool)
                {
                    return Err(ApiError::refused(
                        format!("{tool:?} is not a tool for that operation"),
                        "call `query` with what=tools and the kind for valid tool references",
                    ));
                }
                let ok = self.state_mut().assign_operation(kind, &tool, now);
                done(
                    ok,
                    "assign",
                    "call `query` with what=tools and the kind for valid tool references",
                )?;
                let id = tool.split('@').next().unwrap_or_default().to_owned();
                let row = self
                    .state()
                    .operation_rows()
                    .into_iter()
                    .find(|r| {
                        r.kind == kind
                            && (r.tool == tool || r.tool.split('@').next() == Some(id.as_str()))
                    })
                    .map(|r| r.id.to_string());
                Ok(json!({ "row": row }))
            }
            Operation::Remove { row } => {
                let id = self.row(&row)?;
                done(self.state_mut().remove_row(id, now), "remove", "")
            }
            Operation::Enable { row, enabled } => {
                let id = self.row(&row)?;
                done(
                    self.state_mut().set_operation_row_enabled(id, enabled, now),
                    "enable",
                    "",
                )
            }
            Operation::Reorder { row, to } => {
                let id = self.row(&row)?;
                done(
                    self.state_mut().move_operation_row_to(id, to, now),
                    "reorder",
                    "the position must be within the rows",
                )
            }
            Operation::LockAfterPrevious { row, locked } => {
                let id = self.row(&row)?;
                done(
                    self.state_mut()
                        .set_row_locked_after_previous(id, locked, now),
                    "lock_after_previous",
                    "the first row has nothing before it",
                )
            }
            Operation::SetParam { row, name, value } => {
                let id = self.row(&row)?;
                done(
                    self.state_mut().set_row_param(id, &name, value, now),
                    "set_param",
                    "call `query` with what=row_params for the names and limits",
                )
            }
            Operation::ResetParam { row, name } => {
                let id = self.row(&row)?;
                let ok = match name {
                    Some(n) => self.state_mut().reset_row_param(id, &n, now),
                    None => self.state_mut().reset_row_params(id, now),
                };
                done(
                    ok,
                    "reset_param",
                    "the parameter was already at its default",
                )
            }
            Operation::SetPenColour { row, colour } => {
                let id = self.row(&row)?;
                done(
                    self.state_mut()
                        .set_row_pen_colour(id, colour.as_deref(), now),
                    "set_pen_colour",
                    "use a #rrggbb colour on a pen row",
                )
            }
            Operation::SetFill { row, fill } => {
                let id = self.row(&row)?;
                done(
                    self.state_mut().set_row_fill(id, fill_spec(fill), now),
                    "set_fill",
                    "only fill rows have a fill",
                )
            }
            Operation::SetJob {
                ordering,
                pen_check,
            } => {
                if let Some(mode) = ordering {
                    let mode = match mode {
                        Ordering::MinimiseToolChanges => {
                            ps_model::OrderingMode::MinimiseToolChanges
                        }
                        Ordering::StrictOrder => ps_model::OrderingMode::StrictOrder,
                    };
                    self.state_mut().set_ordering(mode, now);
                }
                if let Some(on) = pen_check {
                    self.state_mut().set_pen_check(on, now);
                }
                Ok(json!({}))
            }
            Operation::SetRules { rules } => {
                let parsed: Result<Vec<ps_model::MappingRule>, _> =
                    rules.into_iter().map(serde_json::from_value).collect();
                let parsed = parsed.map_err(|e| {
                    ApiError::new("bad_arguments", e.to_string())
                        .hint("call `query` with what=rules to see the shape of a rule")
                })?;
                self.state_mut()
                    .set_rules(parsed, now)
                    .map_err(|e| ApiError::refused(e.to_string(), "check the rules"))?;
                Ok(json!({}))
            }
            Operation::SetMachine { machine } => done(
                self.state_mut().set_machine(&machine),
                "set_machine",
                "call `query` with what=machines for ids",
            ),
        }
    }

    pub(super) fn edit_tool(&mut self, op: Edit, now: u64) -> Reply {
        match op {
            Edit::Undo => {
                if !self.state().can_undo() {
                    return Err(ApiError::refused("nothing to undo", "see `edit` history"));
                }
                self.state_mut().run_command("edit.undo", now);
                Ok(json!({}))
            }
            Edit::Redo => {
                if !self.state().can_redo() {
                    return Err(ApiError::refused("nothing to redo", "see `edit` history"));
                }
                self.state_mut().run_command("edit.redo", now);
                Ok(json!({}))
            }
            Edit::History => Ok(json!({
                "can_undo": self.state().can_undo(),
                "can_redo": self.state().can_redo(),
            })),
        }
    }
}

fn guide_axis(a: Axis) -> ps_model::GuideAxis {
    match a {
        Axis::Horizontal => ps_model::GuideAxis::Horizontal,
        Axis::Vertical => ps_model::GuideAxis::Vertical,
    }
}

fn fill_spec(f: Fill) -> FillSpec {
    FillSpec {
        pattern: match f.pattern {
            Pattern::Parallel => ps_geometry::Pattern::Parallel,
            Pattern::Cross => ps_geometry::Pattern::Cross,
            Pattern::ZigZag => ps_geometry::Pattern::ZigZag,
            Pattern::Concentric => ps_geometry::Pattern::Concentric,
        },
        angle_deg: f.angle_deg,
        overlap: f.overlap.unwrap_or(0.15),
        outline_pass: f.outline_pass,
    }
}
