//! The libraries, machine and calibration a compiling command works with.

use std::path::Path;
use std::sync::Arc;

use ps_library::{
    Calibration, LibrarySnapshot, MachineProfile, builtin_entries, install_builtin, open_at,
};

use crate::{Failure, Setup};

/// Everything a command needs to compile.
pub(crate) struct Context {
    pub libs: LibrarySnapshot,
    pub profile: Arc<MachineProfile>,
    pub calibration: Calibration,
}

/// The built-in library, plus a user library folder when one is given.
pub(crate) fn libraries(library_dir: Option<&Path>) -> Result<LibrarySnapshot, Failure> {
    if let Some(dir) = library_dir {
        let mut lib = open_at(dir).map_err(|e| {
            Failure::usage(format!("cannot open library folder {}: {e}", dir.display()))
        })?;
        install_builtin(&mut lib)
            .map_err(|e| Failure::usage(format!("cannot load the built-in library: {e}")))?;
        return lib
            .snapshot()
            .map_err(|e| Failure::usage(format!("cannot read the library: {e}")));
    }
    let entries = builtin_entries().map_err(|issues| {
        Failure::usage(format!(
            "the built-in library is broken: {}",
            issues
                .first()
                .map_or_else(String::new, |i| i.message.clone())
        ))
    })?;
    Ok(LibrarySnapshot::from_entries(entries))
}

/// The newest version of the machine profile with this id.
pub(crate) fn select_profile(
    libs: &LibrarySnapshot,
    machine: Option<&str>,
) -> Result<Arc<MachineProfile>, Failure> {
    let profiles = libs.profiles();
    let mut ids: Vec<&str> = profiles.iter().map(|p| p.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    let Some(wanted) = machine else {
        return Err(Failure::usage(format!(
            "--machine is required; available machines: {}",
            ids.join(", ")
        )));
    };
    profiles
        .iter()
        .filter(|p| p.id == wanted)
        .max_by(|a, b| a.version.cmp(&b.version))
        .cloned()
        .ok_or_else(|| {
            Failure::usage(format!(
                "unknown machine {wanted:?}; available machines: {}",
                ids.join(", ")
            ))
        })
}

fn load_calibration(path: Option<&Path>) -> Result<Calibration, Failure> {
    let Some(path) = path else {
        return Ok(Calibration::default());
    };
    let text = std::fs::read_to_string(path)
        .map_err(|e| Failure::usage(format!("cannot read {}: {e}", path.display())))?;
    let calibration: Calibration = serde_json::from_str(&text).map_err(|e| {
        Failure::usage(format!("{} is not a calibration file: {e}", path.display()))
    })?;
    if let Some(issue) = calibration.validate().into_iter().next() {
        return Err(Failure::usage(format!(
            "{}: {} {}",
            path.display(),
            issue.pointer,
            issue.message
        )));
    }
    Ok(calibration)
}

impl Context {
    /// Builds the context from the command line's setup options.
    pub(crate) fn from_setup(setup: &Setup) -> Result<Self, Failure> {
        let libs = libraries(setup.library_dir.as_deref())?;
        let profile = select_profile(&libs, setup.machine.as_deref())?;
        let calibration = load_calibration(setup.calibration.as_deref())?;
        Ok(Self {
            libs,
            profile,
            calibration,
        })
    }
}
