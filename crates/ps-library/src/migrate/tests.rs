//! Ground truth for schema migrations: older versions are migrated in code before
//! validation; files newer than the app are refused. Steps are pure functions on JSON values.

use serde_json::{Value, json};

use super::*;

/// v1 -> v2: `title` becomes `name`.
fn rename_title(mut v: Value) -> Value {
    let obj = v.as_object_mut().unwrap();
    if let Some(t) = obj.remove("title") {
        obj.insert("name".to_owned(), t);
    }
    v
}

/// v2 -> v3: add a flag.
fn add_flag(mut v: Value) -> Value {
    v.as_object_mut()
        .unwrap()
        .insert("extra".to_owned(), json!(true));
    v
}

const STEPS: [Step; 2] = [rename_title, add_flag];

#[test]
fn an_old_file_is_carried_through_every_step() {
    let out = migrate_with(json!({"schema_version": 1, "title": "Blade"}), &STEPS).unwrap();
    assert_eq!(
        out,
        json!({"schema_version": 3, "name": "Blade", "extra": true})
    );
}

#[test]
fn a_file_halfway_only_runs_the_remaining_steps() {
    let out = migrate_with(json!({"schema_version": 2, "name": "Blade"}), &STEPS).unwrap();
    assert_eq!(
        out,
        json!({"schema_version": 3, "name": "Blade", "extra": true})
    );
}

#[test]
fn a_current_file_is_untouched() {
    let v = json!({"schema_version": 3, "name": "Blade", "extra": false});
    assert_eq!(migrate_with(v.clone(), &STEPS).unwrap(), v);
}

#[test]
fn a_file_from_a_newer_app_is_refused() {
    let err = migrate_with(json!({"schema_version": 4}), &STEPS).unwrap_err();
    assert_eq!(
        err,
        MigrateError::TooNew {
            found: 4,
            supported: 3
        }
    );
    assert!(err.to_string().contains("update"), "{err}");
}

#[test]
fn a_missing_or_nonsense_version_is_refused() {
    for v in [
        json!({}),
        json!({"schema_version": "one"}),
        json!({"schema_version": 1.5}),
        json!({"schema_version": -1}),
    ] {
        assert_eq!(
            migrate_with(v, &STEPS).unwrap_err(),
            MigrateError::MissingVersion
        );
    }
    assert_eq!(
        migrate_with(json!({"schema_version": 0}), &STEPS).unwrap_err(),
        MigrateError::Unsupported(0)
    );
}

#[test]
fn only_objects_can_be_migrated() {
    assert_eq!(
        migrate_with(json!([1, 2]), &STEPS).unwrap_err(),
        MigrateError::NotObject
    );
    assert_eq!(
        migrate_with(json!("x"), &STEPS).unwrap_err(),
        MigrateError::NotObject
    );
}

#[test]
fn with_no_steps_only_version_one_exists() {
    let v = json!({"schema_version": 1, "id": "a"});
    assert_eq!(migrate_with(v.clone(), &[]).unwrap(), v);
    assert_eq!(
        migrate_with(json!({"schema_version": 2}), &[]).unwrap_err(),
        MigrateError::TooNew {
            found: 2,
            supported: 1
        }
    );
}

#[test]
fn the_real_tables_are_current() {
    for kind in LibraryKind::ALL {
        let v = json!({"schema_version": CURRENT_SCHEMA, "id": "a"});
        assert_eq!(to_current(kind, v.clone()).unwrap(), v);
    }
    assert_eq!(CURRENT_SCHEMA, 1);
}
