//! Ground truth for the document types: the shapes printed in the specification, the rule JSON
//! examples from 5.6, line styles that never rely on colour alone (12.4), and structural sharing.

use std::sync::Arc;

use ps_core::{AssetId, MatId, ObjectId, PathKey, hash_bytes};
use ps_geometry::{Affine, FillRule, Path};

use super::*;

#[test]
fn every_operation_kind_has_a_stable_name() {
    let names = [
        (OpKind::Cut, "cut"),
        (OpKind::KissCut, "kiss_cut"),
        (OpKind::Draw, "draw"),
        (OpKind::Fill, "fill"),
        (OpKind::Score, "score"),
        (OpKind::Emboss, "emboss"),
        (OpKind::Deboss, "deboss"),
        (OpKind::Engrave, "engrave"),
        (OpKind::Foil, "foil"),
        (OpKind::FoilFill, "foil_fill"),
        (OpKind::Perforate, "perforate"),
        (OpKind::WavyCut, "wavy_cut"),
    ];
    assert_eq!(OpKind::ALL.len(), names.len());
    for (kind, name) in names {
        assert_eq!(kind.as_str(), name);
        assert_eq!(name.parse::<OpKind>().unwrap(), kind);
        assert_eq!(serde_json::to_string(&kind).unwrap(), format!("\"{name}\""));
    }
    assert!("hammer".parse::<OpKind>().is_err());
}

#[test]
fn documented_line_styles() {
    // Solid cut, dashed score, dotted perforate, double line for kiss cut.
    assert_eq!(OpKind::Cut.line_style(), LineStyle::Solid);
    assert_eq!(OpKind::Score.line_style(), LineStyle::Dashed);
    assert_eq!(OpKind::Perforate.line_style(), LineStyle::Dotted);
    assert_eq!(OpKind::KissCut.line_style(), LineStyle::Double);
}

#[test]
fn no_two_kinds_share_both_line_style_and_icon() {
    let mut seen = std::collections::BTreeSet::new();
    for kind in OpKind::ALL {
        assert!(
            seen.insert((kind.line_style(), kind.icon())),
            "{kind:?} looks like another kind"
        );
    }
}

#[test]
fn param_values_read_as_numbers_booleans_or_text() {
    let n: ParamValue = serde_json::from_str("1.5").unwrap();
    let b: ParamValue = serde_json::from_str("true").unwrap();
    let t: ParamValue = serde_json::from_str("\"spiral\"").unwrap();
    assert_eq!(n, ParamValue::Number(1.5));
    assert_eq!(b, ParamValue::Bool(true));
    assert_eq!(t, ParamValue::Text("spiral".to_owned()));
    assert_eq!(n.as_f64(), Some(1.5));
    assert_eq!(b.as_f64(), None);
    assert_eq!(
        serde_json::from_str::<ParamValue>("350").unwrap().as_f64(),
        Some(350.0)
    );
}

#[test]
fn overrides_set_get_and_remove() {
    let mut o = ParamOverrides::default();
    assert!(o.get(&ParamKey::new("force")).is_none());
    o.set(ParamKey::new("force"), ParamValue::Number(250.0));
    assert_eq!(
        o.get(&ParamKey::new("force")).and_then(ParamValue::as_f64),
        Some(250.0)
    );
    assert_eq!(o.len(), 1);
    assert_eq!(
        o.remove(&ParamKey::new("force")),
        Some(ParamValue::Number(250.0))
    );
    assert_eq!(o.len(), 0);
}

#[test]
fn overrides_serialise_in_key_order() {
    let mut o = ParamOverrides::default();
    o.set(ParamKey::new("speed"), ParamValue::Number(90.0));
    o.set(ParamKey::new("force"), ParamValue::Number(250.0));
    assert_eq!(
        serde_json::to_string(&o).unwrap(),
        r#"{"force":250.0,"speed":90.0}"#
    );
}

#[test]
fn the_tdd_mapping_rule_examples_parse() {
    // The three lines printed in the specification.
    let lines = [
        r##"{ "match": { "stroke": "#FF0000", "tolerance_de": 2.0 }, "assign": { "op": "cut", "tool": "generic.drag-blade" } }"##,
        r#"{ "match": { "layer": "score*" }, "assign": { "op": "score", "tool": "generic.scoring-stylus" } }"#,
        r#"{ "match": { "filled": true }, "assign": { "op": "fill", "tool": "pen:auto" } }"#,
    ];
    let rules: Vec<MappingRule> = lines
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rules[0].matcher.stroke.as_deref(), Some("#FF0000"));
    assert_eq!(rules[0].matcher.tolerance_de, Some(2.0));
    assert_eq!(rules[0].assign.op, OpKind::Cut);
    assert_eq!(rules[0].assign.tool, "generic.drag-blade");
    assert_eq!(rules[1].matcher.layer.as_deref(), Some("score*"));
    assert_eq!(rules[2].matcher.filled, Some(true));
    assert!(rules[2].assign.is_auto_pen());
    assert!(!rules[0].assign.is_auto_pen());
}

#[test]
fn a_rule_round_trips_and_omits_unset_predicates() {
    let rule = MappingRule {
        matcher: Match {
            layer: Some("cut*".to_owned()),
            ..Match::default()
        },
        assign: Assign {
            op: OpKind::Cut,
            tool: "t".to_owned(),
        },
    };
    let json = serde_json::to_string(&rule).unwrap();
    assert_eq!(
        json,
        r#"{"match":{"layer":"cut*"},"assign":{"op":"cut","tool":"t"}}"#
    );
    assert_eq!(serde_json::from_str::<MappingRule>(&json).unwrap(), rule);
}

#[test]
fn mat_sizes_convert_from_inches() {
    let s = MatSize::inches(12.0, 24.0);
    assert!((s.width.get() - 304.8).abs() < 1e-9);
    assert!((s.height.get() - 609.6).abs() < 1e-9);
}

fn rect_object(left: f64, top: f64, width: f64, height: f64, transform: Affine) -> PlacedObject {
    PlacedObject {
        id: ObjectId::at_unix_ms(10),
        source: None,
        transform,
        group: None,
        attach: None,
        paths: Arc::new(vec![Arc::new(ShapePath {
            key: PathKey::source("p"),
            geom: Path::rect(left, top, width, height),
            fill_rule: FillRule::NonZero,
            style: SourceStyle::default(),
            ops: vec![],
            start_locked: false,
        })]),
        locked: false,
        hidden: false,
    }
}

#[test]
fn an_object_reports_its_bounds_in_mat_space() {
    let o = rect_object(0.0, 0.0, 10.0, 5.0, Affine::translate((5.0, 5.0)));
    let b = o.bounds().unwrap();
    assert_eq!((b.x0, b.y0, b.x1, b.y1), (5.0, 5.0, 15.0, 10.0));
    // Rotating by 90 degrees about the origin turns the 10 x 5 rectangle into 5 x 10.
    let r = rect_object(
        0.0,
        0.0,
        10.0,
        5.0,
        Affine::rotate(std::f64::consts::FRAC_PI_2),
    );
    let b = r.bounds().unwrap();
    assert!((b.width() - 5.0).abs() < 1e-9 && (b.height() - 10.0).abs() < 1e-9);
}

#[test]
fn an_object_without_paths_has_no_bounds() {
    let mut o = rect_object(0.0, 0.0, 1.0, 1.0, Affine::IDENTITY);
    o.paths = Arc::new(vec![]);
    assert!(o.bounds().is_none());
}

#[test]
fn source_styles_default_to_fully_opaque() {
    let s = SourceStyle::default();
    assert_eq!(s.opacity, 1.0);
    assert!(s.stroke.is_none() && s.fill.is_none() && s.dash.is_empty());
}

fn small_doc() -> Document {
    let mut doc = Document::new(ProjectMeta::new(
        "Birthday card set",
        "2026-11-17T12:00:00Z",
        "0.1.0",
    ));
    let mat = Mat::new(MatId::at_unix_ms(1), "Mat 1", MatSize::inches(12.0, 12.0));
    let mut mat = mat;
    let obj = rect_object(0.0, 0.0, 10.0, 5.0, Affine::IDENTITY);
    Arc::make_mut(&mut mat.objects).insert(obj.id, Arc::new(obj));
    Arc::make_mut(&mut doc.mats).insert(mat.id, Arc::new(mat));
    doc
}

#[test]
fn a_document_round_trips_through_json() {
    let doc = small_doc();
    let json = serde_json::to_string(&doc).unwrap();
    assert_eq!(serde_json::from_str::<Document>(&json).unwrap(), doc);
}

#[test]
fn a_new_document_is_empty() {
    let doc = Document::new(ProjectMeta::new("x", "t", "v"));
    assert_eq!(doc.mats.len(), 0);
    assert_eq!(doc.assets.len(), 0);
    assert_eq!(doc.rules.len(), 0);
}

#[test]
fn cloning_shares_every_node() {
    let a = small_doc();
    let b = a.clone();
    assert!(Arc::ptr_eq(&a.mats, &b.mats));
    assert!(Arc::ptr_eq(&a.meta, &b.meta));
    let mat_a = a.mats.values().next().unwrap();
    let mat_b = b.mats.values().next().unwrap();
    assert!(Arc::ptr_eq(mat_a, mat_b));
}

#[test]
fn lookups_find_mats_and_objects() {
    let doc = small_doc();
    // `at_unix_ms` has random low bits, so take the ids from the document itself.
    let mat_id = *doc.mats.keys().next().unwrap();
    assert!(doc.mat(mat_id).is_some());
    assert!(doc.mat(MatId::at_unix_ms(2)).is_none());
    let obj_id = *doc.mat(mat_id).unwrap().objects.keys().next().unwrap();
    assert!(doc.object(mat_id, obj_id).is_some());
    assert!(doc.object(mat_id, ObjectId::at_unix_ms(11)).is_none());
}

#[test]
fn mats_keep_insertion_order() {
    let mut doc = Document::new(ProjectMeta::new("x", "t", "v"));
    for ms in [5, 1, 3] {
        let m = Mat::new(MatId::at_unix_ms(ms), "m", MatSize::inches(12.0, 12.0));
        Arc::make_mut(&mut doc.mats).insert(m.id, Arc::new(m));
    }
    let order: Vec<u64> = doc.mats.keys().map(|k| k.unix_ms().unwrap()).collect();
    assert_eq!(order, [5, 1, 3]);
}

#[test]
fn assets_are_stored_by_content_hash() {
    let bytes = b"<svg/>".to_vec();
    let a = Asset::new(AssetId::at_unix_ms(1), "logo.svg", bytes.clone());
    assert_eq!(a.hash, hash_bytes(&bytes));
    assert_eq!(a.name, "logo.svg");
    assert_eq!(a.bytes.as_slice(), bytes.as_slice());
}

#[test]
fn approximate_size_grows_with_content() {
    let empty = Document::new(ProjectMeta::new("x", "t", "v"));
    assert!(small_doc().approx_bytes() > empty.approx_bytes());
}

#[test]
fn matless_is_a_mat_type() {
    let t: MatType = serde_json::from_str("\"matless\"").unwrap();
    assert_eq!(t, MatType::Matless);
    assert_eq!(
        serde_json::to_string(&MatType::LightGrip).unwrap(),
        "\"light_grip\""
    );
}
