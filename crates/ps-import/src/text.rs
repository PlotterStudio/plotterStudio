//! Text: outline text shaped with `rustybuzz` and outlined with `ttf_parser`, giving
//! closed paths for cutting and filling; and single-stroke text from the Hershey fonts, giving open
//! paths for pens. (The `NewStroke` and Borland fonts are not included: the crate that carries
//! `NewStroke` has a build script that overflows its stack on Windows.)
//!
//! Everything comes out in millimetres with y pointing down and the first line's baseline at
//! y = 0, so a placed text object can be moved like any other.

use ps_geometry::{Path, Point};
use vector_text_core::Renderer;
use vector_text_hershey::{HersheyFont, HersheyRenderer};

/// How lines of text sit against each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Lines start at the left edge.
    #[default]
    Left,
    /// Lines are centred on the widest line.
    Centre,
    /// Lines end at the right edge.
    Right,
}

/// How outline text is laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// The em size in millimetres.
    pub size_mm: f64,
    /// Extra space after each letter, in millimetres.
    pub letter_spacing_mm: f64,
    /// The distance between baselines, as a multiple of the size.
    pub line_height: f64,
    /// How lines align.
    pub align: Align,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            size_mm: 10.0,
            letter_spacing_mm: 0.0,
            line_height: 1.2,
            align: Align::Left,
        }
    }
}

/// Why text could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TextError {
    /// The bytes are not a font this build can read.
    #[error("the font file could not be read")]
    BadFont,
    /// The size is not a positive, finite number.
    #[error("the text size must be a positive number")]
    BadSize,
}

/// Outline text: closed contours and the space the text takes.
#[derive(Debug, Clone, PartialEq)]
pub struct TextOutline {
    /// The contours, line by line, in reading order.
    pub paths: Vec<Path>,
    /// The width of the widest line, in millimetres.
    pub width: f64,
    /// The height from the first baseline up by the size to the last baseline, in millimetres.
    pub height: f64,
}

/// Collects a glyph's contours as paths.
struct Contours {
    paths: Vec<Path>,
    current: Option<Path>,
    scale: f64,
    origin: Point,
}

impl Contours {
    fn point(&self, x: f32, y: f32) -> Point {
        Point::new(
            self.origin.x + f64::from(x) * self.scale,
            self.origin.y - f64::from(y) * self.scale,
        )
    }

    fn finish(&mut self) {
        if let Some(path) = self.current.take() {
            if path.segs.is_empty() {
                return;
            }
            let start = path.start;
            let path = if path.end() == start {
                path
            } else {
                path.line_to(start)
            };
            self.paths.push(path.closed());
        }
    }
}

impl ttf_parser::OutlineBuilder for Contours {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish();
        self.current = Some(Path::new(self.point(x, y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.point(x, y);
        if let Some(p) = self.current.take() {
            self.current = Some(p.line_to(to));
        }
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (q, to) = (self.point(x1, y1), self.point(x, y));
        if let Some(p) = self.current.take() {
            let from = p.end();
            let c1 = Point::new(
                from.x + 2.0 / 3.0 * (q.x - from.x),
                from.y + 2.0 / 3.0 * (q.y - from.y),
            );
            let c2 = Point::new(
                to.x + 2.0 / 3.0 * (q.x - to.x),
                to.y + 2.0 / 3.0 * (q.y - to.y),
            );
            self.current = Some(p.cubic_to(c1, c2, to));
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, to) = (self.point(x1, y1), self.point(x2, y2), self.point(x, y));
        if let Some(p) = self.current.take() {
            self.current = Some(p.cubic_to(c1, c2, to));
        }
    }

    fn close(&mut self) {
        self.finish();
    }
}

fn valid_size(size: f64) -> bool {
    size.is_finite() && size > 0.0
}

/// The advance of each shaped glyph in font units, with its glyph id and offsets.
struct Placed {
    glyph: u32,
    x_advance: i32,
    x_offset: i32,
    y_offset: i32,
}

fn shape(face: &rustybuzz::Face<'_>, line: &str) -> Vec<Placed> {
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(line);
    let shaped = rustybuzz::shape(face, &[], buffer);
    shaped
        .glyph_infos()
        .iter()
        .zip(shaped.glyph_positions())
        .map(|(info, pos)| Placed {
            glyph: info.glyph_id,
            x_advance: pos.x_advance,
            x_offset: pos.x_offset,
            y_offset: pos.y_offset,
        })
        .collect()
}

/// Shapes `text` with the font in `font_data` and returns its outlines.
///
/// # Errors
///
/// [`TextError::BadFont`] when the bytes are not a font; [`TextError::BadSize`] for a size that is
/// not positive and finite.
pub fn outline_text(
    font_data: &[u8],
    text: &str,
    style: &TextStyle,
) -> Result<TextOutline, TextError> {
    if !valid_size(style.size_mm) || !valid_size(style.line_height) {
        return Err(TextError::BadSize);
    }
    let face = rustybuzz::Face::from_slice(font_data, 0).ok_or(TextError::BadFont)?;
    let units = f64::from(face.units_per_em());
    if units <= 0.0 {
        return Err(TextError::BadFont);
    }
    let scale = style.size_mm / units;

    // First pass: shape every line and measure it.
    let lines: Vec<Vec<Placed>> = text.split('\n').map(|l| shape(&face, l)).collect();
    let widths: Vec<f64> = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|g| f64::from(g.x_advance) * scale + style.letter_spacing_mm)
                .sum()
        })
        .collect();
    let widest = widths.iter().copied().fold(0.0_f64, f64::max);

    let mut paths = Vec::new();
    for (row, (line, width)) in lines.iter().zip(&widths).enumerate() {
        let shift = match style.align {
            Align::Left => 0.0,
            Align::Centre => (widest - width) / 2.0,
            Align::Right => widest - width,
        };
        let baseline =
            f64::from(u32::try_from(row).unwrap_or(u32::MAX)) * style.line_height * style.size_mm;
        let mut pen = shift;
        for g in line {
            // Glyph 0 is the font's "missing" box: leave a gap instead of drawing it.
            if g.glyph != 0 {
                let mut contours = Contours {
                    paths: Vec::new(),
                    current: None,
                    scale,
                    origin: Point::new(
                        pen + f64::from(g.x_offset) * scale,
                        baseline - f64::from(g.y_offset) * scale,
                    ),
                };
                let id = u16::try_from(g.glyph).unwrap_or(0);
                let _ = face.outline_glyph(ttf_parser::GlyphId(id), &mut contours);
                contours.finish();
                paths.append(&mut contours.paths);
            }
            pen += f64::from(g.x_advance) * scale + style.letter_spacing_mm;
        }
    }
    let rows = f64::from(u32::try_from(lines.len()).unwrap_or(u32::MAX));
    Ok(TextOutline {
        paths,
        width: widest,
        height: (rows - 1.0) * style.line_height * style.size_mm + style.size_mm,
    })
}

/// A single-stroke font: pen-friendly letters drawn as lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeFont {
    /// Hershey Roman simplex: one thin stroke per line.
    RomanSimplex,
    /// Hershey Roman duplex: doubled strokes for a bolder look.
    RomanDuplex,
    /// Hershey Roman complex: serifs.
    RomanComplex,
    /// Hershey Script simplex.
    ScriptSimplex,
    /// Hershey Italic complex.
    ItalicComplex,
}

impl StrokeFont {
    /// Every stroke font.
    pub const ALL: [Self; 5] = [
        Self::RomanSimplex,
        Self::RomanDuplex,
        Self::RomanComplex,
        Self::ScriptSimplex,
        Self::ItalicComplex,
    ];

    fn hershey(self) -> HersheyFont {
        match self {
            Self::RomanSimplex => HersheyFont::Romans,
            Self::RomanDuplex => HersheyFont::Romand,
            Self::RomanComplex => HersheyFont::Romanc,
            Self::ScriptSimplex => HersheyFont::Scripts,
            Self::ItalicComplex => HersheyFont::Italicc,
        }
    }
}

/// Whether the font's y axis points down (as the Hershey tables store it).
const FONT_Y_DOWN: bool = true;

/// Strokes of one line in font units: each stroke is a list of points.
fn raw_strokes(line: &str, font: StrokeFont) -> Vec<Vec<(f64, f64)>> {
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    for p in HersheyRenderer::render_text(line, font.hershey()) {
        let point = (f64::from(p.x), f64::from(p.y));
        match (p.pen, out.last_mut()) {
            (true, Some(stroke)) => stroke.push(point),
            _ => out.push(vec![point]),
        }
    }
    out.retain(|s| s.len() >= 2);
    out
}

/// Renders single-stroke text with capital letters `cap_height_mm` tall.
///
/// # Errors
///
/// [`TextError::BadSize`] for a size that is not positive and finite.
pub fn stroke_text(
    text: &str,
    font: StrokeFont,
    cap_height_mm: f64,
) -> Result<Vec<Path>, TextError> {
    if !valid_size(cap_height_mm) {
        return Err(TextError::BadSize);
    }
    // Scale and baseline come from a capital "H": its height is the cap height and its foot is
    // the baseline.
    let h = raw_strokes("H", font);
    let ys: Vec<f64> = h.iter().flatten().map(|p| p.1).collect();
    let (lo, hi) = (
        ys.iter().copied().fold(f64::INFINITY, f64::min),
        ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    );
    let cap = hi - lo;
    if !cap.is_finite() || cap <= 0.0 {
        return Err(TextError::BadSize);
    }
    let scale = cap_height_mm / cap;
    let baseline_y = if FONT_Y_DOWN { hi } else { lo };

    let mut paths = Vec::new();
    for (row, line) in text.split('\n').enumerate() {
        let drop = f64::from(u32::try_from(row).unwrap_or(u32::MAX)) * 1.5 * cap_height_mm;
        for stroke in raw_strokes(line, font) {
            let points: Vec<Point> = stroke
                .iter()
                .map(|&(x, y)| {
                    let up = if FONT_Y_DOWN {
                        baseline_y - y
                    } else {
                        y - baseline_y
                    };
                    Point::new(x * scale, -up * scale + drop)
                })
                .collect();
            paths.push(Path::polyline(&points, false));
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests;
