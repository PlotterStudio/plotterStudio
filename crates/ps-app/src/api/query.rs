//! The `query` tool: one read-only entry point for everything a caller may want to know.

use ps_core::Diagnostic;
use ps_geometry::Rect;
use ps_model::PlacedObject;
use serde_json::{Value, json};

use super::edit::op_kind;
use super::types::Query;
use super::{ApiError, Reply, mat_id, object_id};
use crate::l10n::Arg;
use crate::ui::App;

/// A rectangle as `{x0, y0, x1, y1}`.
pub(super) fn rect_json(r: Rect) -> Value {
    json!({"x0": r.x0, "y0": r.y0, "x1": r.x1, "y1": r.y1})
}

/// The bounds of an object on the mat, from its paths.
fn object_bounds(o: &PlacedObject) -> Option<Rect> {
    o.paths
        .iter()
        .map(|p| p.geom.transformed(o.transform).bounds())
        .reduce(|a, b| a.union(b))
}

impl App {
    /// A diagnostic as JSON: severity, key, arguments and the message in the app's language.
    pub(super) fn diagnostic_json(&self, d: &Diagnostic) -> Value {
        let args: Vec<(&str, Arg<'_>)> = d
            .args
            .iter()
            .map(|(k, v)| (k.as_str(), Arg::Text(v.as_str())))
            .collect();
        json!({
            "severity": format!("{:?}", d.severity).to_lowercase(),
            "key": d.key.as_str(),
            "message": self.loc.tr_args(d.key.as_str(), &args),
        })
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one arm per operation of the tool; each arm is a few lines and splitting would only scatter the dispatch"
    )]
    pub(super) fn query_tool(&self, what: Query) -> Reply {
        let state = self.state();
        match what {
            Query::Document => {
                let doc = state.document();
                Ok(json!({
                    "name": doc.meta.name,
                    "path": state.project_path().map(|p| p.display().to_string()),
                    "dirty": state.is_dirty(),
                    "mats": doc.mats.len(),
                    "machine": state.current_machine_id(),
                    "ordering": format!("{:?}", state.ordering()),
                    "pen_check": state.pen_check(),
                }))
            }
            Query::Mats => {
                let mats: Vec<Value> = state
                    .mat_summaries()
                    .into_iter()
                    .map(|m| {
                        json!({"id": m.id.to_string(), "name": m.name, "objects": m.objects, "current": m.current})
                    })
                    .collect();
                Ok(json!({
                    "mats": mats,
                    "material": state.current_material_reference(),
                }))
            }
            Query::Objects { mat } => {
                let id = match mat {
                    Some(text) => mat_id(&text)?,
                    None => state.current_mat_id(),
                };
                let doc = state.document();
                let Some(mat) = doc.mat(id) else {
                    return Err(ApiError::new("unknown_mat", format!("no mat {id}"))
                        .hint("call `query` with what=mats"));
                };
                let objects: Vec<Value> = mat
                    .objects
                    .values()
                    .map(|o| {
                        let paths: Vec<Value> = o
                            .paths
                            .iter()
                            .map(|p| {
                                json!({
                                    "closed": p.geom.closed,
                                    "operations": p.ops.iter().map(|op| json!({
                                        "kind": op.kind.as_str(),
                                        "enabled": op.enabled,
                                        "row": op.row.to_string(),
                                    })).collect::<Vec<_>>(),
                                })
                            })
                            .collect();
                        json!({
                            "id": o.id.to_string(),
                            "bounds": object_bounds(o).map(rect_json),
                            "locked": o.locked,
                            "hidden": o.hidden,
                            "paths": paths,
                        })
                    })
                    .collect();
                Ok(json!({"mat": id.to_string(), "objects": objects}))
            }
            Query::Selection => {
                let ids: Vec<String> = state
                    .selection()
                    .objects()
                    .iter()
                    .map(ToString::to_string)
                    .collect();
                Ok(json!({"objects": ids}))
            }
            Query::Rows => {
                let rows: Vec<Value> = state
                    .operation_rows()
                    .into_iter()
                    .map(|r| {
                        json!({
                            "id": r.id.to_string(),
                            "kind": r.kind.as_str(),
                            "tool": r.tool,
                            "tool_name": r.tool_name,
                            "holder": r.holder,
                            "paths": r.paths,
                            "length_mm": r.length_mm,
                            "enabled": r.enabled,
                            "partly_enabled": r.partly,
                            "locked_after_previous": r.locked_after_previous,
                            "preset": r.preset,
                            "time_ms": r.time_ms,
                        })
                    })
                    .collect();
                Ok(json!({"rows": rows}))
            }
            Query::RowParams { row } => {
                let id = super::row_id(&row)?;
                if !state.operation_rows().iter().any(|r| r.id == id) {
                    return Err(
                        ApiError::new("unknown_row", format!("no operations row {row}"))
                            .hint("call `query` with what=rows"),
                    );
                }
                let params: Vec<Value> = state
                    .row_params(id)
                    .into_iter()
                    .map(|p| {
                        json!({
                            "name": p.name, "unit": p.unit, "min": p.min, "max": p.max,
                            "step": p.step, "value": p.value, "modified": p.modified,
                            "origin": format!("{:?}", p.origin).to_lowercase(),
                        })
                    })
                    .collect();
                Ok(json!({"params": params}))
            }
            Query::Tools { kind } => {
                let tools: Vec<Value> = state
                    .tools_for(op_kind(kind))
                    .into_iter()
                    .map(|t| json!({"reference": t.reference, "name": t.name}))
                    .collect();
                Ok(json!({"tools": tools}))
            }
            Query::Materials => {
                let materials: Vec<Value> = state
                    .material_choices()
                    .into_iter()
                    .map(|m| json!({"reference": m.reference, "name": m.name}))
                    .collect();
                Ok(json!({
                    "materials": materials,
                    "current": state.current_material_reference(),
                }))
            }
            Query::Machines => {
                let machines: Vec<Value> = state
                    .machine_choices()
                    .into_iter()
                    .map(|m| json!({"id": m.id, "name": m.name}))
                    .collect();
                Ok(json!({"machines": machines, "current": state.current_machine_id()}))
            }
            Query::Rules => {
                let rules: Vec<Value> = state
                    .document()
                    .rules
                    .iter()
                    .filter_map(|r| serde_json::to_value(r).ok())
                    .collect();
                Ok(json!({"rules": rules}))
            }
            Query::Problems => {
                let problems: Vec<Value> = state
                    .problems()
                    .iter()
                    .map(|d| self.diagnostic_json(d))
                    .collect();
                Ok(json!({"problems": problems}))
            }
            Query::Bounds { objects } => {
                let doc = state.document();
                let bounds = match objects {
                    Some(ids) => {
                        let mut all = Vec::new();
                        for text in &ids {
                            let id = object_id(text)?;
                            let found = doc.mats.values().find_map(|m| m.objects.get(&id));
                            let Some(o) = found else {
                                return Err(ApiError::new(
                                    "unknown_object",
                                    format!("no object {text}"),
                                )
                                .hint(
                                    "call `query` with what=objects to list the ids that exist",
                                ));
                            };
                            all.extend(object_bounds(o));
                        }
                        all.into_iter().reduce(|a, b| a.union(b))
                    }
                    None => state
                        .current_mat()
                        .objects
                        .values()
                        .filter_map(|o| object_bounds(o))
                        .reduce(|a, b| a.union(b)),
                };
                Ok(json!({"bounds": bounds.map(rect_json)}))
            }
        }
    }
}
