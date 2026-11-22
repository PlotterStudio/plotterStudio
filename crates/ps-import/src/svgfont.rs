//! SVG single-stroke fonts: the EMS and Hershey sets used by the Inkscape Hershey Text
//! extension are SVG fonts whose glyph paths are strokes, not outlines. This is the small parser
//! the design calls for: it reads `<font>` with `<glyph unicode d horiz-adv-x>`, and lays text out
//! as open paths for a pen.
//!
//! Font units have y up. The output is millimetres with y down and the first line's baseline at
//! y = 0. XML is read with `roxmltree`, which refuses DTDs and so never expands entities or
//! fetches external references.

use std::collections::BTreeMap;

use ps_geometry::{Path, Point};
use roxmltree::Document;
use svgtypes::{SimplePathSegment, SimplifyingPathParser};

/// Why an SVG font could not be read or used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SvgFontError {
    /// The file is not well-formed XML (or has a DTD, which is refused).
    #[error("not a readable SVG file: {0}")]
    Xml(String),
    /// The file has no `<font>` element.
    #[error("the file has no SVG font")]
    NoFont,
    /// The text size is not positive and finite.
    #[error("the text size must be positive")]
    BadSize,
}

#[derive(Debug, Clone)]
struct Glyph {
    advance: f64,
    path: Vec<Path>,
}

/// A single-stroke font read from an SVG font file.
#[derive(Debug, Clone)]
pub struct SvgStrokeFont {
    name: String,
    glyphs: BTreeMap<char, Glyph>,
    missing_advance: f64,
    cap_height: f64,
}

fn number(value: Option<&str>) -> Option<f64> {
    value.and_then(|v| v.trim().parse::<f64>().ok())
}

fn quad_to_cubic(from: Point, control: Point, to: Point) -> (Point, Point) {
    let c1 = Point::new(
        from.x + 2.0 / 3.0 * (control.x - from.x),
        from.y + 2.0 / 3.0 * (control.y - from.y),
    );
    let c2 = Point::new(
        to.x + 2.0 / 3.0 * (control.x - to.x),
        to.y + 2.0 / 3.0 * (control.y - to.y),
    );
    (c1, c2)
}

/// The paths of one glyph in font units (y up); path data that does not parse gives none.
fn glyph_paths(d: &str) -> Vec<Path> {
    let mut paths: Vec<Path> = Vec::new();
    let mut current: Option<Path> = None;
    for segment in SimplifyingPathParser::from(d) {
        let Ok(segment) = segment else {
            return Vec::new();
        };
        match segment {
            SimplePathSegment::MoveTo { x, y } => {
                if let Some(done) = current.take().filter(|p| !p.segs.is_empty()) {
                    paths.push(done);
                }
                current = Some(Path::new(Point::new(x, y)));
            }
            SimplePathSegment::LineTo { x, y } => {
                current = current.take().map(|c| c.line_to(Point::new(x, y)));
            }
            SimplePathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                current = current
                    .take()
                    .map(|c| c.cubic_to(Point::new(x1, y1), Point::new(x2, y2), Point::new(x, y)));
            }
            SimplePathSegment::Quadratic { x1, y1, x, y } => {
                current = current.take().map(|c| {
                    let to = Point::new(x, y);
                    let (c1, c2) = quad_to_cubic(c.end(), Point::new(x1, y1), to);
                    c.cubic_to(c1, c2, to)
                });
            }
            SimplePathSegment::ClosePath => {
                if let Some(c) = current.take() {
                    let start = c.start;
                    let closed = if c.end() == start {
                        c.closed()
                    } else {
                        c.line_to(start).closed()
                    };
                    paths.push(closed);
                    current = Some(Path::new(start));
                }
            }
        }
    }
    if let Some(done) = current.filter(|p| !p.segs.is_empty()) {
        paths.push(done);
    }
    paths
}

/// The height of the letter H in a glyph table, used when the font gives no cap height.
fn height_of_h(glyphs: &BTreeMap<char, Glyph>) -> Option<f64> {
    let h = glyphs.get(&'H')?;
    let top = h
        .path
        .iter()
        .flat_map(|p| std::iter::once(p.start).chain(p.segs.iter().map(ps_geometry::Seg::to)))
        .map(|p| p.y)
        .fold(f64::NEG_INFINITY, f64::max);
    (top.is_finite() && top > 0.0).then_some(top)
}

impl SvgStrokeFont {
    /// Reads the first `<font>` of an SVG file.
    ///
    /// # Errors
    ///
    /// [`SvgFontError`] when the XML is unreadable or holds no font.
    pub fn parse(source: &str) -> Result<Self, SvgFontError> {
        let doc = Document::parse(source).map_err(|e| SvgFontError::Xml(e.to_string()))?;
        let font = doc
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "font")
            .ok_or(SvgFontError::NoFont)?;
        let default_advance = number(font.attribute("horiz-adv-x")).unwrap_or(1000.0);
        let face = font
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "font-face");
        let name = face
            .and_then(|f| f.attribute("font-family"))
            .unwrap_or("")
            .to_owned();
        let mut missing_advance = default_advance;
        let mut glyphs = BTreeMap::new();
        for node in font.children().filter(roxmltree::Node::is_element) {
            match node.tag_name().name() {
                "missing-glyph" => {
                    missing_advance =
                        number(node.attribute("horiz-adv-x")).unwrap_or(default_advance);
                }
                "glyph" => {
                    let mut chars = node.attribute("unicode").unwrap_or("").chars();
                    let (Some(c), None) = (chars.next(), chars.next()) else {
                        continue;
                    };
                    glyphs.insert(
                        c,
                        Glyph {
                            advance: number(node.attribute("horiz-adv-x"))
                                .unwrap_or(default_advance),
                            path: glyph_paths(node.attribute("d").unwrap_or("")),
                        },
                    );
                }
                _ => {}
            }
        }
        let cap_height = number(face.and_then(|f| f.attribute("cap-height")))
            .or_else(|| height_of_h(&glyphs))
            .filter(|c| *c > 0.0)
            .unwrap_or(default_advance * 0.7);
        Ok(Self {
            name,
            glyphs,
            missing_advance,
            cap_height,
        })
    }

    /// The font family name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the font draws or spaces this character.
    #[must_use]
    pub fn has_glyph(&self, c: char) -> bool {
        self.glyphs.contains_key(&c)
    }

    /// Lays `text` out with capitals `cap_height_mm` tall: open paths with y down and the first
    /// baseline at y = 0. Lines are 1.5 capital heights apart. A character the font lacks leaves
    /// a gap the width of its missing glyph.
    ///
    /// # Errors
    ///
    /// [`SvgFontError::BadSize`] for a size that is not positive and finite.
    pub fn text(&self, text: &str, cap_height_mm: f64) -> Result<Vec<Path>, SvgFontError> {
        if !(cap_height_mm.is_finite() && cap_height_mm > 0.0) {
            return Err(SvgFontError::BadSize);
        }
        let scale = cap_height_mm / self.cap_height;
        let mut out = Vec::new();
        for (row, line) in text.split('\n').enumerate() {
            let drop = f64::from(u32::try_from(row).unwrap_or(u32::MAX)) * 1.5 * cap_height_mm;
            let mut pen = 0.0;
            for c in line.chars() {
                let Some(glyph) = self.glyphs.get(&c) else {
                    pen += self.missing_advance;
                    continue;
                };
                let place = ps_geometry::Affine::translate((0.0, drop))
                    * ps_geometry::Affine::new([scale, 0.0, 0.0, -scale, pen * scale, 0.0]);
                out.extend(glyph.path.iter().map(|p| p.transformed(place)));
                pen += glyph.advance;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
