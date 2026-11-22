//! SVG and SVGZ import.
//!
//! A pre-pass with `roxmltree` records layer information (Inkscape layers, Illustrator
//! `data-name`, Affinity `serif:id`, top-level group ids) and gives layer groups without an id a
//! synthetic one, so they survive `usvg`'s group flattening. `usvg` then resolves the viewBox,
//! units, nested transforms, CSS, `use`/`symbol` and text; the importer walks its tree and
//! converts every path to millimetres with all transforms applied.
//!
//! Clip paths are applied geometrically: filled regions are intersected with the clip region and
//! strokes are clipped as polylines. Masks and filters are ignored with an info diagnostic; so are
//! gradient and pattern paints. Bitmaps are skipped with a diagnostic.

use std::collections::BTreeMap;
use std::io::Read;

use ps_core::{Diagnostic, Diagnostics, L10nKey, PathKey, PtUm};
use ps_geometry::{BoolOp, FillRule, Path, Point, Ring, boolean, clip_lines, flatten_to_grid};
use ps_model::SourceStyle;
use usvg::tiny_skia_path::{PathSegment, Transform};

use crate::drawing::{ImportedDrawing, ImportedPath, LayerInfo, fingerprint};
use crate::units::{UnitGuess, detect_generator, svg_size_candidates};

const MM_PER_INCH: f64 = 25.4;
/// Tolerance when turning curves into grid polylines for clipping, in mm.
const GRID_TOL: f64 = 0.002;
const INKSCAPE_NS: &str = "http://www.inkscape.org/namespaces/inkscape";
const SERIF_NS: &str = "http://www.serif.com/";

/// Import settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgOptions {
    /// Pixels per inch for unitless files (96, or 72 for print-style generators).
    pub pixels_per_inch: f64,
    /// Whether to load system fonts for text (embedded fonts always work).
    pub load_system_fonts: bool,
    /// Largest accepted size of the (decompressed) file, in bytes.
    pub max_bytes: usize,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            pixels_per_inch: 96.0,
            load_system_fonts: false,
            max_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Why a file could not be imported.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    /// The file is not valid UTF-8.
    #[error("the file is not valid UTF-8")]
    NotUtf8,
    /// The file (or its decompressed form) is larger than the limit.
    #[error("the file is too large")]
    TooLarge,
    /// The file is not a usable SVG.
    #[error("not a usable SVG: {0}")]
    Svg(String),
}

// ---- pre-pass ----

#[derive(Default)]
struct Layers {
    infos: Vec<LayerInfo>,
    by_id: BTreeMap<String, usize>,
    insertions: Vec<(usize, String)>,
}

fn attr<'a>(node: roxmltree::Node<'a, '_>, ns: &str, name: &str) -> Option<&'a str> {
    node.attribute((ns, name))
}

fn hidden(node: roxmltree::Node<'_, '_>) -> bool {
    let style = node.attribute("style").unwrap_or("").replace(' ', "");
    style.contains("display:none") || node.attribute("display") == Some("none")
}

fn is_top_level(node: roxmltree::Node<'_, '_>) -> bool {
    node.parent().is_some_and(|p| p.parent_element().is_none())
}

fn layer_name(node: roxmltree::Node<'_, '_>) -> Option<String> {
    attr(node, INKSCAPE_NS, "label")
        .or_else(|| node.attribute("data-name"))
        .or_else(|| attr(node, SERIF_NS, "id"))
        .or_else(|| node.attribute("id"))
        .map(str::to_owned)
}

fn scan_layers(doc: &roxmltree::Document<'_>) -> Layers {
    let groups: Vec<roxmltree::Node<'_, '_>> = doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "g")
        .collect();
    let explicit = |n: &roxmltree::Node<'_, '_>| {
        attr(*n, INKSCAPE_NS, "groupmode") == Some("layer")
            || (is_top_level(*n)
                && (n.attribute("data-name").is_some() || attr(*n, SERIF_NS, "id").is_some()))
    };
    let mut chosen: Vec<roxmltree::Node<'_, '_>> =
        groups.iter().copied().filter(explicit).collect();
    if chosen.is_empty() {
        chosen = groups
            .iter()
            .copied()
            .filter(|n| is_top_level(*n) && n.attribute("id").is_some())
            .collect();
    }

    let text = doc.input_text();
    let mut out = Layers::default();
    let mut synthetic = 0_usize;
    for (order, node) in chosen.iter().enumerate() {
        let Some(name) = layer_name(*node) else {
            synthetic += 1;
            continue;
        };
        let id = if let Some(id) = node.attribute("id") {
            id.to_owned()
        } else {
            let id = format!("__ps_layer_{synthetic}");
            synthetic += 1;
            let start = node.range().start;
            let at = text[start..]
                .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .map_or(start + 2, |i| start + i);
            out.insertions.push((at, format!(" id=\"{id}\"")));
            id
        };
        out.by_id.insert(id, out.infos.len());
        out.infos.push(LayerInfo {
            name,
            visible: !hidden(*node),
            order: u32::try_from(order).unwrap_or(u32::MAX),
        });
    }
    out
}

fn patch_ids(text: &str, insertions: &[(usize, String)]) -> String {
    let mut patched = text.to_owned();
    let mut sorted: Vec<&(usize, String)> = insertions.iter().collect();
    sorted.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    for (at, snippet) in sorted {
        patched.insert_str(*at, snippet);
    }
    patched
}

/// True when a root `width`/`height` carries a physical unit.
fn has_physical_unit(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        let v = v.trim();
        ["mm", "cm", "in", "pt", "pc", "q"]
            .iter()
            .any(|u| v.ends_with(u))
    })
}

// ---- tree walk ----

struct Ctx<'a> {
    layers: &'a Layers,
    layer_stack: Vec<usize>,
    clips: Vec<Vec<Ring>>,
    items: Vec<ImportedPath>,
    diags: Diagnostics,
    mask_noted: bool,
    filter_noted: bool,
    paint_noted: bool,
    element: u64,
    mm_per_px: f64,
}

fn apply(t: &Transform, x: f32, y: f32) -> (f64, f64) {
    let (x, y) = (f64::from(x), f64::from(y));
    (
        f64::from(t.sx) * x + f64::from(t.kx) * y + f64::from(t.tx),
        f64::from(t.ky) * x + f64::from(t.sy) * y + f64::from(t.ty),
    )
}

fn scale_of(t: &Transform) -> f64 {
    let det = f64::from(t.sx) * f64::from(t.sy) - f64::from(t.kx) * f64::from(t.ky);
    libm::sqrt(det.abs())
}

struct Sub {
    path: Path,
}

/// Splits usvg path data into subpaths in millimetres, transforms applied.
fn subpaths(data: &usvg::tiny_skia_path::Path, transform: &Transform, mm_per_px: f64) -> Vec<Sub> {
    let to_mm = |point: usvg::tiny_skia_path::Point| {
        let (x, y) = apply(transform, point.x, point.y);
        Point::new(x * mm_per_px, y * mm_per_px)
    };
    let flush = |active: &mut Option<Path>, subs: &mut Vec<Sub>| {
        if let Some(path) = active.take()
            && !path.segs.is_empty()
        {
            subs.push(Sub { path });
        }
    };
    let mut subs: Vec<Sub> = Vec::new();
    let mut active: Option<Path> = None;
    let mut last = Point::ORIGIN;
    for segment in data.segments() {
        match segment {
            PathSegment::MoveTo(to) => {
                flush(&mut active, &mut subs);
                last = to_mm(to);
                active = Some(Path::new(last));
            }
            PathSegment::LineTo(to) => {
                if let Some(path) = active.take() {
                    last = to_mm(to);
                    active = Some(path.line_to(last));
                }
            }
            PathSegment::QuadTo(control, to) => {
                if let Some(path) = active.take() {
                    // A quadratic is raised to a cubic exactly: both controls sit 2/3 of the way
                    // from an end point to the quadratic's control point.
                    let (control, to) = (to_mm(control), to_mm(to));
                    let third = 2.0 / 3.0;
                    let first = Point::new(
                        last.x + third * (control.x - last.x),
                        last.y + third * (control.y - last.y),
                    );
                    let second = Point::new(
                        to.x + third * (control.x - to.x),
                        to.y + third * (control.y - to.y),
                    );
                    last = to;
                    active = Some(path.cubic_to(first, second, to));
                }
            }
            PathSegment::CubicTo(first, second, to) => {
                if let Some(path) = active.take() {
                    let to = to_mm(to);
                    last = to;
                    active = Some(path.cubic_to(to_mm(first), to_mm(second), to));
                }
            }
            PathSegment::Close => {
                if let Some(path) = active.take() {
                    let closed = path.closed();
                    last = closed.start;
                    flush(&mut Some(closed), &mut subs);
                }
            }
        }
    }
    flush(&mut active, &mut subs);
    subs
}

fn to_ring(path: &Path) -> Option<Ring> {
    flatten_to_grid(path, GRID_TOL)
        .ok()
        .filter(|r| r.len() >= 3)
}

fn grid_to_path(points: &[PtUm], closed: bool) -> Option<Path> {
    if points.len() < 2 {
        return None;
    }
    let pts: Vec<Point> = points
        .iter()
        .map(|p| {
            let (x, y) = p.to_mm();
            Point::new(x, y)
        })
        .collect();
    Some(Path::polyline(&pts, closed))
}

/// The region a clip path keeps, in grid coordinates.
///
/// `usvg` gives the clip's children transforms in the clip's own coordinate system, so they are
/// placed with the referencing group's absolute transform, then the clip's own transform.
fn region_of(clip: &usvg::ClipPath, group_abs: Transform, mm_per_px: f64) -> Vec<Ring> {
    fn collect(group: &usvg::Group, base: Transform, mm_per_px: f64, rings: &mut Vec<Ring>) {
        for node in group.children() {
            match node {
                usvg::Node::Group(g) => collect(g, base, mm_per_px, rings),
                usvg::Node::Path(p) => {
                    let placed = base.pre_concat(p.abs_transform());
                    for sub in subpaths(p.data(), &placed, mm_per_px) {
                        if let Some(ring) = to_ring(&sub.path.closed()) {
                            rings.push(ring);
                        }
                    }
                }
                usvg::Node::Image(_) | usvg::Node::Text(_) => {}
            }
        }
    }
    let base = group_abs.pre_concat(clip.transform());
    let mut rings = Vec::new();
    collect(clip.root(), base, mm_per_px, &mut rings);
    let mut region: Vec<Ring> = boolean(&rings, &[], BoolOp::Union, FillRule::NonZero)
        .into_iter()
        .flatten()
        .collect();
    if let Some(parent) = clip.clip_path() {
        let outer = region_of(parent, group_abs, mm_per_px);
        region = boolean(&region, &outer, BoolOp::Intersect, FillRule::NonZero)
            .into_iter()
            .flatten()
            .collect();
    }
    region
}

impl Ctx<'_> {
    fn layer_name(&self) -> Option<String> {
        self.layer_stack
            .last()
            .map(|&i| self.layers.infos[i].name.clone())
    }

    fn style(
        &mut self,
        p: &usvg::Path,
        scale: f64,
        with_fill: bool,
        with_stroke: bool,
    ) -> SourceStyle {
        let mut style = SourceStyle {
            layer: self.layer_name(),
            ..SourceStyle::default()
        };
        let colour = |paint: &usvg::Paint, diags: &mut Diagnostics, noted: &mut bool| {
            if let usvg::Paint::Color(rgb) = paint {
                Some([rgb.red, rgb.green, rgb.blue])
            } else {
                if !*noted {
                    *noted = true;
                    diags.push(Diagnostic::info(L10nKey::new("diag-import-paint-server")));
                }
                None
            }
        };
        if with_fill && let Some(fill) = p.fill() {
            style.fill = colour(fill.paint(), &mut self.diags, &mut self.paint_noted);
            style.opacity = f64::from(fill.opacity().get());
        }
        if with_stroke && let Some(stroke) = p.stroke() {
            style.stroke = colour(stroke.paint(), &mut self.diags, &mut self.paint_noted);
            let k = scale * self.mm_per_px;
            style.stroke_width_mm = Some(f64::from(stroke.width().get()) * k);
            if let Some(dash) = stroke.dasharray() {
                style.dash = dash.iter().map(|d| f64::from(*d) * k).collect();
            }
            if style.fill.is_none() {
                style.opacity = f64::from(stroke.opacity().get());
            }
        }
        style
    }

    fn push(&mut self, key: PathKey, geom: Path, style: SourceStyle) {
        self.items.push(ImportedPath {
            key,
            layer: self.layer_name(),
            geom,
            style,
        });
    }

    fn add_path(&mut self, p: &usvg::Path) {
        if !p.is_visible() {
            return;
        }
        self.element += 1;
        let element = self.element;
        let transform = p.abs_transform();
        let scale = scale_of(&transform);
        let subs = subpaths(p.data(), &transform, self.mm_per_px);
        let many = subs.len() > 1;
        let layer_path = self.layer_name().unwrap_or_default();
        let rule = match p.fill().map(usvg::Fill::rule) {
            Some(usvg::FillRule::EvenOdd) => FillRule::EvenOdd,
            _ => FillRule::NonZero,
        };

        for (n, sub) in subs.into_iter().enumerate() {
            let n = n as u64;
            let key_for = |piece: u64, geom: &Path| -> PathKey {
                if p.id().is_empty() {
                    fingerprint(&layer_path, element, n, piece, geom)
                } else if many || piece > 0 {
                    PathKey::source(format!("{}#{n}.{piece}", p.id()))
                } else {
                    PathKey::source(p.id())
                }
            };
            if self.clips.is_empty() {
                let style = self.style(p, scale, true, true);
                let key = if p.id().is_empty() {
                    fingerprint(&layer_path, element, n, 0, &sub.path)
                } else if many {
                    PathKey::source(format!("{}#{n}", p.id()))
                } else {
                    PathKey::source(p.id())
                };
                self.push(key, sub.path, style);
                continue;
            }
            // Clipped: fills are intersected with the clip region, strokes clipped as polylines.
            let mut piece = 0_u64;
            if p.fill().is_some()
                && let Some(ring) = to_ring(&sub.path.clone().closed())
            {
                let mut rings = vec![ring];
                let mut current_rule = rule;
                for region in self.clips.clone() {
                    rings = boolean(&rings, &region, BoolOp::Intersect, current_rule)
                        .into_iter()
                        .flatten()
                        .collect();
                    current_rule = FillRule::NonZero;
                }
                let style = self.style(p, scale, true, false);
                for ring in rings {
                    if let Some(geom) = grid_to_path(&ring, true) {
                        let key = key_for(piece, &geom);
                        piece += 1;
                        self.push(key, geom, style.clone());
                    }
                }
            }
            if p.stroke().is_some()
                && let Ok(mut line) = flatten_to_grid(&sub.path, GRID_TOL)
            {
                if sub.path.closed
                    && let Some(&first) = line.first()
                {
                    line.push(first);
                }
                let mut lines = vec![line];
                for region in self.clips.clone() {
                    lines = clip_lines(&lines, &region, FillRule::NonZero);
                }
                let style = self.style(p, scale, false, true);
                for line in lines {
                    if let Some(geom) = grid_to_path(&line, false) {
                        let key = key_for(piece, &geom);
                        piece += 1;
                        self.push(key, geom, style.clone());
                    }
                }
            }
        }
    }

    fn walk(&mut self, group: &usvg::Group) {
        for node in group.children() {
            match node {
                usvg::Node::Group(g) => self.enter(g),
                usvg::Node::Path(p) => self.add_path(p),
                usvg::Node::Text(t) => self.walk(t.flattened()),
                usvg::Node::Image(_) => {}
            }
        }
    }

    fn enter(&mut self, g: &usvg::Group) {
        if g.mask().is_some() && !self.mask_noted {
            self.mask_noted = true;
            self.diags
                .push(Diagnostic::info(L10nKey::new("diag-import-mask-ignored")));
        }
        if !g.filters().is_empty() && !self.filter_noted {
            self.filter_noted = true;
            self.diags
                .push(Diagnostic::info(L10nKey::new("diag-import-filter-ignored")));
        }
        let layer = self.layers.by_id.get(g.id()).copied();
        if let Some(i) = layer {
            self.layer_stack.push(i);
        }
        let clip = g
            .clip_path()
            .map(|c| region_of(c, g.abs_transform(), self.mm_per_px));
        let clipped = clip.is_some();
        if let Some(region) = clip {
            self.clips.push(region);
        }
        self.walk(g);
        if clipped {
            self.clips.pop();
        }
        if layer.is_some() {
            self.layer_stack.pop();
        }
    }
}

/// Imports an SVG file.
///
/// # Errors
///
/// [`ImportError`] when the file is not UTF-8, is too large, or is not a usable SVG.
pub fn import_svg(bytes: &[u8], options: &SvgOptions) -> Result<ImportedDrawing, ImportError> {
    if bytes.len() > options.max_bytes {
        return Err(ImportError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ImportError::NotUtf8)?;
    let doc = roxmltree::Document::parse(text).map_err(|e| ImportError::Svg(e.to_string()))?;
    let layers = scan_layers(&doc);
    let root = doc.root_element();
    let unitless =
        !has_physical_unit(root.attribute("width")) && !has_physical_unit(root.attribute("height"));
    let generator = detect_generator(text);
    let patched = patch_ids(text, &layers.insertions);

    let ppi = if options.pixels_per_inch > 0.0 {
        options.pixels_per_inch
    } else {
        96.0
    };
    let mut opt = usvg::Options::default();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a DPI is a small positive number"
    )]
    {
        opt.dpi = ppi as f32;
    }
    if options.load_system_fonts {
        opt.fontdb_mut().load_system_fonts();
    }
    let tree = usvg::Tree::from_str(&patched, &opt).map_err(|e| ImportError::Svg(e.to_string()))?;

    let mm_per_px = MM_PER_INCH / ppi;
    let mut diags = Diagnostics::new();
    for node in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "image")
    {
        let _ = node;
        diags.push(Diagnostic::info(L10nKey::new("diag-import-image-skipped")));
    }
    let mut ctx = Ctx {
        layers: &layers,
        layer_stack: Vec::new(),
        clips: Vec::new(),
        items: Vec::new(),
        diags,
        mask_noted: false,
        filter_noted: false,
        paint_noted: false,
        element: 0,
        mm_per_px,
    };
    ctx.walk(tree.root());

    let size = tree.size();
    let (w_px, h_px) = (f64::from(size.width()), f64::from(size.height()));
    let (unit_guess, size_candidates) = if unitless {
        let guess = if (ppi - 72.0).abs() < 1e-9 {
            UnitGuess::Px72
        } else {
            UnitGuess::Px96
        };
        (guess, svg_size_candidates(w_px, h_px, true, generator))
    } else {
        (
            UnitGuess::Mm,
            svg_size_candidates(w_px * mm_per_px, h_px * mm_per_px, false, generator),
        )
    };
    Ok(ImportedDrawing {
        unit_guess,
        size_candidates,
        layers: layers.infos.clone(),
        items: ctx.items,
        diagnostics: ctx.diags,
    })
}

/// Imports an SVG or SVGZ file, detecting gzip by its magic bytes.
///
/// # Errors
///
/// As for [`import_svg`], plus [`ImportError::TooLarge`] when the decompressed file exceeds the
/// limit (a decompression bomb) and [`ImportError::Svg`] for a malformed archive.
pub fn import_svg_auto(bytes: &[u8], options: &SvgOptions) -> Result<ImportedDrawing, ImportError> {
    if bytes.starts_with(&[0x1F, 0x8B]) {
        let limit = u64::try_from(options.max_bytes)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .take(limit)
            .read_to_end(&mut out)
            .map_err(|e| ImportError::Svg(format!("malformed gzip: {e}")))?;
        if out.len() > options.max_bytes {
            return Err(ImportError::TooLarge);
        }
        import_svg(&out, options)
    } else {
        import_svg(bytes, options)
    }
}

#[cfg(test)]
mod tests;
