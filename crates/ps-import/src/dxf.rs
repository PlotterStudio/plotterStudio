//! DXF import, using the `dxf` crate.
//!
//! Entities map as follows: LINE and ARC directly; CIRCLE as two arcs; LWPOLYLINE and POLYLINE
//! with bulges as lines and arcs (bulge is the tangent of a quarter of the sweep); ELLIPSE as
//! cubic Béziers; SPLINE as cubic Béziers by knot insertion (exact for non-rational cubics and
//! quadratics; everything else is sampled and fitted within 0.002 mm); INSERT recursively with
//! block transforms (including arrays) and a depth limit of 32. Units come from `$INSUNITS`;
//! unitless files default to millimetres and the dialog asks. DXF is y-up, so y is flipped to
//! keep drawings upright; arcs keep their geometry and swap direction.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use dxf::Drawing;
use dxf::entities::{Entity, EntityType};
use ps_core::{Diagnostic, Diagnostics, L10nKey, PathKey};
use ps_geometry::arc::center_from_bulge;
use ps_geometry::{Affine, Path, Point, refit_polyline};
use ps_model::SourceStyle;

use crate::drawing::{ImportedDrawing, ImportedPath, LayerInfo, fingerprint};
use crate::units::{SizeCandidate, UnitGuess, dxf_is_unitless, mm_per_unit};

/// Most nested INSERTs followed before giving up.
const MAX_INSERT_DEPTH: usize = 32;
/// Tolerance when fitting rational splines, in mm.
const FIT_TOL_MM: f64 = 0.002;
/// A bulge below this is a straight segment.
const MIN_BULGE: f64 = 1e-9;

/// Why a DXF file could not be imported.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DxfError {
    /// The file is not a readable DXF.
    #[error("not a readable DXF file: {0}")]
    Parse(String),
}

// ---- colours ----

/// The RGB of an `AutoCAD` Color Index. Index 7 (white on a dark screen) reads as black ink on
/// paper.
#[must_use]
pub fn aci_rgb(index: u8) -> [u8; 3] {
    match index {
        1 => [255, 0, 0],
        2 => [255, 255, 0],
        3 => [0, 255, 0],
        4 => [0, 255, 255],
        5 => [0, 0, 255],
        6 => [255, 0, 255],
        8 => [128, 128, 128],
        9 => [192, 192, 192],
        10..=249 => {
            let n = u32::from(index) - 10;
            let (hue_step, shade) = (n / 10, n % 10);
            let hue = f64::from(hue_step) * 15.0;
            let value = [1.0, 0.8, 0.6, 0.5, 0.3][usize::try_from(shade / 2).unwrap_or(0)];
            let saturation = if shade % 2 == 0 { 1.0 } else { 0.5 };
            hsv(hue, saturation, value)
        }
        250 => [51, 51, 51],
        251 => [91, 91, 91],
        252 => [132, 132, 132],
        253 => [173, 173, 173],
        254 => [214, 214, 214],
        255 => [255, 255, 255],
        _ => [0, 0, 0],
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "channels are floored into 0..=255"
)]
#[expect(
    clippy::many_single_char_names,
    reason = "standard HSV to RGB notation"
)]
fn hsv(hue: f64, saturation: f64, value: f64) -> [u8; 3] {
    let sector = hue / 60.0;
    let c = value * saturation;
    let x = c * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = value - c;
    let to = |v: f64| libm::floor((v + m) * 255.0 + 1e-9) as u8;
    [to(r), to(g), to(b)]
}

fn true_colour(v: i32) -> [u8; 3] {
    [
        u8::try_from((v >> 16) & 0xFF).unwrap_or(0),
        u8::try_from((v >> 8) & 0xFF).unwrap_or(0),
        u8::try_from(v & 0xFF).unwrap_or(0),
    ]
}

// ---- spline helpers ----

#[expect(
    clippy::many_single_char_names,
    reason = "standard B-spline notation (knots t, control points p, span k)"
)]
/// Splits a clamped B-spline into Bézier pieces by knot insertion. `None` when the knot vector is
/// not clamped or does not match the control points.
fn bezier_pieces(degree: usize, knots: &[f64], control: &[Point]) -> Option<Vec<Vec<Point>>> {
    let n = control.len();
    if degree == 0 || n < degree + 1 || knots.len() != n + degree + 1 {
        return None;
    }
    let clamped = knots[..=degree]
        .iter()
        .all(|k| (k - knots[0]).abs() < 1e-12)
        && knots[knots.len() - degree - 1..]
            .iter()
            .all(|k| (k - knots[knots.len() - 1]).abs() < 1e-12);
    if !clamped {
        return None;
    }
    let mut t = knots.to_vec();
    let mut p = control.to_vec();
    // Insert every interior knot until it has multiplicity `degree`.
    let mut i = degree + 1;
    while i < t.len() - degree - 1 {
        let u = t[i];
        let multiplicity = t.iter().filter(|k| (**k - u).abs() < 1e-12).count();
        if multiplicity >= degree {
            while i < t.len() - degree - 1 && (t[i] - u).abs() < 1e-12 {
                i += 1;
            }
            continue;
        }
        // Span k with t[k] <= u < t[k + 1].
        let k = (0..t.len() - 1)
            .rev()
            .find(|&k| t[k] <= u && u < t[k + 1])?;
        let mut q = Vec::with_capacity(p.len() + 1);
        for j in 0..=p.len() {
            if j + degree <= k {
                q.push(p[j]);
            } else if j > k {
                q.push(p[j - 1]);
            } else {
                let a = (u - t[j]) / (t[j + degree] - t[j]);
                q.push(Point::new(
                    a * p[j].x + (1.0 - a) * p[j - 1].x,
                    a * p[j].y + (1.0 - a) * p[j - 1].y,
                ));
            }
        }
        p = q;
        t.insert(k + 1, u);
    }
    let mut pieces = Vec::new();
    let mut at = 0;
    while at + degree < p.len() {
        pieces.push(p[at..=at + degree].to_vec());
        at += degree;
    }
    Some(pieces)
}

#[expect(clippy::many_single_char_names, reason = "standard de Boor notation")]
/// Evaluates a (possibly rational) B-spline at `u` by de Boor's algorithm in homogeneous
/// coordinates.
fn de_boor(degree: usize, knots: &[f64], control: &[Point], weights: &[f64], u: f64) -> Point {
    let n = control.len();
    let k = (degree..n)
        .rev()
        .find(|&k| knots[k] <= u)
        .unwrap_or(degree)
        .min(n - 1);
    let mut d: Vec<[f64; 3]> = (0..=degree)
        .map(|j| {
            let i = j + k - degree;
            let w = weights.get(i).copied().unwrap_or(1.0);
            [control[i].x * w, control[i].y * w, w]
        })
        .collect();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = j + k - degree;
            let denom = knots[i + degree + 1 - r] - knots[i];
            let a = if denom == 0.0 {
                0.0
            } else {
                (u - knots[i]) / denom
            };
            for c in 0..3 {
                d[j][c] = (1.0 - a) * d[j - 1][c] + a * d[j][c];
            }
        }
    }
    let [x, y, w] = d[degree];
    if w == 0.0 {
        Point::new(x, y)
    } else {
        Point::new(x / w, y / w)
    }
}

fn spline_path(s: &dxf::entities::Spline, to_mm: f64) -> Option<Path> {
    let control: Vec<Point> = s
        .control_points
        .iter()
        .map(|p| Point::new(p.x, p.y))
        .collect();
    let weights: Vec<f64> = s.weight_values.clone();
    let degree = usize::try_from(s.degree_of_curve)
        .ok()
        .filter(|d| *d >= 1)?;
    let rational = weights.iter().any(|w| (w - 1.0).abs() > 1e-12);
    let tol = FIT_TOL_MM / to_mm.max(f64::MIN_POSITIVE);

    if control.is_empty() {
        let fit: Vec<Point> = s.fit_points.iter().map(|p| Point::new(p.x, p.y)).collect();
        return (fit.len() >= 2).then(|| refit_polyline(&fit, false, tol));
    }
    if !rational
        && degree <= 3
        && let Some(pieces) = bezier_pieces(degree, &s.knot_values, &control)
    {
        let start = *pieces.first()?.first()?;
        let mut path = Path::new(start);
        for piece in &pieces {
            let (a, b) = (piece[0], piece[piece.len() - 1]);
            path = match piece.len() {
                2 => path.line_to(b),
                3 => {
                    let c1 = Point::new(
                        a.x + 2.0 / 3.0 * (piece[1].x - a.x),
                        a.y + 2.0 / 3.0 * (piece[1].y - a.y),
                    );
                    let c2 = Point::new(
                        b.x + 2.0 / 3.0 * (piece[1].x - b.x),
                        b.y + 2.0 / 3.0 * (piece[1].y - b.y),
                    );
                    path.cubic_to(c1, c2, b)
                }
                _ => path.cubic_to(piece[1], piece[2], b),
            };
        }
        return Some(path);
    }

    // Rational or higher degree: sample the curve densely and fit.
    let knots = if s.knot_values.len() == control.len() + degree + 1 {
        s.knot_values.clone()
    } else {
        let spans = control.len().saturating_sub(degree).max(1);
        #[expect(clippy::cast_precision_loss, reason = "span counts are small")]
        let spans_f = spans as f64;
        let mut k = vec![0.0; degree + 1];
        for i in 1..spans {
            #[expect(clippy::cast_precision_loss, reason = "span counts are small")]
            k.push(i as f64 / spans_f);
        }
        k.extend(vec![1.0; degree + 1]);
        k
    };
    let (u0, u1) = (knots[degree], knots[control.len()]);
    let samples = (control.len() * 48).clamp(96, 4_096);
    let points: Vec<Point> = (0..=samples)
        .map(|i| {
            #[expect(clippy::cast_precision_loss, reason = "sample counts are small")]
            let t = i as f64 / samples as f64;
            let u = (u0 + (u1 - u0) * t).min(u1 - 1e-12 * (u1 - u0).abs());
            let u = if i == samples { u1 - 1e-12 } else { u };
            de_boor(degree, &knots, &control, &weights, u)
        })
        .collect();
    Some(refit_polyline(&points, false, tol))
}

// ---- entity geometry (in drawing units, y up) ----

fn bulged_path(points: &[(Point, f64)], closed: bool) -> Option<Path> {
    let first = points.first()?.0;
    let mut path = Path::new(first);
    let count = if closed {
        points.len()
    } else {
        points.len().saturating_sub(1)
    };
    for i in 0..count {
        let (p0, bulge) = points[i];
        let p1 = points[(i + 1) % points.len()].0;
        if closed && i + 1 == points.len() && p1 == p0 {
            continue;
        }
        let centre = if bulge.abs() > MIN_BULGE {
            center_from_bulge(p0, p1, bulge)
        } else {
            None
        };
        path = match centre {
            Some(center) => path.arc_to(p1, center, bulge > 0.0),
            None => path.line_to(p1),
        };
    }
    if path.segs.is_empty() {
        return None;
    }
    if closed {
        path = path.closed();
    }
    Some(path)
}

fn ellipse_path(ellipse: &dxf::entities::Ellipse) -> Option<Path> {
    let centre = Point::new(ellipse.center.x, ellipse.center.y);
    let major = (ellipse.major_axis.x, ellipse.major_axis.y);
    let semi_major = libm::hypot(major.0, major.1);
    if semi_major == 0.0 {
        return None;
    }
    let semi_minor = semi_major * ellipse.minor_axis_ratio;
    // Unit vectors along the major and minor axes.
    let along = (major.0 / semi_major, major.1 / semi_major);
    let across = (-along.1, along.0);
    // The point at parameter `param` and the derivative there.
    let at = |param: f64| {
        let (cos, sin) = (libm::cos(param), libm::sin(param));
        (
            Point::new(
                centre.x + semi_major * cos * along.0 + semi_minor * sin * across.0,
                centre.y + semi_major * cos * along.1 + semi_minor * sin * across.1,
            ),
            (
                -semi_major * sin * along.0 + semi_minor * cos * across.0,
                -semi_major * sin * along.1 + semi_minor * cos * across.1,
            ),
        )
    };
    let tau = std::f64::consts::TAU;
    let (from, mut to) = (ellipse.start_parameter, ellipse.end_parameter);
    while to <= from {
        to += tau;
    }
    let sweep = (to - from).min(tau);
    let full = (sweep - tau).abs() < 1e-9;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a small positive piece count"
    )]
    let pieces = ((sweep / std::f64::consts::FRAC_PI_2).ceil() as usize).max(1);
    #[expect(clippy::cast_precision_loss, reason = "piece counts are tiny")]
    let step = sweep / pieces as f64;
    let handle = 4.0 / 3.0 * libm::tan(step / 4.0);
    let start = at(from).0;
    let mut path = Path::new(start);
    for piece in 0..pieces {
        #[expect(clippy::cast_precision_loss, reason = "piece counts are tiny")]
        let (first, second) = (from + step * piece as f64, from + step * (piece + 1) as f64);
        let ((begin, begin_slope), (mut end, end_slope)) = (at(first), at(second));
        if full && piece + 1 == pieces {
            // The last piece must end exactly where the first began, or closing adds a sliver.
            end = start;
        }
        path = path.cubic_to(
            Point::new(
                begin.x + handle * begin_slope.0,
                begin.y + handle * begin_slope.1,
            ),
            Point::new(end.x - handle * end_slope.0, end.y - handle * end_slope.1),
            end,
        );
    }
    Some(if full { path.closed() } else { path })
}

fn arc_path(a: &dxf::entities::Arc) -> Option<Path> {
    let c = Point::new(a.center.x, a.center.y);
    let at = |deg: f64| {
        let r = deg.to_radians();
        Point::new(c.x + a.radius * libm::cos(r), c.y + a.radius * libm::sin(r))
    };
    let (start, end) = (at(a.start_angle), at(a.end_angle));
    if a.radius <= 0.0 || start == end {
        return None;
    }
    Some(Path::new(start).arc_to(end, c, true))
}

// ---- walking ----

struct Ctx<'a> {
    /// Handles written in the file (the crate invents handles for entities that lack one).
    explicit_handles: BTreeSet<u64>,
    blocks: BTreeMap<&'a str, &'a dxf::Block>,
    layer_colours: BTreeMap<String, Option<[u8; 3]>>,
    hidden_layers: Vec<String>,
    to_mm: f64,
    items: Vec<ImportedPath>,
    unsupported: BTreeMap<String, usize>,
    missing_blocks: Vec<String>,
    depth_noted: bool,
    element: u64,
}

fn mirrored(normal_z: f64) -> bool {
    normal_z < 0.0
}

impl Ctx<'_> {
    /// The ink colour of an entity, or `None` when it is turned off.
    fn colour_of(&self, e: &Entity, inherited: Option<[u8; 3]>) -> Option<[u8; 3]> {
        let common = &e.common;
        if common.color.is_turned_off() {
            return None;
        }
        if common.color_24_bit != 0 {
            return Some(true_colour(common.color_24_bit));
        }
        if let Some(index) = common.color.index() {
            return Some(aci_rgb(index));
        }
        if common.color.is_by_block()
            && let Some(parent) = inherited
        {
            return Some(parent);
        }
        Some(
            self.layer_colours
                .get(&common.layer)
                .copied()
                .flatten()
                .unwrap_or([0, 0, 0]),
        )
    }

    fn geometry(&mut self, e: &Entity) -> Option<Path> {
        let flip = |p: Path, z: f64| {
            if mirrored(z) {
                p.transformed(Affine::new([-1.0, 0.0, 0.0, 1.0, 0.0, 0.0]))
            } else {
                p
            }
        };
        match &e.specific {
            EntityType::Line(l) => {
                let (a, b) = (Point::new(l.p1.x, l.p1.y), Point::new(l.p2.x, l.p2.y));
                (a != b).then(|| Path::polyline(&[a, b], false))
            }
            EntityType::Arc(a) => arc_path(a).map(|p| flip(p, a.normal.z)),
            EntityType::Circle(c) => (c.radius > 0.0)
                .then(|| Path::circle(Point::new(c.center.x, c.center.y), c.radius))
                .map(|p| flip(p, c.normal.z)),
            EntityType::LwPolyline(l) => {
                let pts: Vec<(Point, f64)> = l
                    .vertices
                    .iter()
                    .map(|v| (Point::new(v.x, v.y), v.bulge))
                    .collect();
                bulged_path(&pts, l.flags & 1 != 0).map(|p| flip(p, l.extrusion_direction.z))
            }
            EntityType::Polyline(pl) => {
                if pl.flags & (8 | 16 | 64) != 0 {
                    *self
                        .unsupported
                        .entry("POLYLINE (3D or mesh)".to_owned())
                        .or_default() += 1;
                    return None;
                }
                let pts: Vec<(Point, f64)> = pl
                    .vertices()
                    .map(|v| (Point::new(v.location.x, v.location.y), v.bulge))
                    .collect();
                bulged_path(&pts, pl.flags & 1 != 0).map(|p| flip(p, pl.normal.z))
            }
            EntityType::Ellipse(el) => ellipse_path(el).map(|p| flip(p, el.normal.z)),
            EntityType::Spline(s) => spline_path(s, self.to_mm).map(|p| flip(p, s.normal.z)),
            other => {
                let name = format!("{other:?}");
                let name = name.split(['(', ' ', '{']).next().unwrap_or("?").to_owned();
                *self.unsupported.entry(name).or_default() += 1;
                None
            }
        }
    }

    fn visible(&self, e: &Entity) -> bool {
        e.common.is_visible && !self.hidden_layers.contains(&e.common.layer)
    }

    fn walk(
        &mut self,
        entities: &[Entity],
        at: Affine,
        depth: usize,
        chain: &str,
        inherited: Option<[u8; 3]>,
    ) {
        for e in entities {
            if !self.visible(e) {
                continue;
            }
            if let EntityType::Insert(ins) = &e.specific {
                self.insert(e, ins, at, depth, chain, inherited);
                continue;
            }
            let Some(colour) = self.colour_of(e, inherited) else {
                continue;
            };
            let Some(path) = self.geometry(e) else {
                continue;
            };
            self.element += 1;
            let placed = path.transformed(at);
            let layer = e.common.layer.clone();
            let key = if self.explicit_handles.contains(&e.common.handle.0) {
                let id = format!("{:X}", e.common.handle.0);
                PathKey::source(if chain.is_empty() {
                    id
                } else {
                    format!("{chain}/{id}")
                })
            } else {
                fingerprint(&format!("{chain}/{layer}"), self.element, 0, 0, &placed)
            };
            self.items.push(ImportedPath {
                key,
                layer: Some(layer.clone()),
                geom: placed,
                style: SourceStyle {
                    stroke: Some(colour),
                    layer: Some(layer),
                    ..SourceStyle::default()
                },
            });
        }
    }

    fn insert(
        &mut self,
        e: &Entity,
        ins: &dxf::entities::Insert,
        at: Affine,
        depth: usize,
        chain: &str,
        inherited: Option<[u8; 3]>,
    ) {
        let Some(block) = self.blocks.get(ins.name.as_str()).copied() else {
            if !self.missing_blocks.contains(&ins.name) {
                self.missing_blocks.push(ins.name.clone());
            }
            return;
        };
        if depth >= MAX_INSERT_DEPTH {
            self.depth_noted = true;
            return;
        }
        let colour = self.colour_of(e, inherited).or(inherited);
        let (cols, rows) = (
            i32::from(ins.column_count.max(1)),
            i32::from(ins.row_count.max(1)),
        );
        let angle = ins.rotation.to_radians();
        let rotation = Affine::new([
            libm::cos(angle),
            libm::sin(angle),
            -libm::sin(angle),
            libm::cos(angle),
            0.0,
            0.0,
        ]);
        let scale = Affine::scale_non_uniform(ins.x_scale_factor, ins.y_scale_factor);
        let base = Affine::translate((-block.base_point.x, -block.base_point.y));
        let id = format!("{:X}", e.common.handle.0);
        for r in 0..rows {
            for c in 0..cols {
                let offset = Affine::translate((
                    f64::from(c) * ins.column_spacing,
                    f64::from(r) * ins.row_spacing,
                ));
                let m = at
                    * Affine::translate((ins.location.x, ins.location.y))
                    * rotation
                    * offset
                    * scale
                    * base;
                let link = if rows * cols > 1 {
                    format!("{id}#{r}.{c}")
                } else {
                    id.clone()
                };
                let chain = if chain.is_empty() {
                    link
                } else {
                    format!("{chain}/{link}")
                };
                self.walk(&block.entities, m, depth + 1, &chain, colour);
            }
        }
    }
}

/// The entity handles (group code 5) actually written in an ASCII DXF file.
///
/// The `dxf` crate assigns handles to entities that have none, which would make unrelated
/// entities look like old ones on re-import; only these real handles become source ids. Binary
/// DXF yields none, so its paths fall back to fingerprints.
fn explicit_handles(bytes: &[u8]) -> BTreeSet<u64> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let mut found = BTreeSet::new();
    while let (Some(code), Some(value)) = (lines.next(), lines.next()) {
        if code.trim() == "5"
            && let Ok(handle) = u64::from_str_radix(value.trim(), 16)
        {
            found.insert(handle);
        }
    }
    found
}

/// Imports a DXF file.
///
/// # Errors
///
/// [`DxfError::Parse`] when the bytes are not a readable DXF.
pub fn import_dxf(bytes: &[u8]) -> Result<ImportedDrawing, DxfError> {
    let drawing =
        Drawing::load(&mut Cursor::new(bytes)).map_err(|e| DxfError::Parse(e.to_string()))?;
    let code = drawing.header.default_drawing_units as i32;
    let unitless = dxf_is_unitless(code);
    let to_mm = mm_per_unit(UnitGuess::DxfInsUnits(code));

    let mut layers = Vec::new();
    let mut layer_colours = BTreeMap::new();
    let mut hidden_layers = Vec::new();
    for (order, layer) in drawing.layers().enumerate() {
        let visible = layer.is_layer_on && !layer.color.is_turned_off();
        layer_colours.insert(layer.name.clone(), layer.color.index().map(aci_rgb));
        if !visible {
            hidden_layers.push(layer.name.clone());
        }
        layers.push(LayerInfo {
            name: layer.name.clone(),
            visible,
            order: u32::try_from(order).unwrap_or(u32::MAX),
        });
    }

    let mut ctx = Ctx {
        explicit_handles: explicit_handles(bytes),
        blocks: drawing.blocks().map(|b| (b.name.as_str(), b)).collect(),
        layer_colours,
        hidden_layers,
        to_mm,
        items: Vec::new(),
        unsupported: BTreeMap::new(),
        missing_blocks: Vec::new(),
        depth_noted: false,
        element: 0,
    };
    // Scale to millimetres and flip y in one step; arcs stay arcs under a similarity.
    let base = Affine::new([to_mm, 0.0, 0.0, -to_mm, 0.0, 0.0]);
    let entities: Vec<Entity> = drawing.entities().cloned().collect();
    ctx.walk(&entities, base, 0, "", None);

    let mut diagnostics = Diagnostics::new();
    if unitless {
        diagnostics.push(Diagnostic::info(L10nKey::new("diag-import-unitless")));
    }
    for (name, count) in &ctx.unsupported {
        diagnostics.push(
            Diagnostic::info(L10nKey::new("diag-import-dxf-unsupported"))
                .with_arg("entity", name.clone())
                .with_arg("count", count.to_string()),
        );
    }
    for name in &ctx.missing_blocks {
        diagnostics.push(
            Diagnostic::warning(L10nKey::new("diag-import-dxf-missing-block"))
                .with_arg("block", name.clone()),
        );
    }
    if ctx.depth_noted {
        diagnostics.push(Diagnostic::warning(L10nKey::new(
            "diag-import-insert-depth",
        )));
    }

    let bounds = ctx
        .items
        .iter()
        .map(|i| i.geom.bounds())
        .reduce(|a, b| a.union(b));
    let size_candidates = bounds.map_or_else(Vec::new, |b| {
        let (w, h) = (b.width(), b.height());
        if unitless {
            vec![
                SizeCandidate::of(UnitGuess::Mm, w, h),
                SizeCandidate::of(UnitGuess::In, w, h),
            ]
        } else {
            vec![SizeCandidate::of(UnitGuess::Mm, w, h)]
        }
    });
    Ok(ImportedDrawing {
        unit_guess: UnitGuess::DxfInsUnits(code),
        size_candidates,
        layers,
        items: ctx.items,
        diagnostics,
    })
}

#[cfg(test)]
mod tests;
