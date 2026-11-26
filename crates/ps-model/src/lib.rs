//! Plotter Studio document model.

mod command;
mod project;
mod recovery;
mod resolve;
mod save;
mod types;
mod validate;

pub use command::{
    AddAsset, AddMat, AddObject, AssignOperation, Command, CommandError, DocumentStore,
    MERGE_WINDOW_MS, MergeKey, OverrideTarget, RemoveMat, RemoveObject, RemoveOperation,
    RemovePath, SetGuides, SetJobSettings, SetMatMaterial, SetMirror, SetOpEnabled, SetOpFill,
    SetOpOrder, SetOverride, SetPathGeometry, SetPenColour, SetRules, SetRunAfter, SetStartPoint,
    SetTransform, SplitPath,
};
pub use project::{
    EmbeddedFile, Loaded, MIGRATIONS, MIN_READER_VERSION, Migration, Project, ProjectError,
    ReadOnly, SCHEMA_VERSION, Thumbnail, migrate_with,
};
pub use recovery::{
    AutosaveClock, AutosaveEntry, Autosaver, CrashReport, LockInfo, ProcessProbe, Recovery,
    RecoveryError, acquire, format_report, inspect, mark_clean_exit, write_report,
};
pub use resolve::{
    ParamSource, ResolveParamsError, Resolved, ResolvedParams, clamp_to_machine, resolve,
};
pub use save::{SaveError, SaveOptions, backup_path, save_bytes, save_with};
pub use types::{
    Asset, Assign, Document, Guide, GuideAxis, JobSettings, LineStyle, MappingRule, Mat, MatSize,
    MatType, Match, OpKind, Operation, OrderingMode, Orientation, OriginSpec, ParamKey,
    ParamOverrides, ParamValue, PlacedObject, Polygon, ProjectMeta, ShapePath, SourceLink,
    SourceStyle, ToolSlotRef, Units, UnknownOpKind,
};
pub use validate::validate;
