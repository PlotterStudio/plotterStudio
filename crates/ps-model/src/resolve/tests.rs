//! Ground truth for parameter resolution. The chain is: operation override, mat
//! override, project override, material preset (machine-specific value first, then generic),
//! tool default; finally the machine profile's limits. Clamping never hides: the pre-clamp value
//! is kept and a warning raised. Fixtures are the real built-in library.

use ps_core::{LibRef, MatId, OpId, OpRowId, Severity};
use ps_library::{LibrarySnapshot, MachineProfile, builtin_entries};

use super::*;
use crate::{
    JobSettings, Mat, MatSize, OpKind, Operation, ParamKey, ParamOverrides, ParamValue, ToolSlotRef,
};

fn libs() -> LibrarySnapshot {
    LibrarySnapshot::from_entries(builtin_entries().unwrap())
}

fn profile(id: &str) -> MachineProfile {
    let r: LibRef = format!("{id}@1.0.0").parse().unwrap();
    (*libs().resolve_profile(&r).unwrap().item).clone()
}

fn blade_op(kind: OpKind) -> Operation {
    Operation {
        id: OpId::at_unix_ms(1),
        kind,
        tool: ToolSlotRef {
            tool: "generic.drag-blade@1.0.0".parse().unwrap(),
            pen_colour: None,
        },
        overrides: ParamOverrides::default(),
        fill: None,
        enabled: true,
        row: OpRowId::at_unix_ms(1),
    }
}

fn mat(material: Option<&str>) -> Mat {
    let mut m = Mat::new(MatId::at_unix_ms(1), "Mat 1", MatSize::inches(12.0, 12.0));
    m.material = material.map(|r| r.parse().unwrap());
    m
}

fn set(o: &mut ParamOverrides, k: &str, v: f64) {
    o.set(ParamKey::new(k), ParamValue::Number(v));
}

fn run(op: &Operation, m: &Mat, project: &JobSettings, machine: &str) -> ResolvedParams {
    resolve(op, m, project, &libs(), &profile(machine)).unwrap()
}

const CARD: &str = "community.cardstock-80lb@1.0.0";
const AIR2: &str = "generic.cutter-small";

#[test]
fn only_the_tools_declared_parameters_are_resolved() {
    let r = run(
        &blade_op(OpKind::Cut),
        &mat(None),
        &JobSettings::default(),
        AIR2,
    );
    let names: Vec<&str> = r.params.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        [
            "force",
            "force_ramp",
            "lead_in_mm",
            "lead_out_mm",
            "overcut",
            "passes",
            "speed",
            "tab_count",
            "tab_width_mm",
        ]
    );
}

#[test]
fn with_nothing_else_the_tool_default_applies() {
    let r = run(
        &blade_op(OpKind::Cut),
        &mat(None),
        &JobSettings::default(),
        AIR2,
    );
    let force = &r.params["force"];
    assert_eq!(force.value, 350.0);
    assert!(
        matches!(force.source, ParamSource::ToolDefault { .. }),
        "{:?}",
        force.source
    );
    assert_eq!(force.clamped_by_machine, None);
    assert_eq!(r.params["overcut"].value, 1.0);
}

#[test]
fn a_material_preset_beats_the_tool_default() {
    let r = run(
        &blade_op(OpKind::Cut),
        &mat(Some(CARD)),
        &JobSettings::default(),
        AIR2,
    );
    let force = &r.params["force"];
    assert_eq!(force.value, 210.0);
    match &force.source {
        ParamSource::MaterialPreset {
            material,
            machine_specific,
        } => {
            assert_eq!(material.to_string(), CARD);
            assert!(
                !machine_specific,
                "the small cutter has no override in this preset"
            );
        }
        other => panic!("{other:?}"),
    }
    // Speed also comes from the preset; the overcut is not in it, so the tool default stays.
    assert_eq!(r.params["speed"].value, 100.0);
    assert!(matches!(
        r.params["overcut"].source,
        ParamSource::ToolDefault { .. }
    ));
}

#[test]
fn a_machine_specific_preset_value_beats_the_generic_one() {
    let r = run(
        &blade_op(OpKind::Cut),
        &mat(Some(CARD)),
        &JobSettings::default(),
        "generic.cutter-medium",
    );
    let force = &r.params["force"];
    assert_eq!(force.value, 230.0);
    assert!(matches!(
        force.source,
        ParamSource::MaterialPreset {
            machine_specific: true,
            ..
        }
    ));
    // Only the force is overridden for that machine; the speed is the generic preset value.
    assert!(matches!(
        r.params["speed"].source,
        ParamSource::MaterialPreset {
            machine_specific: false,
            ..
        }
    ));
}

#[test]
fn the_project_override_beats_the_preset() {
    let mut project = JobSettings::default();
    set(&mut project.overrides, "force", 260.0);
    let r = run(&blade_op(OpKind::Cut), &mat(Some(CARD)), &project, AIR2);
    assert_eq!(r.params["force"].value, 260.0);
    assert_eq!(r.params["force"].source, ParamSource::ProjectOverride);
}

#[test]
fn the_mat_override_beats_the_project_override() {
    let mut project = JobSettings::default();
    set(&mut project.overrides, "force", 260.0);
    let mut m = mat(Some(CARD));
    set(&mut m.overrides, "force", 280.0);
    let r = run(&blade_op(OpKind::Cut), &m, &project, AIR2);
    assert_eq!(r.params["force"].value, 280.0);
    assert_eq!(r.params["force"].source, ParamSource::MatOverride);
}

#[test]
fn the_operation_override_beats_everything() {
    let mut project = JobSettings::default();
    set(&mut project.overrides, "force", 260.0);
    let mut m = mat(Some(CARD));
    set(&mut m.overrides, "force", 280.0);
    let mut op = blade_op(OpKind::Cut);
    set(&mut op.overrides, "force", 300.0);
    let r = run(&op, &m, &project, AIR2);
    assert_eq!(r.params["force"].value, 300.0);
    assert_eq!(r.params["force"].source, ParamSource::OperationOverride);
    // Other parameters are unaffected.
    assert_eq!(r.params["speed"].value, 100.0);
}

#[test]
fn a_preset_for_another_operation_is_ignored() {
    // The cardstock preset for this blade is for "cut"; a kiss cut falls through to defaults.
    let r = run(
        &blade_op(OpKind::KissCut),
        &mat(Some(CARD)),
        &JobSettings::default(),
        AIR2,
    );
    assert!(matches!(
        r.params["force"].source,
        ParamSource::ToolDefault { .. }
    ));
    assert_eq!(r.params["force"].value, 350.0);
}

#[test]
fn a_vinyl_kiss_cut_uses_its_own_preset() {
    let r = run(
        &blade_op(OpKind::KissCut),
        &mat(Some("community.smooth-vinyl@1.0.0")),
        &JobSettings::default(),
        AIR2,
    );
    assert_eq!(r.params["force"].value, 90.0);
}

#[test]
fn a_material_without_a_preset_for_the_tool_falls_through() {
    let mut op = blade_op(OpKind::Cut);
    op.tool.tool = "generic.foil-transfer@1.0.0".parse().unwrap();
    let r = run(&op, &mat(Some(CARD)), &JobSettings::default(), AIR2);
    assert!(matches!(
        r.params["force"].source,
        ParamSource::ToolDefault { .. }
    ));
}

#[test]
fn non_numeric_overrides_are_skipped() {
    let mut op = blade_op(OpKind::Cut);
    op.overrides
        .set(ParamKey::new("force"), ParamValue::Text("lots".to_owned()));
    let r = run(&op, &mat(None), &JobSettings::default(), AIR2);
    assert_eq!(r.params["force"].value, 350.0);
}

#[test]
fn force_is_clamped_to_the_machine_and_the_original_is_kept() {
    let mut op = blade_op(OpKind::Cut);
    set(&mut op.overrides, "force", 900.0);
    let r = run(&op, &mat(None), &JobSettings::default(), AIR2); // the small cutter tops out at 400 gf
    let force = &r.params["force"];
    assert_eq!(force.value, 400.0);
    assert_eq!(force.clamped_by_machine, Some(900.0));
    assert_eq!(force.source, ParamSource::OperationOverride);
    assert_eq!(r.warnings.len(), 1);
    let w = r.warnings.iter().next().unwrap();
    assert_eq!(w.severity, Severity::Warning);
    assert_eq!(w.key.as_str(), "diag-clamped-by-machine");
    assert_eq!(w.args["param"], "force");
    assert_eq!(w.args["requested"], "900");
    assert_eq!(w.args["used"], "400");
}

#[test]
fn speed_is_clamped_to_the_machine_maximum() {
    let mut op = blade_op(OpKind::Cut);
    set(&mut op.overrides, "speed", 800.0);
    let r = run(&op, &mat(None), &JobSettings::default(), AIR2);
    assert_eq!(r.params["speed"].value, 101.6); // the Air 2's measured drawing speed
    assert_eq!(r.params["speed"].clamped_by_machine, Some(800.0));
}

#[test]
fn values_inside_the_machine_limits_are_not_flagged() {
    let r = run(
        &blade_op(OpKind::Cut),
        &mat(Some(CARD)),
        &JobSettings::default(),
        AIR2,
    );
    assert!(r.params.values().all(|p| p.clamped_by_machine.is_none()));
    assert_eq!(r.warnings.len(), 0);
}

#[test]
fn the_maker_allows_the_higher_force_the_explore_refuses() {
    let mut op = blade_op(OpKind::Cut);
    set(&mut op.overrides, "force", 900.0);
    let r = run(
        &op,
        &mat(None),
        &JobSettings::default(),
        "generic.cutter-large",
    ); // up to 4000 gf
    assert_eq!(r.params["force"].value, 900.0);
    assert_eq!(r.params["force"].clamped_by_machine, None);
}

#[test]
fn an_unknown_tool_is_an_error() {
    let mut op = blade_op(OpKind::Cut);
    op.tool.tool = "nobody.nothing@1.0.0".parse().unwrap();
    let err = resolve(
        &op,
        &mat(None),
        &JobSettings::default(),
        &libs(),
        &profile(AIR2),
    )
    .unwrap_err();
    assert!(matches!(err, ResolveParamsError::MissingTool(_)), "{err:?}");
}

#[test]
fn an_unknown_material_is_ignored_with_the_tool_default() {
    // The reference is reported elsewhere (library reference check); resolution still works.
    let r = run(
        &blade_op(OpKind::Cut),
        &mat(Some("nobody.nothing@1.0.0")),
        &JobSettings::default(),
        AIR2,
    );
    assert!(matches!(
        r.params["force"].source,
        ParamSource::ToolDefault { .. }
    ));
}

#[test]
fn resolution_is_pure() {
    let op = blade_op(OpKind::Cut);
    let m = mat(Some(CARD));
    let a = run(&op, &m, &JobSettings::default(), AIR2);
    let b = run(&op, &m, &JobSettings::default(), AIR2);
    assert_eq!(a.params, b.params);
}

#[test]
fn the_machine_clamp_helper() {
    let p = profile(AIR2);
    assert_eq!(clamp_to_machine("force", 900.0, &p), (400.0, Some(900.0)));
    assert_eq!(clamp_to_machine("force", 100.0, &p), (100.0, None));
    assert_eq!(clamp_to_machine("force", -5.0, &p), (0.0, Some(-5.0)));
    assert_eq!(clamp_to_machine("speed", 800.0, &p), (101.6, Some(800.0)));
    assert_eq!(clamp_to_machine("passes", 99.0, &p), (99.0, None)); // no machine limit
}
