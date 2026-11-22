//! `cargo xtask package`: builds each plugin that has a `manifest.json` for
//! `wasm32-wasip2` and packs it as `target/packages/<folder>.psdevice`.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};

/// The package file name for a plugin folder.
pub(crate) fn package_name(folder: &str) -> String {
    format!("{folder}.psdevice")
}

/// The folders under `plugins/` that have a `manifest.json`, sorted.
pub(crate) fn plugin_folders(root: &Path) -> Result<Vec<String>> {
    let dir = root.join("plugins");
    let mut out: Vec<String> = std::fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(Result::ok)
        .filter(|e| e.path().join("manifest.json").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    Ok(out)
}

/// A `.psdevice` file: the manifest and the component in a zip with fixed timestamps.
pub(crate) fn pack_plugin(manifest: &str, wasm: &[u8]) -> Result<Vec<u8>> {
    let value: serde_json::Value = serde_json::from_str(manifest).context("manifest.json")?;
    for field in ["id", "name", "version", "filters"] {
        if value.get(field).is_none() {
            bail!("manifest.json has no \"{field}\"");
        }
    }
    if wasm.is_empty() {
        bail!("the plugin component is empty");
    }
    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut cursor);
    let options =
        zip::write::SimpleFileOptions::default().last_modified_time(zip::DateTime::default());
    for (name, bytes) in [
        ("manifest.json", manifest.as_bytes()),
        ("plugin.wasm", wasm),
    ] {
        zip.start_file(name, options)?;
        zip.write_all(bytes)?;
    }
    zip.finish()?;
    Ok(cursor.into_inner())
}

/// Builds and packs every plugin, returning the files written.
pub(crate) fn package_all(root: &Path) -> Result<Vec<std::path::PathBuf>> {
    let out_dir = root.join("target/packages");
    std::fs::create_dir_all(&out_dir)?;
    let mut written = Vec::new();
    for folder in plugin_folders(root)? {
        let crate_name = format!("{folder}-plugin");
        let status = std::process::Command::new("cargo")
            .args([
                "build",
                "-p",
                &crate_name,
                "--target",
                "wasm32-wasip2",
                "--release",
                "--locked",
            ])
            .current_dir(root)
            .status()
            .context("running cargo build")?;
        if !status.success() {
            bail!("building {crate_name} failed");
        }
        let wasm = std::fs::read(
            root.join("target/wasm32-wasip2/release")
                .join(format!("{}.wasm", crate_name.replace('-', "_"))),
        )
        .with_context(|| format!("reading the built {crate_name}"))?;
        let manifest =
            std::fs::read_to_string(root.join("plugins").join(&folder).join("manifest.json"))?;
        let file = out_dir.join(package_name(&folder));
        std::fs::write(&file, pack_plugin(&manifest, &wasm)?)?;
        written.push(file);
    }
    Ok(written)
}

#[cfg(test)]
mod tests;
