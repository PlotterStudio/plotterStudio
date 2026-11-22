//! `cargo xtask schemas`: writes the published JSON Schemas for tools, materials and
//! machine profiles to `schemas/<kind>/v1.json`, and `cargo xtask check-schemas` fails if the
//! committed files differ from what the Rust types generate.

use std::path::{Path, PathBuf};

use ps_library::{LibraryKind, schema_json};

/// One generated file: a path relative to the repository root and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Generated {
    pub(crate) path: PathBuf,
    pub(crate) text: String,
}

/// The value with every object's keys in sorted order, so the text does not depend on whether some
/// crate in the build turned on `serde_json`'s `preserve_order` feature.
fn sorted(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// Generates every schema file, in a fixed order.
pub(crate) fn generate() -> Vec<Generated> {
    LibraryKind::ALL
        .into_iter()
        .map(|kind| {
            let mut value = schema_json(kind);
            if let Some(object) = value.as_object_mut() {
                // schemars writes draft 2020-12 URLs already; make sure the draft is stated.
                object
                    .entry("$schema".to_owned())
                    .or_insert_with(|| "https://json-schema.org/draft/2020-12/schema".into());
            }
            let mut text = serde_json::to_string_pretty(&sorted(value)).unwrap_or_default();
            text.push('\n');
            Generated {
                path: Path::new("schemas").join(kind.name()).join("v1.json"),
                text,
            }
        })
        .collect()
}

/// Writes generated files under `root`, creating folders.
pub(crate) fn write_all(root: &Path, files: &[Generated]) -> std::io::Result<()> {
    for file in files {
        let target = root.join(&file.path);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(target, &file.text)?;
    }
    Ok(())
}

/// How the files under `root/schemas` differ from the generated ones: changed, missing, or extra.
pub(crate) fn differences(root: &Path, files: &[Generated]) -> Vec<String> {
    let mut out = Vec::new();
    for file in files {
        let shown = file.path.to_string_lossy().replace('\\', "/");
        match std::fs::read_to_string(root.join(&file.path)) {
            Ok(text) if text == file.text => {}
            Ok(_) => out.push(format!("{shown} differs from the generated schema")),
            Err(_) => out.push(format!("{shown} is missing")),
        }
    }
    let expected: Vec<&Path> = files.iter().map(|f| f.path.as_path()).collect();
    let mut found = Vec::new();
    collect(&root.join("schemas"), Path::new("schemas"), &mut found);
    for path in found {
        if !expected.contains(&path.as_path()) {
            out.push(format!(
                "{} is unexpected (no type generates it)",
                path.to_string_lossy().replace('\\', "/")
            ));
        }
    }
    out
}

fn collect(dir: &Path, relative: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        let child = relative.join(&name);
        if entry.path().is_dir() {
            collect(&entry.path(), &child, out);
        } else {
            out.push(child);
        }
    }
}

#[cfg(test)]
mod tests;
