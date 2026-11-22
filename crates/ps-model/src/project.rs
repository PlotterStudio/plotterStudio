//! The project file: a `.pstudio` zip.
//!
//! ```text
//! project.json          schema_version, min_reader_version, template, document
//! assets/<blake3>.<ext> original imported files, stored once however many objects use them
//! thumbnails/mat-NN.png thumbnails for the file browser
//! libraries/...         optionally embedded tools, materials and profile ("export for sharing")
//! ```
//!
//! Already-compressed assets are stored uncompressed and everything else is deflated. The zip uses
//! fixed timestamps and a sorted entry order, so saving is deterministic. A file written by a newer
//! schema or one that needs a newer app opens read-only with a warning, and unknown fields survive
//! a round trip at every depth, so a re-save by an older app does not drop data. Migrations are
//! pure functions on `serde_json::Value`, chained from the file's version to the current one.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::sync::Arc;

use ps_core::{ContentHash, ZipError, ZipLimits, hash_bytes, is_safe_path, read_archive};
use semver::Version;
use serde_json::{Map, Value};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::Document;

/// The project layout version this build writes.
pub const SCHEMA_VERSION: u32 = 1;
/// The oldest app version that can read the files this build writes.
pub const MIN_READER_VERSION: &str = "0.1.0";

/// Extensions whose content is already compressed, so zip stores them as they are.
const STORED_EXTENSIONS: [&str; 8] = ["svgz", "pdf", "png", "jpg", "jpeg", "gz", "zip", "webp"];
const KNOWN_TOP_LEVEL: [&str; 4] = [
    "schema_version",
    "min_reader_version",
    "template",
    "document",
];

/// One step of the migration chain: upgrades a file by one schema version, in place.
pub type Migration = fn(&mut Value) -> Result<(), String>;

/// The migrations from each released schema to the next; step `i` upgrades version `i` to `i + 1`.
/// There are no released versions to migrate from yet.
pub const MIGRATIONS: [Migration; 0] = [];

/// Why a project could not be read or written.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProjectError {
    /// The zip was refused (not a zip, unsafe path, file outside the layout, size limits).
    #[error(transparent)]
    Zip(#[from] ZipError),
    /// There is no `project.json`.
    #[error("the file has no project.json")]
    MissingProject,
    /// `project.json` is malformed.
    #[error("project.json is malformed: {0}")]
    Json(String),
    /// The document refers to an asset the zip does not hold.
    #[error("the asset {0} is missing from the file")]
    MissingAsset(String),
    /// An asset's content does not match its hash.
    #[error("the asset {0} has been modified")]
    AssetMismatch(String),
    /// A migration failed or the chain has a gap.
    #[error("migrating from schema {from} failed: {message}")]
    Migration {
        /// The version being migrated from.
        from: u32,
        /// What went wrong.
        message: String,
    },
    /// The archive could not be built.
    #[error("could not build the file: {0}")]
    Write(String),
}

/// Why a project opens read-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOnly {
    /// The file was written by a newer schema than this build understands.
    NewerSchema {
        /// The file's schema version.
        found: u32,
        /// The newest this build supports.
        supported: u32,
    },
    /// The file says it needs a newer app.
    NewerApp {
        /// The version the file requires.
        needed: Version,
        /// The version running now.
        running: Version,
    },
}

/// A thumbnail of one mat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    /// The mat's 1-based position.
    pub mat_index: u32,
    /// PNG bytes.
    pub png: Vec<u8>,
}

/// A file embedded for sharing, under `libraries/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedFile {
    /// Path inside the zip, starting with `libraries/`.
    pub path: String,
    /// Content.
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    Key(String),
    Index(usize),
}

/// Fields the loaded file had that this build does not know, kept for the next save.
#[derive(Debug, Clone, Default, PartialEq)]
struct Unknown {
    top: Map<String, Value>,
    nested: Vec<(Vec<Seg>, Value)>,
}

/// A project: the document plus what the file carries beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    /// The document.
    pub document: Document,
    /// Whether this is a job template (opening one creates an untitled copy).
    pub template: bool,
    /// Mat thumbnails.
    pub thumbnails: Vec<Thumbnail>,
    /// Embedded libraries.
    pub embedded: Vec<EmbeddedFile>,
    unknown: Unknown,
}

/// A project read from a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// The project.
    pub project: Project,
    /// Set when the file must not be saved over.
    pub read_only: Option<ReadOnly>,
}

fn is_project_file(name: &str) -> bool {
    if !is_safe_path(name) {
        return false;
    }
    if name == "project.json" {
        return true;
    }
    if let Some(rest) = name.strip_prefix("assets/") {
        let Some((hash, ext)) = rest.split_once('.') else {
            return false;
        };
        return hash.len() == 64
            && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            && !ext.is_empty()
            && ext.len() <= 8
            && ext.bytes().all(|b| b.is_ascii_alphanumeric());
    }
    if let Some(rest) = name.strip_prefix("thumbnails/mat-") {
        return rest
            .strip_suffix(".png")
            .is_some_and(|n| n.len() >= 2 && n.bytes().all(|b| b.is_ascii_digit()));
    }
    name.starts_with("libraries/") && name.len() > "libraries/".len()
}

fn asset_path(hash: &ContentHash, name: &str) -> String {
    let ext = name
        .rsplit_once('.')
        .map_or("bin", |(_, e)| e)
        .to_ascii_lowercase();
    format!("assets/{hash}.{ext}")
}

fn json_err(message: impl std::fmt::Display) -> ProjectError {
    ProjectError::Json(message.to_string())
}

// ---- unknown-field preservation ----

fn collect_unknown(
    raw: &Value,
    typed: &Value,
    path: &mut Vec<Seg>,
    out: &mut Vec<(Vec<Seg>, Value)>,
) {
    match (raw, typed) {
        (Value::Object(raw_map), Value::Object(typed_map)) => {
            for (key, raw_value) in raw_map {
                path.push(Seg::Key(key.clone()));
                match typed_map.get(key) {
                    Some(typed_value) => collect_unknown(raw_value, typed_value, path, out),
                    None => out.push((path.clone(), raw_value.clone())),
                }
                path.pop();
            }
        }
        (Value::Array(raw_items), Value::Array(typed_items)) => {
            for (i, (raw_item, typed_item)) in raw_items.iter().zip(typed_items).enumerate() {
                path.push(Seg::Index(i));
                collect_unknown(raw_item, typed_item, path, out);
                path.pop();
            }
        }
        _ => {}
    }
}

/// Follows `path` into a JSON value, or `None` when the shape does not match.
fn walk<'a>(mut at: &'a mut Value, path: &[Seg]) -> Option<&'a mut Value> {
    for seg in path {
        at = match (seg, at) {
            (Seg::Key(key), Value::Object(map)) => map.get_mut(key)?,
            (Seg::Index(index), Value::Array(items)) => items.get_mut(*index)?,
            _ => return None,
        };
    }
    Some(at)
}

/// Puts the remembered unknown fields back where they were, wherever their parent still exists.
fn inject(target: &mut Value, unknown: &[(Vec<Seg>, Value)]) {
    for (path, value) in unknown {
        let Some((Seg::Key(last), parents)) = path.split_last() else {
            continue;
        };
        if let Some(Value::Object(map)) = walk(target, parents)
            && !map.contains_key(last)
        {
            map.insert(last.clone(), value.clone());
        }
    }
}

// ---- migrations ----

/// Runs the migration steps from the file's `schema_version` up to `current`, returning the
/// migrated JSON and the version it started from.
///
/// # Errors
///
/// [`ProjectError::Json`] when the version is missing; [`ProjectError::Migration`] when a step is
/// missing or fails.
pub fn migrate_with(
    mut value: Value,
    steps: &[Migration],
    current: u32,
) -> Result<(Value, u32), ProjectError> {
    let from = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| json_err("schema_version is missing or not a number"))?;
    let mut at = from;
    while at < current {
        let step = steps
            .get(usize::try_from(at).unwrap_or(usize::MAX))
            .ok_or_else(|| ProjectError::Migration {
                from: at,
                message: "no migration is defined for this version".to_owned(),
            })?;
        step(&mut value).map_err(|message| ProjectError::Migration { from: at, message })?;
        at += 1;
        value["schema_version"] = Value::from(at);
    }
    Ok((value, from))
}

/// Reads the schema version and decides whether the file must open read-only.
fn read_only_state(
    value: &Value,
    app_version: &Version,
) -> Result<(u32, Option<ReadOnly>), ProjectError> {
    let schema = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| json_err("schema_version is missing or not a number"))?;
    let needed = match value.get("min_reader_version") {
        None => Version::new(0, 0, 0),
        Some(v) => v
            .as_str()
            .and_then(|s| s.parse::<Version>().ok())
            .ok_or_else(|| json_err("min_reader_version is not a version number"))?,
    };
    let read_only = if schema > SCHEMA_VERSION {
        Some(ReadOnly::NewerSchema {
            found: schema,
            supported: SCHEMA_VERSION,
        })
    } else if &needed > app_version {
        Some(ReadOnly::NewerApp {
            needed,
            running: app_version.clone(),
        })
    } else {
        None
    };
    Ok((schema, read_only))
}

/// Fills in each asset's content from the assets folder, checking it against its hash.
fn restore_assets(
    document: &mut Document,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), ProjectError> {
    let mut assets = (*document.assets).clone();
    for asset in assets.values_mut() {
        let path = asset_path(&asset.hash, &asset.name);
        let content = files
            .get(&path)
            .ok_or_else(|| ProjectError::MissingAsset(asset.name.clone()))?;
        if hash_bytes(content) != asset.hash {
            return Err(ProjectError::AssetMismatch(asset.name.clone()));
        }
        Arc::make_mut(asset).bytes = Arc::new(content.clone());
    }
    document.assets = Arc::new(assets);
    Ok(())
}

impl Project {
    /// A project with no thumbnails or embedded files.
    #[must_use]
    pub fn new(document: Document) -> Self {
        Self {
            document,
            template: false,
            thumbnails: Vec::new(),
            embedded: Vec::new(),
            unknown: Unknown::default(),
        }
    }

    /// For a template: an untitled copy of its document, ready to edit.
    #[must_use]
    pub fn open_template(&self) -> Document {
        let mut copy = self.document.clone();
        "Untitled".clone_into(&mut Arc::make_mut(&mut copy.meta).name);
        copy
    }

    /// Builds the `.pstudio` bytes.
    ///
    /// # Errors
    ///
    /// [`ProjectError::Json`] if the document cannot be serialised, [`ProjectError::Write`] if
    /// the zip cannot be built.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ProjectError> {
        let mut document = serde_json::to_value(&self.document).map_err(json_err)?;
        inject(&mut document, &self.unknown.nested);

        let mut top = Map::new();
        top.insert("schema_version".into(), Value::from(SCHEMA_VERSION));
        top.insert("min_reader_version".into(), Value::from(MIN_READER_VERSION));
        top.insert("template".into(), Value::from(self.template));
        top.insert("document".into(), document);
        for (key, value) in &self.unknown.top {
            top.entry(key.clone()).or_insert_with(|| value.clone());
        }
        let mut json = serde_json::to_vec_pretty(&Value::Object(top)).map_err(json_err)?;
        json.push(b'\n');

        let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for asset in self.document.assets.values() {
            files
                .entry(asset_path(&asset.hash, &asset.name))
                .or_insert_with(|| asset.bytes.as_ref().clone());
        }
        for t in &self.thumbnails {
            files.insert(
                format!("thumbnails/mat-{:02}.png", t.mat_index),
                t.png.clone(),
            );
        }
        for e in &self.embedded {
            files.insert(e.path.clone(), e.bytes.clone());
        }

        let base = SimpleFileOptions::default()
            .last_modified_time(DateTime::default())
            .unix_permissions(0o644);
        let method_for = |name: &str| {
            let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
            if STORED_EXTENSIONS.contains(&ext) {
                CompressionMethod::Stored
            } else {
                CompressionMethod::Deflated
            }
        };
        let write_err = |e: &dyn std::fmt::Display| ProjectError::Write(e.to_string());
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(
            "project.json",
            base.compression_method(CompressionMethod::Deflated),
        )
        .map_err(|e| write_err(&e))?;
        zip.write_all(&json).map_err(|e| write_err(&e))?;
        for (name, bytes) in &files {
            zip.start_file(name.as_str(), base.compression_method(method_for(name)))
                .map_err(|e| write_err(&e))?;
            zip.write_all(bytes).map_err(|e| write_err(&e))?;
        }
        Ok(zip.finish().map_err(|e| write_err(&e))?.into_inner())
    }

    /// Reads a `.pstudio` file, verifying every asset's hash.
    ///
    /// # Errors
    ///
    /// [`ProjectError`] for the first problem found.
    pub fn from_bytes(bytes: &[u8], app_version: &Version) -> Result<Loaded, ProjectError> {
        let mut files = read_archive(bytes, &ZipLimits::default(), &is_project_file)?;
        let json = files
            .remove("project.json")
            .ok_or(ProjectError::MissingProject)?;
        let value: Value = serde_json::from_slice(&json).map_err(json_err)?;
        if !value.is_object() {
            return Err(json_err("project.json must be an object"));
        }

        let (schema, read_only) = read_only_state(&value, app_version)?;

        let value = if schema < SCHEMA_VERSION {
            migrate_with(value, &MIGRATIONS, SCHEMA_VERSION)?.0
        } else {
            value
        };

        let raw_document = value
            .get("document")
            .cloned()
            .ok_or_else(|| json_err("the document is missing"))?;
        let mut document: Document =
            serde_json::from_value(raw_document.clone()).map_err(json_err)?;
        let template = value
            .get("template")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut unknown = Unknown::default();
        if let Value::Object(top) = &value {
            for (key, v) in top {
                if !KNOWN_TOP_LEVEL.contains(&key.as_str()) {
                    unknown.top.insert(key.clone(), v.clone());
                }
            }
        }
        let retyped = serde_json::to_value(&document).map_err(json_err)?;
        collect_unknown(
            &raw_document,
            &retyped,
            &mut Vec::new(),
            &mut unknown.nested,
        );

        restore_assets(&mut document, &files)?;

        let mut thumbnails = Vec::new();
        let mut embedded = Vec::new();
        for (name, content) in files {
            if let Some(n) = name
                .strip_prefix("thumbnails/mat-")
                .and_then(|r| r.strip_suffix(".png"))
                .and_then(|n| n.parse::<u32>().ok())
            {
                thumbnails.push(Thumbnail {
                    mat_index: n,
                    png: content,
                });
            } else if name.starts_with("libraries/") {
                embedded.push(EmbeddedFile {
                    path: name,
                    bytes: content,
                });
            }
        }
        thumbnails.sort_by_key(|t| t.mat_index);

        Ok(Loaded {
            project: Project {
                document,
                template,
                thumbnails,
                embedded,
                unknown,
            },
            read_only,
        })
    }
}

#[cfg(test)]
mod tests;
