//! Ground truth for incremental compiles: stage 3 and 4 outputs are memoised per
//! (object, operation) under a BLAKE3 key of path geometry, object transform, resolved
//! parameters, tool definition and calibration values. A single-object edit re-prepares only that
//! object; the cache is an LRU capped by bytes; cached and uncached compiles are identical.

use std::sync::Arc;

use ps_core::{ContentHash, MatId, ObjectId, OpId, OpRowId, PathKey};
use ps_geometry::{Affine, FillRule, Path};
use ps_library::{Calibration, LibrarySnapshot, MachineProfile, builtin_entries};
use ps_model::{
    Document, Mat, MatSize, OpKind, Operation, ParamOverrides, PlacedObject, ProjectMeta,
    ShapePath, SourceStyle, ToolSlotRef,
};

use super::*;
use crate::{CompileInput, MatSelection, compile, compile_cached};

const BLADE: &str = "generic.drag-blade@1.0.0";
const PEN: &str = "pen.fineliner-black-0.4@1.0.0";

fn libs() -> LibrarySnapshot {
    LibrarySnapshot::from_entries(builtin_entries().unwrap())
}

fn air2() -> MachineProfile {
    let r: ps_core::LibRef = "generic.cutter-small@1.0.0".parse().unwrap();
    (*libs().resolve_profile(&r).unwrap().item).clone()
}

fn op(tool: &str, kind: OpKind) -> Operation {
    Operation {
        id: OpId::new(),
        kind,
        tool: ToolSlotRef {
            tool: tool.parse().unwrap(),
            pen_colour: None,
        },
        overrides: ParamOverrides::default(),
        fill: None,
        enabled: true,
        row: OpRowId::at_unix_ms(1),
    }
}

fn object(x: f64, tool: &str, kind: OpKind) -> PlacedObject {
    PlacedObject {
        id: ObjectId::new(),
        source: None,
        transform: Affine::IDENTITY,
        group: None,
        attach: None,
        paths: Arc::new(vec![Arc::new(ShapePath {
            key: PathKey::source("p"),
            geom: Path::rect(x, 10.0, 20.0, 20.0),
            fill_rule: FillRule::NonZero,
            style: SourceStyle::default(),
            ops: vec![op(tool, kind)],
            start_locked: false,
        })]),
        locked: false,
        hidden: false,
    }
}

fn doc(objects: Vec<PlacedObject>) -> Document {
    let mut mat = Mat::new(MatId::new(), "Mat", MatSize::inches(12.0, 12.0));
    for o in objects {
        Arc::make_mut(&mut mat.objects).insert(o.id, Arc::new(o));
    }
    let mut d = Document::new(ProjectMeta::new("t", "t", "v"));
    Arc::make_mut(&mut d.mats).insert(mat.id, Arc::new(mat));
    d
}

fn three() -> Document {
    doc(vec![
        object(10.0, PEN, OpKind::Draw),
        object(40.0, PEN, OpKind::Draw),
        object(70.0, BLADE, OpKind::Cut),
    ])
}

fn run(d: &Document, cal: &Calibration, cache: &mut CompileCache) -> ps_job::Job {
    let (libraries, profile) = (libs(), air2());
    compile_cached(
        &CompileInput {
            doc: d,
            libs: &libraries,
            profile: &profile,
            calibration: cal,
            mats: MatSelection::All,
        },
        cache,
    )
    .unwrap()
}

fn uncached(d: &Document, cal: &Calibration) -> ps_job::Job {
    let (libraries, profile) = (libs(), air2());
    compile(&CompileInput {
        doc: d,
        libs: &libraries,
        profile: &profile,
        calibration: cal,
        mats: MatSelection::All,
    })
    .unwrap()
}

#[test]
fn a_cold_compile_misses_once_per_object_and_operation() {
    let mut cache = CompileCache::default();
    run(&three(), &Calibration::default(), &mut cache);
    let s = cache.stats();
    assert_eq!((s.hits, s.misses, s.entries), (0, 3, 3));
    assert_ne!(s.bytes, 0);
}

#[test]
fn recompiling_the_same_document_hits_everything() {
    let d = three();
    let mut cache = CompileCache::default();
    let cal = Calibration::default();
    let first = run(&d, &cal, &mut cache);
    let second = run(&d, &cal, &mut cache);
    let s = cache.stats();
    assert_eq!((s.hits, s.misses), (3, 3));
    assert_eq!(first, second);
}

#[test]
fn a_cached_compile_equals_an_uncached_one() {
    let d = three();
    let cal = Calibration::default();
    let mut cache = CompileCache::default();
    run(&d, &cal, &mut cache);
    assert_eq!(run(&d, &cal, &mut cache), uncached(&d, &cal));
}

#[test]
fn editing_one_object_re_prepares_only_that_object() {
    let d = three();
    let cal = Calibration::default();
    let mut cache = CompileCache::default();
    run(&d, &cal, &mut cache);

    // Move the first object by changing its transform.
    let mut edited = d.clone();
    let mat_id = *edited.mats.keys().next().unwrap();
    let mut mat = (**edited.mats.get(&mat_id).unwrap()).clone();
    let first_id = *mat.objects.keys().next().unwrap();
    let mut moved = (**mat.objects.get(&first_id).unwrap()).clone();
    moved.transform = Affine::translate((5.0, 0.0));
    Arc::make_mut(&mut mat.objects).insert(first_id, Arc::new(moved));
    Arc::make_mut(&mut edited.mats).insert(mat_id, Arc::new(mat));

    let before = cache.stats();
    let job = run(&edited, &cal, &mut cache);
    let after = cache.stats();
    assert_eq!(after.hits - before.hits, 2, "two untouched objects hit");
    assert_eq!(after.misses - before.misses, 1, "the moved object misses");
    assert_eq!(
        job,
        uncached(&edited, &cal),
        "and the result is still right"
    );
}

#[test]
fn a_calibration_change_invalidates_only_what_it_affects() {
    let d = three();
    let mut cache = CompileCache::default();
    run(&d, &Calibration::default(), &mut cache);

    let mut cal = Calibration::default();
    cal.blade_offsets_mm
        .insert("generic.drag-blade".into(), 0.5);
    let before = cache.stats();
    let job = run(&d, &cal, &mut cache);
    let after = cache.stats();
    assert_eq!(
        after.misses - before.misses,
        1,
        "only the blade cut depends on the blade offset"
    );
    assert_eq!(after.hits - before.hits, 2);
    assert_eq!(job, uncached(&d, &cal));
}

#[test]
fn an_override_invalidates_only_operations_whose_tool_declares_the_parameter() {
    let d = three();
    let mut cache = CompileCache::default();
    run(&d, &Calibration::default(), &mut cache);
    // Change the overcut through the project settings. Only the blade declares an overcut
    // parameter; the pens never resolve one, so their cached geometry stays valid.
    let mut edited = d.clone();
    let settings = Arc::make_mut(&mut edited.settings);
    settings.overrides.set(
        ps_model::ParamKey::new("overcut"),
        ps_model::ParamValue::Number(2.0),
    );
    let before = cache.stats();
    let job = run(&edited, &Calibration::default(), &mut cache);
    let after = cache.stats();
    assert_eq!(
        after.misses - before.misses,
        1,
        "the blade cut resolves a new overcut"
    );
    assert_eq!(after.hits - before.hits, 2, "the pens are unaffected");
    assert_eq!(job, uncached(&edited, &Calibration::default()));
}

#[test]
fn a_zero_capacity_cache_keeps_nothing_but_still_compiles() {
    let d = three();
    let cal = Calibration::default();
    let mut cache = CompileCache::new(0);
    let job = run(&d, &cal, &mut cache);
    assert_eq!(cache.stats().entries, 0);
    assert_eq!(job, uncached(&d, &cal));
}

#[test]
fn the_least_recently_used_entry_is_evicted_when_over_the_cap() {
    let d = three();
    let cal = Calibration::default();
    // Learn the size of one entry, then allow a little over two of them.
    let mut probe = CompileCache::default();
    run(&d, &cal, &mut probe);
    let total = probe.stats().bytes;
    let mut cache = CompileCache::new(total * 2 / 3 + 1);
    run(&d, &cal, &mut cache);
    let s = cache.stats();
    assert!(s.entries < 3, "{s:?}");
    assert!(s.bytes <= total * 2 / 3 + 1, "{s:?}");
}

#[test]
fn clearing_empties_the_cache() {
    let mut cache = CompileCache::default();
    run(&three(), &Calibration::default(), &mut cache);
    cache.clear();
    let s = cache.stats();
    assert_eq!((s.entries, s.bytes), (0, 0));
}

#[test]
fn the_default_cap_is_256_mebibytes() {
    assert_eq!(CompileCache::default().capacity_bytes(), 256 * 1024 * 1024);
}

// ---- the key ----

fn key_for(path: &ShapePath, to_machine: Affine, params: &BTreeMap<String, f64>) -> ContentHash {
    memo_key(&PrepareInput {
        path,
        to_machine,
        kind: OpKind::Draw,
        drive: ps_library::DriveType::FixedPoint,
        tip_width_mm: Some(0.4),
        fill: None,
        blade_offset_mm: 0.0,
        wheel_radius_mm: None,
        params,
        turn_step_deg: None,
    })
}

fn shape(x: f64) -> ShapePath {
    ShapePath {
        key: PathKey::source("p"),
        geom: Path::rect(x, 0.0, 10.0, 10.0),
        fill_rule: FillRule::NonZero,
        style: SourceStyle::default(),
        ops: Vec::new(),
        start_locked: false,
    }
}

use std::collections::BTreeMap;

#[test]
fn the_key_ignores_identity_but_follows_geometry() {
    let p = BTreeMap::new();
    let a = key_for(&shape(0.0), Affine::IDENTITY, &p);
    let mut renamed = shape(0.0);
    renamed.key = PathKey::source("another-name");
    assert_eq!(
        a,
        key_for(&renamed, Affine::IDENTITY, &p),
        "path keys are identity, not geometry"
    );
    assert_ne!(a, key_for(&shape(1.0), Affine::IDENTITY, &p));
}

#[test]
fn the_key_follows_the_transform_and_the_parameters() {
    let s = shape(0.0);
    let p = BTreeMap::new();
    let a = key_for(&s, Affine::IDENTITY, &p);
    assert_ne!(a, key_for(&s, Affine::translate((0.001, 0.0)), &p));
    let mut q = BTreeMap::new();
    q.insert("passes".to_owned(), 2.0);
    assert_ne!(a, key_for(&s, Affine::IDENTITY, &q));
}

#[test]
fn the_key_follows_the_tool_and_the_operation() {
    let s = shape(0.0);
    let p = BTreeMap::new();
    let base = PrepareInput {
        path: &s,
        to_machine: Affine::IDENTITY,
        kind: OpKind::Draw,
        drive: ps_library::DriveType::FixedPoint,
        tip_width_mm: Some(0.4),
        fill: None,
        blade_offset_mm: 0.0,
        wheel_radius_mm: None,
        params: &p,
        turn_step_deg: None,
    };
    let k = memo_key(&base);
    assert_ne!(
        k,
        memo_key(&PrepareInput {
            kind: OpKind::Cut,
            ..base
        })
    );
    assert_ne!(
        k,
        memo_key(&PrepareInput {
            drive: ps_library::DriveType::Drag,
            ..base
        })
    );
    assert_ne!(
        k,
        memo_key(&PrepareInput {
            tip_width_mm: Some(0.5),
            ..base
        })
    );
    assert_ne!(
        k,
        memo_key(&PrepareInput {
            wheel_radius_mm: Some(2.0),
            ..base
        })
    );
    assert_ne!(
        k,
        memo_key(&PrepareInput {
            blade_offset_mm: 0.3,
            ..base
        })
    );
    assert_eq!(k, memo_key(&base), "stable");
}
