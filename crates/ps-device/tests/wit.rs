//! The published plugin contract must be valid WIT and must agree with the Rust types the
//! host uses. The WIT file is what plugin authors in other languages generate bindings from.

use ps_device::{
    AlertLevel, Capabilities, ConnectionState, MachineState, PowerState, TransportKind, wit,
};
use ps_gcode::Command;
use wit_parser::{Resolve, TypeDefKind, WorldItem};

const WIT: &str = include_str!("../wit/ps-device.wit");

fn resolve() -> (Resolve, wit_parser::PackageId) {
    let mut resolve = Resolve::new();
    let id = resolve
        .push_str("ps-device.wit", WIT)
        .expect("the WIT file is valid");
    (resolve, id)
}

fn type_def<'a>(resolve: &'a Resolve, interface: &str, name: &str) -> &'a TypeDefKind {
    let iface = resolve
        .interfaces
        .iter()
        .find_map(|(_, i)| (i.name.as_deref() == Some(interface)).then_some(i))
        .unwrap_or_else(|| panic!("no interface {interface}"));
    let id = iface
        .types
        .get(name)
        .unwrap_or_else(|| panic!("no type {name}"));
    &resolve.types[*id].kind
}

fn enum_cases(resolve: &Resolve, name: &str) -> Vec<String> {
    match type_def(resolve, "types", name) {
        TypeDefKind::Enum(e) => e.cases.iter().map(|c| c.name.clone()).collect(),
        other => panic!("{name} is not an enum: {other:?}"),
    }
}

fn variant_cases(resolve: &Resolve, name: &str) -> Vec<String> {
    match type_def(resolve, "types", name) {
        TypeDefKind::Variant(v) => v.cases.iter().map(|c| c.name.clone()).collect(),
        other => panic!("{name} is not a variant: {other:?}"),
    }
}

fn record_fields(resolve: &Resolve, name: &str) -> Vec<String> {
    match type_def(resolve, "types", name) {
        TypeDefKind::Record(r) => r.fields.iter().map(|f| f.name.clone()).collect(),
        other => panic!("{name} is not a record: {other:?}"),
    }
}

#[test]
fn the_package_is_plotter_device_one_point_zero() {
    let (resolve, id) = resolve();
    let package = &resolve.packages[id];
    assert_eq!(package.name.namespace, "plotter");
    assert_eq!(package.name.name, "device");
    assert_eq!(
        package
            .name
            .version
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("1.0.0")
    );
}

#[test]
fn the_plugin_world_imports_transport_discovery_and_host_and_exports_the_driver() {
    let (resolve, id) = resolve();
    let world = resolve.packages[id]
        .worlds
        .get("device-plugin")
        .map(|w| &resolve.worlds[*w])
        .expect("the world");
    let name_of = |item: &WorldItem| match item {
        WorldItem::Interface { id, .. } => resolve.interfaces[*id].name.clone().unwrap_or_default(),
        _ => String::new(),
    };
    let mut imports: Vec<String> = world.imports.values().map(name_of).collect();
    imports.sort();
    // A resolved world also lists interfaces pulled in by `use` (here `types`); the three declared
    // imports must be there and nothing else may be.
    for declared in ["host", "transport", "discovery"] {
        assert!(
            imports.contains(&declared.to_owned()),
            "{declared}: {imports:?}"
        );
    }
    assert!(
        imports
            .iter()
            .all(|i| ["host", "transport", "discovery", "types"].contains(&i.as_str())),
        "{imports:?}"
    );
    let exports: Vec<String> = world.exports.values().map(name_of).collect();
    assert_eq!(exports, vec!["driver"]);
}

#[test]
fn the_host_lends_a_plugin_the_four_buses_and_the_plugin_names_what_to_offer() {
    let (resolve, _) = resolve();
    let function_names = |interface: &str| -> Vec<String> {
        let found = resolve
            .interfaces
            .iter()
            .find(|(_, i)| i.name.as_deref() == Some(interface))
            .map(|(_, i)| i)
            .unwrap();
        let mut names: Vec<String> = found.functions.keys().cloned().collect();
        names.sort();
        names
    };
    assert_eq!(
        function_names("discovery"),
        ["browse-mdns", "list-serial-ports", "list-usb", "scan-ble"]
    );
    let driver = function_names("driver");
    assert!(driver.contains(&"discover".to_owned()), "{driver:?}");
    assert!(driver.contains(&"identify".to_owned()), "{driver:?}");
}

#[test]
fn the_session_resource_offers_every_driver_call() {
    let (resolve, _) = resolve();
    let driver = resolve
        .interfaces
        .iter()
        .find(|(_, i)| i.name.as_deref() == Some("driver"))
        .map(|(_, i)| i)
        .unwrap();
    let mut methods: Vec<String> = driver
        .functions
        .keys()
        .filter_map(|k| k.strip_prefix("[method]session."))
        .map(str::to_owned)
        .collect();
    methods.sort();
    let mut expected = vec![
        "begin-job",
        "cancel",
        "capabilities",
        "disconnect",
        "end-job",
        "info",
        "jog",
        "pause",
        "resume",
        "send",
        "status",
        "user-action-done",
        "wait-event",
    ];
    expected.sort_unstable();
    assert_eq!(methods, expected);
    assert!(driver.functions.contains_key("identify"));
    assert!(driver.functions.contains_key("[constructor]session"));
}

#[test]
fn the_host_offers_logging_time_sleep_and_a_small_store() {
    let (resolve, _) = resolve();
    let host = resolve
        .interfaces
        .iter()
        .find(|(_, i)| i.name.as_deref() == Some("host"))
        .map(|(_, i)| i)
        .unwrap();
    let mut names: Vec<&str> = host.functions.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["kv-get", "kv-set", "log", "now-ms", "sleep"]);
}

#[test]
fn the_wit_enums_match_the_rust_enums() {
    let (resolve, _) = resolve();
    let wit_connection = enum_cases(&resolve, "connection-state");
    for c in [
        ConnectionState::Disconnected,
        ConnectionState::Connecting,
        ConnectionState::Connected,
        ConnectionState::Lost,
    ] {
        assert!(
            wit_connection.contains(&wit::connection_state(c).to_owned()),
            "{c:?}"
        );
    }
    let power = enum_cases(&resolve, "power-state");
    for p in [
        PowerState::On,
        PowerState::Standby,
        PowerState::Off,
        PowerState::Unknown,
    ] {
        assert!(power.contains(&wit::power_state(p).to_owned()), "{p:?}");
    }
    let transport = enum_cases(&resolve, "transport-kind");
    for t in [
        TransportKind::Usb,
        TransportKind::Serial,
        TransportKind::BluetoothLe,
        TransportKind::Tcp,
        TransportKind::Udp,
        TransportKind::WebSocket,
        TransportKind::Http,
    ] {
        assert!(
            transport.contains(&wit::transport_kind(t).to_owned()),
            "{t:?}"
        );
    }
    let alerts = enum_cases(&resolve, "alert-level");
    for a in [AlertLevel::Info, AlertLevel::Warning, AlertLevel::Error] {
        assert!(alerts.contains(&wit::alert_level(a).to_owned()), "{a:?}");
    }
}

#[test]
fn every_machine_state_has_a_wit_case() {
    let (resolve, _) = resolve();
    let cases = variant_cases(&resolve, "machine-state");
    assert_eq!(cases.len(), 8, "{cases:?}");
    let states = [
        MachineState::Idle,
        MachineState::Busy,
        MachineState::RunningJob,
        MachineState::Paused,
        MachineState::WaitingForUser(ps_device::UserAction {
            id: 0,
            kind: ps_device::UserActionKind::Confirm,
            message: String::new(),
        }),
        MachineState::Fault(ps_device::DeviceError::new("c", "m")),
        MachineState::UpdatingFirmware,
        MachineState::Unknown,
    ];
    for s in &states {
        assert!(
            cases.contains(&wit::machine_state_case(s).to_owned()),
            "{s:?}"
        );
    }
}

#[test]
fn the_capabilities_record_has_exactly_the_rust_fields() {
    let (resolve, _) = resolve();
    let mut wit_fields = record_fields(&resolve, "capabilities");
    wit_fields.sort();
    // Destructuring without `..` makes adding a Rust field break this test until the WIT follows.
    let Capabilities {
        pause: _,
        resume: _,
        cancel: _,
        jog: _,
        home: _,
        load_mat: _,
        unload_mat: _,
        tool_detection: _,
        reports_position: _,
        reports_temperature: _,
        max_inflight_lines: _,
    } = Capabilities::default();
    let mut rust_fields: Vec<String> = [
        "pause",
        "resume",
        "cancel",
        "jog",
        "home",
        "load-mat",
        "unload-mat",
        "tool-detection",
        "reports-position",
        "reports-temperature",
        "max-inflight-lines",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    rust_fields.sort();
    assert_eq!(wit_fields, rust_fields);
}

#[test]
fn every_parsed_command_has_a_step_case() {
    let (resolve, _) = resolve();
    let cases = variant_cases(&resolve, "step");
    let commands = [
        Command::Comment(String::new()),
        Command::Units { millimetres: true },
        Command::Absolute,
        Command::Rapid {
            to: ps_core::PtUm::new(0, 0),
        },
        Command::Linear {
            to: ps_core::PtUm::new(0, 0),
            a_mdeg: None,
            feed_mm_min: 1,
        },
        Command::Arc {
            to: ps_core::PtUm::new(0, 0),
            centre_offset: ps_core::PtUm::new(0, 0),
            ccw: true,
            feed_mm_min: 1,
        },
        Command::Dwell { ms: 1 },
        Command::Pause(String::new()),
        Command::ToolChange { slot: 1 },
        Command::ToolDown { force: 1 },
        Command::ToolUp,
        Command::Temperature {
            celsius: 1,
            wait: false,
        },
        Command::End,
    ];
    for c in &commands {
        assert!(cases.contains(&wit::step_case(c).to_owned()), "{c:?}");
    }
    // And the other way round: no WIT case is without a command.
    let used: Vec<&str> = commands.iter().map(wit::step_case).collect();
    for case in &cases {
        assert!(
            used.contains(&case.as_str()),
            "WIT step case {case} has no Rust command"
        );
    }
}

#[test]
fn the_record_for_a_line_carries_the_text_and_the_parsed_form() {
    let (resolve, _) = resolve();
    assert_eq!(
        record_fields(&resolve, "gcode-line"),
        vec!["number", "text", "step"]
    );
    assert_eq!(
        record_fields(&resolve, "job-header"),
        vec!["job-id", "mat-index", "profile-id", "tools"]
    );
}
