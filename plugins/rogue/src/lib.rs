//! A device plugin that misbehaves on purpose, so the host's sandbox can be tested: its discovery
//! names a device it was never shown (and hangs on a port named `spin`), its status
//! call never returns, its send call asks for far more memory than a plugin may have, and its
//! jog call tries to read the host's files.

wit_bindgen::generate!({
    path: "../../crates/ps-device/wit",
    world: "device-plugin",
});

use exports::plotter::device::driver::{Guest, GuestSession};
use plotter::device::transport::Connection;
use plotter::device::types::{
    Capabilities, DeviceError, DeviceInfo, Event, GcodeLine, JobHeader, MachineState, PowerState,
    Status,
};

struct Component;

struct Rogue;

impl Guest for Component {
    type Session = Rogue;

    fn discover() -> Vec<String> {
        let shown = plotter::device::discovery::list_serial_ports();
        // A port named "spin" makes it hang, so the host's discovery deadline can be tested.
        if shown.iter().any(|c| c.name == "spin") {
            let mut n = 0_u64;
            loop {
                n = std::hint::black_box(n.wrapping_add(1));
            }
        }
        // It names what it was shown and a device it made up; the host must drop the latter.
        let mut ids: Vec<String> = shown.into_iter().map(|c| c.id).collect();
        ids.push("fabricated:1".to_owned());
        ids
    }

    fn identify(_conn: &Connection) -> Option<DeviceInfo> {
        None
    }
}

impl GuestSession for Rogue {
    fn new(_conn: Connection) -> Self {
        Self
    }

    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            model: "rogue".to_owned(),
            serial_number: None,
            firmware: None,
            profile_id: None,
            transport: plotter::device::types::TransportKind::Serial,
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            pause: false,
            resume: false,
            cancel: false,
            jog: false,
            home: false,
            load_mat: false,
            unload_mat: false,
            tool_detection: false,
            reports_position: false,
            reports_temperature: false,
            max_inflight_lines: 1,
        }
    }

    fn status(&self) -> Status {
        // Never returns.
        let mut n = 0_u64;
        loop {
            n = std::hint::black_box(n.wrapping_add(1));
        }
    }

    fn wait_event(&self, _timeout_ms: u32) -> Option<Event> {
        None
    }

    fn begin_job(&self, header: JobHeader) -> Result<(), DeviceError> {
        // Dials the internet host named by the job id and reports whether the host allowed it.
        let result = plotter::device::transport::open_declared_host(
            &header.job_id,
            plotter::device::types::TransportKind::Http,
        );
        let (code, message) = match result {
            Ok(_) => ("host-opened".to_owned(), String::new()),
            Err(e) => ("host-refused".to_owned(), e.message),
        };
        Err(DeviceError {
            code,
            message,
            recoverable: true,
        })
    }

    fn send(&self, _lines: Vec<GcodeLine>) -> Result<u32, DeviceError> {
        // 200 MB, touched so it is really allocated.
        let big = std::hint::black_box(vec![1_u8; 200 * 1024 * 1024]);
        let touched: u64 = big.iter().step_by(4096).map(|b| u64::from(*b)).sum();
        Ok(u32::try_from(touched % 7).unwrap_or(0))
    }

    fn end_job(&self) -> Result<(), DeviceError> {
        Ok(())
    }

    fn pause(&self) -> Result<(), DeviceError> {
        Ok(())
    }

    fn resume(&self) -> Result<(), DeviceError> {
        Ok(())
    }

    fn cancel(&self) -> Result<(), DeviceError> {
        Ok(())
    }

    fn user_action_done(&self, _id: u32) -> Result<(), DeviceError> {
        Ok(())
    }

    fn jog(&self, _dx_mm: f64, _dy_mm: f64) -> Result<(), DeviceError> {
        // Tries to read the host's files; the sandbox must refuse.
        let code = match std::fs::read_dir(".") {
            Ok(_) => "fs-allowed",
            Err(_) => "fs-denied",
        };
        Err(DeviceError {
            code: code.to_owned(),
            message: String::new(),
            recoverable: true,
        })
    }

    fn disconnect(&self) {}
}

export!(Component);

#[allow(dead_code)]
fn _states() -> (MachineState, PowerState) {
    (MachineState::Idle, PowerState::On)
}
