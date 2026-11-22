//! Ground truth for plugin packaging and plugin checks: a `.psdevice` file is a
//! zip of a manifest and a component; a recorded session is replayed to a plugin to see whether it
//! recognises the device and writes what the recording says it should; the simulated serial
//! machine lets a plugin run a whole job with no hardware.

use std::time::Duration;

use super::*;
use crate::host::DeviceFilter;
use crate::transcript::{Direction, Transcript};
use crate::types::TransportKind;
use crate::wasm::BlockingTransport;

fn manifest() -> PluginManifest {
    PluginManifest {
        id: "acme.grbl".into(),
        name: "Acme GRBL".into(),
        filters: vec![
            DeviceFilter {
                transport: TransportKind::Serial,
                usb: None,
            },
            DeviceFilter {
                transport: TransportKind::Usb,
                usb: Some((0x1a86, 0x7523)),
            },
        ],
        internet_hosts: vec!["updates.acme.example".into()],
    }
}

#[test]
fn a_package_round_trips_its_manifest_and_component() {
    let bytes = pack(&manifest(), b"\0asm\x0d\0\x01\0", "1.2.0").unwrap();
    let unpacked = unpack(&bytes).unwrap();
    assert_eq!(unpacked.manifest, manifest());
    assert_eq!(unpacked.version, "1.2.0");
    assert_eq!(unpacked.wasm, b"\0asm\x0d\0\x01\0");
}

#[test]
fn a_package_without_a_manifest_or_component_is_refused() {
    use std::io::Write;
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        zip.start_file("plugin.wasm", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();
    }
    assert!(
        matches!(unpack(cursor.get_ref()), Err(PackageError::Missing(f)) if f == "manifest.json")
    );
    assert!(matches!(
        unpack(b"not a zip"),
        Err(PackageError::NotAPackage(_))
    ));
}

#[test]
fn a_manifest_with_an_unknown_transport_is_refused() {
    use std::io::Write;
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("manifest.json", opts).unwrap();
        zip.write_all(br#"{"id":"a","name":"A","version":"1","filters":[{"transport":"telepathy"}],"internet_hosts":[]}"#).unwrap();
        zip.start_file("plugin.wasm", opts).unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();
    }
    assert!(matches!(
        unpack(cursor.get_ref()),
        Err(PackageError::Manifest(_))
    ));
}

#[test]
fn a_package_has_a_size_limit() {
    let huge = vec![0_u8; MAX_PLUGIN_BYTES + 1];
    assert!(matches!(
        pack(&manifest(), &huge, "1"),
        Err(PackageError::TooLarge)
    ));
}

// ---- replay ----

fn session() -> Transcript {
    let mut t = Transcript::default();
    t.record(Direction::Out, 0, &[0x18]);
    t.record(Direction::In, 10, b"\r\nGrbl 1.1h ['$' for help]\r\n");
    t.record(Direction::Out, 20, b"?");
    t.record(Direction::In, 30, b"<Idle|MPos:1.000,2.000,0.000>\nok\n");
    t
}

#[test]
fn a_replay_plays_back_the_machines_side_when_the_host_has_written() {
    let (mut replay, handle) = ReplayTransport::new(&session(), TransportKind::Serial);
    assert!(
        replay.read(64, Duration::ZERO).unwrap().is_empty(),
        "nothing before the reset"
    );
    replay.write(&[0x18]).unwrap();
    let banner = replay.read(1000, Duration::ZERO).unwrap();
    assert_eq!(banner, b"\r\nGrbl 1.1h ['$' for help]\r\n");
    assert!(!handle.finished());
    replay.write(b"?").unwrap();
    assert!(
        replay
            .read(1000, Duration::ZERO)
            .unwrap()
            .starts_with(b"<Idle")
    );
    assert!(handle.finished());
    assert_eq!(handle.mismatch(), None);
}

#[test]
fn a_replay_reads_in_pieces_when_asked_for_less() {
    let (mut replay, _) = ReplayTransport::new(&session(), TransportKind::Serial);
    replay.write(&[0x18]).unwrap();
    assert_eq!(replay.read(2, Duration::ZERO).unwrap(), b"\r\n");
    assert_eq!(replay.read(4, Duration::ZERO).unwrap(), b"Grbl");
}

#[test]
fn a_write_that_differs_from_the_recording_is_a_mismatch() {
    let (mut replay, handle) = ReplayTransport::new(&session(), TransportKind::Serial);
    assert!(replay.write(b"!").is_err());
    let why = handle.mismatch().expect("recorded");
    assert!(why.contains("21") || why.contains('!'), "{why}");
}

#[test]
fn writing_more_than_the_recording_has_is_a_mismatch() {
    let (mut replay, handle) = ReplayTransport::new(&Transcript::default(), TransportKind::Serial);
    assert!(replay.write(b"x").is_err());
    assert!(handle.mismatch().is_some());
}

#[test]
fn a_write_may_span_several_recorded_entries() {
    let mut t = Transcript::default();
    t.record(Direction::Out, 0, b"G21\n");
    t.record(Direction::Out, 1, b"G90\n");
    let (mut replay, handle) = ReplayTransport::new(&t, TransportKind::Serial);
    replay.write(b"G21\nG90\n").unwrap();
    assert!(handle.finished());
}

// ---- checking a plugin ----

#[tokio::test]
async fn a_plugin_that_recognises_the_recording_passes_the_check() {
    let wasm = crate::wasm::tests_support::grbl();
    let report = check_against_recording(&wasm, &session()).await;
    assert!(report.passed(), "{report:?}");
    assert_eq!(report.identified.as_deref(), Some("GRBL 1.1h"));
}

#[tokio::test]
async fn a_recording_the_plugin_does_not_recognise_fails_the_check() {
    let mut t = Transcript::default();
    t.record(Direction::Out, 0, &[0x18]);
    t.record(Direction::In, 1, b"Marlin 2.1\n");
    let wasm = crate::wasm::tests_support::grbl();
    let report = check_against_recording(&wasm, &t).await;
    assert!(!report.passed());
    assert!(report.identified.is_none());
}

#[tokio::test]
async fn a_plugin_that_writes_something_else_fails_the_check() {
    let mut t = Transcript::default();
    t.record(Direction::Out, 0, b"$$\n");
    let wasm = crate::wasm::tests_support::grbl();
    let report = check_against_recording(&wasm, &t).await;
    assert!(!report.passed());
    assert!(report.mismatch.is_some());
}

#[tokio::test]
async fn a_plugin_runs_a_whole_job_against_the_simulated_machine() {
    let wasm = crate::wasm::tests_support::grbl();
    let report = check_in_simulation(&wasm).await;
    assert!(report.passed(), "{report:?}");
    assert!(report.lines_sent > 5);
}

#[tokio::test]
async fn bytes_that_are_not_a_component_fail_the_simulation_with_a_reason() {
    let report = check_in_simulation(b"junk").await;
    assert!(!report.passed());
    assert!(
        report.problems.iter().any(|p| p.contains("plugin-invalid")),
        "{report:?}"
    );
}

#[test]
fn a_manifest_file_parses_into_a_plugin_manifest_and_version() {
    let (manifest, version) = manifest_from_json(
        r#"{"id":"a.b","name":"AB","version":"2.1.0","filters":[{"transport":"usb","usb":[6790,29987]}],"internet_hosts":["x.example"]}"#,
    )
    .unwrap();
    assert_eq!(version, "2.1.0");
    assert_eq!(manifest.id, "a.b");
    assert_eq!(manifest.filters[0].usb, Some((6790, 29987)));
    assert!(matches!(
        manifest_from_json("{"),
        Err(PackageError::Manifest(_))
    ));
}

#[test]
fn the_grbl_plugins_committed_manifest_is_valid() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/grbl/manifest.json"),
    )
    .unwrap();
    let (manifest, version) = manifest_from_json(&text).unwrap();
    assert_eq!(manifest.id, "plotter-studio.grbl");
    assert_eq!(version, "1.0.0");
    // It only looks at serial ports (a self-made board is a USB-serial adapter), so that is all
    // its manifest asks the host to lend it.
    assert_eq!(
        manifest.filters,
        vec![crate::host::DeviceFilter {
            transport: crate::types::TransportKind::Serial,
            usb: None,
        }]
    );
    assert!(
        manifest.internet_hosts.is_empty(),
        "the reference plugin needs no internet"
    );
}
