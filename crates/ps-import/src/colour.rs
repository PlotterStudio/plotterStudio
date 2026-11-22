//! Colour matching for mapping rules.
//!
//! Colours match exactly or within a CIEDE2000 tolerance; `pen:auto` picks the nearest owned pen.
//! CIEDE2000 comes from the `palette` crate; sRGB is converted to CIELAB under D65.

use palette::color_difference::Ciede2000;
use palette::white_point::D65;
use palette::{FromColor, Lab, Srgb};

/// CIEDE2000 difference between two CIELAB colours given as `[L, a, b]`.
#[must_use]
pub fn delta_e2000_lab(first: [f64; 3], second: [f64; 3]) -> f64 {
    let a = Lab::<D65, f64>::new(first[0], first[1], first[2]);
    let b = Lab::<D65, f64>::new(second[0], second[1], second[2]);
    a.difference(b)
}

fn to_lab([r, g, b]: [u8; 3]) -> [f64; 3] {
    let srgb = Srgb::new(
        f64::from(r) / 255.0,
        f64::from(g) / 255.0,
        f64::from(b) / 255.0,
    );
    let lab = Lab::<D65, f64>::from_color(srgb.into_linear());
    [lab.l, lab.a, lab.b]
}

/// CIEDE2000 difference between two sRGB colours.
#[must_use]
pub fn delta_e2000(first: [u8; 3], second: [u8; 3]) -> f64 {
    delta_e2000_lab(to_lab(first), to_lab(second))
}

/// True when two colours are equal, or (with a positive tolerance) within it in CIEDE2000.
#[must_use]
pub fn within(first: [u8; 3], second: [u8; 3], tolerance: f64) -> bool {
    first == second || (tolerance > 0.0 && delta_e2000(first, second) <= tolerance)
}

/// The index of the palette colour nearest to `colour`; ties go to the first.
#[must_use]
pub fn nearest(colour: [u8; 3], palette: &[[u8; 3]]) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, candidate) in palette.iter().enumerate() {
        let d = delta_e2000(colour, *candidate);
        if best.is_none_or(|(_, b)| d < b) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// Parses `#RRGGBB`, `RRGGBB` or `#RGB`.
#[must_use]
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    let byte = |s: &str| u8::from_str_radix(s, 16).ok();
    match digits.len() {
        6 => Some([
            byte(&digits[0..2])?,
            byte(&digits[2..4])?,
            byte(&digits[4..6])?,
        ]),
        3 => {
            let one = |i: usize| byte(&digits[i..=i]).map(|v| v * 17);
            Some([one(0)?, one(1)?, one(2)?])
        }
        _ => None,
    }
}

/// Prints a colour as `#RRGGBB`.
#[must_use]
pub fn to_hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

#[cfg(test)]
mod tests;
