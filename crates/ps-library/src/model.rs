//! The library data model: tools (pens included), materials with presets,
//! and machine profiles. Rust types are the source of truth; JSON Schemas are generated from
//! them (see [`crate::schema`]).

use std::collections::BTreeMap;

use ps_core::LibRef;
use schemars::JsonSchema;
use semver::Version;
use serde::{Deserialize, Serialize};

/// What kind of library entry a file describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryKind {
    /// A tool (blade, pen, stylus, foil tool …).
    Tool,
    /// A material with its presets.
    Material,
    /// A machine profile.
    Profile,
}

impl LibraryKind {
    /// Every kind, in a fixed order.
    pub const ALL: [Self; 3] = [Self::Tool, Self::Material, Self::Profile];

    /// The stable `$id` of this kind's published JSON Schema.
    #[must_use]
    pub fn schema_id(self) -> String {
        format!(
            "https://plotter-studio.github.io/schemas/{}/v1.json",
            self.name()
        )
    }

    /// The singular name, as used in schema URLs and messages.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::Material => "material",
            Self::Profile => "profile",
        }
    }

    /// The storage folder name.
    #[must_use]
    pub fn folder(self) -> &'static str {
        match self {
            Self::Tool => "tools",
            Self::Material => "materials",
            Self::Profile => "profiles",
        }
    }
}

/// The kind of tool.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolFamily {
    /// A cutting blade.
    Blade,
    /// A pen or marker.
    Pen,
    /// A foil transfer tool.
    Foil,
    /// A scoring stylus or wheel.
    Scoring,
    /// An embossing tool.
    Emboss,
    /// An engraving tool.
    Engrave,
    /// A perforating blade.
    Perforate,
    /// A wheel tool.
    Wheel,
    /// A heated tool.
    Heated,
    /// Anything else.
    Other,
}

/// How a tool follows a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DriveType {
    /// A swivel blade dragged behind the holder.
    Drag,
    /// A tool with no direction, such as a pen.
    FixedPoint,
    /// A driven blade that turns to follow the path.
    Tangential,
    /// A rolling wheel.
    Rolling,
    /// A heated tip.
    Heated,
}

/// The type of a tool parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    /// Any real number.
    Number,
    /// A whole number.
    Integer,
}

/// The range and default of one tool parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ParamSpec {
    /// Number or integer.
    #[serde(rename = "type")]
    pub kind: ParamType,
    /// The unit, such as `gf` or `mm/s`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Smallest allowed value.
    pub min: f64,
    /// Largest allowed value.
    pub max: f64,
    /// Value used when nothing overrides it.
    pub default: f64,
}

/// Physical dimensions of a tool.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Geometry {
    /// Distance from the swivel axis to the blade tip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blade_offset_mm: Option<f64>,
    /// The smallest feature the tool can make.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_feature_mm: Option<f64>,
    /// Width of a pen's line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_width_mm: Option<f64>,
    /// Radius of a wheel tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wheel_radius_mm: Option<f64>,
}

/// How the tool is drawn in the preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Render {
    /// The effect name, such as `cut` or `ink`.
    pub effect: String,
}

/// A named colour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Colour {
    /// `#RRGGBB`.
    pub hex: String,
    /// A display name.
    pub name: String,
}

impl Colour {
    /// Parses the hex colour into red, green and blue bytes.
    #[must_use]
    pub fn rgb(&self) -> Option<[u8; 3]> {
        let digits = self.hex.strip_prefix('#')?;
        if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
        Some([byte(0)?, byte(2)?, byte(4)?])
    }
}

/// Extra fields of a pen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PenInfo {
    /// The ink colour.
    pub colour: Colour,
    /// The tip type, such as `fibre` or `ballpoint`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_type: Option<String>,
    /// Set in the user's overlay when the pen is in the pen inventory.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub owned: bool,
}

/// Keeps an id usable as a [`LibRef`] id: invalid characters become `_`.
fn clean_id(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "_".to_owned()
    } else {
        cleaned
    }
}

fn lib_ref(id: &str, version: &Version) -> LibRef {
    // `clean_id` guarantees a valid id, so construction cannot fail.
    LibRef::new(clean_id(id), version.clone())
        .unwrap_or_else(|_| LibRef::new("_", Version::new(0, 0, 0)).expect("`_` is a valid id"))
}

/// A tool definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Tool {
    /// Layout version of this file.
    pub schema_version: u32,
    /// Stable id, such as `generic.drag-blade`.
    pub id: String,
    /// Semantic version of this definition.
    #[schemars(with = "String")]
    pub version: Version,
    /// Display name.
    pub name: String,
    /// The kind of tool.
    pub family: ToolFamily,
    /// How it follows a path.
    pub drive: DriveType,
    /// Physical dimensions.
    #[serde(default)]
    pub geometry: Geometry,
    /// Operations it can perform, such as `cut` or `draw`.
    pub operations: Vec<String>,
    /// Holder ids it fits.
    pub holders: Vec<String>,
    /// Tunable parameters.
    pub parameters: BTreeMap<String, ParamSpec>,
    /// Preview style.
    pub render: Render,
    /// Licence of this definition.
    pub license: String,
    /// Pen details, for pens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen: Option<PenInfo>,
}

impl Tool {
    /// This tool as a library reference.
    #[must_use]
    pub fn lib_ref(&self) -> LibRef {
        lib_ref(&self.id, &self.version)
    }
}

/// Texture maps of a material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Textures {
    /// Base colour map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_color: Option<String>,
    /// Normal map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal: Option<String>,
    /// Roughness map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<String>,
    /// Height map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<String>,
    /// Metallic map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic: Option<String>,
    /// Physical size of one texture tile in mm.
    pub scale_mm: f64,
}

impl Textures {
    /// The referenced file names, in a fixed order.
    #[must_use]
    pub fn files(&self) -> Vec<&str> {
        [
            &self.base_color,
            &self.normal,
            &self.roughness,
            &self.height,
            &self.metallic,
        ]
        .into_iter()
        .filter_map(|f| f.as_deref())
        .collect()
    }
}

/// A material's settings for one tool and operation, with optional per-machine overrides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Preset {
    /// The tool id.
    pub tool: String,
    /// The operation, such as `cut`.
    pub operation: String,
    /// Parameter values (force, speed, passes …).
    pub params: BTreeMap<String, f64>,
    /// Values that replace the generic ones on a specific machine, keyed by machine id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub machines: BTreeMap<String, BTreeMap<String, f64>>,
}

impl Preset {
    /// The parameters for a machine: the generic values with that machine's overrides applied.
    #[must_use]
    pub fn params_for(&self, machine_id: &str) -> BTreeMap<String, f64> {
        let mut out = self.params.clone();
        if let Some(overrides) = self.machines.get(machine_id) {
            out.extend(overrides.iter().map(|(k, v)| (k.clone(), *v)));
        }
        out
    }

    /// True when this preset has its own values for the machine.
    #[must_use]
    pub fn has_machine_override(&self, machine_id: &str) -> bool {
        self.machines.contains_key(machine_id)
    }
}

/// A material definition with its presets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Material {
    /// Layout version of this file.
    pub schema_version: u32,
    /// Stable id.
    pub id: String,
    /// Semantic version.
    #[schemars(with = "String")]
    pub version: Version,
    /// Display name.
    pub name: String,
    /// Category, such as `paper` or `vinyl`.
    pub category: String,
    /// Brand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brand: Option<String>,
    /// Thickness in mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness_mm: Option<f64>,
    /// Mat grips it works with.
    #[serde(default)]
    pub grip: Vec<String>,
    /// Preview textures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub textures: Option<Textures>,
    /// Settings per tool and operation.
    #[serde(default)]
    pub presets: Vec<Preset>,
    /// Licence of this definition.
    pub license: String,
    /// Translated display names by language code.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub names: BTreeMap<String, String>,
}

impl Material {
    /// This material as a library reference.
    #[must_use]
    pub fn lib_ref(&self) -> LibRef {
        lib_ref(&self.id, &self.version)
    }
}

/// A width and height in mm.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Size {
    /// Width in mm.
    pub width_mm: f64,
    /// Height in mm.
    pub height_mm: f64,
}

/// A named mat size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MatSizeSpec {
    /// Display name, such as `12x12 in`.
    pub name: String,
    /// Width in mm.
    pub width_mm: f64,
    /// Height in mm.
    pub height_mm: f64,
}

/// A rectangle on the mat.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RectMm {
    /// Left edge.
    pub x_mm: f64,
    /// Top edge.
    pub y_mm: f64,
    /// Width.
    pub width_mm: f64,
    /// Height.
    pub height_mm: f64,
}

/// The corner a machine measures coordinates from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Top left (the convention of most cutting plotters).
    TopLeft,
    /// Top right.
    TopRight,
    /// Bottom left.
    BottomLeft,
    /// Bottom right.
    BottomRight,
}

/// A tool holder of a machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Holder {
    /// Holder id, such as `generic.holder-a`.
    pub id: String,
    /// Display name.
    pub label: String,
    /// The tool families it accepts.
    pub accepts: Vec<ToolFamily>,
}

/// A machine's force range and the mapping to its own units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ForceRange {
    /// Lowest force in grams-force.
    pub min_gf: f64,
    /// Highest force in grams-force.
    pub max_gf: f64,
    /// Machine units per gram-force.
    pub units_per_gf: f64,
}

impl ForceRange {
    /// Converts grams-force to the machine's units.
    #[must_use]
    pub fn to_machine_units(&self, gf: f64) -> f64 {
        gf * self.units_per_gf
    }

    /// Limits a force to the machine's range.
    #[must_use]
    pub fn clamp_gf(&self, gf: f64) -> f64 {
        gf.clamp(self.min_gf, self.max_gf)
    }
}

/// The output dialect choices a machine may need.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GcodeSettings {
    /// The dialect, such as `ps-gcode/1`.
    pub dialect: String,
    /// Emit G2/G3 arcs.
    pub arcs: bool,
    /// Chord tolerance when arcs are off, in mm.
    pub chord_tolerance_mm: f64,
}

/// A roll-fed machine's limits (matless cutting).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Roll {
    /// Roll width.
    pub width_mm: f64,
    /// Longest cut.
    pub max_length_mm: f64,
}

/// A machine profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MachineProfile {
    /// Layout version of this file.
    pub schema_version: u32,
    /// Stable id.
    pub id: String,
    /// Semantic version.
    #[schemars(with = "String")]
    pub version: Version,
    /// Display name.
    pub name: String,
    /// The cuttable area.
    pub work_area: Size,
    /// Mat sizes the machine takes.
    pub mat_sizes: Vec<MatSizeSpec>,
    /// Where coordinates start.
    pub origin: Origin,
    /// Regions the tools must not enter.
    pub keep_outs: Vec<RectMm>,
    /// The tool holders.
    pub holders: Vec<Holder>,
    /// Force range.
    pub force: ForceRange,
    /// Top speed while drawing, in mm/s. A cut's feed is held to this.
    pub max_speed_mm_s: f64,
    /// Top speed of moves with the tool up, in mm/s; [`MachineProfile::max_speed_mm_s`] when
    /// absent. For a machine that travels at a different speed than it draws.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_travel_speed_mm_s: Option<f64>,
    /// Acceleration in mm/s², at the machine's top speed.
    pub max_accel_mm_s2: f64,
    /// How the acceleration follows the speed, as `(speed mm/s, acceleration mm/s²)` points in
    /// ascending speed, for a machine that accelerates harder when it goes faster. Empty means
    /// [`MachineProfile::max_accel_mm_s2`] at every speed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accel_by_speed: Vec<(f64, f64)>,
    /// The time the machine spends on each vector it draws, as `(speed mm/s, milliseconds)`
    /// points in ascending speed, for a machine whose time depends on how many vectors it is given
    /// and not only on how far it moves. Empty means no such cost.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vector_ms_by_speed: Vec<(f64, f64)>,
    /// Where set, the hairpins that join the rows of a fill are drawn as semicircles whose chords
    /// turn by at most this many degrees, for a machine that slows at sharp corners.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_turn_step_deg: Option<f64>,
    /// Junction deviation for the look-ahead planner, in mm.
    pub junction_deviation_mm: f64,
    /// Time to raise the tool.
    pub tool_up_ms: u32,
    /// Time to lower the tool.
    pub tool_down_ms: u32,
    /// The machine supports G2/G3 arcs.
    pub arcs: bool,
    /// The machine has a tangential (A) axis.
    pub tangential_axis: bool,
    /// The machine supports heated tools.
    pub heated: bool,
    /// Heating rate in °C per second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heating_rate_c_s: Option<f64>,
    /// G-code dialect settings.
    pub gcode: GcodeSettings,
    /// Roll limits, for matless machines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roll: Option<Roll>,
    /// Licence of this definition.
    pub license: String,
}

impl MachineProfile {
    /// Top speed of moves with the tool up, in mm/s.
    #[must_use]
    pub fn travel_speed_mm_s(&self) -> f64 {
        self.max_travel_speed_mm_s.unwrap_or(self.max_speed_mm_s)
    }
}

impl MachineProfile {
    /// This profile as a library reference.
    #[must_use]
    pub fn lib_ref(&self) -> LibRef {
        lib_ref(&self.id, &self.version)
    }

    /// The holders that take a tool: the tool lists the holder and the holder accepts its family.
    pub fn holders_for<'a>(&'a self, tool: &'a Tool) -> impl Iterator<Item = &'a Holder> + 'a {
        self.holders
            .iter()
            .filter(move |h| tool.holders.contains(&h.id) && h.accepts.contains(&tool.family))
    }
}

#[cfg(test)]
mod tests;
