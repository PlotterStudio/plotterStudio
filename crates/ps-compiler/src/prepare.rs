//! Geometry preparation.
//!
//! For one path and one operation: apply the transform to machine space, generate the operation's
//! geometry (passes, perforation pattern, fills), then drive-type processing (drag compensation,
//! tangential corner splitting). The result is a list of strokes the holder will follow.

use std::collections::BTreeMap;

use ps_core::PtUm;
use ps_geometry::{
    Affine, FillRule, FillSpec, Path, Pattern, Point, fill, flatten, flatten_to_grid, normalize,
};
use ps_library::DriveType;
use ps_model::{OpKind, ShapePath};

use crate::drag::{DragParams, DragWarning, compensate};
use crate::tangential::split_at_corners;

/// Tolerance for turning curves into grid polylines, in mm.
const GRID_TOL: f64 = 0.002;
/// Tolerance when flattening curves for tangential tools, in mm.
const TANGENT_TOL: f64 = 0.01;
/// Default corner-lift angle for tangential tools, in degrees.
const DEFAULT_LIFT_DEG: f64 = 30.0;
/// Default swivel threshold for drag blades, in degrees.
const DEFAULT_CORNER_DEG: f64 = 10.0;
/// Most passes one operation may ask for.
const MAX_PASSES: f64 = 100.0;
/// Default perforation cut and gap lengths, in mm.
const DEFAULT_CUT_MM: f64 = 2.0;
const DEFAULT_GAP_MM: f64 = 1.0;
/// Default fill overlap.
const DEFAULT_OVERLAP: f64 = 0.15;

/// Everything preparation reads for one path and operation.
#[derive(Debug, Clone, Copy)]
pub struct PrepareInput<'a> {
    /// The source path.
    pub path: &'a ShapePath,
    /// Object transform, mirror, origin and holder offset, composed.
    pub to_machine: Affine,
    /// What the operation does.
    pub kind: OpKind,
    /// How the tool follows a path.
    pub drive: DriveType,
    /// The tool's tip width, for fills.
    pub tip_width_mm: Option<f64>,
    /// The operation's fill settings.
    pub fill: Option<FillSpec>,
    /// The swivel offset of a drag blade.
    pub blade_offset_mm: f64,
    /// The radius of a rolling wheel, from the tool's geometry.
    pub wheel_radius_mm: Option<f64>,
    /// Resolved parameters by name.
    pub params: &'a BTreeMap<String, f64>,
    /// Where set, the hairpins that join fill rows are drawn as semicircles whose chords turn by at
    /// most this many degrees (the machine slows at sharp corners).
    pub turn_step_deg: Option<f64>,
}

/// A path the holder follows, with what ordering needs to know about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    /// The holder path (lines and arcs), in machine mm.
    pub path: Path,
    /// For closed artwork, the ring of the tip path on the grid, for containment.
    pub tip_ring: Option<Vec<PtUm>>,
    /// Whether the stroke may be run backwards.
    pub reversible: bool,
    /// Whether the tool angle follows the heading.
    pub tangential: bool,
    /// The fraction of the operation's force this stroke presses with (1 unless a pass ramp).
    pub force_scale: f64,
    /// True when the user chose the start of this closed path, so ordering keeps it.
    pub fixed_start: bool,
    /// Strokes of one group (a separate piece of a fill, such as one letter) are drawn together
    /// before ordering moves on to another group; 0 for strokes that belong to no group.
    pub group: u32,
}

/// Something preparation wants reported.
#[derive(Debug, Clone, PartialEq)]
pub enum Note {
    /// A wavy cut is drawn as a plain stroke.
    WavyCutAsStroke,
    /// A drag-blade arc is tighter than the blade offset.
    MinRadius {
        /// The arc's radius in mm.
        radius_mm: f64,
    },
}

/// The strokes of one path and operation, plus notes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Prepared {
    /// Strokes in the order they were generated.
    pub strokes: Vec<Stroke>,
    /// Things worth reporting.
    pub notes: Vec<Note>,
}

fn param(p: &BTreeMap<String, f64>, name: &str, default: f64) -> f64 {
    p.get(name).copied().unwrap_or(default)
}

fn ring_of(path: &Path) -> Option<Vec<PtUm>> {
    if !path.closed {
        return None;
    }
    flatten_to_grid(path, GRID_TOL)
        .ok()
        .filter(|r| r.len() >= 3)
}

fn polyline_from_grid(points: &[PtUm]) -> Option<Path> {
    let mut pts: Vec<Point> = points
        .iter()
        .map(|p| {
            let (x, y) = p.to_mm();
            Point::new(x, y)
        })
        .collect();
    if pts.len() < 2 {
        return None;
    }
    let closed = pts.len() > 2 && pts.first() == pts.last();
    if closed {
        pts.pop();
    }
    Some(Path::polyline(&pts, closed))
}

/// How many row spacings apart two stroke ends may be and still be joined with the pen down.
const LINK_ROWS: f64 = 8.0;
/// The longest pen-down join, in millimetres, whatever the pen: beyond this lifting is cheaper.
const LINK_MAX_MM: f64 = 3.0;
/// How far past the ink edge a join may stray, in millimetres. The ends of a hatch row lie on the
/// boundary, so a join between two of them hugs it; the slack keeps that from being a coin toss.
const LINK_SLACK_MM: f64 = 0.04;

/// Joins the rows of a hatch into longer strokes wherever the pen can stay down inside the shape,
/// so it lifts once per run of rows instead of once per row. Concentric rings are left alone.
fn link_rows(
    lines: Vec<Vec<PtUm>>,
    shape: &ps_geometry::Shape,
    tip: f64,
    spec: &FillSpec,
    turn_step_deg: Option<f64>,
) -> Vec<Vec<PtUm>> {
    if spec.pattern == Pattern::Concentric {
        return lines;
    }
    let region =
        ps_geometry::inset_region(std::slice::from_ref(shape), (tip - LINK_SLACK_MM).max(0.0));
    let rings: Vec<_> = region.iter().flatten().cloned().collect();
    let spacing = ps_geometry::spacing(tip, spec.overlap);
    let reach = ps_core::um_from_mm((spacing * LINK_ROWS).min(LINK_MAX_MM)).unwrap_or(0);
    crate::link::connect_rounded(
        lines,
        &ps_geometry::Region::new(&rings),
        reach,
        turn_step_deg,
    )
}

fn fill_strokes(geom: &Path, input: &PrepareInput<'_>) -> Vec<Stroke> {
    fill_region(std::slice::from_ref(geom), input)
}

/// Fills the region made by several closed paths together, with the first input's fill rule, so
/// an inner ring is a hole in the outer one (the counter of an "o").
fn fill_region(geoms: &[Path], input: &PrepareInput<'_>) -> Vec<Stroke> {
    let rings: Vec<_> = geoms.iter().filter_map(ring_of).collect();
    if rings.is_empty() {
        return Vec::new();
    }
    let rule: FillRule = input.path.fill_rule;
    let shapes = normalize(&rings, rule);
    let spec = input.fill.unwrap_or(FillSpec {
        pattern: Pattern::Parallel,
        angle_deg: 0.0,
        overlap: DEFAULT_OVERLAP,
        outline_pass: false,
    });
    let tip = input.tip_width_mm.unwrap_or(0.4);
    // Hatching plus the centre lines of the parts too thin for the hatch to reach. Each separate
    // piece (a letter, say) is its own group, so the pen finishes one before starting the next.
    let mut out = Vec::new();
    for (i, shape) in shapes.iter().enumerate() {
        let group = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let lines = fill(std::slice::from_ref(shape), tip, &spec).strokes;
        out.extend(
            link_rows(lines, shape, tip, &spec, input.turn_step_deg)
                .iter()
                .filter_map(|line| polyline_from_grid(line))
                .map(|path| Stroke {
                    path,
                    tip_ring: None,
                    reversible: true,
                    tangential: false,
                    force_scale: 1.0,
                    fixed_start: false,
                    group,
                }),
        );
    }
    out
}

/// Prepares several fill inputs that belong to one object as a single region, so rings inside
/// rings become holes. The inputs share the first one's settings.
#[must_use]
pub fn prepare_compound(inputs: &[PrepareInput<'_>]) -> Prepared {
    let Some(first) = inputs.first() else {
        return Prepared::default();
    };
    let geoms: Vec<Path> = inputs
        .iter()
        .map(|i| i.path.geom.transformed(i.to_machine))
        .filter(|g| !g.is_empty())
        .collect();
    Prepared {
        strokes: fill_region(&geoms, first),
        ..Prepared::default()
    }
}

/// Most tabs one cut may have.
const MAX_TABS: f64 = 100.0;

fn is_cut(kind: OpKind) -> bool {
    matches!(kind, OpKind::Cut | OpKind::KissCut)
}

/// Joins open pieces end to start into one path.
fn joined(first: &Path, then: &Path) -> Path {
    let mut path = Path::new(first.start);
    path.segs.extend(first.segs.iter().copied());
    path.segs.extend(then.segs.iter().copied());
    path
}

/// A cut with tabs: `tab_count` gaps of `tab_width_mm`, spread evenly, that stay uncut so the piece
/// holds in the sheet. The gaps together never take more than half the path.
fn with_tabs(geom: &Path, params: &BTreeMap<String, f64>) -> Vec<Path> {
    let count = libm::round(param(params, "tab_count", 0.0).clamp(0.0, MAX_TABS));
    let total = geom.length();
    let width = param(params, "tab_width_mm", 1.0).max(0.0);
    if count < 1.0 || width <= 0.0 || total <= 0.0 {
        return vec![geom.clone()];
    }
    let width = width.min(total * 0.5 / count);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=100 tabs"
    )]
    let n = count as usize;
    let mut cuts = Vec::with_capacity(n * 2);
    for i in 0..n {
        #[expect(clippy::cast_precision_loss, reason = "at most 100 tabs")]
        let index = i as f64;
        let centre = if geom.closed {
            (index + 0.5) / count * total
        } else {
            (index + 1.0) / (count + 1.0) * total
        };
        cuts.push(centre - width / 2.0);
        cuts.push(centre + width / 2.0);
    }
    let mut open = geom.clone();
    open.closed = false;
    let pieces: Vec<Path> = open.split_at_lengths(&cuts);
    let mut kept: Vec<Path> = pieces
        .into_iter()
        .enumerate()
        .filter_map(|(i, p)| (i % 2 == 0 && !p.segs.is_empty()).then_some(p))
        .collect();
    if geom.closed && kept.len() > 1 {
        // The last piece runs on into the first across the start of the closed path.
        let first = kept.remove(0);
        if let Some(last) = kept.pop() {
            kept.push(joined(&last, &first));
        } else {
            kept.push(first);
        }
    }
    kept
}

/// An open cut extended along its end directions so the blade is up to speed and aligned before
/// the first mark and carries on past the last.
fn with_leads(path: &Path, params: &BTreeMap<String, f64>) -> Path {
    let lead_in = param(params, "lead_in_mm", 0.0).max(0.0);
    let lead_out = param(params, "lead_out_mm", 0.0).max(0.0);
    if path.closed || path.is_empty() || (lead_in <= 0.0 && lead_out <= 0.0) {
        return path.clone();
    }
    let unit = |v: ps_geometry::Vec2| {
        let n = ps_geometry::arc::length(v);
        (n > 1e-12).then(|| ps_geometry::Vec2::new(v.x / n, v.y / n))
    };
    let mut out = path.clone();
    if lead_in > 0.0
        && let Some((start, dir)) = path.sample_at(0.0)
        && let Some(d) = unit(dir)
    {
        let new_start = Point::new(start.x - d.x * lead_in, start.y - d.y * lead_in);
        let mut led = Path::new(new_start).line_to(start);
        led.segs.extend(out.segs.iter().copied());
        out = led;
    }
    if lead_out > 0.0
        && let Some((end, dir)) = path.sample_at(path.length())
        && let Some(d) = unit(dir)
    {
        out = out.line_to(Point::new(end.x + d.x * lead_out, end.y + d.y * lead_out));
    }
    out
}

/// How hard pass `index` of `passes` presses, as a fraction of the full force: with a ramp the
/// first pass is lighter and each later one presses harder until the last is at full force.
fn pass_scale(index: usize, passes: usize, ramp: f64) -> f64 {
    if passes <= 1 {
        return 1.0;
    }
    let ramp = ramp.clamp(0.0, 1.0);
    #[expect(clippy::cast_precision_loss, reason = "at most 100 passes")]
    let (i, n) = ((index + 1) as f64, passes as f64);
    1.0 - ramp + ramp * i / n
}

fn perforate(geom: &Path, params: &BTreeMap<String, f64>) -> Vec<Path> {
    let cut = param(params, "cut_mm", DEFAULT_CUT_MM).max(0.01);
    let gap = param(params, "gap_mm", DEFAULT_GAP_MM).max(0.0);
    let total = geom.length();
    let mut at = 0.0;
    let mut distances = Vec::new();
    while at < total {
        at += cut;
        distances.push(at);
        at += gap;
        distances.push(at);
    }
    geom.split_at_lengths(&distances)
        .into_iter()
        .enumerate()
        .filter_map(|(i, p)| (i % 2 == 0).then_some(p))
        .collect()
}

fn plain(path: Path, ring: Option<Vec<PtUm>>, reversible: bool) -> Stroke {
    Stroke {
        path,
        tip_ring: ring,
        reversible,
        tangential: false,
        force_scale: 1.0,
        fixed_start: false,
        group: 0,
    }
}

/// The geometry of `path` rotated to where its stroke starts, in the object's own space.
///
/// This is the single calculation of a start point. A locked start is the path as stored. Otherwise
/// a drag blade starts in the middle of the longest straight edge (so it does not swivel at a
/// corner) and every other tool where the path starts. The compiler cuts this geometry and the
/// Prepare tab draws its start marker from it, so the two always agree.
#[must_use]
pub fn effective_start_geom(path: &ShapePath, drive: DriveType) -> Path {
    if path.start_locked || !path.geom.closed || drive != DriveType::Drag {
        path.geom.clone()
    } else {
        crate::drag::start_on_longest_line(&path.geom)
    }
}

/// Prepares one path for one operation.
#[must_use]
pub fn prepare(input: &PrepareInput<'_>) -> Prepared {
    let is_fill = matches!(input.kind, OpKind::Fill | OpKind::FoilFill);
    // The one place a start is decided: the Prepare tab's marker calls `effective_start_geom` on the
    // same object-space geometry, so the marker and the cut cannot disagree.
    let geom = if is_fill {
        input.path.geom.clone()
    } else {
        effective_start_geom(input.path, input.drive)
    }
    .transformed(input.to_machine);
    let mut out = Prepared::default();
    if geom.is_empty() {
        return out;
    }

    let mut strokes: Vec<Stroke> = if is_fill {
        fill_strokes(&geom, input)
    } else if input.kind == OpKind::Perforate {
        perforate(&geom, input.params)
            .into_iter()
            .map(|p| plain(p, None, true))
            .collect()
    } else {
        if input.kind == OpKind::WavyCut {
            out.notes.push(Note::WavyCutAsStroke);
        }
        cut_strokes(&geom, input)
    };

    // The start is already decided above; nothing downstream may move it.
    for s in &mut strokes {
        s.fixed_start = s.path.closed && !is_fill;
    }
    if !is_fill {
        match input.drive {
            DriveType::Drag => strokes = drag_strokes(strokes, input, &mut out.notes),
            DriveType::Tangential | DriveType::Rolling => {
                strokes = tangential_strokes(strokes, input);
            }
            DriveType::FixedPoint | DriveType::Heated => {}
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 1..=100 passes"
    )]
    let passes = libm::round(param(input.params, "passes", 1.0).clamp(1.0, MAX_PASSES)) as usize;
    let ramp = param(input.params, "force_ramp", 0.0);
    for pass in 0..passes {
        let scale = pass_scale(pass, passes, ramp);
        out.strokes.extend(strokes.iter().cloned().map(|mut s| {
            s.force_scale = scale;
            s
        }));
    }
    out
}

/// The strokes of a plain (cut, kiss cut, score, draw …) path: tabs and leads for cuts.
fn cut_strokes(geom: &Path, input: &PrepareInput<'_>) -> Vec<Stroke> {
    if !is_cut(input.kind) {
        return vec![plain(geom.clone(), ring_of(geom), true)];
    }
    let mut ring = ring_of(geom);
    with_tabs(geom, input.params)
        .into_iter()
        .map(|piece| plain(with_leads(&piece, input.params), ring.take(), true))
        .collect()
}

fn drag_strokes(
    strokes: Vec<Stroke>,
    input: &PrepareInput<'_>,
    notes: &mut Vec<Note>,
) -> Vec<Stroke> {
    let mut out = Vec::new();
    for s in strokes {
        let closed = s.path.closed;
        let params = DragParams {
            offset_mm: input.blade_offset_mm,
            corner_threshold_deg: param(input.params, "corner_threshold_deg", DEFAULT_CORNER_DEG),
            overcut_mm: if closed {
                param(input.params, "overcut", 0.0)
            } else {
                0.0
            },
            align_mm: param(input.params, "align_mm", 0.0),
            keep_start: s.fixed_start,
        };
        let c = compensate(&s.path, &params);
        for w in &c.warnings {
            let DragWarning::MinRadius { radius_mm, .. } = *w;
            notes.push(Note::MinRadius { radius_mm });
        }
        if c.path.is_empty() {
            continue;
        }
        out.push(Stroke {
            path: c.path,
            tip_ring: s.tip_ring,
            reversible: false,
            tangential: false,
            force_scale: 1.0,
            fixed_start: s.fixed_start,
            group: s.group,
        });
    }
    out
}

/// Moves each lifted corner's ends so a rolling wheel's contact point reaches the corner: the
/// piece before runs on by the wheel radius, and the piece after starts one radius early, so the
/// wheel is already turned to the new heading when it arrives.
fn overshoot_corners(pieces: &mut [Vec<PtUm>], radius: f64) {
    let shift = |from: PtUm, toward: PtUm, by: f64| -> Option<PtUm> {
        let (fx, fy) = from.to_mm();
        let (tx, ty) = toward.to_mm();
        let (dx, dy) = (tx - fx, ty - fy);
        let len = libm::hypot(dx, dy);
        if len <= 0.0 {
            return None;
        }
        PtUm::from_mm(fx + dx / len * by, fy + dy / len * by).ok()
    };
    let last = pieces.len().saturating_sub(1);
    for (i, piece) in pieces.iter_mut().enumerate() {
        if piece.len() < 2 {
            continue;
        }
        if i < last
            && let Some(end) = shift(piece[piece.len() - 1], piece[piece.len() - 2], -radius)
        {
            let n = piece.len();
            piece[n - 1] = end;
        }
        if i > 0
            && let Some(start) = shift(piece[0], piece[1], -radius)
        {
            piece[0] = start;
        }
    }
}

fn tangential_strokes(strokes: Vec<Stroke>, input: &PrepareInput<'_>) -> Vec<Stroke> {
    let lift = param(input.params, "corner_lift_deg", DEFAULT_LIFT_DEG);
    let wheel = input.wheel_radius_mm.unwrap_or(0.0);
    let mut out = Vec::new();
    for s in strokes {
        let mut pts = flatten(&s.path, TANGENT_TOL);
        if s.path.closed
            && let Some(&first) = pts.first()
        {
            pts.push(first);
        }
        let grid: Vec<PtUm> = pts
            .iter()
            .filter_map(|p| PtUm::from_mm(p.x, p.y).ok())
            .collect();
        let mut pieces = split_at_corners(&grid, lift);
        if input.drive == DriveType::Rolling && wheel > 0.0 && !s.path.closed {
            overshoot_corners(&mut pieces, wheel);
        }
        for piece in pieces {
            if let Some(path) = polyline_from_grid(&piece) {
                out.push(Stroke {
                    path,
                    tip_ring: s.tip_ring.clone(),
                    reversible: true,
                    tangential: true,
                    force_scale: 1.0,
                    fixed_start: s.fixed_start,
                    group: s.group,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
