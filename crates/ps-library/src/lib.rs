//! Plotter Studio libraries: tools, materials and machine profiles.

mod builtin;
mod calibration;
mod conflict;
mod dirs;
mod index;
mod migrate;
mod model;
mod overlay;
mod package;
mod schema;
mod settings;
mod snapshot;
mod store;
#[cfg(test)]
mod testdata;
mod texture;

pub use builtin::{entries as builtin_entries, install_into as install_builtin};
pub use calibration::{Calibration, Xy, file_name as calibration_file_name};
pub use conflict::{Action, Choice, Resolution, action, keep_both_id, resolve};
pub use dirs::{
    default_plugins_dir, default_root, default_settings_path, default_state_dir, open_at,
};
pub use index::{INDEX_SCHEMA, Index, IndexEntry, Query, sha256_hex};
pub use migrate::{CURRENT_SCHEMA, MigrateError, Step, migrate_with, to_current};
pub use model::{
    Colour, DriveType, ForceRange, GcodeSettings, Geometry, Holder, LibraryKind, MachineProfile,
    MatSizeSpec, Material, Origin, ParamSpec, ParamType, PenInfo, Preset, RectMm, Render, Roll,
    Size, Textures, Tool, ToolFamily,
};
pub use overlay::{HistoryEntry, Overlay, diff, merge_patch};
pub use package::{
    ImportReport, ImportedMaterial, LibraryInfo, PackageError, export_package, read_package,
};
pub use schema::{
    Entry, Issue, check_material, check_profile, check_tool, group_by_file, schema_json, validate,
};
pub use settings::{
    DecimalSeparator, DisplayUnits, GpuQuality, MAX_SCREEN_CALIBRATION, MIN_SCREEN_CALIBRATION,
    Network, Settings, SettingsError, Theme, calibration_from_measurement,
};
pub use snapshot::{LibrarySnapshot, ResolveError, ResolveWarning, Resolved};
pub use store::{Library, Outcome, StoreError, Summary};
pub use texture::{
    CachedTexture, MAX_DIMENSION, TextureError, cache_key, decode, mip_chain, mip_level_count,
    read_cache, write_cache,
};
