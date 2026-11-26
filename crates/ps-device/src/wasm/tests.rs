//! Ground truth for the WebAssembly plugin host: the real GRBL component is loaded
//! and driven through a scripted transport, and a misbehaving component shows the sandbox limits:
//! a call that never returns is stopped, a plugin that wants too much memory is refused, and a
//! plugin has no file or network access of its own. The plugins are built for `wasm32-wasip2` by
//! these tests.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ps_gcode::parse_program;

use super::*;
use crate::bus::ScopedBuses;
use crate::driver::{Driver, DriverEvent};
use crate::host::{DeviceId, DiscoveredDevice};
use crate::types::{GcodeLine, JobHeader, MachineState};
use crate::wasm::tests_support::{FakeBuses, plugin as build, port, serial_filters};

// ---- a scripted blocking transport ----

#[derive(Default)]
struct Wire {
    written: Vec<u8>,
    replies: Vec<u8>,
}

#[derive(Clone, Default)]
struct Mock(Arc<Mutex<Wire>>);

impl Mock {
    fn script(&self, text: &str) {
        self.0
            .lock()
            .unwrap()
            .replies
            .extend_from_slice(text.as_bytes());
    }
    fn written(&self) -> Vec<u8> {
        self.0.lock().unwrap().written.clone()
    }
    fn written_text(&self) -> String {
        String::from_utf8_lossy(&self.written()).into_owned()
    }
}

impl BlockingTransport for Mock {
    fn kind(&self) -> TransportKind {
        TransportKind::Serial
    }
    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        self.0.lock().unwrap().written.extend_from_slice(data);
        Ok(())
    }
    fn read(&mut self, max: usize, _timeout: Duration) -> Result<Vec<u8>, String> {
        let mut wire = self.0.lock().unwrap();
        let n = wire.replies.len().min(max);
        Ok(wire.replies.drain(..n).collect())
    }
}

fn host() -> WasmHost {
    WasmHost::new(WasmLimits::default()).expect("a wasm engine")
}

async fn open_grbl(mock: &Mock) -> WasmDriver {
    host()
        .open(&build("grbl-plugin"), Box::new(mock.clone()))
        .await
        .expect("the GRBL component opens")
}

fn lines(text: &str) -> Vec<GcodeLine> {
    parse_program(text)
        .unwrap()
        .into_iter()
        .map(|l| GcodeLine {
            number: l.number,
            text: l.text,
            command: l.command,
        })
        .collect()
}

// ---- the GRBL component through the host ----

#[tokio::test]
async fn the_component_loads_and_describes_itself() {
    let mock = Mock::default();
    let d = open_grbl(&mock).await;
    assert!(d.info().model.starts_with("GRBL"), "{}", d.info().model);
    let caps = d.capabilities();
    assert!(caps.pause && caps.resume && caps.cancel && caps.jog && caps.reports_position);
    assert!(caps.max_inflight_lines > 0);
}

#[tokio::test]
async fn the_startup_banner_identifies_a_grbl_machine() {
    let mock = Mock::default();
    mock.script("\r\nGrbl 1.1h ['$' for help]\r\n");
    let info = host()
        .identify(&build("grbl-plugin"), Box::new(mock.clone()))
        .await
        .expect("identified");
    assert_eq!(info.model, "GRBL 1.1h");
    assert_eq!(info.firmware.as_deref(), Some("1.1h"));
    assert_eq!(
        mock.written(),
        vec![0x18],
        "a soft reset provokes the banner"
    );
}

#[tokio::test]
async fn something_that_is_not_grbl_is_not_identified() {
    let mock = Mock::default();
    mock.script("Marlin 2.1\n");
    assert!(
        host()
            .identify(&build("grbl-plugin"), Box::new(mock))
            .await
            .is_none()
    );
}

#[tokio::test]
async fn lines_are_translated_by_the_plugin_and_written_to_the_transport() {
    let mock = Mock::default();
    let mut d = open_grbl(&mock).await;
    let accepted = d
        .send(&lines(
            "G21\nG90\nT1 M6\nG0 X1.000 Y2.000\nG1 X3.000 Y2.000 F6000\nM3 S150\nM5\n",
        ))
        .await
        .unwrap();
    assert_eq!(
        accepted, 7,
        "the tool change has no GRBL form but is accepted"
    );
    assert_eq!(
        mock.written_text(),
        "G21\nG90\nG0 X1.000 Y2.000\nG1 X3.000 Y2.000 F6000\nM3 S150\nM5\n"
    );
}

#[tokio::test]
async fn the_plugin_applies_grbls_receive_buffer_limit() {
    let mock = Mock::default();
    let mut d = open_grbl(&mock).await;
    let mut program = String::new();
    for i in 0..40 {
        writeln!(program, "G0 X{i}.000 Y1.000").unwrap();
    }
    let batch = lines(&program);
    let first = d.send(&batch).await.unwrap();
    assert!(first > 0 && (first as usize) < batch.len(), "{first}");
    assert!(mock.written().len() <= 127);
}

#[tokio::test]
async fn status_reports_come_back_in_the_hosts_types() {
    let mock = Mock::default();
    mock.script("<Idle|MPos:12.500,20.000,0.000|FS:0,0>\nok\n");
    let mut d = open_grbl(&mock).await;
    let s = d.status().await.unwrap();
    assert_eq!(s.state, MachineState::Idle);
    assert_eq!(s.position_mm, Some((12.5, 20.0)));
    assert_eq!(mock.written(), b"?");
}

#[tokio::test]
async fn errors_and_prompts_become_driver_events() {
    let mock = Mock::default();
    let mut d = open_grbl(&mock).await;
    d.send(&lines("M0 (Load pen)\nG0 X1.000 Y1.000\n"))
        .await
        .unwrap();
    assert!(d.wait_event(Duration::from_millis(1)).await.is_none());
    mock.script("ok\n");
    match d.wait_event(Duration::from_millis(1)).await {
        Some(DriverEvent::UserActionRequired(a)) => assert_eq!(a.message, "Load pen"),
        other => panic!("{other:?}"),
    }
    mock.script("error:20\n");
    match d.wait_event(Duration::from_millis(1)).await {
        Some(DriverEvent::Fault(e)) => assert_eq!(e.code, "grbl-error-20"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn pause_resume_cancel_and_jog_are_realtime_bytes_and_commands() {
    let mock = Mock::default();
    let mut d = open_grbl(&mock).await;
    d.pause().await.unwrap();
    d.resume().await.unwrap();
    assert_eq!(mock.written(), b"!~");
    d.jog(1.5, -2.0).await.unwrap();
    assert!(
        mock.written_text()
            .ends_with("$J=G91 G21 X1.500 Y-2.000 F1000\n")
    );
    d.cancel().await.unwrap();
    assert_eq!(*mock.written().last().unwrap(), 0x18);
}

#[tokio::test]
async fn a_jog_during_a_job_is_refused_and_leaves_the_job_alone() {
    let mock = Mock::default();
    let mut d = open_grbl(&mock).await;
    d.send(&lines("M0 (Load pen)\nG0 X1.000 Y1.000\n"))
        .await
        .unwrap();
    let before = mock.written();
    assert!(d.jog(1.0, 1.0).await.is_err(), "a job is running");
    assert_eq!(mock.written(), before, "nothing was written for the jog");
    // The job's own acknowledgement still belongs to the job's first line.
    mock.script("ok\n");
    assert!(matches!(
        d.wait_event(Duration::from_millis(1)).await,
        Some(DriverEvent::UserActionRequired(_))
    ));
}

#[tokio::test]
async fn the_acknowledgement_of_a_jog_is_not_taken_for_a_job_line() {
    let mock = Mock::default();
    let mut d = open_grbl(&mock).await;
    d.jog(1.0, 0.0).await.unwrap();
    mock.script("ok\n");
    assert!(d.wait_event(Duration::from_millis(1)).await.is_none());
    d.send(&lines("M0 (Load pen)\n")).await.unwrap();
    assert!(
        d.wait_event(Duration::from_millis(1)).await.is_none(),
        "the prompt waits for the pause line's own ok"
    );
    mock.script("ok\n");
    assert!(matches!(
        d.wait_event(Duration::from_millis(1)).await,
        Some(DriverEvent::UserActionRequired(_))
    ));
}

#[tokio::test]
async fn the_job_is_finished_when_grbl_is_idle_not_when_it_accepts_the_last_line() {
    let mock = Mock::default();
    let mut d = open_grbl(&mock).await;
    d.send(&lines("G0 X1.000 Y1.000\nM2\n")).await.unwrap();
    mock.script("ok\nok\n");
    assert!(d.wait_event(Duration::from_millis(1)).await.is_none());
    mock.script("<Run|MPos:0.500,0.500,0.000>\n");
    assert!(
        d.wait_event(Duration::from_millis(1)).await.is_none(),
        "the planner still holds moves"
    );
    mock.script("<Idle|MPos:1.000,1.000,0.000>\n");
    assert!(matches!(
        d.wait_event(Duration::from_millis(1)).await,
        Some(DriverEvent::JobFinished)
    ));
    assert!(mock.written_text().ends_with('?'), "it asked for the state");
}

#[tokio::test]
async fn an_alarm_clears_the_flow_control_state_because_grbl_flushes_its_buffer() {
    let mock = Mock::default();
    let mut d = open_grbl(&mock).await;
    let mut program = String::new();
    for i in 0..40 {
        writeln!(program, "G0 X{i}.000 Y1.000").unwrap();
    }
    let batch = lines(&program);
    let first = d.send(&batch).await.unwrap();
    assert_eq!(d.send(&batch).await.unwrap(), 0, "the buffer is full");
    mock.script("ALARM:1\n");
    match d.wait_event(Duration::from_millis(1)).await {
        Some(DriverEvent::Fault(e)) => assert_eq!(e.code, "grbl-alarm-1"),
        other => panic!("{other:?}"),
    }
    assert_eq!(d.send(&batch).await.unwrap(), first, "GRBL has room again");
}

#[tokio::test]
async fn a_transport_that_fails_while_waiting_is_a_fault_not_silence() {
    struct Unplugged;
    impl BlockingTransport for Unplugged {
        fn kind(&self) -> TransportKind {
            TransportKind::Serial
        }
        fn write(&mut self, _: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn read(&mut self, _: usize, _: Duration) -> Result<Vec<u8>, String> {
            Err("device unplugged".into())
        }
    }
    let mut d = host()
        .open(&build("grbl-plugin"), Box::new(Unplugged))
        .await
        .unwrap();
    match d.wait_event(Duration::from_millis(1)).await {
        Some(DriverEvent::Fault(e)) => assert_eq!(e.code, "io"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_whole_job_runs_through_the_runner_and_the_wasm_plugin() {
    use crate::runner::{Control, JobOutcome, RunConfig, RunEvent, run_job};
    use tokio::sync::mpsc;

    /// Answers every line like GRBL would.
    #[derive(Clone, Default)]
    struct Emulator(Arc<Mutex<(Vec<u8>, Vec<u8>)>>);
    impl BlockingTransport for Emulator {
        fn kind(&self) -> TransportKind {
            TransportKind::Serial
        }
        fn write(&mut self, data: &[u8]) -> Result<(), String> {
            let mut s = self.0.lock().unwrap();
            for &b in data {
                match b {
                    b'?' => s.1.extend_from_slice(b"<Idle|MPos:0.000,0.000,0.000>\n"),
                    b'\n' => s.1.extend_from_slice(b"ok\n"),
                    _ => {}
                }
            }
            Ok(())
        }
        fn read(&mut self, max: usize, _t: Duration) -> Result<Vec<u8>, String> {
            let mut s = self.0.lock().unwrap();
            let n = s.1.len().min(max);
            Ok(s.1.drain(..n).collect())
        }
    }

    let mut driver = host()
        .open(&build("grbl-plugin"), Box::new(Emulator::default()))
        .await
        .unwrap();
    let program = "G21\nG90\nM0 (Load pen)\nT1 M6\nG0 X12.500 Y20.000\nM3 S150\nG4 P80\nG1 X62.500 Y20.000 F6000\nM5\nM0 (Unload mat)\nM2\n";
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut asked = Vec::new();
    let mut observer = |e: RunEvent| {
        if let RunEvent::UserActionRequired(a) = &e {
            let _ = tx.send(Control::Ack(a.id));
            asked.push(a.message.clone());
        }
    };
    let header = JobHeader {
        job_id: "j".into(),
        mat_index: 1,
        profile_id: "p".into(),
        tools: vec![],
    };
    let outcome = run_job(
        &mut driver,
        program,
        header,
        &mut rx,
        &mut observer,
        &RunConfig::default(),
    )
    .await
    .unwrap();
    assert!(
        matches!(outcome, JobOutcome::Completed { .. }),
        "{outcome:?}"
    );
    assert_eq!(asked, vec!["Load pen", "Unload mat"]);
}

// ---- the sandbox ----

#[tokio::test]
async fn a_call_that_never_returns_is_stopped_at_the_deadline() {
    let limits = WasmLimits {
        call_timeout: Duration::from_millis(300),
        ..WasmLimits::default()
    };
    let host = WasmHost::new(limits).unwrap();
    let mut d = host
        .open(&build("rogue-plugin"), Box::new(Mock::default()))
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let error = d.status().await.expect_err("the status call is cut off");
    assert_eq!(error.code, "plugin-timeout", "{error:?}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_plugin_that_asks_for_too_much_memory_is_refused() {
    let limits = WasmLimits {
        memory_bytes: 64 * 1024 * 1024,
        ..WasmLimits::default()
    };
    let host = WasmHost::new(limits).unwrap();
    let mut d = host
        .open(&build("rogue-plugin"), Box::new(Mock::default()))
        .await
        .unwrap();
    // `send` allocates 200 MB, which is over the 64 MB cap.
    let error = d.send(&[]).await.expect_err("the allocation is refused");
    assert_eq!(error.code, "plugin-memory", "{error:?}");
}

#[tokio::test]
async fn bytes_that_are_not_a_component_are_refused_with_a_reason() {
    let result = host()
        .open(b"not a component", Box::new(Mock::default()))
        .await;
    assert_eq!(result.err().unwrap().code, "plugin-invalid");
}

#[tokio::test]
async fn the_default_limits_are_two_seconds_and_sixty_four_megabytes() {
    let l = WasmLimits::default();
    assert_eq!(l.call_timeout, Duration::from_secs(2));
    assert_eq!(l.memory_bytes, 64 * 1024 * 1024);
}

#[tokio::test]
async fn a_plugin_cannot_read_the_hosts_files() {
    let limits = WasmLimits {
        memory_bytes: 512 * 1024 * 1024,
        ..WasmLimits::default()
    };
    let mut d = WasmHost::new(limits)
        .unwrap()
        .open(&build("rogue-plugin"), Box::new(Mock::default()))
        .await
        .unwrap();
    let error = d
        .jog(0.0, 0.0)
        .await
        .expect_err("the rogue jog reports what it found");
    assert_eq!(
        error.code, "fs-denied",
        "the plugin saw a filesystem: {error:?}"
    );
}

// ---- declared internet hosts ----

struct Echo;

struct EchoLink;

impl BlockingTransport for EchoLink {
    fn kind(&self) -> TransportKind {
        TransportKind::Http
    }
    fn write(&mut self, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn read(&mut self, _: usize, _: Duration) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
    }
}

impl crate::tls::InternetOpener for Echo {
    fn open(&self, _host: &str) -> Result<Box<dyn BlockingTransport>, String> {
        Ok(Box::new(EchoLink))
    }
}

fn dial_header(host: &str) -> JobHeader {
    JobHeader {
        job_id: host.to_owned(),
        mat_index: 1,
        profile_id: "p".to_owned(),
        tools: Vec::new(),
    }
}

fn internet(declared: &[&str], allowed: &[&str]) -> InternetAccess {
    InternetAccess::new(
        crate::tls::HostGate::new(
            declared.iter().map(|s| (*s).to_owned()),
            allowed.iter().map(|s| (*s).to_owned()),
        ),
        Arc::new(Echo),
    )
}

#[tokio::test]
async fn a_plugin_reaches_a_declared_and_allowed_host_and_it_is_listed_as_in_use() {
    let access = internet(&["api.vendor.example"], &["api.vendor.example"]);
    let mut d = host()
        .open_with(
            &build("rogue-plugin"),
            Box::new(Mock::default()),
            Some(access.clone()),
        )
        .await
        .unwrap();
    // The rogue plugin dials the host named by the job id and reports what happened.
    let error = d
        .begin_job(dial_header("api.vendor.example"))
        .await
        .expect_err("the rogue plugin always reports");
    assert_eq!(error.code, "host-opened", "{error:?}");
    assert_eq!(access.in_use(), vec!["api.vendor.example".to_owned()]);
}

#[tokio::test]
async fn an_undeclared_or_unallowed_host_is_refused_to_the_plugin() {
    for (declared, allowed, why) in [
        (&["a.example"][..], &["a.example"][..], "other.example"),
        (&["a.example"][..], &[][..], "a.example"),
    ] {
        let access = internet(declared, allowed);
        let mut d = host()
            .open_with(
                &build("rogue-plugin"),
                Box::new(Mock::default()),
                Some(access.clone()),
            )
            .await
            .unwrap();
        let error = d.begin_job(dial_header(why)).await.expect_err("reports");
        assert_eq!(error.code, "host-refused", "{error:?}");
        assert_eq!(access.in_use(), Vec::<String>::new());
    }
}

#[tokio::test]
async fn without_any_internet_access_every_host_is_refused() {
    let mut d = host()
        .open(&build("rogue-plugin"), Box::new(Mock::default()))
        .await
        .unwrap();
    let error = d
        .begin_job(dial_header("a.example"))
        .await
        .expect_err("reports");
    assert_eq!(error.code, "host-refused", "{error:?}");
}

// ---- discovery: the plugin decides what to look for, the host lends it the lists ----

fn buses(fake: FakeBuses) -> ScopedBuses {
    ScopedBuses::new(Arc::new(fake), serial_filters())
}

#[tokio::test]
async fn a_plugin_can_name_only_devices_the_host_showed_it() {
    // The rogue plugin lists the ports and then also names a device it made up.
    let found = host()
        .discover(
            &build("rogue-plugin"),
            buses(FakeBuses {
                serial: vec![port("COM3", None, None)],
                ..FakeBuses::default()
            }),
        )
        .await
        .expect("discovers");
    let ids: Vec<&str> = found.iter().map(|d| d.id.0.as_str()).collect();
    assert_eq!(ids, ["serial:COM3"], "the made-up device is dropped");
    assert_eq!(
        found[0].address.as_deref(),
        Some("COM3"),
        "the host's own record"
    );
}

#[tokio::test]
async fn a_discovery_that_never_returns_is_stopped_at_its_own_deadline() {
    let limits = WasmLimits {
        discover_timeout: Duration::from_millis(300),
        ..WasmLimits::default()
    };
    let started = std::time::Instant::now();
    // The rogue plugin spins forever when it sees a port named "spin".
    let error = WasmHost::new(limits)
        .unwrap()
        .discover(
            &build("rogue-plugin"),
            buses(FakeBuses {
                serial: vec![port("spin", None, None)],
                ..FakeBuses::default()
            }),
        )
        .await
        .expect_err("cut off");
    assert_eq!(error.code, "plugin-timeout", "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn a_slow_bus_does_not_trip_the_ordinary_call_deadline() {
    // Listing takes longer than an ordinary call may run; discovery has its own, longer limit.
    let limits = WasmLimits {
        call_timeout: Duration::from_millis(200),
        discover_timeout: Duration::from_secs(10),
        ..WasmLimits::default()
    };
    let found = WasmHost::new(limits)
        .unwrap()
        .discover(
            &build("rogue-plugin"),
            buses(FakeBuses {
                serial: vec![port("COM3", None, None)],
                delay: Duration::from_millis(700),
                ..FakeBuses::default()
            }),
        )
        .await
        .expect("a slow scan is not a trap");
    assert_eq!(found.len(), 1);
}

#[tokio::test]
async fn the_grbl_plugin_offers_every_serial_port_and_never_a_usb_device() {
    let found = host()
        .discover(
            &build("grbl-plugin"),
            ScopedBuses::new(
                Arc::new(FakeBuses {
                    serial: vec![
                        port("COM3", Some((0x1a86, 0x7523)), Some("USB-SERIAL CH340")),
                        port("COM9", None, None),
                    ],
                    usb: vec![DiscoveredDevice {
                        id: DeviceId("usb:mouse".into()),
                        name: "Mouse".into(),
                        transport: TransportKind::Usb,
                        usb: Some((1, 2)),
                        address: None,
                        serial: None,
                    }],
                    ..FakeBuses::default()
                }),
                vec![
                    crate::host::DeviceFilter {
                        transport: TransportKind::Serial,
                        usb: None,
                    },
                    crate::host::DeviceFilter {
                        transport: TransportKind::Usb,
                        usb: None,
                    },
                ],
            ),
        )
        .await
        .expect("discovers");
    let ids: Vec<&str> = found.iter().map(|d| d.id.0.as_str()).collect();
    assert_eq!(ids, ["serial:COM3", "serial:COM9"]);
}

#[tokio::test]
async fn a_plugin_whose_manifest_names_no_bus_discovers_nothing() {
    let found = host()
        .discover(
            &build("grbl-plugin"),
            ScopedBuses::new(
                Arc::new(FakeBuses {
                    serial: vec![port("COM3", None, None)],
                    ..FakeBuses::default()
                }),
                Vec::new(),
            ),
        )
        .await
        .expect("discovers");
    assert_eq!(found.len(), 0);
}
