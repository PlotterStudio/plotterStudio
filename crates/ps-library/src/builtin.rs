//! The built-in library shipped inside the binary: the four machine profiles
//! (three generic cutting plotters and a generic GRBL pen plotter) plus starter tools
//! and materials. Files live in `crates/ps-library/builtin/` and are validated like any other.

use crate::LibraryKind;
use crate::schema::{Entry, Issue};
use crate::store::{Library, StoreError};
#[cfg(test)]
use crate::{MachineProfile, Material, Tool};

const TOOLS: [&str; 5] = [
    include_str!("../builtin/tools/generic.drag-blade.json"),
    include_str!("../builtin/tools/generic.scoring-stylus.json"),
    include_str!("../builtin/tools/pen.fineliner-black-0.4.json"),
    include_str!("../builtin/tools/generic.foil-transfer.json"),
    include_str!("../builtin/tools/generic.rotary-wheel.json"),
];

const PROFILES: [&str; 4] = [
    include_str!("../builtin/profiles/generic.cutter-small.json"),
    include_str!("../builtin/profiles/generic.cutter-medium.json"),
    include_str!("../builtin/profiles/generic.cutter-large.json"),
    include_str!("../builtin/profiles/generic.grbl-pen-plotter.json"),
];

const MATERIALS: [&str; 2] = [
    include_str!("../builtin/materials/community.cardstock-80lb.json"),
    include_str!("../builtin/materials/community.smooth-vinyl.json"),
];

/// Loads and validates every built-in entry.
///
/// # Errors
/// The issues of the first built-in file that fails to validate (a bug in the shipped data).
pub fn entries() -> Result<Vec<Entry>, Vec<Issue>> {
    let sets = [
        (LibraryKind::Tool, TOOLS.as_slice()),
        (LibraryKind::Profile, PROFILES.as_slice()),
        (LibraryKind::Material, MATERIALS.as_slice()),
    ];
    let mut out = Vec::new();
    for (kind, texts) in sets {
        for text in texts {
            out.push(Entry::from_json(kind, text)?);
        }
    }
    Ok(out)
}

/// Adds every built-in entry to a library as read-only built-ins.
///
/// # Errors
/// [`StoreError`] for I/O errors, or [`StoreError::Invalid`] if the shipped data is broken.
pub fn install_into(lib: &mut Library) -> Result<(), StoreError> {
    for entry in entries().map_err(StoreError::Invalid)? {
        lib.add_builtin(&entry)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
