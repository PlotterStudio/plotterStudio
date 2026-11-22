//! The tilted and orbiting preview: the mat as a plane seen from a tilt and an azimuth,
//! with parallax occlusion mapping from the height map, so grooves and embossing shift against the
//! surface as the view moves and a raised ridge hides what is behind it.
//!
//! A surface point at ground `(a, b)` (in the view's frame) and height `z` appears at screen
//! `(a, b * cos(tilt) - z * sin(tilt))`: a raised point appears further up the screen than the
//! ground beneath it. Each screen pixel marches a ray down through the height range and takes the
//! colour where it first meets the surface.

use crate::Image;
use crate::preview::{BaseMaterial, EffectMaps, Light, composite};

/// The steepest tilt shown: past this the mat is nearly edge-on and shows nothing useful.
pub const MAX_TILT_DEG: f64 = 80.0;
/// The steps a ray takes down through the relief.
const RAY_STEPS: usize = 48;
/// What shows beside the mat.
const BACKGROUND: [u8; 4] = [30, 30, 34, 255];

/// A view of the mat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    /// The tilt from looking straight down, in degrees (0 to [`MAX_TILT_DEG`]).
    pub tilt_deg: f64,
    /// Where the view comes from, in degrees clockwise: 0 looks from the bottom edge of the mat.
    pub azimuth_deg: f64,
    /// How tall a height of 1 is, in millimetres (0 shows a flat plane).
    pub relief_mm: f64,
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}

fn index(value: f64, size: usize) -> Option<usize> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "checked finite and non-negative; compared with the size next"
    )]
    let i = value.floor() as usize;
    (i < size).then_some(i)
}

fn pixels(mm: f64, scale: f64) -> u32 {
    let px = (mm * scale).round();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a positive size capped to a sane picture"
    )]
    {
        px.clamp(1.0, 16_384.0) as u32
    }
}

/// The mat seen from `orbit`.
#[must_use]
pub fn orbit_view(base: &BaseMaterial, maps: &EffectMaps, light: &Light, orbit: &Orbit) -> Image {
    let flat = composite(base, maps, light);
    let tilt = finite_or(orbit.tilt_deg, 0.0)
        .clamp(0.0, MAX_TILT_DEG)
        .to_radians();
    let azimuth = finite_or(orbit.azimuth_deg, 0.0).to_radians();
    let relief = finite_or(orbit.relief_mm, 0.0).max(0.0);
    if tilt == 0.0 && azimuth.rem_euclid(std::f64::consts::TAU) == 0.0 {
        return flat;
    }
    let (sin_t, cos_t) = (libm::sin(tilt), libm::cos(tilt));
    let (sin_a, cos_a) = (libm::sin(azimuth), libm::cos(azimuth));
    let scale = maps.px_per_mm();
    let (cols, rows) = (maps.width(), maps.height());
    #[expect(clippy::cast_precision_loss, reason = "pixel counts are small")]
    let (w_mm, h_mm) = (cols as f64 / scale, rows as f64 / scale);
    let (cx, cy) = (w_mm / 2.0, h_mm / 2.0);

    // Ground to the view's frame, and back.
    let to_view = |gx: f64, gy: f64| {
        let (dx, dy) = (gx - cx, gy - cy);
        (dx * cos_a - dy * sin_a, dx * sin_a + dy * cos_a)
    };
    let to_ground = |a: f64, b: f64| (a * cos_a + b * sin_a + cx, -a * sin_a + b * cos_a + cy);

    let corners = [(0.0, 0.0), (w_mm, 0.0), (0.0, h_mm), (w_mm, h_mm)].map(|(x, y)| to_view(x, y));
    let (a_min, a_max) = corners
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| {
            (lo.min(c.0), hi.max(c.0))
        });
    let (b_min, b_max) = corners
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| {
            (lo.min(c.1), hi.max(c.1))
        });
    let (z_min, z_max) = (-relief, relief);
    let y_min = b_min * cos_t - z_max * sin_t;
    let y_max = b_max * cos_t - z_min * sin_t;
    let (out_w, out_h) = (pixels(a_max - a_min, scale), pixels(y_max - y_min, scale));

    let mut rgba = Vec::with_capacity(out_w as usize * out_h as usize * 4);
    let steps = if relief > 0.0 { RAY_STEPS } else { 1 };
    for oy in 0..out_h {
        let sy = y_min + (f64::from(oy) + 0.5) / scale;
        for ox in 0..out_w {
            let sx = a_min + (f64::from(ox) + 0.5) / scale;
            let mut colour = BACKGROUND;
            for k in 0..=steps {
                let z = if steps == 1 {
                    0.0
                } else {
                    #[expect(clippy::cast_precision_loss, reason = "a few dozen steps")]
                    let t = k as f64 / steps as f64;
                    z_max - (z_max - z_min) * t
                };
                let b = (sy + z * sin_t) / cos_t;
                let (gx, gy) = to_ground(sx, b);
                let (Some(col), Some(row)) = (index(gx * scale, cols), index(gy * scale, rows))
                else {
                    if steps == 1 {
                        break;
                    }
                    continue;
                };
                // The ray has met the surface once the surface is at or above the ray.
                let surface = f64::from(maps.at(col, row).height) * relief;
                if steps == 1 || surface >= z {
                    let i = (row * cols + col) * 4;
                    colour = [flat.rgba[i], flat.rgba[i + 1], flat.rgba[i + 2], 255];
                    break;
                }
            }
            rgba.extend_from_slice(&colour);
        }
    }
    Image {
        width: out_w,
        height: out_h,
        rgba,
    }
}

#[cfg(test)]
mod tests;
