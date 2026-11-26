//! Ground truth for copying a checked plugin package into the plugins folder: where the file
//! lands, that a hostile id cannot choose a path outside the folder, and that installing a newer
//! build leaves exactly one file for the plugin.

use ps_device::{DeviceFilter, PluginManifest, TransportKind, pack};

use super::*;

fn package(id: &str, version: &str) -> Vec<u8> {
    let manifest = PluginManifest {
        id: id.to_owned(),
        name: format!("Plugin {id}"),
        filters: vec![DeviceFilter {
            transport: TransportKind::Serial,
            usb: None,
        }],
        internet_hosts: Vec::new(),
    };
    pack(&manifest, b"\0asm\x0d\0\x01\0", version).unwrap()
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_package_is_stored_under_its_id_and_the_folder_is_created() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("plugins");
    let bytes = package("acme.saw", "1.0.0");
    let path = store(&dir, "acme.saw", &bytes).unwrap();
    assert_eq!(path, dir.join("acme.saw.psdevice"));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(
        files(&dir),
        ["acme.saw.psdevice"],
        "no temporary file is left"
    );
}

#[test]
fn an_id_cannot_choose_a_path_outside_the_folder() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("plugins");
    for id in [
        "../escape",
        r"..\escape",
        "a/b",
        r"C:\evil",
        "..",
        ".",
        "",
        "/abs",
    ] {
        let result = store(&dir, id, &package("acme.x", "1.0.0"));
        if let Ok(path) = result {
            assert_eq!(path.parent(), Some(dir.as_path()), "{id:?} -> {path:?}");
            assert_eq!(path.extension().and_then(|e| e.to_str()), Some("psdevice"));
        }
    }
    // Whatever was or was not written, it is all inside the plugins folder.
    assert_eq!(files(root.path()), ["plugins"]);
}

#[test]
fn an_id_with_nothing_usable_in_it_is_refused() {
    let root = tempfile::tempdir().unwrap();
    for id in ["", ".", "..", "///"] {
        assert!(
            matches!(
                store(root.path(), id, &package("acme.x", "1.0.0")),
                Err(InstallError::BadId(_))
            ),
            "{id:?}"
        );
    }
}

#[test]
fn a_newer_build_replaces_the_older_file_even_under_another_name() {
    let dir = tempfile::tempdir().unwrap();
    // How someone installed it by hand before: any file name at all.
    std::fs::write(
        dir.path().join("my-saw-plugin.psdevice"),
        package("acme.saw", "1.0.0"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("other.psdevice"),
        package("acme.other", "1.0.0"),
    )
    .unwrap();
    let newer = package("acme.saw", "2.0.0");
    let path = store(dir.path(), "acme.saw", &newer).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), newer);
    assert_eq!(files(dir.path()), ["acme.saw.psdevice", "other.psdevice"]);
}

#[test]
fn files_that_are_not_plugins_are_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), b"mine").unwrap();
    std::fs::write(dir.path().join("broken.psdevice"), b"not a package").unwrap();
    store(dir.path(), "acme.saw", &package("acme.saw", "1.0.0")).unwrap();
    assert_eq!(
        files(dir.path()),
        ["acme.saw.psdevice", "broken.psdevice", "notes.txt"]
    );
}
