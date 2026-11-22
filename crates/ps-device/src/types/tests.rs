//! Ground truth for the device types and the run preconditions. A job may
//! start only when the machine is connected, idle, and its reported profile id matches the profile
//! the job was compiled for. A snapshot older than 2 s is shown as stale.

use std::time::{Duration, Instant};

use super::*;

fn idle() -> MachineStatus {
    MachineStatus {
        connection: ConnectionState::Connected,
        state: MachineState::Idle,
        ..MachineStatus::initial(Instant::now())
    }
}

fn info(profile: Option<&str>) -> DeviceInfo {
    DeviceInfo {
        model: "Generic Cutter M".into(),
        serial_number: Some("SN1".into()),
        firmware: None,
        profile_id: profile.map(str::to_owned),
        transport: TransportKind::Usb,
    }
}

#[test]
fn a_new_snapshot_is_disconnected_and_unknown() {
    let s = MachineStatus::initial(Instant::now());
    assert_eq!(s.connection, ConnectionState::Disconnected);
    assert_eq!(s.state, MachineState::Unknown);
    assert_eq!(s.power, PowerState::Unknown);
    assert!(s.progress.is_none() && s.mat_loaded.is_none() && s.position_mm.is_none());
    assert_eq!(s.alerts.len(), 0);
}

#[test]
fn a_snapshot_older_than_two_seconds_is_stale() {
    let at = Instant::now();
    let s = MachineStatus::initial(at);
    assert!(!s.is_stale(at + Duration::from_millis(1_999)));
    assert!(s.is_stale(at + Duration::from_millis(2_001)));
    assert!(!s.is_stale(at), "never stale before it was taken");
}

#[test]
fn an_idle_connected_machine_with_the_right_profile_may_run() {
    assert_eq!(
        check_can_run(
            &idle(),
            &info(Some("generic.cutter-medium")),
            "generic.cutter-medium"
        ),
        Ok(())
    );
}

#[test]
fn a_machine_that_reports_no_profile_is_not_blocked_by_it() {
    assert_eq!(
        check_can_run(&idle(), &info(None), "generic.cutter-medium"),
        Ok(())
    );
}

#[test]
fn a_profile_mismatch_says_which_profile_to_select() {
    let r = check_can_run(
        &idle(),
        &info(Some("generic.cutter-large")),
        "generic.cutter-medium",
    );
    assert_eq!(
        r,
        Err(RunRefusal::ProfileMismatch {
            machine_reports: "generic.cutter-large".into(),
            job_compiled_for: "generic.cutter-medium".into(),
        })
    );
    let text = r.unwrap_err().to_string();
    assert!(
        text.contains("generic.cutter-large") && text.contains("generic.cutter-medium"),
        "{text}"
    );
}

#[test]
fn every_connection_state_but_connected_refuses() {
    for c in [
        ConnectionState::Disconnected,
        ConnectionState::Connecting,
        ConnectionState::Lost,
    ] {
        let s = MachineStatus {
            connection: c,
            ..idle()
        };
        assert_eq!(
            check_can_run(&s, &info(None), "x"),
            Err(RunRefusal::NotConnected),
            "{c:?}"
        );
    }
}

#[test]
fn every_machine_state_but_idle_refuses() {
    let busy = [
        MachineState::Busy,
        MachineState::RunningJob,
        MachineState::Paused,
        MachineState::UpdatingFirmware,
        MachineState::Unknown,
        MachineState::WaitingForUser(UserAction {
            id: 1,
            kind: UserActionKind::Confirm,
            message: String::new(),
        }),
        MachineState::Fault(DeviceError::new("e", "broken")),
    ];
    for state in busy {
        let s = MachineStatus {
            state: state.clone(),
            ..idle()
        };
        assert_eq!(
            check_can_run(&s, &info(None), "x"),
            Err(RunRefusal::NotIdle),
            "{state:?}"
        );
    }
}

#[test]
fn not_connected_is_reported_before_not_idle() {
    let s = MachineStatus {
        connection: ConnectionState::Lost,
        state: MachineState::Busy,
        ..idle()
    };
    assert_eq!(
        check_can_run(&s, &info(None), "x"),
        Err(RunRefusal::NotConnected)
    );
}

#[test]
fn device_errors_print_their_code_and_message() {
    assert_eq!(
        DeviceError::new("timeout", "no reply").to_string(),
        "timeout: no reply"
    );
}

#[test]
fn conservative_capabilities_by_default() {
    let c = Capabilities::default();
    assert!(!c.pause && !c.resume && !c.cancel && !c.jog && !c.home);
    assert_eq!(c.max_inflight_lines, 1);
}
