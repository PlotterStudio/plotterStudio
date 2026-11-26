//! Ground truth for commands and undo: all changes are commands applied by a
//! `DocumentStore`; undo stores whole document roots that share unchanged nodes; commands with
//! the same merge key within 500 ms merge into one undo step; transactions group commands; the
//! history is capped by memory.

use std::sync::Arc;

use ps_core::{AssetId, L10nKey, LibRef, MatId, ObjectId, OpId, OpRowId, PathKey};
use ps_geometry::{Affine, FillRule, Path};

use super::*;
use crate::{
    Asset, Document, MappingRule, Mat, MatSize, OpKind, Operation, ParamKey, ParamOverrides,
    ParamValue, PlacedObject, ProjectMeta, ShapePath, SourceStyle, ToolSlotRef,
};
use crate::{GuideAxis, JobSettings};

fn base() -> Document {
    Document::new(ProjectMeta::new("Test", "2026-11-17T00:00:00Z", "0.1.0"))
}

fn mat(ms: u64) -> Mat {
    Mat::new(MatId::at_unix_ms(ms), "Mat", MatSize::inches(12.0, 12.0))
}

fn object(ms: u64, segments: usize) -> PlacedObject {
    let mut geom = Path::new(ps_geometry::Point::new(0.0, 0.0));
    for i in 0..segments {
        #[expect(clippy::cast_precision_loss, reason = "tiny test counts")]
        let x = i as f64;
        geom = geom.line_to(ps_geometry::Point::new(x, 1.0));
    }
    PlacedObject {
        id: ObjectId::at_unix_ms(ms),
        source: None,
        transform: Affine::IDENTITY,
        group: None,
        attach: None,
        paths: Arc::new(vec![Arc::new(ShapePath {
            key: PathKey::source("p1"),
            geom,
            fill_rule: FillRule::NonZero,
            style: SourceStyle::default(),
            ops: vec![],
            start_locked: false,
        })]),
        locked: false,
        hidden: false,
    }
}

fn operation(ms: u64) -> Operation {
    Operation {
        id: OpId::at_unix_ms(ms),
        kind: OpKind::Cut,
        tool: ToolSlotRef {
            tool: "generic.drag-blade@1.0.0".parse().unwrap(),
            pen_colour: None,
        },
        overrides: ParamOverrides::default(),
        fill: None,
        enabled: true,
        row: OpRowId::at_unix_ms(ms),
    }
}

/// A store holding one mat with one object, and their ids.
fn store() -> (DocumentStore, MatId, ObjectId) {
    let m = mat(1);
    let o = object(10, 3);
    let (mid, oid) = (m.id, o.id);
    let mut s = DocumentStore::new(base());
    s.apply(&AddMat { mat: m }, 0).unwrap();
    s.apply(
        &AddObject {
            mat: mid,
            object: o,
        },
        0,
    )
    .unwrap();
    (s, mid, oid)
}

fn transform_x(s: &DocumentStore, m: MatId, o: ObjectId) -> f64 {
    s.document().object(m, o).unwrap().transform.translation().x
}

#[test]
fn commands_are_send_and_sync() {
    fn check<T: Send + Sync>() {}
    check::<AddMat>();
    check::<AddObject>();
    check::<SetTransform>();
    check::<SetOverride>();
    check::<DocumentStore>();
}

#[test]
fn adding_a_mat_and_an_object() {
    let (s, mid, oid) = store();
    assert_eq!(s.document().mats.len(), 1);
    assert!(s.document().object(mid, oid).is_some());
}

#[test]
fn adding_a_duplicate_mat_is_refused() {
    let mut s = DocumentStore::new(base());
    let m = mat(1);
    s.apply(&AddMat { mat: m.clone() }, 0).unwrap();
    let err = s.apply(&AddMat { mat: m }, 1).unwrap_err();
    assert!(matches!(err, CommandError::Duplicate(_)), "{err:?}");
    assert_eq!(s.document().mats.len(), 1);
}

#[test]
fn unknown_targets_are_not_found() {
    let (mut s, _m, oid) = store();
    let ghost = MatId::at_unix_ms(99);
    assert!(matches!(
        s.apply(
            &AddObject {
                mat: ghost,
                object: object(11, 1)
            },
            5
        )
        .unwrap_err(),
        CommandError::NotFound(_)
    ));
    assert!(matches!(
        s.apply(
            &RemoveObject {
                mat: ghost,
                object: oid
            },
            5
        )
        .unwrap_err(),
        CommandError::NotFound(_)
    ));
    assert!(matches!(
        s.apply(&RemoveMat { mat: ghost }, 5).unwrap_err(),
        CommandError::NotFound(_)
    ));
}

#[test]
fn removing_objects_and_mats() {
    let (mut s, mid, oid) = store();
    s.apply(
        &RemoveObject {
            mat: mid,
            object: oid,
        },
        10,
    )
    .unwrap();
    assert!(s.document().object(mid, oid).is_none());
    s.apply(&RemoveMat { mat: mid }, 20).unwrap();
    assert_eq!(s.document().mats.len(), 0);
}

#[test]
fn a_failed_command_changes_nothing_and_adds_no_undo_step() {
    let (mut s, _m, _o) = store();
    let before = s.document();
    let before_gen = s.generation();
    assert!(
        s.apply(
            &RemoveMat {
                mat: MatId::at_unix_ms(99)
            },
            5
        )
        .is_err()
    );
    assert!(Arc::ptr_eq(&before.mats, &s.document().mats));
    assert_eq!(s.generation(), before_gen);
}

#[test]
fn an_edit_shares_every_untouched_node() {
    let (mut s, mid, oid) = store();
    s.apply(
        &AddObject {
            mat: mid,
            object: object(11, 3),
        },
        50,
    )
    .unwrap();
    let before = s.document();
    s.apply(
        &SetTransform {
            mat: mid,
            object: oid,
            transform: Affine::translate((5.0, 0.0)),
        },
        100,
    )
    .unwrap();
    let after = s.document();
    assert!(
        Arc::ptr_eq(&before.meta, &after.meta),
        "metadata is untouched"
    );
    assert!(Arc::ptr_eq(&before.settings, &after.settings));
    assert!(
        !Arc::ptr_eq(&before.mats, &after.mats),
        "the mat map had to change"
    );
    let other = before
        .mat(mid)
        .unwrap()
        .objects
        .keys()
        .find(|k| **k != oid)
        .copied()
        .unwrap();
    let a = before.object(mid, other).unwrap();
    let b = after.object(mid, other).unwrap();
    assert!(Arc::ptr_eq(a, b), "the other object is shared, not copied");
    // The paths of the edited object are shared too: only its transform changed.
    let (ea, eb) = (
        before.object(mid, oid).unwrap(),
        after.object(mid, oid).unwrap(),
    );
    assert!(Arc::ptr_eq(&ea.paths, &eb.paths));
}

#[test]
fn undo_and_redo_walk_the_history() {
    let (mut s, mid, oid) = store();
    s.apply(
        &SetTransform {
            mat: mid,
            object: oid,
            transform: Affine::translate((5.0, 0.0)),
        },
        1_000,
    )
    .unwrap();
    assert_eq!(transform_x(&s, mid, oid), 5.0);
    assert!(s.can_undo() && !s.can_redo());
    assert!(s.undo());
    assert_eq!(transform_x(&s, mid, oid), 0.0);
    assert!(s.can_redo());
    assert!(s.redo());
    assert_eq!(transform_x(&s, mid, oid), 5.0);
    assert!(!s.redo());
}

#[test]
fn undoing_everything_returns_the_original_document() {
    let original = base();
    let mut s = DocumentStore::new(original.clone());
    let m = mat(1);
    let mid = m.id;
    s.apply(&AddMat { mat: m }, 0).unwrap();
    s.apply(
        &AddObject {
            mat: mid,
            object: object(10, 2),
        },
        1_000,
    )
    .unwrap();
    while s.undo() {}
    assert_eq!(*s.document(), original);
}

#[test]
fn a_new_edit_clears_the_redo_stack() {
    let (mut s, mid, oid) = store();
    s.apply(
        &SetTransform {
            mat: mid,
            object: oid,
            transform: Affine::translate((5.0, 0.0)),
        },
        1_000,
    )
    .unwrap();
    s.undo();
    assert!(s.can_redo());
    s.apply(
        &SetMirror {
            mat: mid,
            mirror: true,
        },
        2_000,
    )
    .unwrap();
    assert!(!s.can_redo());
}

#[test]
fn commands_with_the_same_merge_key_within_500_ms_make_one_undo_step() {
    let (mut s, mid, oid) = store();
    let drag = |x: f64| SetTransform {
        mat: mid,
        object: oid,
        transform: Affine::translate((x, 0.0)),
    };
    s.apply(&drag(1.0), 10_000).unwrap();
    s.apply(&drag(2.0), 10_100).unwrap();
    s.apply(&drag(3.0), 10_400).unwrap();
    assert_eq!(transform_x(&s, mid, oid), 3.0);
    assert!(s.undo(), "one undo undoes the whole drag");
    assert_eq!(transform_x(&s, mid, oid), 0.0);
}

#[test]
fn the_merge_window_is_500_ms_from_the_previous_command() {
    let (mut s, mid, oid) = store();
    let drag = |x: f64| SetTransform {
        mat: mid,
        object: oid,
        transform: Affine::translate((x, 0.0)),
    };
    s.apply(&drag(1.0), 20_000).unwrap();
    s.apply(&drag(2.0), 20_500).unwrap(); // exactly 500 ms: merges
    s.undo();
    assert_eq!(transform_x(&s, mid, oid), 0.0);
    s.redo();
    s.apply(&drag(3.0), 21_001).unwrap(); // 501 ms after the last one: a new step
    s.undo();
    assert_eq!(transform_x(&s, mid, oid), 2.0);
}

#[test]
fn different_merge_keys_never_merge() {
    let (mut s, mid, oid) = store();
    s.apply(
        &AddObject {
            mat: mid,
            object: object(11, 2),
        },
        30_000,
    )
    .unwrap();
    let second_id = *s
        .document()
        .mat(mid)
        .unwrap()
        .objects
        .keys()
        .nth(1)
        .unwrap();
    s.apply(
        &SetTransform {
            mat: mid,
            object: oid,
            transform: Affine::translate((1.0, 0.0)),
        },
        31_000,
    )
    .unwrap();
    s.apply(
        &SetTransform {
            mat: mid,
            object: second_id,
            transform: Affine::translate((9.0, 0.0)),
        },
        31_100,
    )
    .unwrap();
    s.apply(
        &SetTransform {
            mat: mid,
            object: oid,
            transform: Affine::translate((2.0, 0.0)),
        },
        31_200,
    )
    .unwrap();
    // Three separate steps: moving A, then B, then A again.
    s.undo();
    assert_eq!(transform_x(&s, mid, oid), 1.0);
    s.undo();
    assert_eq!(
        s.document()
            .object(mid, second_id)
            .unwrap()
            .transform
            .translation()
            .x,
        0.0
    );
    s.undo();
    assert_eq!(transform_x(&s, mid, oid), 0.0);
}

#[test]
fn a_slider_moving_one_override_is_one_undo_step() {
    let (mut s, mid, _o) = store();
    let force = |v: f64| SetOverride {
        target: OverrideTarget::Mat(mid),
        key: ParamKey::new("force"),
        value: Some(ParamValue::Number(v)),
    };
    s.apply(&force(200.0), 40_000).unwrap();
    s.apply(&force(210.0), 40_050).unwrap();
    s.apply(&force(230.0), 40_100).unwrap();
    assert_eq!(
        s.document()
            .mat(mid)
            .unwrap()
            .overrides
            .get(&ParamKey::new("force")),
        Some(&ParamValue::Number(230.0))
    );
    s.undo();
    assert_eq!(
        s.document()
            .mat(mid)
            .unwrap()
            .overrides
            .get(&ParamKey::new("force")),
        None
    );
}

#[test]
fn overrides_can_target_the_project_mat_or_an_operation() {
    let (mut s, mid, oid) = store();
    let key = ParamKey::new("speed");
    s.apply(
        &SetOverride {
            target: OverrideTarget::Project,
            key: key.clone(),
            value: Some(ParamValue::Number(80.0)),
        },
        100,
    )
    .unwrap();
    assert_eq!(
        s.document().settings.overrides.get(&key),
        Some(&ParamValue::Number(80.0))
    );

    let op = operation(1);
    let assigned = op.id;
    s.apply(
        &AssignOperation {
            mat: mid,
            object: oid,
            path: PathKey::source("p1"),
            op,
        },
        2_000,
    )
    .unwrap();
    let target = OverrideTarget::Operation {
        mat: mid,
        object: oid,
        path: PathKey::source("p1"),
        op: assigned,
    };
    s.apply(
        &SetOverride {
            target: target.clone(),
            key: key.clone(),
            value: Some(ParamValue::Number(60.0)),
        },
        4_000,
    )
    .unwrap();
    let doc = s.document();
    let path = &doc.object(mid, oid).unwrap().paths[0];
    assert_eq!(
        path.ops[0].overrides.get(&key),
        Some(&ParamValue::Number(60.0))
    );
    // `None` removes the override.
    s.apply(
        &SetOverride {
            target,
            key: key.clone(),
            value: None,
        },
        6_000,
    )
    .unwrap();
    assert!(
        s.document().object(mid, oid).unwrap().paths[0].ops[0]
            .overrides
            .get(&key)
            .is_none()
    );
}

#[test]
fn operations_can_be_assigned_and_toggled() {
    let (mut s, mid, oid) = store();
    let op = operation(2);
    let assigned = op.id;
    let path = PathKey::source("p1");
    s.apply(
        &AssignOperation {
            mat: mid,
            object: oid,
            path: path.clone(),
            op,
        },
        100,
    )
    .unwrap();
    assert_eq!(s.document().object(mid, oid).unwrap().paths[0].ops.len(), 1);
    s.apply(
        &SetOpEnabled {
            mat: mid,
            object: oid,
            path: path.clone(),
            op: assigned,
            enabled: false,
        },
        5_000,
    )
    .unwrap();
    assert!(!s.document().object(mid, oid).unwrap().paths[0].ops[0].enabled);
    let err = s
        .apply(
            &SetOpEnabled {
                mat: mid,
                object: oid,
                path: PathKey::source("zz"),
                op: assigned,
                enabled: true,
            },
            6_000,
        )
        .unwrap_err();
    assert!(matches!(err, CommandError::NotFound(_)));
}

#[test]
fn a_mat_takes_a_material_and_a_mirror_flag() {
    let (mut s, mid, _o) = store();
    let r: LibRef = "community.cardstock-80lb@1.0.0".parse().unwrap();
    s.apply(
        &SetMatMaterial {
            mat: mid,
            material: Some(r.clone()),
        },
        100,
    )
    .unwrap();
    s.apply(
        &SetMirror {
            mat: mid,
            mirror: true,
        },
        5_000,
    )
    .unwrap();
    let m = s.document().mat(mid).unwrap().clone();
    assert_eq!(m.material, Some(r));
    assert!(m.mirror);
}

#[test]
fn assets_are_added_by_command() {
    let mut s = DocumentStore::new(base());
    let a = Asset::new(AssetId::at_unix_ms(1), "logo.svg", b"<svg/>".to_vec());
    s.apply(&AddAsset { asset: a.clone() }, 0).unwrap();
    assert_eq!(s.document().assets.len(), 1);
    assert!(matches!(
        s.apply(&AddAsset { asset: a }, 1_000).unwrap_err(),
        CommandError::Duplicate(_)
    ));
}

#[test]
fn transactions_are_one_undo_step() {
    let (mut s, mid, _o) = store();
    let cmds: Vec<Box<dyn Command>> = vec![
        Box::new(AddObject {
            mat: mid,
            object: object(20, 2),
        }),
        Box::new(AddObject {
            mat: mid,
            object: object(21, 2),
        }),
        Box::new(AddObject {
            mat: mid,
            object: object(22, 2),
        }),
    ];
    s.transaction(L10nKey::new("cmd-fill-mat-with-copies"), &cmds, 9_000)
        .unwrap();
    assert_eq!(s.document().mat(mid).unwrap().objects.len(), 4);
    assert_eq!(s.undo_label().unwrap().as_str(), "cmd-fill-mat-with-copies");
    assert!(s.undo());
    assert_eq!(s.document().mat(mid).unwrap().objects.len(), 1);
}

#[test]
fn a_failing_transaction_changes_nothing() {
    let (mut s, mid, _o) = store();
    let before = s.document();
    let cmds: Vec<Box<dyn Command>> = vec![
        Box::new(AddObject {
            mat: mid,
            object: object(20, 2),
        }),
        Box::new(RemoveMat {
            mat: MatId::at_unix_ms(99),
        }),
    ];
    assert!(s.transaction(L10nKey::new("cmd-x"), &cmds, 9_000).is_err());
    assert_eq!(*s.document(), *before);
    assert!(s.undo(), "the earlier setup steps are still there");
    assert_eq!(s.document().mat(mid).unwrap().objects.len(), 0);
}

#[test]
fn an_empty_transaction_is_a_no_op() {
    let (mut s, _m, _o) = store();
    let before_gen = s.generation();
    s.transaction(L10nKey::new("cmd-nothing"), &[], 1).unwrap();
    assert_eq!(s.generation(), before_gen);
}

#[test]
fn the_generation_counts_every_change_including_undo_and_redo() {
    let mut s = DocumentStore::new(base());
    let g0 = s.generation();
    s.apply(&AddMat { mat: mat(1) }, 0).unwrap();
    let g1 = s.generation();
    assert!(g1 > g0);
    s.undo();
    let g2 = s.generation();
    assert!(g2 > g1);
    s.redo();
    assert!(s.generation() > g2);
}

#[test]
fn labels_name_the_undo_and_redo_steps() {
    let mut s = DocumentStore::new(base());
    assert!(s.undo_label().is_none());
    s.apply(&AddMat { mat: mat(1) }, 0).unwrap();
    assert_eq!(s.undo_label().unwrap().as_str(), "cmd-add-mat");
    s.undo();
    assert_eq!(s.redo_label().unwrap().as_str(), "cmd-add-mat");
    assert!(s.undo_label().is_none());
}

#[test]
fn undo_and_redo_with_nothing_to_do_are_harmless() {
    let mut s = DocumentStore::new(base());
    assert!(!s.undo());
    assert!(!s.redo());
}

#[test]
fn history_is_capped_by_memory_and_drops_the_oldest_steps_first() {
    let mut s = DocumentStore::new(base());
    s.apply(&AddMat { mat: mat(1) }, 0).unwrap();
    let mid = MatId::at_unix_ms(1);
    let mid = *s.document().mats.keys().next().unwrap_or(&mid);
    s.set_history_limit_bytes(40_000);
    for i in 0..60_u64 {
        s.apply(
            &AddObject {
                mat: mid,
                object: object(100 + i, 40),
            },
            i * 1_000,
        )
        .unwrap();
    }
    assert_eq!(
        s.document().mat(mid).unwrap().objects.len(),
        60,
        "the current document is never trimmed"
    );
    let mut undone = 0;
    while s.undo() {
        undone += 1;
    }
    assert!(undone >= 1, "at least one step stays");
    assert!(undone < 60, "but the oldest were dropped: {undone}");
}

#[test]
fn without_a_limit_history_is_unbounded() {
    let mut s = DocumentStore::new(base());
    s.apply(&AddMat { mat: mat(1) }, 0).unwrap();
    let mid = *s.document().mats.keys().next().unwrap();
    for i in 0..30_u64 {
        s.apply(
            &AddObject {
                mat: mid,
                object: object(100 + i, 5),
            },
            i * 1_000,
        )
        .unwrap();
    }
    let mut undone = 0;
    while s.undo() {
        undone += 1;
    }
    assert_eq!(undone, 31);
}

// ---- operation order and mapping rules ----

#[test]
fn setting_the_operation_order_replaces_it_and_can_be_undone() {
    let (mut s, mid, _) = store();
    let order = vec![
        OpRowId::at_unix_ms(1),
        OpRowId::at_unix_ms(2),
        OpRowId::at_unix_ms(3),
    ];
    s.apply(
        &SetOpOrder {
            mat: mid,
            order: order.clone(),
        },
        0,
    )
    .unwrap();
    assert_eq!(*s.document().mat(mid).unwrap().op_order, order);
    let reversed: Vec<OpRowId> = order.iter().rev().copied().collect();
    s.apply(
        &SetOpOrder {
            mat: mid,
            order: reversed.clone(),
        },
        1_000,
    )
    .unwrap();
    assert_eq!(*s.document().mat(mid).unwrap().op_order, reversed);
    assert!(s.undo());
    assert_eq!(*s.document().mat(mid).unwrap().op_order, order);
}

#[test]
fn the_operation_order_of_an_unknown_mat_is_an_error() {
    let (mut s, _, _) = store();
    let result = s.apply(
        &SetOpOrder {
            mat: MatId::at_unix_ms(999),
            order: vec![],
        },
        0,
    );
    assert!(matches!(result, Err(CommandError::NotFound(_))));
}

#[test]
fn a_duplicate_row_in_the_order_is_refused() {
    let (mut s, mid, _) = store();
    let row = OpRowId::at_unix_ms(5);
    let result = s.apply(
        &SetOpOrder {
            mat: mid,
            order: vec![row, row],
        },
        0,
    );
    assert!(matches!(result, Err(CommandError::Duplicate(_))));
}

#[test]
fn setting_the_mapping_rules_replaces_them_and_can_be_undone() {
    let (mut s, _, _) = store();
    let rule = MappingRule {
        matcher: crate::Match {
            stroke: Some("#FF0000".to_owned()),
            ..crate::Match::default()
        },
        assign: crate::Assign {
            op: OpKind::Cut,
            tool: "generic.drag-blade".to_owned(),
        },
    };
    s.apply(
        &SetRules {
            rules: vec![rule.clone()],
        },
        0,
    )
    .unwrap();
    assert_eq!(*s.document().rules, vec![rule]);
    assert!(s.undo());
    assert!(s.document().rules.is_empty());
}

// ---- editing a path's geometry ----

#[test]
fn replacing_a_paths_geometry_keeps_its_operations_and_can_be_undone() {
    let (mut s, mid, oid) = store();
    let key = PathKey::source("p1");
    s.apply(
        &AssignOperation {
            mat: mid,
            object: oid,
            path: key.clone(),
            op: operation(40),
        },
        0,
    )
    .unwrap();
    let new_geom = Path::rect(0.0, 0.0, 7.0, 3.0);
    s.apply(
        &SetPathGeometry {
            mat: mid,
            object: oid,
            path: key.clone(),
            geom: new_geom.clone(),
        },
        1_000,
    )
    .unwrap();
    let doc = s.document();
    let p = &doc.object(mid, oid).unwrap().paths[0];
    assert_eq!(p.geom, new_geom);
    assert_eq!(p.ops.len(), 1, "the operation stays");
    assert!(s.undo());
    assert_ne!(
        s.document().object(mid, oid).unwrap().paths[0].geom,
        new_geom
    );
}

#[test]
fn replacing_geometry_of_an_unknown_path_is_an_error() {
    let (mut s, mid, oid) = store();
    let result = s.apply(
        &SetPathGeometry {
            mat: mid,
            object: oid,
            path: PathKey::source("nope"),
            geom: Path::rect(0.0, 0.0, 1.0, 1.0),
        },
        0,
    );
    assert!(matches!(result, Err(CommandError::NotFound(_))));
}

#[test]
fn quick_geometry_edits_to_one_path_merge_into_one_undo_step() {
    let (mut s, mid, oid) = store();
    let key = PathKey::source("p1");
    let edit = |w: f64| SetPathGeometry {
        mat: mid,
        object: oid,
        path: key.clone(),
        geom: Path::rect(0.0, 0.0, w, 1.0),
    };
    s.apply(&edit(2.0), 1_000).unwrap();
    s.apply(&edit(3.0), 1_100).unwrap();
    s.apply(&edit(4.0), 1_200).unwrap();
    assert!(s.undo());
    assert!(
        !s.can_undo()
            || s.document().object(mid, oid).unwrap().paths[0].geom
                != Path::rect(0.0, 0.0, 3.0, 1.0)
    );
}

// ---- splitting a path (the knife) ----

fn halves() -> Vec<(PathKey, Path)> {
    vec![
        (PathKey::source("p1-a"), Path::rect(0.0, 0.0, 4.0, 3.0)),
        (PathKey::source("p1-b"), Path::rect(4.0, 0.0, 6.0, 3.0)),
    ]
}

#[test]
fn splitting_a_path_replaces_it_with_its_pieces_in_the_same_place() {
    let (mut s, mid, oid) = store();
    let before = s.document().object(mid, oid).unwrap().paths.len();
    s.apply(
        &SplitPath {
            mat: mid,
            object: oid,
            path: PathKey::source("p1"),
            pieces: halves(),
        },
        0,
    )
    .unwrap();
    let doc = s.document();
    let paths = &doc.object(mid, oid).unwrap().paths;
    assert_eq!(paths.len(), before + 1);
    assert_eq!(paths[0].key, PathKey::source("p1-a"));
    assert_eq!(paths[1].key, PathKey::source("p1-b"));
    assert!(paths.iter().all(|p| p.key != PathKey::source("p1")));
}

#[test]
fn every_piece_keeps_the_operations_with_fresh_ids_on_the_same_row() {
    let (mut s, mid, oid) = store();
    let key = PathKey::source("p1");
    let original = operation(40);
    s.apply(
        &AssignOperation {
            mat: mid,
            object: oid,
            path: key.clone(),
            op: original.clone(),
        },
        0,
    )
    .unwrap();
    s.apply(
        &SplitPath {
            mat: mid,
            object: oid,
            path: key,
            pieces: halves(),
        },
        1,
    )
    .unwrap();
    let doc = s.document();
    let paths = &doc.object(mid, oid).unwrap().paths;
    let (a, b) = (&paths[0].ops[0], &paths[1].ops[0]);
    assert_eq!(a.row, original.row);
    assert_eq!(b.row, original.row);
    assert_eq!(a.kind, original.kind);
    assert_ne!(a.id, b.id, "operation ids stay unique");
    assert_ne!(a.id, original.id);
}

#[test]
fn splitting_can_be_undone() {
    let (mut s, mid, oid) = store();
    let before = s.document().object(mid, oid).unwrap().paths.len();
    s.apply(
        &SplitPath {
            mat: mid,
            object: oid,
            path: PathKey::source("p1"),
            pieces: halves(),
        },
        0,
    )
    .unwrap();
    assert!(s.undo());
    assert_eq!(s.document().object(mid, oid).unwrap().paths.len(), before);
}

#[test]
fn splitting_an_unknown_path_or_into_nothing_is_an_error() {
    let (mut s, mid, oid) = store();
    let unknown = s.apply(
        &SplitPath {
            mat: mid,
            object: oid,
            path: PathKey::source("nope"),
            pieces: halves(),
        },
        0,
    );
    assert!(matches!(unknown, Err(CommandError::NotFound(_))));
    let empty = s.apply(
        &SplitPath {
            mat: mid,
            object: oid,
            path: PathKey::source("p1"),
            pieces: Vec::new(),
        },
        0,
    );
    assert!(matches!(empty, Err(CommandError::NotFound(_))));
}

#[test]
fn two_pieces_may_not_share_a_key() {
    let (mut s, mid, oid) = store();
    let clash = s.apply(
        &SplitPath {
            mat: mid,
            object: oid,
            path: PathKey::source("p1"),
            pieces: vec![
                (PathKey::source("same"), Path::rect(0.0, 0.0, 1.0, 1.0)),
                (PathKey::source("same"), Path::rect(2.0, 0.0, 1.0, 1.0)),
            ],
        },
        0,
    );
    assert!(matches!(clash, Err(CommandError::Duplicate(_))));
}

// ---- run-after locks ----

#[test]
fn run_after_locks_are_set_replaced_and_undone() {
    let (mut s, mid, _) = store();
    let (a, b) = (OpRowId::at_unix_ms(70), OpRowId::at_unix_ms(71));
    s.apply(
        &SetRunAfter {
            mat: mid,
            locks: vec![(a, b)],
        },
        0,
    )
    .unwrap();
    assert_eq!(*s.document().mat(mid).unwrap().run_after, vec![(a, b)]);
    s.apply(
        &SetRunAfter {
            mat: mid,
            locks: Vec::new(),
        },
        1_000,
    )
    .unwrap();
    assert_eq!(s.document().mat(mid).unwrap().run_after.len(), 0);
    assert!(s.undo());
    assert_eq!(*s.document().mat(mid).unwrap().run_after, vec![(a, b)]);
}

#[test]
fn a_row_cannot_be_locked_after_itself() {
    let (mut s, mid, _) = store();
    let a = OpRowId::at_unix_ms(70);
    let result = s.apply(
        &SetRunAfter {
            mat: mid,
            locks: vec![(a, a)],
        },
        0,
    );
    assert!(matches!(result, Err(CommandError::Duplicate(_))));
}

#[test]
fn locks_on_an_unknown_mat_are_an_error() {
    let (mut s, _, _) = store();
    let result = s.apply(
        &SetRunAfter {
            mat: MatId::at_unix_ms(999),
            locks: Vec::new(),
        },
        0,
    );
    assert!(matches!(result, Err(CommandError::NotFound(_))));
}

// ---- guides ----

#[test]
fn guides_are_set_and_undone() {
    let (mut s, mid, _) = store();
    let g = vec![
        Guide {
            axis: GuideAxis::Horizontal,
            at_mm: 25.0,
        },
        Guide {
            axis: GuideAxis::Vertical,
            at_mm: 40.5,
        },
    ];
    s.apply(
        &SetGuides {
            mat: mid,
            guides: g.clone(),
        },
        0,
    )
    .unwrap();
    assert_eq!(*s.document().mat(mid).unwrap().guides, g);
    assert!(s.undo());
    assert_eq!(s.document().mat(mid).unwrap().guides.len(), 0);
}

#[test]
fn a_guide_must_be_a_finite_position() {
    let (mut s, mid, _) = store();
    for at in [f64::NAN, f64::INFINITY] {
        let r = s.apply(
            &SetGuides {
                mat: mid,
                guides: vec![Guide {
                    axis: GuideAxis::Vertical,
                    at_mm: at,
                }],
            },
            0,
        );
        assert!(matches!(r, Err(CommandError::NotFound(_))), "{at}");
    }
}

#[test]
fn guides_on_an_unknown_mat_are_an_error() {
    let (mut s, _, _) = store();
    let r = s.apply(
        &SetGuides {
            mat: MatId::at_unix_ms(999),
            guides: Vec::new(),
        },
        0,
    );
    assert!(matches!(r, Err(CommandError::NotFound(_))));
}

#[test]
fn guides_survive_a_project_round_trip() {
    let mut mat = Mat::new(MatId::at_unix_ms(5), "m", MatSize::inches(12.0, 12.0));
    mat.guides = Arc::new(vec![Guide {
        axis: GuideAxis::Horizontal,
        at_mm: 10.0,
    }]);
    let json = serde_json::to_string(&mat).unwrap();
    let back: Mat = serde_json::from_str(&json).unwrap();
    assert_eq!(back.guides, mat.guides);
    // A mat saved before guides existed still loads.
    let plain = serde_json::to_string(&Mat::new(
        MatId::at_unix_ms(6),
        "m",
        MatSize::inches(12.0, 12.0),
    ))
    .unwrap();
    assert!(!plain.contains("guides"));
}

// ---- job settings ----

#[test]
fn job_settings_are_replaced_and_undone() {
    let (mut s, _, _) = store();
    let settings = JobSettings {
        ordering: crate::OrderingMode::StrictOrder,
        pen_check: true,
        ..JobSettings::default()
    };
    s.apply(
        &SetJobSettings {
            settings: settings.clone(),
        },
        0,
    )
    .unwrap();
    assert_eq!(*s.document().settings, settings);
    assert!(s.undo());
    assert_eq!(*s.document().settings, JobSettings::default());
}

#[test]
fn quick_job_settings_edits_merge_into_one_undo_step() {
    let (mut s, _, _) = store();
    let edits = [
        JobSettings {
            pen_check: true,
            ..JobSettings::default()
        },
        JobSettings {
            pen_check: true,
            ordering: crate::OrderingMode::StrictOrder,
            ..JobSettings::default()
        },
        JobSettings {
            pen_check: false,
            ordering: crate::OrderingMode::StrictOrder,
            ..JobSettings::default()
        },
    ];
    for (i, settings) in edits.into_iter().enumerate() {
        s.apply(
            &SetJobSettings { settings },
            100 * u64::try_from(i).unwrap(),
        )
        .unwrap();
    }
    assert!(s.undo());
    assert_eq!(
        *s.document().settings,
        JobSettings::default(),
        "one undo undid all three"
    );
}

// ---- start points ----

fn square_store() -> (DocumentStore, MatId, ObjectId) {
    let m = mat(1);
    let mut o = object(10, 1);
    o.paths = Arc::new(vec![Arc::new(ShapePath {
        key: PathKey::source("sq"),
        geom: Path::rect(0.0, 0.0, 10.0, 10.0),
        fill_rule: FillRule::NonZero,
        style: SourceStyle::default(),
        ops: vec![],
        start_locked: false,
    })]);
    let (mid, oid) = (m.id, o.id);
    let mut s = DocumentStore::new(base());
    s.apply(&AddMat { mat: m }, 0).unwrap();
    s.apply(
        &AddObject {
            mat: mid,
            object: o,
        },
        0,
    )
    .unwrap();
    (s, mid, oid)
}

#[test]
fn a_start_point_on_an_edge_rotates_the_path_to_that_point_and_locks_it() {
    let (mut s, mid, oid) = square_store();
    s.apply(
        &SetStartPoint {
            mat: mid,
            object: oid,
            path: PathKey::source("sq"),
            at: Some(ps_geometry::Point::new(9.0, 10.5)),
        },
        0,
    )
    .unwrap();
    let doc = s.document();
    let p = &doc.object(mid, oid).unwrap().paths[0];
    assert!(p.start_locked);
    // The nearest outline point of the square (0..10) to (9, 10.5) is (9, 10), mid-edge.
    let start = p.geom.start;
    assert!(
        (start.x - 9.0).abs() < 1e-6 && (start.y - 10.0).abs() < 1e-6,
        "{start:?}"
    );
    assert!(p.geom.closed);
    // The same outline, now with the edge split in two by the new start vertex.
    assert_eq!(p.geom.segs.len(), 5);
    assert!((p.geom.length() - 40.0).abs() < 1e-6);
}

#[test]
fn a_start_point_at_a_corner_keeps_the_same_four_edges() {
    let (mut s, mid, oid) = square_store();
    s.apply(
        &SetStartPoint {
            mat: mid,
            object: oid,
            path: PathKey::source("sq"),
            at: Some(ps_geometry::Point::new(10.0, 10.0)),
        },
        0,
    )
    .unwrap();
    let doc = s.document();
    let p = &doc.object(mid, oid).unwrap().paths[0];
    assert_eq!(p.geom.start, ps_geometry::Point::new(10.0, 10.0));
    assert_eq!(p.geom.segs.len(), 4);
}

#[test]
fn clearing_the_start_point_unlocks_the_path() {
    let (mut s, mid, oid) = square_store();
    let key = PathKey::source("sq");
    s.apply(
        &SetStartPoint {
            mat: mid,
            object: oid,
            path: key.clone(),
            at: Some(ps_geometry::Point::new(10.0, 0.0)),
        },
        0,
    )
    .unwrap();
    s.apply(
        &SetStartPoint {
            mat: mid,
            object: oid,
            path: key,
            at: None,
        },
        0,
    )
    .unwrap();
    assert!(!s.document().object(mid, oid).unwrap().paths[0].start_locked);
}

#[test]
fn an_open_path_has_no_start_point_to_set() {
    let (mut s, mid, oid) = store();
    let result = s.apply(
        &SetStartPoint {
            mat: mid,
            object: oid,
            path: PathKey::source("p1"),
            at: Some(ps_geometry::Point::new(0.0, 0.0)),
        },
        0,
    );
    assert!(result.is_err());
}

#[test]
fn setting_a_start_point_is_one_undo_step() {
    let (mut s, mid, oid) = square_store();
    s.apply(
        &SetStartPoint {
            mat: mid,
            object: oid,
            path: PathKey::source("sq"),
            at: Some(ps_geometry::Point::new(10.0, 10.0)),
        },
        0,
    )
    .unwrap();
    assert!(s.undo());
    let doc = s.document();
    let p = &doc.object(mid, oid).unwrap().paths[0];
    assert!(!p.start_locked);
    assert_eq!(p.geom.start, ps_geometry::Point::new(0.0, 0.0));
}

#[test]
fn an_operation_can_be_removed_from_a_path_and_the_removal_undone() {
    let (mut s, mid, oid) = store();
    let path = PathKey::source("p1");
    let (a, b) = (operation(3), operation(4));
    let (a_id, b_id) = (a.id, b.id);
    for op in [a, b] {
        s.apply(
            &AssignOperation {
                mat: mid,
                object: oid,
                path: path.clone(),
                op,
            },
            1_000,
        )
        .unwrap();
    }
    s.apply(
        &RemoveOperation {
            mat: mid,
            object: oid,
            path: path.clone(),
            op: a_id,
        },
        2_000,
    )
    .unwrap();
    let doc = s.document();
    let ops = &doc.object(mid, oid).unwrap().paths[0].ops;
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].id, b_id);
    assert!(s.undo());
    assert_eq!(s.document().object(mid, oid).unwrap().paths[0].ops.len(), 2);
}

#[test]
fn removing_an_operation_that_is_not_there_is_an_error() {
    let (mut s, mid, oid) = store();
    let result = s.apply(
        &RemoveOperation {
            mat: mid,
            object: oid,
            path: PathKey::source("p1"),
            op: operation(5).id,
        },
        1_000,
    );
    assert!(matches!(result, Err(CommandError::NotFound(_))));
}

#[test]
fn a_pens_colour_can_be_changed_and_cleared() {
    let (mut s, mid, oid) = store();
    let path = PathKey::source("p1");
    let op = operation(6);
    let id = op.id;
    s.apply(
        &AssignOperation {
            mat: mid,
            object: oid,
            path: path.clone(),
            op,
        },
        1_000,
    )
    .unwrap();
    s.apply(
        &SetPenColour {
            mat: mid,
            object: oid,
            path: path.clone(),
            op: id,
            colour: Some("#C42B2B".to_owned()),
        },
        2_000,
    )
    .unwrap();
    assert_eq!(
        s.document().object(mid, oid).unwrap().paths[0].ops[0]
            .tool
            .pen_colour
            .as_deref(),
        Some("#C42B2B")
    );
    s.apply(
        &SetPenColour {
            mat: mid,
            object: oid,
            path,
            op: id,
            colour: None,
        },
        4_000,
    )
    .unwrap();
    assert!(
        s.document().object(mid, oid).unwrap().paths[0].ops[0]
            .tool
            .pen_colour
            .is_none()
    );
}

#[test]
fn an_operations_fill_settings_can_be_set_and_cleared() {
    use ps_geometry::{FillSpec, Pattern};
    let (mut s, mid, oid) = store();
    let path = PathKey::source("p1");
    let op = operation(7);
    let id = op.id;
    s.apply(
        &AssignOperation {
            mat: mid,
            object: oid,
            path: path.clone(),
            op,
        },
        1_000,
    )
    .unwrap();
    let spec = FillSpec {
        pattern: Pattern::Cross,
        angle_deg: 45.0,
        overlap: 0.4,
        outline_pass: true,
    };
    s.apply(
        &SetOpFill {
            mat: mid,
            object: oid,
            path: path.clone(),
            op: id,
            fill: Some(spec),
        },
        2_000,
    )
    .unwrap();
    assert_eq!(
        s.document().object(mid, oid).unwrap().paths[0].ops[0].fill,
        Some(spec)
    );
    s.apply(
        &SetOpFill {
            mat: mid,
            object: oid,
            path,
            op: id,
            fill: None,
        },
        4_000,
    )
    .unwrap();
    assert!(
        s.document().object(mid, oid).unwrap().paths[0].ops[0]
            .fill
            .is_none()
    );
}

#[test]
fn removing_a_path_keeps_the_others_and_never_empties_an_object() {
    let (mut s, m, o) = store();
    let mut two = object(20, 1);
    let mut second = (*two.paths[0]).clone();
    second.key = PathKey::source("p2");
    Arc::make_mut(&mut two.paths).push(Arc::new(second));
    let id = two.id;
    s.apply(
        &AddObject {
            mat: m,
            object: two,
        },
        0,
    )
    .unwrap();
    let remove = |object, key: &str| RemovePath {
        mat: m,
        object,
        path: PathKey::source(key),
    };
    s.apply(&remove(id, "p1"), 1).unwrap();
    let doc = s.document();
    let left = &doc.mat(m).unwrap().objects[&id].paths;
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].key, PathKey::source("p2"));
    assert!(s.apply(&remove(id, "p2"), 2).is_err(), "the last one");
    assert!(s.apply(&remove(id, "p1"), 3).is_err(), "already gone");
    assert!(s.apply(&remove(o, "nope"), 4).is_err());
}
