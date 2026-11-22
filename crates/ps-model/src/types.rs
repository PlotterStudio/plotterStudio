//! The document tree.
//!
//! A [`Document`] is a tree of immutable, reference-counted nodes edited by copy-on-write: every
//! edit makes a new root that shares unchanged nodes with the old one. That gives cheap undo,
//! cheap snapshots for background compiles, and safe sharing across threads without locks.
//! Ordered maps ([`IndexMap`]) keep iteration order, and therefore compiler output, stable.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use indexmap::IndexMap;
use ps_core::{
    AssetId, AttachSetId, ContentHash, GroupId, LibRef, MatId, Mm, ObjectId, OpId, OpRowId,
    PathKey, hash_bytes,
};
use ps_geometry::{Affine, FillRule, FillSpec, Path, Point, Rect};
use serde::{Deserialize, Serialize};

/// How a kind of operation is drawn, so colour is never the only cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LineStyle {
    /// An unbroken line.
    Solid,
    /// Dashes.
    Dashed,
    /// Dots.
    Dotted,
    /// Two parallel lines.
    Double,
    /// A wavy line.
    Wavy,
    /// Parallel hatching.
    Hatched,
}

/// What an operation does to the material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    /// Cut through.
    Cut,
    /// Cut the top layer only.
    KissCut,
    /// Draw a line with a pen.
    Draw,
    /// Fill a region with a pen.
    Fill,
    /// Score a fold line.
    Score,
    /// Emboss (raise).
    Emboss,
    /// Deboss (press in).
    Deboss,
    /// Engrave.
    Engrave,
    /// Apply foil along a line.
    Foil,
    /// Apply foil over a region.
    FoilFill,
    /// Perforate.
    Perforate,
    /// Cut a wavy line.
    WavyCut,
}

/// The operation name is unknown.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown operation kind {0:?}")]
pub struct UnknownOpKind(pub String);

impl OpKind {
    /// Every kind, in a fixed order.
    pub const ALL: [Self; 12] = [
        Self::Cut,
        Self::KissCut,
        Self::Draw,
        Self::Fill,
        Self::Score,
        Self::Emboss,
        Self::Deboss,
        Self::Engrave,
        Self::Foil,
        Self::FoilFill,
        Self::Perforate,
        Self::WavyCut,
    ];

    /// The stable `snake_case` name, as used in tool `operations` lists and JSON.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cut => "cut",
            Self::KissCut => "kiss_cut",
            Self::Draw => "draw",
            Self::Fill => "fill",
            Self::Score => "score",
            Self::Emboss => "emboss",
            Self::Deboss => "deboss",
            Self::Engrave => "engrave",
            Self::Foil => "foil",
            Self::FoilFill => "foil_fill",
            Self::Perforate => "perforate",
            Self::WavyCut => "wavy_cut",
        }
    }

    /// The line style this kind is drawn with.
    #[must_use]
    pub fn line_style(self) -> LineStyle {
        match self {
            Self::Cut | Self::Draw | Self::Emboss | Self::Deboss | Self::Engrave | Self::Foil => {
                LineStyle::Solid
            }
            Self::KissCut => LineStyle::Double,
            Self::Score => LineStyle::Dashed,
            Self::Perforate => LineStyle::Dotted,
            Self::WavyCut => LineStyle::Wavy,
            Self::Fill | Self::FoilFill => LineStyle::Hatched,
        }
    }

    /// The name of the icon shown beside this kind.
    #[must_use]
    pub fn icon(self) -> &'static str {
        match self {
            Self::Cut => "cut",
            Self::KissCut => "kiss-cut",
            Self::Draw => "pen",
            Self::Fill => "pen-fill",
            Self::Score => "score",
            Self::Emboss => "emboss",
            Self::Deboss => "deboss",
            Self::Engrave => "engrave",
            Self::Foil => "foil",
            Self::FoilFill => "foil-fill",
            Self::Perforate => "perforate",
            Self::WavyCut => "wavy",
        }
    }
}

impl FromStr for OpKind {
    type Err = UnknownOpKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| UnknownOpKind(s.to_owned()))
    }
}

/// The name of a tool parameter, such as `force`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParamKey(String);

impl ParamKey {
    /// Creates a key.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ParamKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A parameter value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    /// A number.
    Number(f64),
    /// A yes/no setting.
    Bool(bool),
    /// A choice by name.
    Text(String),
}

impl ParamValue {
    /// The value as a number, if it is one.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }
}

/// One layer of the parameter resolution chain.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParamOverrides(BTreeMap<ParamKey, ParamValue>);

impl ParamOverrides {
    /// The override for a key.
    #[must_use]
    pub fn get(&self, key: &ParamKey) -> Option<&ParamValue> {
        self.0.get(key)
    }

    /// Sets an override.
    pub fn set(&mut self, key: ParamKey, value: ParamValue) {
        self.0.insert(key, value);
    }

    /// Removes an override, returning it.
    pub fn remove(&mut self, key: &ParamKey) -> Option<ParamValue> {
        self.0.remove(key)
    }

    /// The number of overrides.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&ParamKey, &ParamValue)> {
        self.0.iter()
    }
}

/// A tool in an operation: the library tool and, for pens, an optional colour variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSlotRef {
    /// The tool definition.
    pub tool: LibRef,
    /// `#RRGGBB` of the pen colour variant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen_colour: Option<String>,
}

/// Appearance in the source file, kept so mapping rules and re-import can use it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceStyle {
    /// Stroke colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<[u8; 3]>,
    /// Stroke width in mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width_mm: Option<f64>,
    /// Dash pattern in mm.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dash: Vec<f64>,
    /// Fill colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<[u8; 3]>,
    /// Opacity from 0 to 1.
    #[serde(default = "full_opacity")]
    pub opacity: f64,
    /// The layer or group name in the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
}

fn full_opacity() -> f64 {
    1.0
}

impl Default for SourceStyle {
    fn default() -> Self {
        Self {
            stroke: None,
            stroke_width_mm: None,
            dash: Vec::new(),
            fill: None,
            opacity: 1.0,
            layer: None,
        }
    }
}

/// One operation on a path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    /// Identity.
    pub id: OpId,
    /// What it does.
    pub kind: OpKind,
    /// The tool it uses.
    pub tool: ToolSlotRef,
    /// Operation-level parameter overrides.
    #[serde(default)]
    pub overrides: ParamOverrides,
    /// Fill settings, for fills.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<FillSpec>,
    /// Whether it runs.
    pub enabled: bool,
    /// The operations-panel row it belongs to.
    pub row: OpRowId,
}

/// A path of an object with its operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapePath {
    /// Stable identity.
    pub key: PathKey,
    /// The geometry in the object's own space.
    pub geom: Path,
    /// How self-overlaps decide what is inside.
    pub fill_rule: FillRule,
    /// Appearance in the source.
    #[serde(default)]
    pub style: SourceStyle,
    /// Operations (one path can carry several).
    #[serde(default)]
    pub ops: Vec<Operation>,
    /// True when the user chose where a closed path starts, so ordering must not move it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub start_locked: bool,
}

/// Serde helper: skips a `false` flag.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "the signature serde requires"
)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// Where an object came from, for re-import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLink {
    /// The stored original file.
    pub asset: AssetId,
    /// The element within it.
    pub element: String,
}

/// An object placed on a mat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacedObject {
    /// Identity.
    pub id: ObjectId,
    /// Where it came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceLink>,
    /// Object space to mat space.
    pub transform: Affine,
    /// Objects edited together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupId>,
    /// Objects whose registration is locked across tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach: Option<AttachSetId>,
    /// The paths.
    pub paths: Arc<Vec<Arc<ShapePath>>>,
    /// Cannot be edited.
    #[serde(default)]
    pub locked: bool,
    /// Not shown and not cut.
    #[serde(default)]
    pub hidden: bool,
}

impl PlacedObject {
    /// The box around all paths in mat space, or `None` for an object with no paths.
    #[must_use]
    pub fn bounds(&self) -> Option<Rect> {
        self.paths
            .iter()
            .map(|p| p.geom.transformed(self.transform).bounds())
            .reduce(|a, b| a.union(b))
    }
}

/// A mat's physical size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MatSize {
    /// Width.
    pub width: Mm,
    /// Height.
    pub height: Mm,
}

impl MatSize {
    /// A size given in inches.
    #[must_use]
    pub fn inches(width: f64, height: f64) -> Self {
        Self {
            width: Mm::from_inches(width),
            height: Mm::from_inches(height),
        }
    }
}

/// The grip of the mat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatType {
    /// Light grip.
    LightGrip,
    /// Standard grip.
    Standard,
    /// Strong grip.
    StrongGrip,
    /// Fabric grip.
    FabricGrip,
    /// No mat (roll-fed cutting).
    Matless,
}

/// How the mat sits in the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    /// Taller than wide.
    Portrait,
    /// Wider than tall.
    Landscape,
}

/// Where the job's origin is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginSpec {
    /// The corner the machine profile uses.
    Profile,
    /// A point on the mat, in mm.
    Custom(Point),
}

/// A polygon on the mat, in mm.
pub type Polygon = Vec<Point>;

/// One mat of the project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mat {
    /// Identity.
    pub id: MatId,
    /// Display name.
    pub name: String,
    /// Physical size.
    pub size: MatSize,
    /// Grip.
    pub mat_type: MatType,
    /// The material on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<LibRef>,
    /// Orientation.
    pub orientation: Orientation,
    /// Origin.
    pub origin: OriginSpec,
    /// Mirror the job at compile time (never the geometry).
    #[serde(default)]
    pub mirror: bool,
    /// Used areas to avoid.
    #[serde(default)]
    pub scrap_regions: Vec<Polygon>,
    /// Mat-level parameter overrides.
    #[serde(default)]
    pub overrides: ParamOverrides,
    /// The objects.
    pub objects: Arc<IndexMap<ObjectId, Arc<PlacedObject>>>,
    /// The user's order of the operations panel.
    #[serde(default)]
    pub op_order: Arc<Vec<OpRowId>>,
    /// Run-after locks: each pair is (runs first, runs later).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run_after: Arc<Vec<(OpRowId, OpRowId)>>,
    /// Guide lines for aligning and snapping.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Arc<Vec<Guide>>,
}

/// Which way a guide runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuideAxis {
    /// A line of constant y.
    Horizontal,
    /// A line of constant x.
    Vertical,
}

/// A guide line on a mat.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    /// Which way it runs.
    pub axis: GuideAxis,
    /// Its position: y for a horizontal guide, x for a vertical one, in millimetres.
    pub at_mm: f64,
}

impl Mat {
    /// An empty standard-grip mat.
    #[must_use]
    pub fn new(id: MatId, name: &str, size: MatSize) -> Self {
        Self {
            id,
            name: name.to_owned(),
            size,
            mat_type: MatType::Standard,
            material: None,
            orientation: Orientation::Portrait,
            origin: OriginSpec::Profile,
            mirror: false,
            scrap_regions: Vec::new(),
            overrides: ParamOverrides::default(),
            objects: Arc::new(IndexMap::new()),
            op_order: Arc::new(Vec::new()),
            run_after: Arc::new(Vec::new()),
            guides: Arc::new(Vec::new()),
        }
    }
}

/// An original imported file, stored once by content hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    /// Identity.
    pub id: AssetId,
    /// File name.
    pub name: String,
    /// BLAKE3 of the content.
    pub hash: ContentHash,
    /// The content. It is stored beside `project.json` as `assets/<hash>`, not inside it.
    #[serde(skip)]
    pub bytes: Arc<Vec<u8>>,
}

impl Asset {
    /// Wraps file content, computing its hash.
    #[must_use]
    pub fn new(id: AssetId, name: &str, bytes: Vec<u8>) -> Self {
        Self {
            id,
            name: name.to_owned(),
            hash: hash_bytes(&bytes),
            bytes: Arc::new(bytes),
        }
    }
}

/// Display units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    /// Millimetres.
    Mm,
    /// Inches.
    In,
}

/// Project identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMeta {
    /// Display name.
    pub name: String,
    /// Display units.
    pub units: Units,
    /// When it was created (RFC 3339 text).
    pub created: String,
    /// The app version that created it.
    pub app_version: String,
}

impl ProjectMeta {
    /// Metadata in millimetres.
    #[must_use]
    pub fn new(name: &str, created: &str, app_version: &str) -> Self {
        Self {
            name: name.to_owned(),
            units: Units::Mm,
            created: created.to_owned(),
            app_version: app_version.to_owned(),
        }
    }
}

/// How the compiler orders tool use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderingMode {
    /// Merge adjacent groups with the same tool where constraints allow.
    #[default]
    MinimiseToolChanges,
    /// Keep the row order and insert changes as needed.
    StrictOrder,
}

/// Project-level settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct JobSettings {
    /// Tool ordering mode.
    #[serde(default)]
    pub ordering: OrderingMode,
    /// Project-level parameter overrides.
    #[serde(default)]
    pub overrides: ParamOverrides,
    /// Draw a 2 mm check mark in the mat corner at the start of each pen.
    #[serde(default)]
    pub pen_check: bool,
}

/// Which source paths a rule applies to. Unset predicates match everything.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Match {
    /// Stroke or fill colour, `#RRGGBB`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    /// CIEDE2000 tolerance for the colour match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance_de: Option<f64>,
    /// A glob on the layer name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    /// Filled or open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filled: Option<bool>,
    /// Dashed strokes only (or only solid).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashed: Option<bool>,
    /// Smallest stroke width in mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_width_mm: Option<f64>,
    /// Largest stroke width in mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width_mm: Option<f64>,
}

/// What a rule assigns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assign {
    /// The operation.
    pub op: OpKind,
    /// A tool id, or `pen:auto` for the nearest owned pen.
    pub tool: String,
}

impl Assign {
    /// True for `pen:auto`.
    #[must_use]
    pub fn is_auto_pen(&self) -> bool {
        self.tool == "pen:auto"
    }
}

/// A colour/layer to operation rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MappingRule {
    /// The predicate.
    #[serde(rename = "match")]
    pub matcher: Match,
    /// The assignment.
    pub assign: Assign,
}

/// The project: mats, objects, operations, overrides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// Identity.
    pub meta: Arc<ProjectMeta>,
    /// Project-level settings.
    pub settings: Arc<JobSettings>,
    /// Original imported files.
    pub assets: Arc<IndexMap<AssetId, Arc<Asset>>>,
    /// The mats.
    pub mats: Arc<IndexMap<MatId, Arc<Mat>>>,
    /// Mapping rules in use.
    pub rules: Arc<Vec<MappingRule>>,
}

impl Document {
    /// An empty document.
    #[must_use]
    pub fn new(meta: ProjectMeta) -> Self {
        Self {
            meta: Arc::new(meta),
            settings: Arc::new(JobSettings::default()),
            assets: Arc::new(IndexMap::new()),
            mats: Arc::new(IndexMap::new()),
            rules: Arc::new(Vec::new()),
        }
    }

    /// Finds a mat.
    #[must_use]
    pub fn mat(&self, id: MatId) -> Option<&Arc<Mat>> {
        self.mats.get(&id)
    }

    /// Finds an object on a mat.
    #[must_use]
    pub fn object(&self, mat: MatId, object: ObjectId) -> Option<&Arc<PlacedObject>> {
        self.mat(mat)?.objects.get(&object)
    }

    /// A rough size in bytes, for history limits.
    #[must_use]
    pub fn approx_bytes(&self) -> usize {
        let mut total = 256;
        for mat in self.mats.values() {
            total += 256;
            for obj in mat.objects.values() {
                total += 128;
                for path in obj.paths.iter() {
                    total += 192 + path.geom.segs.len() * 48 + path.ops.len() * 160;
                }
            }
        }
        total
            + self
                .assets
                .values()
                .map(|a| 128 + a.bytes.len())
                .sum::<usize>()
    }
}

#[cfg(test)]
mod tests;
