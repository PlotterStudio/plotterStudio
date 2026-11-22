//! Ground truth for placement: one `PlacedObject` per layer (or per
//! path, or one for everything, the user's choice), linked to the stored original asset, with the
//! operations the mapping rules assigned. Operations that share a tool and kind share an
//! operation row, so they form one block downstream. Unmapped paths get no operation.

use ps_core::{AssetId, Diagnostics, LibRef, PathKey, Severity};
use ps_geometry::{Affine, Path};
use ps_model::{OpKind, PlacedObject, SourceStyle};

use super::*;
use crate::drawing::{ImportedDrawing, ImportedPath, LayerInfo};
use crate::rules::Mapping;
use crate::units::UnitGuess;

fn rect(key: &str, layer: Option<&str>) -> ImportedPath {
    ImportedPath {
        key: PathKey::source(key),
        layer: layer.map(str::to_owned),
        geom: Path::rect(0.0, 0.0, 10.0, 10.0),
        style: SourceStyle {
            stroke: Some([255, 0, 0]),
            layer: layer.map(str::to_owned),
            ..SourceStyle::default()
        },
    }
}

fn drawing(items: Vec<ImportedPath>, layers: &[(&str, bool)]) -> ImportedDrawing {
    ImportedDrawing {
        unit_guess: UnitGuess::Mm,
        size_candidates: Vec::new(),
        layers: layers
            .iter()
            .enumerate()
            .map(|(i, (name, visible))| LayerInfo {
                name: (*name).to_owned(),
                visible: *visible,
                order: u32::try_from(i).unwrap(),
            })
            .collect(),
        items,
        diagnostics: Diagnostics::new(),
    }
}

fn assigned(op: OpKind, tool: &str) -> Mapping {
    Mapping::Assigned {
        rule: 0,
        op,
        tool: tool.to_owned(),
        pen_colour: None,
    }
}

fn resolver(id: &str) -> Option<LibRef> {
    (id != "unknown.tool").then(|| format!("{id}@1.0.0").parse().unwrap())
}

fn place_with(d: &ImportedDrawing, m: &[Mapping], mode: PlaceMode) -> Placement {
    place(&PlaceInput {
        drawing: d,
        mappings: m,
        asset: AssetId::new(),
        mode,
        resolve_tool: &resolver,
    })
}

fn path_keys(o: &PlacedObject) -> Vec<String> {
    o.paths.iter().map(|p| p.key.to_string()).collect()
}

#[test]
fn per_layer_placement_makes_one_object_per_layer_in_layer_order() {
    let d = drawing(
        vec![
            rect("a", Some("Draw")),
            rect("b", Some("Cut")),
            rect("c", Some("Draw")),
        ],
        &[("Cut", true), ("Draw", true)],
    );
    let m = vec![assigned(OpKind::Draw, "pen"); 3];
    let p = place_with(&d, &m, PlaceMode::PerLayer);
    assert_eq!(p.objects.len(), 2);
    assert_eq!(
        path_keys(&p.objects[0]),
        vec!["src:b"],
        "Cut comes first in the layer order"
    );
    assert_eq!(path_keys(&p.objects[1]), vec!["src:a", "src:c"]);
}

#[test]
fn paths_with_no_layer_share_one_object_after_the_named_layers() {
    let d = drawing(
        vec![rect("a", None), rect("b", Some("L")), rect("c", None)],
        &[("L", true)],
    );
    let m = vec![Mapping::Unmapped; 3];
    let p = place_with(&d, &m, PlaceMode::PerLayer);
    assert_eq!(p.objects.len(), 2);
    assert_eq!(path_keys(&p.objects[0]), vec!["src:b"]);
    assert_eq!(path_keys(&p.objects[1]), vec!["src:a", "src:c"]);
}

#[test]
fn per_path_placement_makes_one_object_each() {
    let d = drawing(vec![rect("a", None), rect("b", None)], &[]);
    let p = place_with(
        &d,
        &[Mapping::Unmapped, Mapping::Unmapped],
        PlaceMode::PerPath,
    );
    assert_eq!(p.objects.len(), 2);
    assert!(p.objects.iter().all(|o| o.paths.len() == 1));
}

#[test]
fn single_placement_keeps_everything_in_one_object() {
    let d = drawing(
        vec![rect("a", Some("X")), rect("b", Some("Y"))],
        &[("X", true), ("Y", true)],
    );
    let p = place_with(
        &d,
        &[Mapping::Unmapped, Mapping::Unmapped],
        PlaceMode::Single,
    );
    assert_eq!(p.objects.len(), 1);
    assert_eq!(p.objects[0].paths.len(), 2);
}

#[test]
fn objects_link_to_the_stored_original() {
    let d = drawing(vec![rect("a", Some("Cut"))], &[("Cut", true)]);
    let asset = AssetId::new();
    let p = place(&PlaceInput {
        drawing: &d,
        mappings: &[Mapping::Unmapped],
        asset,
        mode: PlaceMode::PerLayer,
        resolve_tool: &resolver,
    });
    let link = p.objects[0].source.as_ref().unwrap();
    assert_eq!(link.asset, asset);
    assert_eq!(link.element, "Cut");
    let per_path = place(&PlaceInput {
        drawing: &d,
        mappings: &[Mapping::Unmapped],
        asset,
        mode: PlaceMode::PerPath,
        resolve_tool: &resolver,
    });
    assert_eq!(
        per_path.objects[0].source.as_ref().unwrap().element,
        "src:a"
    );
}

#[test]
fn mapped_paths_carry_their_operation_and_tool() {
    let d = drawing(vec![rect("a", None)], &[]);
    let p = place_with(
        &d,
        &[assigned(OpKind::Cut, "generic.drag-blade")],
        PlaceMode::PerLayer,
    );
    let shape = &p.objects[0].paths[0];
    assert_eq!(shape.ops.len(), 1);
    let op = &shape.ops[0];
    assert_eq!(op.kind, OpKind::Cut);
    assert_eq!(op.tool.tool.to_string(), "generic.drag-blade@1.0.0");
    assert!(op.enabled);
}

#[test]
fn the_pen_colour_chosen_by_pen_auto_is_recorded() {
    let d = drawing(vec![rect("a", None)], &[]);
    let m = Mapping::Assigned {
        rule: 0,
        op: OpKind::Draw,
        tool: "pen.red".into(),
        pen_colour: Some([200, 30, 30]),
    };
    let p = place_with(&d, &[m], PlaceMode::PerLayer);
    assert_eq!(
        p.objects[0].paths[0].ops[0].tool.pen_colour.as_deref(),
        Some("#C81E1E")
    );
}

#[test]
fn operations_sharing_a_tool_and_kind_share_a_row() {
    let d = drawing(vec![rect("a", None), rect("b", None), rect("c", None)], &[]);
    let m = vec![
        assigned(OpKind::Cut, "blade"),
        assigned(OpKind::Draw, "pen"),
        assigned(OpKind::Cut, "blade"),
    ];
    let p = place_with(&d, &m, PlaceMode::PerLayer);
    let rows: Vec<_> = p.objects[0].paths.iter().map(|s| s.ops[0].row).collect();
    assert_eq!(rows[0], rows[2]);
    assert_ne!(rows[0], rows[1]);
    assert_eq!(p.rows.len(), 2);
    assert_eq!(p.rows[0].kind, OpKind::Cut);
    assert_eq!(p.rows[1].kind, OpKind::Draw);
}

#[test]
fn unmapped_paths_have_no_operations() {
    let d = drawing(vec![rect("a", None)], &[]);
    let p = place_with(&d, &[Mapping::Unmapped], PlaceMode::PerLayer);
    assert_eq!(p.objects[0].paths[0].ops.len(), 0);
    assert_eq!(p.rows.len(), 0);
}

#[test]
fn a_tool_the_library_does_not_know_is_reported_and_left_unassigned() {
    let d = drawing(vec![rect("a", None)], &[]);
    let p = place_with(
        &d,
        &[assigned(OpKind::Cut, "unknown.tool")],
        PlaceMode::PerLayer,
    );
    assert_eq!(p.objects[0].paths[0].ops.len(), 0);
    let d0 = p.diagnostics.iter().next().unwrap();
    assert_eq!(d0.key.as_str(), "diag-import-unknown-tool");
    assert_eq!(d0.severity, Severity::Warning);
}

#[test]
fn geometry_and_style_are_carried_over_unchanged() {
    let d = drawing(vec![rect("a", Some("L"))], &[("L", true)]);
    let p = place_with(&d, &[Mapping::Unmapped], PlaceMode::PerLayer);
    let shape = &p.objects[0].paths[0];
    assert_eq!(shape.geom, d.items[0].geom);
    assert_eq!(shape.style, d.items[0].style);
    assert_eq!(shape.key, d.items[0].key);
}

#[test]
fn placed_objects_start_at_the_source_position_unlocked_and_visible() {
    let d = drawing(vec![rect("a", None)], &[]);
    let p = place_with(&d, &[Mapping::Unmapped], PlaceMode::PerLayer);
    let o = &p.objects[0];
    assert_eq!(o.transform, Affine::IDENTITY);
    assert!(!o.locked && !o.hidden);
}

#[test]
fn a_hidden_layer_yields_a_hidden_object() {
    let d = drawing(vec![rect("a", Some("Hidden"))], &[("Hidden", false)]);
    let p = place_with(&d, &[Mapping::Unmapped], PlaceMode::PerLayer);
    assert!(p.objects[0].hidden);
}

#[test]
fn every_object_has_a_distinct_id() {
    let d = drawing(
        vec![rect("a", Some("A")), rect("b", Some("B"))],
        &[("A", true), ("B", true)],
    );
    let p = place_with(
        &d,
        &[Mapping::Unmapped, Mapping::Unmapped],
        PlaceMode::PerLayer,
    );
    assert_ne!(p.objects[0].id, p.objects[1].id);
}

#[test]
fn an_empty_drawing_places_nothing() {
    let p = place_with(&drawing(vec![], &[]), &[], PlaceMode::PerLayer);
    assert_eq!((p.objects.len(), p.rows.len()), (0, 0));
}

#[test]
fn a_missing_mapping_is_treated_as_unmapped() {
    let d = drawing(vec![rect("a", None), rect("b", None)], &[]);
    let p = place_with(&d, &[assigned(OpKind::Cut, "blade")], PlaceMode::PerLayer);
    assert_eq!(p.objects[0].paths[1].ops.len(), 0);
}
