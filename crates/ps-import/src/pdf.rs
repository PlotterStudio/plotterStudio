//! PDF and AI import: `hayro-interpret` walks a page's content stream and calls a device
//! of ours that records what is drawn as paths.
//!
//! The device keeps each fill and stroke path with its transform (points converted to millimetres,
//! y down from the page's top-left corner), colour, stroke width and dash. Glyphs that come with
//! outlines become filled paths. Images, shadings and text without a usable font are not paths; a
//! diagnostic says how many were skipped. Optional content groups become layers. An Illustrator
//! file that carries a PDF-compatible stream imports through the same path.

use std::sync::Arc;

use hayro_interpret::font::{Glyph, GlyphRun};
use hayro_interpret::hayro_syntax::Pdf;
use hayro_interpret::hayro_syntax::object::Dict;
use hayro_interpret::kurbo::{Affine, BezPath, PathEl, Point as PdfPoint, Rect};
use hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps,
    InterpreterCache, InterpreterSettings, Paint, SoftMask, interpret_page,
};
use ps_core::{Diagnostic, Diagnostics, L10nKey};
use ps_geometry::{Path, Point};
use ps_model::SourceStyle;

use crate::drawing::{ImportedDrawing, ImportedPath, LayerInfo, fingerprint};
use crate::units::UnitGuess;

/// Millimetres in a PDF point.
const MM_PER_POINT: f64 = 25.4 / 72.0;
/// The largest PDF accepted, in bytes.
pub const MAX_PDF_BYTES: usize = 64 * 1024 * 1024;

/// How a PDF is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PdfOptions {
    /// The page to import, counting from 0.
    pub page: usize,
}

/// Why a PDF could not be imported.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PdfError {
    /// The bytes are not a readable PDF.
    #[error("not a readable PDF: {0}")]
    Parse(String),
    /// The file has no PDF inside it (an Illustrator file saved without PDF compatibility).
    #[error(
        "the file has no PDF-compatible stream: save it from Illustrator with \"Create PDF Compatible File\" turned on"
    )]
    NoPdfStream,
    /// The file is over the size limit.
    #[error("the PDF is larger than the {MAX_PDF_BYTES} byte limit")]
    TooLarge,
    /// The file has no such page.
    #[error("the PDF has {pages} pages, so there is no page {page}")]
    NoSuchPage {
        /// The page asked for.
        page: usize,
        /// How many pages the file has.
        pages: usize,
    },
}

/// What the device counted and recorded.
#[derive(Default)]
struct Collector {
    items: Vec<ImportedPath>,
    layer_stack: Vec<Option<String>>,
    layers: Vec<String>,
    element: u64,
    images: usize,
    unoutlined_text: usize,
    patterns: usize,
}

fn rgb(paint: &Paint<'_>) -> Option<([u8; 3], f64)> {
    match paint {
        Paint::Color(colour) => {
            let bytes = colour.to_rgba().to_rgba8();
            Some(([bytes[0], bytes[1], bytes[2]], f64::from(bytes[3]) / 255.0))
        }
        Paint::Pattern(_) => None,
    }
}

/// How much a transform scales lengths, taken as the square root of its area scale.
fn length_scale(transform: &Affine) -> f64 {
    let m = transform.as_coeffs();
    (m[0] * m[3] - m[1] * m[2]).abs().sqrt()
}

/// The subpaths of a path, each as a ps-geometry path, transformed.
fn subpaths(path: &BezPath, transform: Affine) -> Vec<Path> {
    let map = |p: PdfPoint| {
        let q = transform * p;
        Point::new(q.x, q.y)
    };
    let mut out: Vec<Path> = Vec::new();
    let mut current: Option<Path> = None;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                if let Some(done) = current.take().filter(|p| !p.segs.is_empty()) {
                    out.push(done);
                }
                current = Some(Path::new(map(p)));
            }
            PathEl::LineTo(p) => {
                if let Some(c) = current.take() {
                    current = Some(c.line_to(map(p)));
                }
            }
            PathEl::QuadTo(q, p) => {
                if let Some(c) = current.take() {
                    let from = c.end();
                    let (q, p) = (map(q), map(p));
                    let c1 = Point::new(
                        from.x + 2.0 / 3.0 * (q.x - from.x),
                        from.y + 2.0 / 3.0 * (q.y - from.y),
                    );
                    let c2 =
                        Point::new(p.x + 2.0 / 3.0 * (q.x - p.x), p.y + 2.0 / 3.0 * (q.y - p.y));
                    current = Some(c.cubic_to(c1, c2, p));
                }
            }
            PathEl::CurveTo(a, b, p) => {
                if let Some(c) = current.take() {
                    current = Some(c.cubic_to(map(a), map(b), map(p)));
                }
            }
            PathEl::ClosePath => {
                if let Some(c) = current.take() {
                    let start = c.start;
                    let closed = if c.end() == start {
                        c.closed()
                    } else {
                        c.line_to(start).closed()
                    };
                    out.push(closed);
                    // The next segment starts a new subpath at the same point, as in PDF.
                    current = Some(Path::new(start));
                }
            }
        }
    }
    if let Some(done) = current.filter(|p| !p.segs.is_empty()) {
        out.push(done);
    }
    out
}

impl Collector {
    fn layer(&self) -> Option<String> {
        self.layer_stack.iter().rev().find_map(Clone::clone)
    }

    fn record(&mut self, path: &BezPath, props: &DrawProps<'_>, mode: &DrawMode) {
        let (fill, stroke) = match mode {
            DrawMode::Fill(_) => (true, None),
            DrawMode::Stroke(s) => (false, Some(s)),
            DrawMode::FillAndStroke(_, s) => (true, Some(s)),
            DrawMode::Invisible => return,
        };
        let mut style = SourceStyle {
            layer: self.layer(),
            ..SourceStyle::default()
        };
        let Some((colour, alpha)) = rgb(&props.paint) else {
            self.patterns += 1;
            return;
        };
        if fill {
            style.fill = Some(colour);
        }
        if let Some(s) = stroke {
            let k = length_scale(&props.transform);
            style.stroke = Some(colour);
            style.stroke_width_mm = Some(f64::from(s.line_width) * k);
            style.dash = s.dash_array.iter().map(|d| f64::from(*d) * k).collect();
        }
        style.opacity = alpha;
        // A fill and a stroke are painted with their own colours in PDF, but the paint handed to
        // the device for a combined draw is the fill's; a stroke drawn alone has its own.
        self.element += 1;
        let layer = style.layer.clone().unwrap_or_default();
        for (piece, geom) in subpaths(path, props.transform).into_iter().enumerate() {
            let key = fingerprint(
                &layer,
                self.element,
                0,
                u64::try_from(piece).unwrap_or(0),
                &geom,
            );
            self.items.push(ImportedPath {
                key,
                layer: style.layer.clone(),
                geom,
                style: style.clone(),
            });
        }
    }
}

impl<'a> Device<'a> for Collector {
    fn draw_path(&mut self, path: &BezPath, props: DrawProps<'a>, mode: &DrawMode) {
        self.record(path, &props, mode);
    }

    fn push_clip_path(&mut self, _clip: &ClipPath) {}

    fn push_transparency_group(
        &mut self,
        _opacity: f32,
        _mask: Option<SoftMask<'a>>,
        _blend: BlendMode,
    ) {
    }

    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, mode: &DrawMode) {
        let mut any = false;
        for glyph in run.glyphs() {
            if let Glyph::Outline(outline) = &**glyph {
                let path = glyph.transform() * outline.outline();
                self.record(&path, &props, mode);
                any = true;
            }
        }
        if !any && !run.glyphs().is_empty() {
            self.unoutlined_text += 1;
        }
    }

    fn draw_image(&mut self, _image: Image<'a, '_>, _props: ImageDrawProps<'a>) {
        self.images += 1;
    }

    fn pop_clip(&mut self) {}

    fn pop_transparency_group(&mut self) {}

    fn begin_marked_content(&mut self, tag: &[u8], properties: Option<&Dict<'_>>) {
        let name = (tag == b"OC")
            .then(|| {
                properties.and_then(|d| {
                    d.get::<hayro_interpret::hayro_syntax::object::String<'_>>(
                        hayro_interpret::hayro_syntax::object::dict::keys::NAME,
                    )
                })
            })
            .flatten()
            .map(|s| String::from_utf8_lossy(s.as_bytes()).into_owned());
        if let Some(n) = &name
            && !self.layers.contains(n)
        {
            self.layers.push(n.clone());
        }
        self.layer_stack.push(name);
    }

    fn end_marked_content(&mut self) {
        let _ = self.layer_stack.pop();
    }
}

/// Imports one page of a PDF (or of an Illustrator file with a PDF-compatible stream).
///
/// # Errors
///
/// [`PdfError`] when the file is too large, is not a readable PDF, or has no such page.
pub fn import_pdf(bytes: &[u8], options: &PdfOptions) -> Result<ImportedDrawing, PdfError> {
    if bytes.len() > MAX_PDF_BYTES {
        return Err(PdfError::TooLarge);
    }
    let head = &bytes[..bytes.len().min(1024)];
    if !head.windows(5).any(|w| w == b"%PDF-") && !bytes.is_empty() && bytes.starts_with(b"%!") {
        return Err(PdfError::NoPdfStream);
    }
    let pdf = Pdf::new(Arc::new(bytes.to_vec())).map_err(|e| PdfError::Parse(format!("{e:?}")))?;
    let pages = pdf.pages();
    let page = pages.get(options.page).ok_or(PdfError::NoSuchPage {
        page: options.page,
        pages: pages.len(),
    })?;
    let (width, height) = page.render_dimensions();
    let to_mm = Affine::scale(MM_PER_POINT);
    let initial = to_mm * hayro_interpret::TransformExt::to_kurbo(&page.initial_transform(true));
    let cache = InterpreterCache::new();
    let bbox = Rect::new(
        0.0,
        0.0,
        f64::from(width) * MM_PER_POINT,
        f64::from(height) * MM_PER_POINT,
    );
    let mut context = Context::new(
        initial,
        bbox,
        &cache,
        page.xref(),
        InterpreterSettings::default(),
    );
    let mut device = Collector::default();
    interpret_page(page, &mut context, &mut device);

    let mut diagnostics = Diagnostics::default();
    for (count, key) in [
        (device.images, "diag-pdf-images-skipped"),
        (device.unoutlined_text, "diag-pdf-text-skipped"),
        (device.patterns, "diag-pdf-patterns-skipped"),
    ] {
        if count > 0 {
            diagnostics
                .push(Diagnostic::info(L10nKey::new(key)).with_arg("count", count.to_string()));
        }
    }
    let layers = device
        .layers
        .iter()
        .enumerate()
        .map(|(i, name)| LayerInfo {
            name: name.clone(),
            visible: true,
            order: u32::try_from(i).unwrap_or(u32::MAX),
        })
        .collect();
    Ok(ImportedDrawing {
        unit_guess: UnitGuess::Px72,
        size_candidates: Vec::new(),
        layers,
        items: device.items,
        diagnostics,
    })
}

#[cfg(test)]
mod tests;
