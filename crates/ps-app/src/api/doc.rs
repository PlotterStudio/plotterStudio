//! The `project`, `import`, `library` and `settings` tools: files, drawings, packages and
//! preferences.

use std::collections::BTreeSet;
use std::path::Path;

use ps_core::ObjectId;
use ps_geometry::Point;
use ps_library::LibraryKind as Kind;
use ps_model::{Document, ProjectMeta};
use serde_json::{Value, json};

use super::types::{Conflict, Import, Library, LibraryKind, Project, Settings};
use super::{ApiError, Reply, created, shapes};
use crate::clock::rfc3339_utc;
use crate::fonts::{BUNDLED_FAMILY, FontLibrary};
use crate::project_io::{open_project, save_project};
use crate::text_object::TextParams;
use crate::ui::App;

fn all_objects(app: &App) -> BTreeSet<ObjectId> {
    app.state()
        .document()
        .mats
        .values()
        .flat_map(|m| m.objects.keys().copied().collect::<Vec<_>>())
        .collect()
}

fn id_strings(ids: impl IntoIterator<Item = ObjectId>) -> Vec<String> {
    ids.into_iter().map(|id| id.to_string()).collect()
}

impl App {
    pub(super) fn project_tool(&mut self, op: Project, now_ms: u64) -> Reply {
        match op {
            Project::New => {
                let meta = ProjectMeta::new(
                    "Untitled",
                    &rfc3339_utc(now_ms / 1_000),
                    env!("CARGO_PKG_VERSION"),
                );
                self.state_mut().load_document(Document::new(meta));
                self.notice = None;
                Ok(json!({}))
            }
            Project::Open { path } => {
                let opened = open_project(Path::new(&path)).map_err(|e| {
                    ApiError::new("open_failed", e.to_string())
                        .hint("path must be a .pstudio project file")
                })?;
                self.state_mut().adopt_project(opened, Path::new(&path));
                self.notice = None;
                Ok(json!({}))
            }
            Project::Save { path } => {
                let target = path
                    .map(std::path::PathBuf::from)
                    .or_else(|| self.state().project_path().map(Path::to_path_buf));
                let Some(target) = target else {
                    return Err(ApiError::new("needs_path", "the project has no file yet")
                        .hint("pass `path` ending in .pstudio"));
                };
                save_project(self.state_mut(), &target)
                    .map_err(|e| ApiError::new("save_failed", e.to_string()))?;
                Ok(json!({ "path": target.display().to_string() }))
            }
            Project::Info => {
                let doc = self.state().document();
                Ok(json!({
                    "name": doc.meta.name,
                    "path": self.state().project_path().map(|p| p.display().to_string()),
                    "dirty": self.state().is_dirty(),
                    "mats": doc.mats.len(),
                }))
            }
        }
    }

    pub(super) fn import_tool(&mut self, op: Import, now_ms: u64) -> Reply {
        match op {
            Import::File { path } => {
                let before = all_objects(self);
                let report = crate::import::import_file(self.state_mut(), Path::new(&path), now_ms)
                    .map_err(|e| ApiError::new("import_failed", e.to_string()))?;
                let after = all_objects(self);
                let new = id_strings(after.difference(&before).copied());
                let mut reply = created(&new);
                reply["name"] = json!(report.name);
                reply["objects"] = json!(report.objects);
                reply["diagnostics"] = json!(
                    report
                        .diagnostics
                        .iter()
                        .map(|d| json!({"severity": format!("{:?}", d.severity), "key": d.key.to_string()}))
                        .collect::<Vec<_>>()
                );
                Ok(reply)
            }
            Import::Text {
                text,
                font,
                size_mm,
                at,
            } => {
                let params = TextParams {
                    text,
                    family: font.unwrap_or_else(|| BUNDLED_FAMILY.to_owned()),
                    size_mm,
                    ..TextParams::default()
                };
                let fonts = self.fonts.get_or_insert_with(FontLibrary::system);
                let result = self
                    .state
                    .add_text(&params, fonts, Point::new(at.x, at.y), now_ms);
                result.map_err(|e| ApiError::new("invalid_text", e))?;
                let ids = self.state().selection().objects();
                Ok(created(&id_strings(ids)))
            }
            Import::Shape { shape } => {
                let (geom, origin) = shapes::build(&shape)?;
                let id = self
                    .state_mut()
                    .add_shape(geom, origin, now_ms)
                    .ok_or_else(|| {
                        ApiError::refused("the shape could not be added", "check the numbers")
                    })?;
                Ok(created(&[id.to_string()]))
            }
        }
    }

    pub(super) fn library_tool(&mut self, op: Library) -> Reply {
        match op {
            Library::List { kind } => {
                let libs = self.state().libraries();
                let entries: Vec<Value> = match kind {
                    LibraryKind::Tool => libs
                        .tools()
                        .iter()
                        .map(|t| json!({"id": t.id, "name": t.name, "version": t.version.to_string()}))
                        .collect(),
                    LibraryKind::Material => libs
                        .materials()
                        .iter()
                        .map(|m| json!({"id": m.id, "name": m.name, "version": m.version.to_string()}))
                        .collect(),
                    LibraryKind::Machine => libs
                        .profiles()
                        .iter()
                        .map(|p| json!({"id": p.id, "name": p.name, "version": p.version.to_string()}))
                        .collect(),
                };
                Ok(json!({ "entries": entries }))
            }
            Library::Install { path, conflict } => {
                let bytes = std::fs::read(&path)
                    .map_err(|e| ApiError::new("install_failed", format!("{path}: {e}")))?;
                let choice = match conflict {
                    Conflict::Replace => ps_library::Choice::Replace,
                    Conflict::KeepBoth => ps_library::Choice::KeepBoth,
                    Conflict::Skip => ps_library::Choice::Skip,
                };
                let Some(manager) = self.library.as_mut() else {
                    return Err(ApiError::new(
                        "no_library_store",
                        "this session has no library folder to install into",
                    ));
                };
                let report = manager.install_package(&bytes, Some(choice), false);
                let snapshot = manager.snapshot();
                if !report.installed.is_empty()
                    && let Ok(snapshot) = snapshot
                {
                    self.state_mut().replace_libraries(snapshot);
                }
                Ok(json!({
                    "installed": report.installed,
                    "needs_answer": report.needs_answer,
                    "problems": report.problems,
                }))
            }
            Library::Uninstall { kind, id } => {
                let kind = match kind {
                    LibraryKind::Tool => Kind::Tool,
                    LibraryKind::Material => Kind::Material,
                    LibraryKind::Machine => Kind::Profile,
                };
                let Some(manager) = self.library.as_mut() else {
                    return Err(ApiError::new(
                        "no_library_store",
                        "this session has no library folder",
                    ));
                };
                manager
                    .uninstall(kind, &id)
                    .map_err(|e| ApiError::refused(e, "built-in entries cannot be removed"))?;
                let snapshot = manager.snapshot();
                if let Ok(snapshot) = snapshot {
                    self.state_mut().replace_libraries(snapshot);
                }
                Ok(json!({}))
            }
        }
    }

    pub(super) fn settings_tool(&mut self, op: Settings) -> Reply {
        match op {
            Settings::Get => serde_json::to_value(self.state().settings())
                .map_err(|e| ApiError::new("internal", e.to_string())),
            Settings::Set { values } => {
                let Value::Object(changes) = values else {
                    return Err(ApiError::new(
                        "bad_arguments",
                        "`values` must be a JSON object",
                    ));
                };
                let mut current = serde_json::to_value(self.state().settings())
                    .map_err(|e| ApiError::new("internal", e.to_string()))?;
                let Some(fields) = current.as_object_mut() else {
                    return Err(ApiError::new("internal", "settings are not an object"));
                };
                for (key, value) in changes {
                    if !fields.contains_key(&key) {
                        let known: Vec<_> = fields.keys().cloned().collect();
                        return Err(ApiError::new(
                            "bad_arguments",
                            format!("no setting named {key:?}"),
                        )
                        .hint(format!("settings: {}", known.join(", "))));
                    }
                    fields.insert(key, value);
                }
                let updated: ps_library::Settings = serde_json::from_value(current)
                    .map_err(|e| ApiError::new("bad_arguments", e.to_string()))?;
                self.state_mut().update_settings(|s| *s = updated);
                Ok(json!({}))
            }
            Settings::CalibrateScreen { measured_mm } => {
                if self.state_mut().calibrate_screen(measured_mm) {
                    Ok(json!({}))
                } else {
                    Err(ApiError::refused(
                        "that length cannot be used",
                        "measure the 100 mm bar and pass its length in millimetres",
                    ))
                }
            }
        }
    }
}
