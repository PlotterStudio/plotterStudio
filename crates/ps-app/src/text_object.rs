//! The text dialog's settings turned into paths: outline text for cutting and filling,
//! or single-stroke text for pens, with the text's top-left corner at the origin.

use ps_geometry::{Affine, Path};
use ps_import::{Align, StrokeFont, TextStyle, outline_text, stroke_text};

use crate::fonts::{BUNDLED_FAMILY, FontLibrary};

/// What the text dialog collects.
#[derive(Debug, Clone, PartialEq)]
pub struct TextParams {
    /// The text; lines are separated by newlines.
    pub text: String,
    /// The font family for outline text.
    pub family: String,
    /// The em size in millimetres, or the capital height for single-stroke text.
    pub size_mm: f64,
    /// Extra space after each letter, in millimetres.
    pub letter_spacing_mm: f64,
    /// The distance between baselines, as a multiple of the size.
    pub line_height: f64,
    /// How lines align.
    pub align: Align,
    /// A single-stroke font for pens; `None` for outline text.
    pub stroke: Option<StrokeFont>,
    /// The name of a loaded SVG single-stroke font; it wins over `stroke` when set.
    pub svg_font: Option<String>,
}

impl Default for TextParams {
    fn default() -> Self {
        Self {
            text: "Text".to_owned(),
            family: BUNDLED_FAMILY.to_owned(),
            size_mm: 20.0,
            letter_spacing_mm: 0.0,
            line_height: 1.2,
            align: Align::Left,
            stroke: None,
            svg_font: None,
        }
    }
}

/// The paths for `params`, moved so the top-left corner of the text is the origin.
///
/// # Errors
///
/// A message when the text is blank, the size is not usable, or the font cannot be read.
pub fn text_paths(params: &TextParams, fonts: &FontLibrary) -> Result<Vec<Path>, String> {
    if params.text.trim().is_empty() {
        return Err("there is no text to place".to_owned());
    }
    let paths = if let Some(name) = &params.svg_font {
        let font = fonts
            .stroke_font(name)
            .ok_or_else(|| format!("the single-stroke font {name} is not loaded"))?;
        font.text(&params.text, params.size_mm)
            .map_err(|e| e.to_string())?
    } else if let Some(font) = params.stroke {
        stroke_text(&params.text, font, params.size_mm).map_err(|e| e.to_string())?
    } else {
        let (_, data) = fonts.resolve(&params.family);
        outline_text(
            &data,
            &params.text,
            &TextStyle {
                size_mm: params.size_mm,
                letter_spacing_mm: params.letter_spacing_mm,
                line_height: params.line_height,
                align: params.align,
            },
        )
        .map_err(|e| e.to_string())?
        .paths
    };
    let Some(first) = paths.first() else {
        return Err("the text has no visible letters".to_owned());
    };
    let bounds = paths
        .iter()
        .map(Path::bounds)
        .fold(first.bounds(), |a, b| a.union(b));
    let shift = Affine::translate((-bounds.x0, -bounds.y0));
    Ok(paths.iter().map(|p| p.transformed(shift)).collect())
}

#[cfg(test)]
mod tests;
