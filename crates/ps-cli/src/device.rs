//! `pstudio devices` and `pstudio run`: list machines and send a `.psjob` to one.
//!
//! Both use the same device host and job runner as the app. Hardware transports are not wired
//! into the command line yet, so the only machine it can reach is the built-in simulator
//! (`--simulate`, id `sim`); a request for any other id is refused with the ids that exist.

use std::sync::Arc;

use ps_device::{
    ConnectionState, DeviceHost, DeviceId, HandleConfig, HandleEvent, JobHeader, JobOutcome,
    MachineHandle, MachineState, PermissionStore, RunRequest, SIMULATOR_ID, SimulatorPlugin,
};
use ps_job::Bundle;
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;

use crate::{DeviceTestArgs, DevicesArgs, Failure, Io, RunArgs};

const SIM_ID: &str = SIMULATOR_ID;

fn runtime() -> Result<tokio::runtime::Runtime, Failure> {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|e| Failure::usage(format!("could not start the async runtime: {e}")))
}

fn host(simulate: bool) -> DeviceHost {
    let host = DeviceHost::new(PermissionStore::default(), HandleConfig::default());
    if simulate {
        host.add_plugin(Arc::new(SimulatorPlugin::new()));
    }
    host
}

pub(crate) fn devices(args: &DevicesArgs, io: &mut Io<'_>) -> Result<(), Failure> {
    let rt = runtime()?;
    let host = host(args.simulate);
    rt.block_on(host.refresh());
    let list = host.devices().borrow().clone();
    if args.json {
        let items: Vec<_> = list
            .iter()
            .map(|d| {
                json!({
                    "id": d.id.0,
                    "model": d.info.model,
                    "plugin": d.plugin_id,
                    "serial_number": d.info.serial_number,
                    "approved": d.approved,
                })
            })
            .collect();
        let _ = writeln!(io.out, "{}", json!({ "devices": items }));
        return Ok(());
    }
    if list.is_empty() {
        let _ = writeln!(
            io.out,
            "no devices found (use --simulate to try the built-in simulator)"
        );
        return Ok(());
    }
    for d in &list {
        let _ = writeln!(
            io.out,
            "{}\t{}\t{}",
            d.id.0,
            d.info.model,
            if d.approved {
                "approved"
            } else {
                "needs approval"
            }
        );
    }
    Ok(())
}

pub(crate) fn run(args: &RunArgs, io: &mut Io<'_>) -> Result<(), Failure> {
    let bytes = std::fs::read(&args.input)
        .map_err(|e| Failure::usage(format!("{}: {e}", args.input.display())))?;
    let bundle = Bundle::from_bytes(&bytes)
        .map_err(|e| Failure::refused(format!("{}: {e}", args.input.display())))?;

    let mats: Vec<_> = bundle
        .manifest
        .mats
        .iter()
        .filter(|m| args.mat.is_none_or(|n| n == m.index))
        .collect();
    if mats.is_empty() {
        return Err(Failure::usage(match args.mat {
            Some(n) => format!("the bundle has no mat {n}"),
            None => "the bundle has no mats".to_owned(),
        }));
    }

    let rt = runtime()?;
    // Hardware transports are not wired in yet: `--simulate` is implied for the one id we have.
    let host = host(args.device == SIM_ID || args.simulate);
    rt.block_on(host.refresh());
    let id = DeviceId(args.device.clone());
    let known: Vec<String> = host
        .devices()
        .borrow()
        .iter()
        .map(|d| d.id.0.clone())
        .collect();
    if !known.contains(&id.0) {
        return Err(Failure::usage(format!(
            "no device {:?}; available: {}",
            args.device,
            if known.is_empty() {
                "none (hardware transports are not available in the command line yet)".to_owned()
            } else {
                known.join(", ")
            }
        )));
    }
    // Running from the command line is itself the user's approval of this machine.
    host.approve(&id);

    let mut failed = false;
    for mat in mats {
        let Some(program) = bundle.gcode(mat.index) else {
            return Err(Failure::refused(format!("mat {} has no G-code", mat.index)));
        };
        let handle = rt
            .block_on(host.connect(&id))
            .map_err(|e| Failure::usage(e.to_string()))?;
        let header = JobHeader {
            job_id: bundle.manifest.job_id.clone(),
            mat_index: mat.index,
            profile_id: bundle.manifest.profile.to_string(),
            tools: Vec::new(),
        };
        let outcome = rt.block_on(run_on(&handle, program, header, args.yes, io));
        match outcome {
            Ok(JobOutcome::Completed { lines }) => {
                let _ = writeln!(io.out, "mat {}: completed ({lines} lines)", mat.index);
            }
            Ok(JobOutcome::Cancelled { lines_done }) => {
                failed = true;
                let _ = writeln!(
                    io.out,
                    "mat {}: cancelled after {lines_done} lines",
                    mat.index
                );
            }
            Ok(JobOutcome::Lost { lines_done }) => {
                failed = true;
                let _ = writeln!(
                    io.out,
                    "mat {}: the machine stopped answering after {lines_done} lines",
                    mat.index
                );
            }
            Ok(JobOutcome::Faulted { error, .. }) => {
                failed = true;
                let _ = writeln!(io.out, "mat {}: faulted: {error}", mat.index);
            }
            Err(message) => return Err(Failure::refused(message)),
        }
        if failed {
            break;
        }
    }
    if failed {
        Err(Failure::refused("the job did not complete"))
    } else {
        Ok(())
    }
}

/// Runs one mat on a connected machine, confirming prompts only when `yes`.
async fn run_on(
    handle: &MachineHandle,
    program: &str,
    header: JobHeader,
    yes: bool,
    io: &mut Io<'_>,
) -> Result<JobOutcome, String> {
    let mut events = handle.subscribe();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let status = handle.status();
        if status.connection == ConnectionState::Connected && status.state == MachineState::Idle {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            return Err("the machine did not become ready".to_owned());
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    handle
        .run(RunRequest {
            program: program.to_owned(),
            header,
        })
        .map_err(|e| e.to_string())?;
    loop {
        match events.recv().await {
            Ok(HandleEvent::UserActionRequired(action)) => {
                let _ = writeln!(io.out, "prompt: {}", action.message);
                if yes {
                    handle.ack_user_action(action.id);
                } else {
                    let _ = writeln!(
                        io.err,
                        "pstudio: the job needs you; rerun with --yes to confirm prompts"
                    );
                    handle.cancel();
                }
            }
            Ok(HandleEvent::JobFinished(outcome)) => return Ok(outcome),
            Ok(HandleEvent::Lost) => return Err("the machine stopped answering".to_owned()),
            Ok(HandleEvent::StatusChanged(_)) | Err(RecvError::Lagged(_)) => {}
            Err(RecvError::Closed) => return Err("the device session ended".to_owned()),
        }
    }
}

/// `pstudio device test`: checks a plugin package against a recording and/or a simulated machine.
pub(crate) fn test_plugin(args: &DeviceTestArgs, io: &mut Io<'_>) -> Result<(), Failure> {
    use ps_device::{Transcript, check_against_recording, check_in_simulation, unpack};

    if args.transcript.is_none() && !args.simulate {
        return Err(Failure::usage(
            "give --transcript <session.json> and/or --simulate to say what to test the plugin against",
        ));
    }
    let bytes = std::fs::read(&args.plugin)
        .map_err(|e| Failure::usage(format!("cannot read {}: {e}", args.plugin.display())))?;
    let package = unpack(&bytes).map_err(|e| Failure::refused(e.to_string()))?;
    let _ = writeln!(
        io.out,
        "plugin {} ({}) version {}",
        package.manifest.name, package.manifest.id, package.version
    );
    let recording = match &args.transcript {
        Some(path) => Some(
            Transcript::load(path)
                .map_err(|e| Failure::usage(format!("cannot read {}: {e}", path.display())))?,
        ),
        None => None,
    };
    let rt = runtime()?;
    let mut failures = Vec::new();
    if let Some(recording) = &recording {
        let report = rt.block_on(check_against_recording(&package.wasm, recording));
        match &report.identified {
            Some(model) => {
                let _ = writeln!(io.out, "identified the recorded device as {model}");
            }
            None => failures.push("the plugin did not recognise the recorded device".to_owned()),
        }
        failures.extend(report.mismatch);
        failures.extend(
            report
                .problems
                .into_iter()
                .filter(|p| !p.contains("did not recognise")),
        );
    }
    if args.simulate {
        let report = rt.block_on(check_in_simulation(&package.wasm));
        if report.passed() {
            let _ = writeln!(
                io.out,
                "ran a {}-line job against the simulated machine",
                report.lines_sent
            );
        }
        failures.extend(report.problems);
    }
    if failures.is_empty() {
        let _ = writeln!(io.out, "passed");
        Ok(())
    } else {
        for f in &failures {
            let _ = writeln!(io.out, "  - {f}");
        }
        let _ = writeln!(io.out, "failed");
        Err(Failure::refused(format!(
            "the plugin failed {} check(s)",
            failures.len()
        )))
    }
}
