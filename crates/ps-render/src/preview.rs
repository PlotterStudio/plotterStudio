//! The material preview: the compiled job rasterised into effect maps, then composited
//! over the material with lighting.
//!
//! Ink is coverage from a capsule at the real tip width, score, deboss and engrave press the
//! height down and emboss pushes it up, cuts leave a kerf (through or kiss), foil is a mask with a
//! colour. The composite lights the surface from the height map, shows ink in proportion to how
//! well it contrasts with the material (light ink on dark card is faint), and tints foil.
//! The resolution steps down on weak hardware.

use ps_geometry::Point;
use ps_job::Step;

use crate::Image;
use crate::toolpath::{SegmentKind, segments};

/// The most pixels per millimetre an effect map uses.
pub const MAX_PX_PER_MM: f64 = 20.0;
/// How far height maps are pressed or raised, in map units (1 is the full effect).
const SCORE_DEPTH: f32 = 1.0;
const ENGRAVE_DEPTH: f32 = 0.6;
/// How strongly the height gradient tilts the surface normal.
const BUMP: f32 = 2.0;
/// How much of the ink's colour shows when it is lighter than the material under it.
const LIGHT_INK_STRENGTH: f32 = 0.45;
const KERF: [f32; 3] = [15.0 / 255.0; 3];
const THROUGH_KERF: f32 = 0.9;
const KISS_KERF: f32 = 0.35;

/// What a tool does to the material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Draws with ink.
    Ink,
    /// Cuts through.
    Cut,
    /// Cuts the top layer only.
    KissCut,
    /// Presses a groove.
    Score,
    /// Raises the surface.
    Emboss,
    /// Presses the surface down.
    Deboss,
    /// Cuts a shallow groove.
    Engrave,
    /// Transfers foil.
    Foil,
}

/// How the tool in a holder slot looks on the material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolLook {
    /// The tool slot.
    pub slot: u8,
    /// What it does.
    pub effect: Effect,
    /// Its colour (ink or foil), `RGB`.
    pub colour: [u8; 3],
    /// The width of its mark in millimetres.
    pub width_mm: f64,
}

/// What the effect maps hold at one pixel.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Texel {
    /// Ink colour, 0 to 1.
    pub ink_rgb: [f32; 3],
    /// Ink coverage.
    pub ink_alpha: f32,
    /// Surface height: negative is pressed down, positive raised.
    pub height: f32,
    /// Foil colour, 0 to 1.
    pub foil_rgb: [f32; 3],
    /// Foil coverage.
    pub foil_alpha: f32,
    /// Kerf coverage.
    pub cut: f32,
    /// True where the kerf goes through the material.
    pub through: bool,
}

/// The effect maps of a mat: one [`Texel`] per pixel.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectMaps {
    width: usize,
    height: usize,
    scale: f64,
    texels: Vec<Texel>,
}

fn unit(c: [u8; 3]) -> [f32; 3] {
    [
        f32::from(c[0]) / 255.0,
        f32::from(c[1]) / 255.0,
        f32::from(c[2]) / 255.0,
    ]
}

/// Mixes `over` onto `under` with the given coverage (premultiplied-correct for the pair).
fn over(under_rgb: [f32; 3], under_a: f32, over_rgb: [f32; 3], c: f32) -> ([f32; 3], f32) {
    let a = c + under_a * (1.0 - c);
    if a <= 0.0 {
        return (under_rgb, 0.0);
    }
    let mix = |u: f32, o: f32| (o * c + u * under_a * (1.0 - c)) / a;
    (
        [
            mix(under_rgb[0], over_rgb[0]),
            mix(under_rgb[1], over_rgb[1]),
            mix(under_rgb[2], over_rgb[2]),
        ],
        a,
    )
}

/// The shortest distance from `p` to the segment `a`..`b`.
fn segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= 0.0 {
        0.0
    } else {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a.x + t * dx, a.y + t * dy);
    (p.x - cx).hypot(p.y - cy)
}

fn dimension(mm: f64, scale: f64) -> usize {
    let px = (mm * scale).ceil();
    if px.is_finite() && px >= 1.0 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a positive pixel count of a mat, capped well below usize"
        )]
        {
            px.min(32_768.0) as usize
        }
    } else {
        1
    }
}

impl EffectMaps {
    /// Rasterises the drawn moves among the first `up_to` steps into maps of a
    /// `mat_width_mm` x `mat_depth_mm` mat at `px_per_mm` pixels per millimetre.
    #[must_use]
    pub fn rasterise(
        steps: &[Step],
        looks: &[ToolLook],
        mat_width_mm: f64,
        mat_depth_mm: f64,
        px_per_mm: f64,
        up_to: usize,
    ) -> Self {
        let scale = if px_per_mm.is_finite() && px_per_mm > 0.0 {
            px_per_mm.min(MAX_PX_PER_MM)
        } else {
            1.0
        };
        let (width, height) = (
            dimension(mat_width_mm, scale),
            dimension(mat_depth_mm, scale),
        );
        let mut maps = Self {
            width,
            height,
            scale,
            texels: vec![Texel::default(); width * height],
        };
        for seg in segments(steps, up_to) {
            let SegmentKind::Draw { slot: Some(slot) } = seg.kind else {
                continue;
            };
            if let Some(look) = looks.iter().find(|l| l.slot == slot) {
                maps.stamp(seg.from, seg.to, look);
            }
        }
        maps
    }

    fn stamp(&mut self, from: Point, to: Point, look: &ToolLook) {
        let radius = (look.width_mm / 2.0).max(0.0);
        let margin = radius + 1.5 / self.scale;
        let (left, right) = (from.x.min(to.x) - margin, from.x.max(to.x) + margin);
        let (top, bottom) = (from.y.min(to.y) - margin, from.y.max(to.y) + margin);
        let last_col = f64::from(u32::try_from(self.width.saturating_sub(1)).unwrap_or(u32::MAX));
        let last_row = f64::from(u32::try_from(self.height.saturating_sub(1)).unwrap_or(u32::MAX));
        let pixel = |mm: f64, last: f64| -> usize {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped into the pixel range first"
            )]
            {
                (mm * self.scale).floor().clamp(0.0, last) as usize
            }
        };
        let (col0, col1) = (pixel(left, last_col), pixel(right, last_col));
        let (row0, row1) = (pixel(top, last_row), pixel(bottom, last_row));
        let ink = unit(look.colour);
        for row in row0..=row1 {
            for col in col0..=col1 {
                #[expect(clippy::cast_precision_loss, reason = "pixel indices are small")]
                let centre = Point::new(
                    (col as f64 + 0.5) / self.scale,
                    (row as f64 + 0.5) / self.scale,
                );
                let dist = segment_distance(centre, from, to);
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "coverage is a 0 to 1 weight"
                )]
                let cover = (0.5 + (radius - dist) * self.scale).clamp(0.0, 1.0) as f32;
                if cover <= 0.0 {
                    continue;
                }
                let texel = &mut self.texels[row * self.width + col];
                match look.effect {
                    Effect::Ink => {
                        let (rgb, alpha) = over(texel.ink_rgb, texel.ink_alpha, ink, cover);
                        texel.ink_rgb = rgb;
                        texel.ink_alpha = alpha;
                    }
                    Effect::Foil => {
                        let (rgb, alpha) = over(texel.foil_rgb, texel.foil_alpha, ink, cover);
                        texel.foil_rgb = rgb;
                        texel.foil_alpha = alpha;
                    }
                    Effect::Score | Effect::Deboss => {
                        texel.height = texel.height.min(-cover * SCORE_DEPTH);
                    }
                    Effect::Engrave => {
                        texel.height = texel.height.min(-cover * ENGRAVE_DEPTH);
                    }
                    Effect::Emboss => texel.height = texel.height.max(cover),
                    Effect::Cut | Effect::KissCut => {
                        if cover > texel.cut {
                            texel.cut = cover;
                        }
                        if look.effect == Effect::Cut && cover > 0.5 {
                            texel.through = true;
                        }
                    }
                }
            }
        }
    }

    /// The width in pixels.
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    /// The height in pixels.
    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    /// The scale in pixels per millimetre.
    #[must_use]
    pub fn px_per_mm(&self) -> f64 {
        self.scale
    }

    pub(crate) fn at(&self, x: usize, y: usize) -> Texel {
        self.texels[y * self.width + x]
    }

    /// The pixel at a position in millimetres; empty outside the mat.
    #[must_use]
    pub fn texel_at(&self, x_mm: f64, y_mm: f64) -> Texel {
        let (fx, fy) = ((x_mm * self.scale).floor(), (y_mm * self.scale).floor());
        if !(fx.is_finite() && fy.is_finite()) || fx < 0.0 || fy < 0.0 {
            return Texel::default();
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "checked non-negative and finite above"
        )]
        let (x, y) = (fx as usize, fy as usize);
        if x >= self.width || y >= self.height {
            return Texel::default();
        }
        self.at(x, y)
    }
}

/// How the material looks apart from the job.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BaseMaterial {
    /// Its base colour.
    pub colour: [u8; 3],
    /// Its roughness, 0 (glossy) to 1 (matt).
    pub roughness: f32,
    /// Its metalness, 0 to 1.
    pub metallic: f32,
}

/// A light shining on the mat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    /// The unit direction from the surface towards the light.
    pub direction: [f32; 3],
}

impl Light {
    /// A light straight above the mat.
    #[must_use]
    pub fn overhead() -> Self {
        Self {
            direction: [0.0, 0.0, 1.0],
        }
    }

    /// A low light that shows relief: `azimuth_deg` is where it comes from, 0 being the top edge of
    /// the mat and increasing clockwise.
    #[must_use]
    pub fn raking(azimuth_deg: f64) -> Self {
        let elevation = 20.0_f64.to_radians();
        let az = azimuth_deg.to_radians();
        #[expect(clippy::cast_possible_truncation, reason = "shading uses f32")]
        let direction = [
            (libm::sin(az) * libm::cos(elevation)) as f32,
            (-libm::cos(az) * libm::cos(elevation)) as f32,
            libm::sin(elevation) as f32,
        ];
        Self { direction }
    }
}

fn luminance(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn byte(v: f32) -> u8 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=255 first"
    )]
    {
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    }
}

/// Lights the material and lays the job's effects over it.
#[must_use]
pub fn composite(base: &BaseMaterial, maps: &EffectMaps, light: &Light) -> Image {
    let (cols, rows) = (maps.width, maps.height);
    let base_rgb = unit(base.colour);
    let toward = light.direction;
    let flat = 0.55 + 0.45 * toward[2].max(0.0);
    let mut rgba = Vec::with_capacity(cols * rows * 4);
    for row in 0..rows {
        for col in 0..cols {
            let texel = maps.at(col, row);
            let height_at = |across: isize, down: isize| -> f32 {
                let nc = col.saturating_add_signed(across).min(cols - 1);
                let nr = row.saturating_add_signed(down).min(rows - 1);
                maps.at(nc, nr).height
            };
            let slope_x = (height_at(1, 0) - height_at(-1, 0)) * 0.5 * BUMP;
            let slope_y = (height_at(0, 1) - height_at(0, -1)) * 0.5 * BUMP;
            let len = (slope_x * slope_x + slope_y * slope_y + 1.0).sqrt();
            let normal = [-slope_x / len, -slope_y / len, 1.0 / len];
            let lit =
                (normal[0] * toward[0] + normal[1] * toward[1] + normal[2] * toward[2]).max(0.0);
            let shade = (0.55 + 0.45 * lit) / flat;
            let mut rgb = [
                base_rgb[0] * shade,
                base_rgb[1] * shade,
                base_rgb[2] * shade,
            ];
            if texel.ink_alpha > 0.0 {
                let strength = if luminance(texel.ink_rgb) <= luminance(base_rgb) {
                    1.0
                } else {
                    LIGHT_INK_STRENGTH
                };
                let ink = [
                    texel.ink_rgb[0] * shade,
                    texel.ink_rgb[1] * shade,
                    texel.ink_rgb[2] * shade,
                ];
                rgb = mix(rgb, ink, texel.ink_alpha * strength);
            }
            if texel.foil_alpha > 0.0 {
                let sheen = 0.85 + 0.3 * (1.0 - base.roughness.clamp(0.0, 1.0)) * shade;
                let foil = [
                    texel.foil_rgb[0] * sheen,
                    texel.foil_rgb[1] * sheen,
                    texel.foil_rgb[2] * sheen,
                ];
                rgb = mix(rgb, foil, texel.foil_alpha);
            }
            if texel.cut > 0.0 {
                let strength = if texel.through {
                    THROUGH_KERF
                } else {
                    KISS_KERF
                };
                rgb = mix(rgb, KERF, texel.cut * strength);
            }
            rgba.extend_from_slice(&[byte(rgb[0]), byte(rgb[1]), byte(rgb[2]), 255]);
        }
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "bounded by the dimension cap"
    )]
    Image {
        width: cols as u32,
        height: rows as u32,
        rgba,
    }
}

/// How much detail the preview affords.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// Full resolution.
    High,
    /// Half resolution.
    Medium,
    /// A quarter of the resolution.
    Low,
}

/// The effect-map scale for a wanted scale at a quality: resolution drops first on weak hardware.
#[must_use]
pub fn adaptive_scale(wanted: f64, quality: Quality) -> f64 {
    let factor = match quality {
        Quality::High => 1.0,
        Quality::Medium => 0.5,
        Quality::Low => 0.25,
    };
    (wanted * factor).max(1.0)
}

/// The effect-map scale for a view at `zoom` pixels per millimetre, from one pixel per millimetre
/// up to [`MAX_PX_PER_MM`].
#[must_use]
pub fn scale_for_zoom(zoom: f64) -> f64 {
    if zoom.is_finite() {
        zoom.clamp(1.0, MAX_PX_PER_MM)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests;
