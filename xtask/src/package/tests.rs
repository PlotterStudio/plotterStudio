//! Ground truth for `cargo xtask package`: device plugins in the repository are built for
//! `wasm32-wasip2` and packed as `.psdevice` files, a zip of `manifest.json` and `plugin.wasm`
//! that the app and `pstudio device test` read.

use std::io::Read;

use super::*;

const MANIFEST: &str = r#"{"id":"acme.x","name":"X","version":"1.2.3","filters":[{"transport":"serial"}],"internet_hosts":[]}"#;

fn read(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}

#[test]
fn a_package_holds_the_manifest_and_the_component() {
    let bytes = pack_plugin(MANIFEST, b"\0asm\x0d\0\x01\0").unwrap();
    assert_eq!(read(&bytes, "manifest.json"), MANIFEST.as_bytes());
    assert_eq!(read(&bytes, "plugin.wasm"), b"\0asm\x0d\0\x01\0");
}

#[test]
fn packing_is_deterministic() {
    assert_eq!(
        pack_plugin(MANIFEST, b"x").unwrap(),
        pack_plugin(MANIFEST, b"x").unwrap()
    );
}

#[test]
fn a_manifest_missing_a_required_field_is_refused() {
    for bad in [
        r#"{"name":"X","version":"1","filters":[]}"#,
        r#"{"id":"a","version":"1","filters":[]}"#,
        r#"{"id":"a","name":"X","filters":[]}"#,
        r#"{"id":"a","name":"X","version":"1"}"#,
        "not json",
    ] {
        assert!(pack_plugin(bad, b"x").is_err(), "{bad}");
    }
}

#[test]
fn an_empty_component_is_refused() {
    assert!(pack_plugin(MANIFEST, b"").is_err());
}

#[test]
fn the_package_is_named_after_the_plugin_folder() {
    assert_eq!(package_name("grbl"), "grbl.psdevice");
}

#[test]
fn the_plugin_folders_are_the_ones_with_a_manifest() {
    let dir = tempfile::tempdir().unwrap();
    for (name, with_manifest) in [("grbl", true), ("rogue", false), ("other", true)] {
        let d = dir.path().join("plugins").join(name);
        std::fs::create_dir_all(&d).unwrap();
        if with_manifest {
            std::fs::write(d.join("manifest.json"), MANIFEST).unwrap();
        }
    }
    assert_eq!(
        plugin_folders(dir.path()).unwrap(),
        vec!["grbl".to_owned(), "other".to_owned()]
    );
}

#[test]
fn the_repositorys_own_plugin_folders_include_grbl() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    assert!(plugin_folders(root).unwrap().contains(&"grbl".to_owned()));
}
