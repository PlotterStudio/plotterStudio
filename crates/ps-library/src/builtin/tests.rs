//! Ground truth for the built-in library: four machine profiles (three generic
//! cutting plotters and a generic GRBL pen plotter) plus starter tools and materials.
//! They must validate, and they must be consistent with each other.

use std::collections::BTreeSet;

use super::*;
use crate::{Entry, Library, LibraryKind};

fn all() -> Vec<Entry> {
    entries().expect("built-in library must be valid")
}

fn tools() -> Vec<Tool> {
    all()
        .into_iter()
        .filter_map(|e| {
            if let Entry::Tool(t) = e {
                Some(t)
            } else {
                None
            }
        })
        .collect()
}

fn profiles() -> Vec<MachineProfile> {
    all()
        .into_iter()
        .filter_map(|e| {
            if let Entry::Profile(p) = e {
                Some(p)
            } else {
                None
            }
        })
        .collect()
}

fn materials() -> Vec<Material> {
    all()
        .into_iter()
        .filter_map(|e| {
            if let Entry::Material(m) = e {
                Some(m)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn the_four_documented_profiles_are_present() {
    let ids: BTreeSet<String> = profiles().into_iter().map(|p| p.id).collect();
    for id in [
        "generic.cutter-small",
        "generic.cutter-medium",
        "generic.cutter-large",
        "generic.grbl-pen-plotter",
    ] {
        assert!(ids.contains(id), "{id}");
    }
}

#[test]
fn the_built_in_library_loads_and_validates() {
    assert!(entries().is_ok(), "{:?}", entries().err());
    assert!(tools().len() >= 4);
    assert_ne!(materials().len(), 0);
}

#[test]
fn ids_are_unique_within_each_kind() {
    for kind in LibraryKind::ALL {
        let ids: Vec<String> = all()
            .into_iter()
            .filter(|e| e.kind() == kind)
            .map(|e| e.id().to_owned())
            .collect();
        let unique: BTreeSet<&String> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len(), "{kind:?}");
    }
}

#[test]
fn machine_capabilities_match_the_design_document() {
    let p = |id: &str| profiles().into_iter().find(|p| p.id == id).unwrap();
    assert!(
        p("generic.cutter-large").tangential_axis,
        "The large cutter has a rotary tangential blade"
    );
    assert!(!p("generic.cutter-small").tangential_axis);
    assert_eq!(p("generic.cutter-small").origin, crate::Origin::TopLeft);
    assert_eq!(
        p("generic.grbl-pen-plotter").origin,
        crate::Origin::BottomLeft
    );
    for profile in profiles() {
        assert!(
            profile.gcode.dialect.starts_with("ps-gcode/"),
            "{}",
            profile.id
        );
    }
}

#[test]
fn every_tool_fits_a_holder_of_some_built_in_machine() {
    let machines = profiles();
    for t in tools() {
        assert!(
            machines.iter().any(|m| m.holders_for(&t).count() > 0),
            "{} has no holder anywhere",
            t.id
        );
    }
}

#[test]
fn every_machine_can_hold_at_least_one_built_in_tool() {
    let ts = tools();
    for m in profiles() {
        assert!(
            ts.iter().any(|t| m.holders_for(t).count() > 0),
            "{} holds nothing",
            m.id
        );
    }
}

#[test]
fn presets_refer_to_real_tools_and_operations() {
    let ts = tools();
    for m in materials() {
        for p in &m.presets {
            let tool = ts
                .iter()
                .find(|t| t.id == p.tool)
                .unwrap_or_else(|| panic!("{}: unknown tool {}", m.id, p.tool));
            assert!(
                tool.operations.contains(&p.operation),
                "{}: {} cannot {}",
                m.id,
                tool.id,
                p.operation
            );
            for (name, value) in &p.params {
                let spec = tool
                    .parameters
                    .get(name)
                    .unwrap_or_else(|| panic!("{}: {} has no parameter {name}", m.id, tool.id));
                assert!(
                    *value >= spec.min && *value <= spec.max,
                    "{}: {name}={value} outside {}..{}",
                    m.id,
                    spec.min,
                    spec.max
                );
            }
        }
    }
}

#[test]
fn machine_overrides_name_real_machines() {
    let machine_ids: BTreeSet<String> = profiles().into_iter().map(|p| p.id).collect();
    for m in materials() {
        for p in &m.presets {
            for machine in p.machines.keys() {
                assert!(
                    machine_ids.contains(machine),
                    "{}: unknown machine {machine}",
                    m.id
                );
            }
        }
    }
}

#[test]
fn the_tdd_example_blade_is_included_exactly() {
    let blade = tools()
        .into_iter()
        .find(|t| t.id == "generic.drag-blade")
        .unwrap();
    assert_eq!(blade.geometry.blade_offset_mm, Some(0.25));
    assert_eq!(blade.parameters["force"].default, 350.0);
}

#[test]
fn built_in_pens_have_a_colour_and_tip_width() {
    let pens: Vec<Tool> = tools()
        .into_iter()
        .filter(|t| t.family == crate::ToolFamily::Pen)
        .collect();
    assert_ne!(pens.len(), 0);
    for p in pens {
        assert!(
            p.pen.is_some() && p.geometry.tip_width_mm.is_some(),
            "{}",
            p.id
        );
    }
}

#[test]
fn installing_into_a_library_makes_everything_visible_and_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = Library::open(dir.path()).unwrap();
    install_into(&mut lib).unwrap();
    assert_eq!(
        lib.list(LibraryKind::Profile).unwrap().len(),
        profiles().len()
    );
    assert!(
        lib.list(LibraryKind::Tool)
            .unwrap()
            .iter()
            .all(|s| s.builtin)
    );
    assert!(lib.snapshot().is_ok());
}

#[test]
fn machines_that_cut_without_a_mat_declare_their_roll() {
    let p = |id: &str| profiles().into_iter().find(|p| p.id == id).unwrap();
    for id in ["generic.cutter-medium", "generic.cutter-large"] {
        let roll = p(id).roll.expect(id);
        assert!((roll.width_mm - 330.2).abs() < 1e-9, "13 in wide: {id}");
        assert!(
            (roll.max_length_mm - 3657.6).abs() < 1e-9,
            "12 ft long: {id}"
        );
    }
    assert!(p("generic.cutter-small").roll.is_none());
    assert!(p("generic.grbl-pen-plotter").roll.is_none());
}
