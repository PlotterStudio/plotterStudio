//! Ground truth for the machine controller: the app's view of the device
//! host. A machine is found, the user approves it once, a session is opened, and a compiled mat is
//! run with prompts answered by the user. These tests use the built-in simulator, so no hardware
//! is involved.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use ps_device::{ConnectError, ConnectionState, DeviceId, JobHeader, JobOutcome, RunRefusal};
use ps_geometry::Point;
use ps_library::Settings;
use ps_model::OpKind;

use super::*;
use crate::commands::Mods;
use crate::export::gcode_for_mat;
use crate::state::AppState;
use crate::tools::CanvasEvent;

const NONE: Mods = Mods {
    command: false,
    shift: false,
    alt: false,
};

fn sim() -> DeviceId {
    DeviceId("sim".to_owned())
}

fn connected() -> MachineController {
    let mut c = MachineController::new(true);
    c.refresh();
    c.approve(&sim());
    c.connect(&sim()).unwrap();
    wait_until(&mut c, MachineController::is_ready);
    c
}

fn wait_until(c: &mut MachineController, done: impl Fn(&MachineController) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        c.poll();
        if done(c) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn compiled_state() -> AppState {
    let mut s = AppState::new(Settings::default());
    s.run_command("tool.rectangle", 0);
    s.canvas_event(CanvasEvent::Press(Point::new(20.0, 20.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(Point::new(80.0, 60.0)), NONE, 0);
    s.assign_operation(OpKind::Cut, "generic.drag-blade@1.0.0", 0);
    s.tick(100);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    s
}

#[test]
fn without_the_simulator_there_are_no_machines() {
    let mut c = MachineController::new(false);
    c.refresh();
    assert_eq!(c.devices().len(), 0);
}

#[test]
fn the_simulator_is_found_but_needs_approval_first() {
    let mut c = MachineController::new(true);
    c.refresh();
    let list = c.devices();
    assert_eq!(list.len(), 1);
    assert!(!list[0].approved);
    assert!(matches!(
        c.connect(&sim()),
        Err(ConnectError::NotApproved { .. })
    ));
    assert!(!c.is_ready());
}

#[test]
fn an_approved_machine_connects_and_becomes_ready() {
    let c = connected();
    assert!(c.is_ready());
    let s = c.status().unwrap();
    assert_eq!(s.connection, ConnectionState::Connected);
    assert_eq!(c.model().as_deref(), Some("Simulated plotter"));
}

#[test]
fn approval_is_remembered_for_the_next_connection() {
    let mut c = connected();
    c.disconnect();
    assert!(c.connect(&sim()).is_ok(), "no second prompt");
}

#[test]
fn revoking_makes_it_ask_again() {
    let mut c = MachineController::new(true);
    c.refresh();
    c.approve(&sim());
    c.revoke(&sim());
    assert!(matches!(
        c.connect(&sim()),
        Err(ConnectError::NotApproved { .. })
    ));
}

#[test]
fn disconnecting_drops_the_session() {
    let mut c = connected();
    c.disconnect();
    assert!(!c.is_ready());
    assert!(c.status().is_none());
    assert!(c.model().is_none());
}

#[test]
fn sending_needs_a_connection() {
    let mut c = MachineController::new(true);
    let s = compiled_state();
    let header = job_header(&s, 0).unwrap();
    let program = gcode_for_mat(&s, 0).unwrap();
    assert_eq!(c.send(program, header), Err(RunRefusal::NotConnected));
}

#[test]
fn the_job_header_names_the_job_the_mat_and_the_profile() {
    let s = compiled_state();
    let h = job_header(&s, 0).unwrap();
    assert!(h.job_id.starts_with("job-"));
    assert_eq!(h.mat_index, 1);
    assert_eq!(h.profile_id, "generic.cutter-small");
    assert!(job_header(&s, 5).is_none());
    assert!(
        job_header(&AppState::new(Settings::default()), 0).is_none(),
        "nothing compiled"
    );
}

/// A state with one rectangle drawn with `op` and `tool`.
fn compiled_state_with(op: OpKind, tool: &str) -> AppState {
    let mut s = AppState::new(Settings::default());
    s.run_command("tool.rectangle", 0);
    s.canvas_event(CanvasEvent::Press(Point::new(20.0, 20.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(Point::new(80.0, 60.0)), NONE, 0);
    s.assign_operation(op, tool, 0);
    s.tick(100);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    s
}

/// Some machines have no holder-select command: the plugin chooses how a job starts from which holder
/// each tool is in, so the header must carry the compiler's tool table.
#[test]
fn the_job_header_carries_each_tools_slot_and_holder() {
    let blade = job_header(&compiled_state(), 0).unwrap();
    assert_eq!(
        blade.tools,
        vec![(
            1,
            "generic.drag-blade".to_owned(),
            "generic.holder-b".to_owned()
        )]
    );

    let pen = job_header(
        &compiled_state_with(OpKind::Draw, "pen.fineliner-black-0.4@1.0.0"),
        0,
    )
    .unwrap();
    assert_eq!(
        pen.tools,
        vec![(
            1,
            "pen.fineliner-black-0.4".to_owned(),
            "generic.holder-a".to_owned()
        )]
    );
}

#[test]
fn a_compiled_mat_runs_to_completion_with_the_users_help() {
    let s = compiled_state();
    let mut c = connected();
    c.send(gcode_for_mat(&s, 0).unwrap(), job_header(&s, 0).unwrap())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut answered = Vec::new();
    while c.last_outcome().is_none() {
        c.poll();
        if let Some(prompt) = c.prompt() {
            answered.push(prompt.message.clone());
            c.answer_prompt();
        }
        assert!(Instant::now() < deadline, "the job never finished");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        matches!(c.last_outcome(), Some(JobOutcome::Completed { .. })),
        "{:?}",
        c.last_outcome()
    );
    assert!(c.is_ready(), "the machine is idle again afterwards");
}

#[test]
fn prompts_are_shown_until_the_user_answers_them() {
    let mut c = connected();
    let program = "G21\nG90\nM0 (Load pen)\nG0 X1.000 Y1.000\nM2\n".to_owned();
    c.send(program, plain_header()).unwrap();
    wait_until(&mut c, |c| {
        c.prompt().is_some() || c.last_outcome().is_some()
    });
    let prompt = c.prompt().expect("the pause asks the user").clone();
    assert_eq!(prompt.message, "Load pen");
    assert!(c.last_outcome().is_none(), "the job waits");
    c.answer_prompt();
    assert!(c.prompt().is_none());
    wait_until(&mut c, |c| c.last_outcome().is_some());
    assert!(matches!(
        c.last_outcome(),
        Some(JobOutcome::Completed { .. })
    ));
}

#[test]
fn a_run_can_be_cancelled_at_a_prompt() {
    let mut c = connected();
    c.send(
        "G21\nG90\nM0 (Load pen)\nG0 X1.000 Y1.000\nM2\n".to_owned(),
        plain_header(),
    )
    .unwrap();
    wait_until(&mut c, |c| c.prompt().is_some());
    c.cancel();
    wait_until(&mut c, |c| c.last_outcome().is_some());
    assert!(matches!(
        c.last_outcome(),
        Some(JobOutcome::Cancelled { .. })
    ));
    assert!(c.prompt().is_none(), "the prompt goes away with the job");
}

#[test]
fn a_new_run_forgets_the_previous_outcome() {
    let mut c = connected();
    c.send("G21\nG90\nM2\n".to_owned(), plain_header()).unwrap();
    wait_until(&mut c, |c| c.last_outcome().is_some());
    c.clear_outcome();
    assert!(c.last_outcome().is_none());
}

#[test]
fn an_invalid_program_is_refused_before_anything_is_sent() {
    let mut c = connected();
    assert!(matches!(
        c.send("G999 nonsense\n".to_owned(), plain_header()),
        Err(RunRefusal::InvalidProgram(_))
    ));
}

#[test]
fn progress_is_reported_while_running_and_the_run_completes() {
    let mut c = connected();
    let mut program = String::from(
        "G21
G90
",
    );
    for i in 0..200 {
        let _ = writeln!(program, "G0 X{}.000 Y1.000", i % 100);
    }
    program.push_str(
        "M2
",
    );
    c.send(program, plain_header()).unwrap();
    let mut best = 0;
    let deadline = Instant::now() + Duration::from_secs(60);
    while c.last_outcome().is_none() {
        c.poll();
        if let Some(p) = c.progress() {
            best = best.max(p.lines_done);
        }
        assert!(Instant::now() < deadline, "the job never finished");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(
        c.last_outcome(),
        Some(JobOutcome::Completed { .. })
    ));
    assert!(best > 0, "progress moved");
}

fn plain_header() -> JobHeader {
    JobHeader {
        job_id: "job-test".to_owned(),
        mat_index: 1,
        profile_id: "generic.cutter-small".to_owned(),
        tools: Vec::new(),
    }
}

// ---- installed plugins ----

fn package_bytes(id: &str) -> Vec<u8> {
    use ps_device::{DeviceFilter, PluginManifest, TransportKind, pack};
    // A tiny stand-in component is enough here: loading a package only reads its manifest.
    let manifest = PluginManifest {
        id: id.to_owned(),
        name: format!("Plugin {id}"),
        filters: vec![DeviceFilter {
            transport: TransportKind::Serial,
            usb: None,
        }],
        internet_hosts: Vec::new(),
    };
    pack(&manifest, b"\0asm\x0d\0\x01\0", "1.0.0").unwrap()
}

#[test]
fn plugins_in_a_folder_are_loaded_and_bad_files_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("good.psdevice"), package_bytes("acme.good")).unwrap();
    std::fs::write(dir.path().join("also.psdevice"), package_bytes("acme.also")).unwrap();
    std::fs::write(dir.path().join("bad.psdevice"), b"not a package").unwrap();
    std::fs::write(dir.path().join("readme.txt"), b"ignored").unwrap();
    let mut c = MachineController::new(false);
    let load = c.install_plugins(dir.path());
    let mut ids = load.loaded.clone();
    ids.sort();
    assert_eq!(ids, vec!["acme.also".to_owned(), "acme.good".to_owned()]);
    assert_eq!(load.problems.len(), 1, "{:?}", load.problems);
    assert!(
        load.problems[0].contains("bad.psdevice"),
        "{:?}",
        load.problems
    );
}

#[test]
fn a_missing_plugin_folder_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = MachineController::new(false);
    let load = c.install_plugins(&dir.path().join("nope"));
    assert!(load.loaded.is_empty() && load.problems.is_empty());
}

#[test]
fn the_same_plugin_id_twice_is_loaded_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.psdevice"), package_bytes("acme.same")).unwrap();
    std::fs::write(dir.path().join("b.psdevice"), package_bytes("acme.same")).unwrap();
    let mut c = MachineController::new(false);
    let load = c.install_plugins(dir.path());
    assert_eq!(load.loaded, vec!["acme.same".to_owned()]);
    assert_eq!(load.problems.len(), 1, "{:?}", load.problems);
}

#[test]
fn a_controller_with_no_plugin_can_find_and_connect_to_nothing() {
    let mut c = MachineController::new(false);
    assert_eq!(c.plugin_count(), 0);
    c.refresh();
    assert_eq!(c.devices().len(), 0);
    assert_eq!(c.unclaimed().len(), 0);
}

#[test]
fn installed_plugins_are_counted_and_the_simulator_only_when_simulating() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.psdevice"), package_bytes("acme.a")).unwrap();
    std::fs::write(dir.path().join("b.psdevice"), package_bytes("acme.b")).unwrap();
    let mut c = MachineController::new(false);
    c.install_plugins(dir.path());
    assert_eq!(c.plugin_count(), 2);
    assert_eq!(MachineController::new(true).plugin_count(), 1);
}

#[test]
fn nothing_is_looked_for_until_asked_and_each_look_asks_the_plugins_again() {
    let mut c = MachineController::new(true);
    assert!(c.devices().is_empty(), "no look has happened yet");
    c.refresh();
    assert_eq!(c.devices().len(), 1);
    c.refresh();
    assert_eq!(c.devices().len(), 1);
}
// ---- approvals that last ----

fn cloud_package(id: &str, hosts: &[&str]) -> Vec<u8> {
    use ps_device::{DeviceFilter, PluginManifest, TransportKind, pack};
    let manifest = PluginManifest {
        id: id.to_owned(),
        name: format!("Plugin {id}"),
        filters: vec![DeviceFilter {
            transport: TransportKind::Serial,
            usb: None,
        }],
        internet_hosts: hosts.iter().map(|h| (*h).to_owned()).collect(),
    };
    pack(&manifest, b"\0asm\x0d\0\x01\0", "1.0.0").unwrap()
}

#[test]
fn an_approval_is_saved_and_found_again_by_the_next_session() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("permissions.json");
    let mut first = MachineController::new(true);
    first.persist_permissions(&file).unwrap();
    first.refresh();
    first.approve(&sim());
    assert!(file.exists(), "saved as soon as the user answered");

    let mut second = MachineController::new(true);
    second.persist_permissions(&file).unwrap();
    second.refresh();
    assert!(second.devices().iter().any(|d| d.id == sim() && d.approved));
}

#[test]
fn a_revoked_approval_stays_revoked_next_time() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("permissions.json");
    let mut first = MachineController::new(true);
    first.persist_permissions(&file).unwrap();
    first.refresh();
    first.approve(&sim());
    first.revoke(&sim());
    let mut second = MachineController::new(true);
    second.persist_permissions(&file).unwrap();
    second.refresh();
    assert!(second.devices().iter().all(|d| !d.approved));
}

#[test]
fn a_damaged_permissions_file_is_reported_and_the_session_starts_with_nothing_approved() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("permissions.json");
    std::fs::write(&file, "not json").unwrap();
    let mut c = MachineController::new(true);
    assert!(c.persist_permissions(&file).is_err());
    c.refresh();
    assert!(c.devices().iter().all(|d| !d.approved));
}

#[test]
fn a_declared_internet_host_is_blocked_until_allowed_and_the_choice_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let plugins = dir.path().join("plugins");
    std::fs::create_dir(&plugins).unwrap();
    std::fs::write(
        plugins.join("cloud.psdevice"),
        cloud_package("acme.cloud", &["api.acme.example"]),
    )
    .unwrap();
    let file = dir.path().join("permissions.json");

    let mut first = MachineController::new(false);
    first.persist_permissions(&file).unwrap();
    first.install_plugins(&plugins);
    let hosts = first.declared_hosts();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].host, "api.acme.example");
    assert!(!hosts[0].allowed);
    assert!(first.allow_host("acme.cloud", "api.acme.example"));
    assert!(!first.allow_host("acme.cloud", "other.example"));

    let mut second = MachineController::new(false);
    second.persist_permissions(&file).unwrap();
    second.install_plugins(&plugins);
    assert!(second.declared_hosts()[0].allowed);
    assert!(second.revoke_host("acme.cloud", "api.acme.example"));
    assert!(!second.declared_hosts()[0].allowed);
}

#[test]
fn a_plugin_that_uses_no_internet_lists_nothing_in_use() {
    let c = MachineController::new(true);
    assert_eq!(c.internet_in_use(), Vec::<(String, String)>::new());
}

// ---- scanning must not freeze the window, and scans can be turned off ----

#[test]
fn looking_again_in_the_background_returns_at_once_and_finds_the_machines_later() {
    let mut c = MachineController::new(true);
    c.refresh_in_background();
    assert_eq!(c.devices().len(), 0, "nothing is known until the scan ends");
    wait_until(&mut c, |c| !c.devices().is_empty());
    wait_until(&mut c, |c| !c.is_refreshing());
    assert_eq!(c.devices().len(), 1);
}

#[test]
fn a_second_look_while_one_is_running_does_not_start_another() {
    let mut c = MachineController::new(true);
    c.refresh_in_background();
    c.refresh_in_background();
    wait_until(&mut c, |c| !c.is_refreshing());
    assert_eq!(c.devices().len(), 1);
}
