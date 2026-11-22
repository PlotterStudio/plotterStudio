//! Schema migrations: older files are migrated in code, as pure functions on JSON
//! values, before validation. Files newer than this app are refused.

use serde_json::Value;

use crate::LibraryKind;

/// The schema version this build writes and reads.
pub const CURRENT_SCHEMA: u32 = 1;

/// One migration step: converts a value from version `n` to `n + 1`.
pub type Step = fn(Value) -> Value;

/// Why a file could not be migrated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MigrateError {
    /// The file is not a JSON object.
    #[error("library file must be a JSON object")]
    NotObject,
    /// `schema_version` is missing or not a whole number.
    #[error("library file has no valid schema_version")]
    MissingVersion,
    /// The version is below the oldest supported one.
    #[error("schema version {0} is not supported")]
    Unsupported(u32),
    /// The file was written by a newer app.
    #[error(
        "file uses schema {found}, newer than the {supported} this app supports; update the app"
    )]
    TooNew {
        /// Version in the file.
        found: u32,
        /// Newest version this build reads.
        supported: u32,
    },
}

/// Migrates `value` to the version implied by `steps` (`steps.len() + 1`).
///
/// # Errors
/// [`MigrateError`] when the value is not an object, has no usable version, or is newer or
/// older than the table covers.
pub fn migrate_with(mut value: Value, steps: &[Step]) -> Result<Value, MigrateError> {
    let current = u32::try_from(steps.len())
        .unwrap_or(u32::MAX)
        .saturating_add(1);
    let obj = value.as_object().ok_or(MigrateError::NotObject)?;
    let found = obj
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(MigrateError::MissingVersion)?;
    if found == 0 {
        return Err(MigrateError::Unsupported(found));
    }
    if found > current {
        return Err(MigrateError::TooNew {
            found,
            supported: current,
        });
    }
    for step in &steps[(found - 1) as usize..] {
        value = step(value);
    }
    if let Some(obj) = value.as_object_mut() {
        obj.insert("schema_version".to_owned(), Value::from(current));
    }
    Ok(value)
}

/// Migrates a library file of any kind to the current schema.
///
/// # Errors
/// See [`migrate_with`].
pub fn to_current(_kind: LibraryKind, value: Value) -> Result<Value, MigrateError> {
    // Version 1 is the first schema, so no kind has migration steps yet.
    migrate_with(value, &[])
}

#[cfg(test)]
mod tests;
