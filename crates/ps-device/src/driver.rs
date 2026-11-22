//! The interface a device plugin implements.
//!
//! A driver translates the fixed `ps-gcode/1` dialect into one machine's protocol and reports the
//! machine's state. Drivers never parse G-code themselves: every line arrives with the host's
//! parsed form beside the exact text. A sandboxed WebAssembly plugin is one implementation of
//! this trait; the simulated machine and native drivers are others.

use std::time::Duration;

use async_trait::async_trait;

use crate::types::{
    Capabilities, DeviceError, DeviceInfo, GcodeLine, JobHeader, Status, UserAction,
};

/// Something a driver tells the host without being asked.
#[derive(Debug, Clone, PartialEq)]
pub enum DriverEvent {
    /// The machine's status changed.
    StatusChanged(Status),
    /// The machine needs the user before the job can continue.
    UserActionRequired(UserAction),
    /// The job finished on the machine.
    JobFinished,
    /// The machine or the driver hit a fault.
    Fault(DeviceError),
}

/// One machine, reached through one transport.
#[async_trait]
pub trait Driver: Send {
    /// What the device is.
    fn info(&self) -> DeviceInfo;

    /// What the device can do.
    fn capabilities(&self) -> Capabilities;

    /// Reads the device's status.
    async fn status(&mut self) -> Result<Status, DeviceError>;

    /// Waits up to `timeout` for the next event; `None` when there is none.
    async fn wait_event(&mut self, timeout: Duration) -> Option<DriverEvent>;

    /// Starts a job.
    async fn begin_job(&mut self, header: JobHeader) -> Result<(), DeviceError>;

    /// Offers lines to the machine and returns how many it accepted, from the front.
    async fn send(&mut self, lines: &[GcodeLine]) -> Result<u32, DeviceError>;

    /// Ends the job.
    async fn end_job(&mut self) -> Result<(), DeviceError>;

    /// Pauses the job.
    async fn pause(&mut self) -> Result<(), DeviceError>;

    /// Resumes a paused job.
    async fn resume(&mut self) -> Result<(), DeviceError>;

    /// Cancels the job.
    async fn cancel(&mut self) -> Result<(), DeviceError>;

    /// Tells the machine the user answered a prompt.
    async fn user_action_done(&mut self, id: u32) -> Result<(), DeviceError>;

    /// Moves the carriage by an offset in mm.
    async fn jog(&mut self, dx_mm: f64, dy_mm: f64) -> Result<(), DeviceError>;

    /// Closes the session.
    async fn disconnect(&mut self);
}
