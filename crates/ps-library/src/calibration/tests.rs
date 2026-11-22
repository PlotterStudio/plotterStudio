//! Ground truth for per-machine calibration: blade offsets, holder X/Y offsets,
//! force scale and time scale, stored per machine and applied by the compiler.

use super::*;

#[test]
fn defaults_change_nothing() {
    let c = Calibration::default();
    assert_eq!(c.force_scale, 1.0);
    assert_eq!(c.time_scale, 1.0);
    assert_eq!(c.scaled_force(300.0), 300.0);
    assert_eq!(c.blade_offset("any", 0.25), 0.25);
    assert_eq!(
        c.holder_offset("clamp-a"),
        Xy {
            x_mm: 0.0,
            y_mm: 0.0
        }
    );
    assert_eq!(c.validate(), vec![]);
}

#[test]
fn a_measured_blade_offset_replaces_the_tool_default() {
    let mut c = Calibration::default();
    c.blade_offsets_mm
        .insert("generic.drag-blade".to_owned(), 0.31);
    assert_eq!(c.blade_offset("generic.drag-blade", 0.25), 0.31);
    assert_eq!(c.blade_offset("generic.deep-cut-blade", 0.25), 0.25);
}

#[test]
fn holder_offsets_are_per_holder() {
    let mut c = Calibration::default();
    c.holder_offsets_mm.insert(
        "clamp-b".to_owned(),
        Xy {
            x_mm: 0.4,
            y_mm: -0.2,
        },
    );
    assert_eq!(
        c.holder_offset("clamp-b"),
        Xy {
            x_mm: 0.4,
            y_mm: -0.2
        }
    );
    assert_eq!(
        c.holder_offset("clamp-a"),
        Xy {
            x_mm: 0.0,
            y_mm: 0.0
        }
    );
}

#[test]
fn force_scale_multiplies_the_force() {
    let c = Calibration {
        force_scale: 1.2,
        ..Calibration::default()
    };
    assert!((c.scaled_force(250.0) - 300.0).abs() < 1e-9);
}

#[test]
fn an_empty_file_is_the_default() {
    let c: Calibration = serde_json::from_str("{}").unwrap();
    assert_eq!(c, Calibration::default());
}

#[test]
fn partial_files_keep_defaults_for_the_rest() {
    let c: Calibration = serde_json::from_str(r#"{"time_scale": 1.08}"#).unwrap();
    assert_eq!(c.time_scale, 1.08);
    assert_eq!(c.force_scale, 1.0);
}

#[test]
fn it_round_trips() {
    let mut c = Calibration {
        time_scale: 0.93,
        ..Calibration::default()
    };
    c.blade_offsets_mm.insert("t".to_owned(), 0.3);
    c.holder_offsets_mm.insert(
        "h".to_owned(),
        Xy {
            x_mm: 1.0,
            y_mm: 2.0,
        },
    );
    let text = serde_json::to_string(&c).unwrap();
    assert_eq!(serde_json::from_str::<Calibration>(&text).unwrap(), c);
}

#[test]
fn nonsense_values_are_reported() {
    let mut c = Calibration {
        force_scale: 0.0,
        time_scale: f64::NAN,
        ..Calibration::default()
    };
    c.blade_offsets_mm.insert("t".to_owned(), -1.0);
    let p: Vec<String> = c.validate().into_iter().map(|i| i.pointer).collect();
    assert!(p.contains(&"/force_scale".to_owned()));
    assert!(p.contains(&"/time_scale".to_owned()));
    assert!(p.contains(&"/blade_offsets_mm/t".to_owned()));
}

#[test]
fn files_are_named_after_the_machine() {
    assert_eq!(
        file_name("generic.cutter-medium"),
        "generic.cutter-medium.json"
    );
}
