//! Ground truth for the validation pass: a cheap check on every document change,
//! separate from the compiler, that finds layout problems (outside the usable area, in a keep-out,
//! feature smaller than the tool's minimum, missing operation, unsupported tool) and library
//! references that cannot be resolved. Each diagnostic carries a location.

use std::sync::Arc;

use ps_core::{MatId, ObjectId, OpId, OpRowId, PathKey, Severity};
use ps_geometry::{Affine, FillRule, Path};
use ps_library::{LibrarySnapshot, MachineProfile, RectMm, builtin_entries};

use super::*;
use crate::{
    Document, Mat, MatSize, OpKind, Operation, ParamOverrides, PlacedObject, ProjectMeta,
    ShapePath, SourceStyle, ToolSlotRef,
};

fn libs() -> LibrarySnapshot {
    LibrarySnapshot::from_entries(builtin_entries().unwrap())
}

fn profile(id: &str) -> MachineProfile {
    let r: ps_core::LibRef = format!("{id}@1.0.0").parse().unwrap();
    (*libs().resolve_profile(&r).unwrap().item).clone()
}

fn op(tool: &str, kind: OpKind, enabled: bool) -> Operation {
    Operation {
        id: OpId::at_unix_ms(1),
        kind,
        tool: ToolSlotRef {
            tool: tool.parse().unwrap(),
            pen_colour: None,
        },
        overrides: ParamOverrides::default(),
        fill: None,
        enabled,
        row: OpRowId::at_unix_ms(1),
    }
}

fn cut_op() -> Operation {
    op("generic.drag-blade@1.0.0", OpKind::Cut, true)
}

fn object(x: f64, y: f64, w: f64, h: f64, ops: Vec<Operation>, hidden: bool) -> PlacedObject {
    PlacedObject {
        id: ObjectId::at_unix_ms(10),
        source: None,
        transform: Affine::IDENTITY,
        group: None,
        attach: None,
        paths: Arc::new(vec![Arc::new(ShapePath {
            key: PathKey::source("p"),
            geom: Path::rect(x, y, w, h),
            fill_rule: FillRule::NonZero,
            style: SourceStyle::default(),
            ops,
            start_locked: false,
        })]),
        locked: false,
        hidden,
    }
}

fn doc_with(mat: Mat, objects: Vec<PlacedObject>) -> Document {
    let mut mat = mat;
    for o in objects {
        Arc::make_mut(&mut mat.objects).insert(o.id, Arc::new(o));
    }
    let mut d = Document::new(ProjectMeta::new("t", "t", "v"));
    Arc::make_mut(&mut d.mats).insert(mat.id, Arc::new(mat));
    d
}

fn mat12() -> Mat {
    Mat::new(MatId::at_unix_ms(1), "Mat 1", MatSize::inches(12.0, 12.0))
}

fn keys(d: &Diagnostics) -> Vec<String> {
    d.iter().map(|x| x.key.as_str().to_owned()).collect()
}

const AIR2: &str = "generic.cutter-small";

#[test]
fn a_good_layout_has_no_diagnostics() {
    let d = doc_with(
        mat12(),
        vec![object(10.0, 10.0, 50.0, 30.0, vec![cut_op()], false)],
    );
    let found = validate(&d, &libs(), &profile(AIR2));
    assert_eq!(keys(&found), Vec::<String>::new(), "{found:?}");
}

#[test]
fn an_object_beyond_the_mat_edge_is_outside_the_usable_area() {
    let d = doc_with(
        mat12(),
        vec![object(280.0, 10.0, 50.0, 30.0, vec![cut_op()], false)],
    );
    let found = validate(&d, &libs(), &profile(AIR2));
    assert_eq!(keys(&found), ["diag-outside-usable-area"]);
    let w = found.iter().next().unwrap();
    assert_eq!(w.severity, Severity::Warning);
    let loc = w.location.as_ref().unwrap();
    assert!(loc.mat.is_some() && loc.object.is_some());
}

#[test]
fn negative_coordinates_are_outside_too() {
    let d = doc_with(
        mat12(),
        vec![object(-5.0, 10.0, 20.0, 20.0, vec![cut_op()], false)],
    );
    assert_eq!(
        keys(&validate(&d, &libs(), &profile(AIR2))),
        ["diag-outside-usable-area"]
    );
}

#[test]
fn an_object_touching_the_edge_exactly_is_fine() {
    let d = doc_with(
        mat12(),
        vec![object(0.0, 0.0, 304.8, 304.8, vec![cut_op()], false)],
    );
    assert_eq!(
        keys(&validate(&d, &libs(), &profile(AIR2))),
        Vec::<String>::new()
    );
}

#[test]
fn the_machines_keep_out_zones_are_respected() {
    let mut p = profile(AIR2);
    p.keep_outs.push(RectMm {
        x_mm: 0.0,
        y_mm: 0.0,
        width_mm: 20.0,
        height_mm: 20.0,
    });
    let d = doc_with(
        mat12(),
        vec![object(10.0, 10.0, 30.0, 30.0, vec![cut_op()], false)],
    );
    assert_eq!(keys(&validate(&d, &libs(), &p)), ["diag-in-keep-out"]);
    let clear = doc_with(
        mat12(),
        vec![object(25.0, 25.0, 30.0, 30.0, vec![cut_op()], false)],
    );
    assert_eq!(keys(&validate(&clear, &libs(), &p)), Vec::<String>::new());
}

#[test]
fn used_scrap_regions_are_avoided() {
    let mut m = mat12();
    m.scrap_regions.push(vec![
        ps_geometry::Point::new(0.0, 0.0),
        ps_geometry::Point::new(100.0, 0.0),
        ps_geometry::Point::new(100.0, 100.0),
        ps_geometry::Point::new(0.0, 100.0),
    ]);
    let d = doc_with(
        m,
        vec![object(50.0, 50.0, 20.0, 20.0, vec![cut_op()], false)],
    );
    assert_eq!(
        keys(&validate(&d, &libs(), &profile(AIR2))),
        ["diag-in-scrap-region"]
    );
}

#[test]
fn a_path_without_an_operation_is_reported() {
    let d = doc_with(mat12(), vec![object(10.0, 10.0, 20.0, 20.0, vec![], false)]);
    let found = validate(&d, &libs(), &profile(AIR2));
    assert_eq!(keys(&found), ["diag-missing-operation"]);
    assert!(
        found
            .iter()
            .next()
            .unwrap()
            .location
            .as_ref()
            .unwrap()
            .path
            .is_some()
    );
}

#[test]
fn a_path_whose_only_operations_are_disabled_counts_as_missing() {
    let d = doc_with(
        mat12(),
        vec![object(
            10.0,
            10.0,
            20.0,
            20.0,
            vec![op("generic.drag-blade@1.0.0", OpKind::Cut, false)],
            false,
        )],
    );
    assert_eq!(
        keys(&validate(&d, &libs(), &profile(AIR2))),
        ["diag-missing-operation"]
    );
}

#[test]
fn hidden_objects_are_not_checked() {
    let d = doc_with(
        mat12(),
        vec![object(400.0, 400.0, 20.0, 20.0, vec![], true)],
    );
    assert_eq!(
        keys(&validate(&d, &libs(), &profile(AIR2))),
        Vec::<String>::new()
    );
}

#[test]
fn features_smaller_than_the_tools_minimum_are_flagged() {
    // The fine-point blade cannot cut features under 0.8 mm.
    let tiny = doc_with(
        mat12(),
        vec![object(10.0, 10.0, 0.5, 0.5, vec![cut_op()], false)],
    );
    let found = validate(&tiny, &libs(), &profile(AIR2));
    assert_eq!(keys(&found), ["diag-feature-too-small"]);
    let w = found.iter().next().unwrap();
    assert_eq!(w.args["min_mm"], "0.8");
    assert_eq!(w.args["tool"], "generic.drag-blade");
    let ok = doc_with(
        mat12(),
        vec![object(10.0, 10.0, 2.0, 2.0, vec![cut_op()], false)],
    );
    assert_eq!(
        keys(&validate(&ok, &libs(), &profile(AIR2))),
        Vec::<String>::new()
    );
}

#[test]
fn tools_without_a_minimum_feature_never_trigger_it() {
    let pen = op("pen.fineliner-black-0.4@1.0.0", OpKind::Draw, true);
    let d = doc_with(
        mat12(),
        vec![object(10.0, 10.0, 0.2, 0.2, vec![pen], false)],
    );
    assert_eq!(
        keys(&validate(&d, &libs(), &profile(AIR2))),
        Vec::<String>::new()
    );
}

#[test]
fn a_tool_the_machine_cannot_hold_is_an_error() {
    let foil = op("generic.foil-transfer@1.0.0", OpKind::Foil, true);
    let d = doc_with(
        mat12(),
        vec![object(10.0, 10.0, 20.0, 20.0, vec![foil], false)],
    );
    let found = validate(&d, &libs(), &profile("generic.grbl-pen-plotter"));
    assert!(
        keys(&found).contains(&"diag-unsupported-tool".to_owned()),
        "{found:?}"
    );
    let e = found
        .iter()
        .find(|x| x.key.as_str() == "diag-unsupported-tool")
        .unwrap();
    assert_eq!(e.severity, Severity::Error);
}

#[test]
fn the_same_tool_is_fine_on_a_machine_that_holds_it() {
    let foil = op("generic.foil-transfer@1.0.0", OpKind::Foil, true);
    let d = doc_with(
        mat12(),
        vec![object(10.0, 10.0, 20.0, 20.0, vec![foil], false)],
    );
    assert!(
        !keys(&validate(&d, &libs(), &profile(AIR2))).contains(&"diag-unsupported-tool".to_owned())
    );
}

#[test]
fn disabled_operations_do_not_need_a_supported_tool() {
    let foil_off = op("generic.foil-transfer@1.0.0", OpKind::Foil, false);
    let d = doc_with(
        mat12(),
        vec![object(
            10.0,
            10.0,
            20.0,
            20.0,
            vec![
                foil_off,
                op("pen.fineliner-black-0.4@1.0.0", OpKind::Draw, true),
            ],
            false,
        )],
    );
    assert!(
        !keys(&validate(&d, &libs(), &profile("generic.grbl-pen-plotter")))
            .contains(&"diag-unsupported-tool".to_owned())
    );
}

#[test]
fn a_missing_tool_blocks_export() {
    let ghost = op("nobody.nothing@1.0.0", OpKind::Cut, true);
    let d = doc_with(
        mat12(),
        vec![object(10.0, 10.0, 20.0, 20.0, vec![ghost], false)],
    );
    let found = validate(&d, &libs(), &profile(AIR2));
    let e = found
        .iter()
        .find(|x| x.key.as_str() == "diag-missing-library-entry")
        .unwrap();
    assert_eq!(e.severity, Severity::Error);
    assert_eq!(e.args["id"], "nobody.nothing");
}

#[test]
fn a_substituted_tool_version_is_a_warning() {
    let newer = op("generic.drag-blade@9.9.9", OpKind::Cut, true);
    let d = doc_with(
        mat12(),
        vec![object(10.0, 10.0, 20.0, 20.0, vec![newer], false)],
    );
    let found = validate(&d, &libs(), &profile(AIR2));
    let w = found
        .iter()
        .find(|x| x.key.as_str() == "diag-version-substituted")
        .unwrap();
    assert_eq!(w.severity, Severity::Warning);
    assert_eq!(w.args["wanted"], "generic.drag-blade@9.9.9");
    assert_eq!(w.args["used"], "generic.drag-blade@1.0.0");
    assert!(!found.has_errors());
}

#[test]
fn a_missing_material_is_reported_once_per_mat() {
    let mut m = mat12();
    m.material = Some("nobody.nothing@1.0.0".parse().unwrap());
    let d = doc_with(m, vec![]);
    let found = validate(&d, &libs(), &profile(AIR2));
    assert_eq!(keys(&found), ["diag-missing-library-entry"]);
    assert!(
        found
            .iter()
            .next()
            .unwrap()
            .location
            .as_ref()
            .unwrap()
            .mat
            .is_some()
    );
}

#[test]
fn a_known_material_is_fine() {
    let mut m = mat12();
    m.material = Some("community.cardstock-80lb@1.0.0".parse().unwrap());
    assert_eq!(
        keys(&validate(&doc_with(m, vec![]), &libs(), &profile(AIR2))),
        Vec::<String>::new()
    );
}

#[test]
fn a_mat_the_machine_does_not_take_is_flagged() {
    let m = Mat::new(
        MatId::at_unix_ms(1),
        "Odd",
        MatSize {
            width: ps_core::Mm(100.0),
            height: ps_core::Mm(100.0),
        },
    );
    assert_eq!(
        keys(&validate(&doc_with(m, vec![]), &libs(), &profile(AIR2))),
        ["diag-mat-size-unsupported"]
    );
}

#[test]
fn a_mat_matches_a_supported_size_in_either_orientation() {
    let m = Mat::new(
        MatId::at_unix_ms(1),
        "Landscape",
        MatSize::inches(24.0, 12.0),
    );
    assert_eq!(
        keys(&validate(&doc_with(m, vec![]), &libs(), &profile(AIR2))),
        Vec::<String>::new()
    );
}

#[test]
fn transforms_are_applied_before_checking_bounds() {
    let mut o = object(0.0, 0.0, 50.0, 30.0, vec![cut_op()], false);
    o.transform = Affine::translate((300.0, 0.0));
    assert_eq!(
        keys(&validate(
            &doc_with(mat12(), vec![o]),
            &libs(),
            &profile(AIR2)
        )),
        ["diag-outside-usable-area"]
    );
}

#[test]
fn results_are_sorted_and_repeatable() {
    let mut o = object(304.6, 10.0, 0.5, 0.5, vec![], false);
    o.transform = Affine::IDENTITY;
    let d = doc_with(mat12(), vec![o]);
    let a = validate(&d, &libs(), &profile(AIR2));
    let b = validate(&d, &libs(), &profile(AIR2));
    assert_eq!(a, b);
    assert!(a.len() >= 2);
}

#[test]
fn an_empty_document_is_clean() {
    let d = Document::new(ProjectMeta::new("t", "t", "v"));
    assert_eq!(validate(&d, &libs(), &profile(AIR2)).len(), 0);
}
