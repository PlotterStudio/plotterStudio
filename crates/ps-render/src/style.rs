//! How each kind of operation looks. Colour is never the only cue: each kind also
//! has a line style and an icon.

use ps_model::{LineStyle, OpKind};

/// The line style an operation kind is drawn with.
#[must_use]
pub fn style_of(kind: OpKind) -> LineStyle {
    kind.line_style()
}

/// The name of the icon shown beside an operation kind.
#[must_use]
pub fn icon_name(kind: OpKind) -> &'static str {
    kind.icon()
}

/// The display colour of an operation kind as `[r, g, b]`.
#[must_use]
pub fn colour_of(kind: OpKind) -> [u8; 3] {
    match kind {
        OpKind::Cut => [0xFF, 0x8A, 0x1F],
        OpKind::KissCut => [0xFB, 0x8C, 0x00],
        OpKind::Draw => [0x4C, 0x8D, 0xFF],
        OpKind::Fill => [0x5E, 0x35, 0xB1],
        OpKind::Score => [0x1B, 0xB5, 0xA2],
        OpKind::Emboss => [0x00, 0x89, 0x7B],
        OpKind::Deboss => [0x6D, 0x4C, 0x41],
        OpKind::Engrave => [0x8E, 0x24, 0xAA],
        OpKind::Foil => [0xFF, 0xC2, 0x3D],
        OpKind::FoilFill => [0xD4, 0xAF, 0x37],
        OpKind::Perforate => [0xD8, 0x1B, 0x60],
        OpKind::WavyCut => [0x00, 0xAC, 0xC1],
    }
}

/// The dash pattern in screen pixels as on/off pairs; empty for a continuous line.
#[must_use]
pub fn dash_pattern_px(style: LineStyle) -> Vec<f64> {
    match style {
        LineStyle::Dashed => vec![8.0, 4.0],
        LineStyle::Dotted => vec![1.5, 3.5],
        LineStyle::Solid | LineStyle::Double | LineStyle::Wavy | LineStyle::Hatched => Vec::new(),
    }
}

/// The dash pattern in millimetres at `zoom` pixels per millimetre.
#[must_use]
pub fn dash_pattern_mm(style: LineStyle, zoom: f64) -> Vec<f64> {
    dash_pattern_px(style)
        .into_iter()
        .map(|px| px / zoom)
        .collect()
}

/// The perpendicular offsets in pixels a style draws its line at: two for a double line.
#[must_use]
pub fn parallel_offsets_px(style: LineStyle) -> Vec<f64> {
    match style {
        LineStyle::Double => vec![-1.5, 1.5],
        _ => vec![0.0],
    }
}

/// How travel moves are drawn in the toolpath view.
#[must_use]
pub fn travel_style() -> LineStyle {
    LineStyle::Dashed
}

#[cfg(test)]
mod tests;
