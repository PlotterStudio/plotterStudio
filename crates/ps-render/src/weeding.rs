//! The weeded view: through-cut pieces become separate layers lifted along the view
//! normal, and for vinyl the waste is removed and the liner shows.
//!
//! Pieces come from the closed through-cut contours of the job. A contour is nested some number of
//! levels deep inside other contours; odd depth is a piece, even depth is waste (the outside of
//! everything, and the inside of an "O"). Each piece keeps its own id, so a lifted piece moves as
//! one.

use ps_geometry::Point;
use ps_job::Step;

use crate::Image;
use crate::preview::{BaseMaterial, Effect, EffectMaps, Light, ToolLook, composite};
use crate::toolpath::{SegmentKind, segments};

/// A contour whose ends are this close (millimetres) counts as closed: cuts overcut past their
/// start, and the overcut is at most this long.
const CLOSE_GAP_MM: f64 = 2.0;
/// How much darker the liner is in a lifted piece's shadow.
const SHADOW: f32 = 0.72;
/// How far past the lifted piece the shadow reaches, in millimetres.
const SHADOW_REACH_MM: f64 = 1.5;

/// Where the pieces of a mat are: one id per pixel, 0 for waste.
#[derive(Debug, Clone, PartialEq)]
pub struct Pieces {
    width: usize,
    height: usize,
    scale: f64,
    ids: Vec<u32>,
    count: usize,
}

fn distance(a: Point, b: Point) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// The closed contours of the through cuts among the first `up_to` steps.
fn contours(steps: &[Step], looks: &[ToolLook], up_to: usize) -> Vec<Vec<Point>> {
    let through = |slot: u8| {
        looks
            .iter()
            .any(|l| l.slot == slot && l.effect == Effect::Cut)
    };
    let mut rings = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    let mut flush = |run: &mut Vec<Point>| {
        if run.len() >= 4 {
            let (first, last) = (run[0], run[run.len() - 1]);
            let gap = distance(first, last);
            let length: f64 = run.windows(2).map(|w| distance(w[0], w[1])).sum();
            if gap <= CLOSE_GAP_MM && length >= gap * 4.0 {
                rings.push(std::mem::take(run));
            }
        }
        run.clear();
    };
    for seg in segments(steps, up_to) {
        match seg.kind {
            SegmentKind::Draw { slot: Some(slot) } if through(slot) => {
                if current.last().is_none_or(|p| distance(*p, seg.from) > 1e-6) {
                    flush(&mut current);
                    current.push(seg.from);
                }
                current.push(seg.to);
            }
            _ => flush(&mut current),
        }
    }
    flush(&mut current);
    rings
}

fn area(points: &[Point]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

fn pixel_count(mm: f64, scale: f64) -> usize {
    let px = (mm * scale).ceil();
    if px.is_finite() && px >= 1.0 {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a positive pixel count of a mat, capped well below usize"
        )]
        {
            px.min(32_768.0) as usize
        }
    } else {
        1
    }
}

fn clamp_index(value: f64, last: usize) -> usize {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped into the pixel range first"
    )]
    {
        value
            .floor()
            .clamp(0.0, f64::from(u32::try_from(last).unwrap_or(u32::MAX))) as usize
    }
}

impl Pieces {
    /// Finds the pieces freed by the closed through-cut contours among the first `up_to` steps,
    /// on a map of a `mat_width_mm` x `mat_depth_mm` mat at `px_per_mm`.
    #[must_use]
    pub fn from_steps(
        steps: &[Step],
        looks: &[ToolLook],
        mat_width_mm: f64,
        mat_depth_mm: f64,
        px_per_mm: f64,
        up_to: usize,
    ) -> Self {
        let scale = if px_per_mm.is_finite() && px_per_mm > 0.0 {
            px_per_mm.min(crate::preview::MAX_PX_PER_MM)
        } else {
            1.0
        };
        let (width, height) = (
            pixel_count(mat_width_mm, scale),
            pixel_count(mat_depth_mm, scale),
        );
        let mut rings = contours(steps, looks, up_to);
        // Big contours first, so a smaller one nested inside is painted later and wins as the
        // innermost.
        rings.sort_by(|big, small| area(small).total_cmp(&area(big)));
        let mut depth = vec![0_u32; width * height];
        let mut innermost = vec![0_u32; width * height];
        for (index, contour) in rings.iter().enumerate() {
            let id = u32::try_from(index + 1).unwrap_or(u32::MAX);
            fill(contour, scale, width, height, |at| {
                depth[at] += 1;
                innermost[at] = id;
            });
        }
        let mut ids = vec![0_u32; width * height];
        let mut seen = std::collections::BTreeSet::new();
        for at in 0..ids.len() {
            if depth[at] % 2 == 1 {
                ids[at] = innermost[at];
                seen.insert(innermost[at]);
            }
        }
        Self {
            width,
            height,
            scale,
            ids,
            count: seen.len(),
        }
    }

    /// How many pieces there are.
    #[must_use]
    pub fn count(&self) -> usize {
        self.count
    }

    /// The piece at a position in millimetres, or `None` for waste or outside the mat.
    #[must_use]
    pub fn piece_at(&self, x_mm: f64, y_mm: f64) -> Option<u32> {
        let (fx, fy) = ((x_mm * self.scale).floor(), (y_mm * self.scale).floor());
        if !(fx.is_finite() && fy.is_finite()) || fx < 0.0 || fy < 0.0 {
            return None;
        }
        let (x, y) = (
            clamp_index(fx, usize::MAX >> 1),
            clamp_index(fy, usize::MAX >> 1),
        );
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(self.ids[y * self.width + x]).filter(|id| *id != 0)
    }
}

/// Calls `paint` with the index of every pixel whose centre is inside the polygon (even-odd).
fn fill(points: &[Point], scale: f64, width: usize, height: usize, mut paint: impl FnMut(usize)) {
    let n = points.len();
    let top = points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let bottom = points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
    let first_row = clamp_index(top * scale - 1.0, height.saturating_sub(1));
    let last_row = clamp_index(bottom * scale + 1.0, height.saturating_sub(1));
    for row in first_row..=last_row {
        #[expect(clippy::cast_precision_loss, reason = "pixel indices are small")]
        let y = (row as f64 + 0.5) / scale;
        let mut crossings: Vec<f64> = Vec::new();
        for i in 0..n {
            let (a, b) = (points[i], points[(i + 1) % n]);
            if (a.y <= y) != (b.y <= y) {
                crossings.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
            }
        }
        crossings.sort_by(f64::total_cmp);
        for pair in crossings.as_chunks::<2>().0 {
            // A pixel is inside when its centre is.
            let start = clamp_index((pair[0] * scale - 0.5).ceil(), width.saturating_sub(1));
            let end = clamp_index((pair[1] * scale - 0.5).floor(), width.saturating_sub(1));
            #[expect(clippy::cast_precision_loss, reason = "pixel indices are small")]
            for col in start..=end {
                let cx = (col as f64 + 0.5) / scale;
                if cx >= pair[0] && cx <= pair[1] {
                    paint(row * width + col);
                }
            }
        }
    }
}

/// How the weeded view is laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weeding {
    /// How far pieces are lifted, in millimetres (0 leaves them in place).
    pub lift_mm: f64,
    /// Which way a lift moves the pieces across the view (it need not be a unit vector).
    pub direction: (f64, f64),
    /// Whether the waste is removed (vinyl): the liner shows where it was.
    pub remove_waste: bool,
    /// The liner's colour.
    pub liner: [u8; 3],
}

fn pixel_offset(weeding: &Weeding, scale: f64) -> (isize, isize) {
    let (dx, dy) = weeding.direction;
    let len = dx.hypot(dy);
    if !(len.is_finite() && len > 0.0 && weeding.lift_mm.is_finite()) {
        return (0, 0);
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "an offset of a few hundred pixels"
    )]
    let to_px = |v: f64| (v / len * weeding.lift_mm * scale).round() as isize;
    (to_px(dx), to_px(dy))
}

fn shifted(index: usize, width: usize, height: usize, by: (isize, isize)) -> Option<usize> {
    let (row, col) = (index / width, index % width);
    let r = row.checked_add_signed(by.1)?;
    let c = col.checked_add_signed(by.0)?;
    (r < height && c < width).then_some(r * width + c)
}

/// The mat with its pieces lifted and, for vinyl, its waste removed.
#[must_use]
pub fn weeded(
    base: &BaseMaterial,
    maps: &EffectMaps,
    pieces: &Pieces,
    light: &Light,
    weeding: &Weeding,
) -> Image {
    let flat = composite(base, maps, light);
    let (width, height) = (flat.width as usize, flat.height as usize);
    if (pieces.width, pieces.height) != (width, height) {
        return flat;
    }
    let lifted = weeding.lift_mm > 0.0;
    let liner = [weeding.liner[0], weeding.liner[1], weeding.liner[2], 255];
    let at = |image: &[u8], i: usize| [image[i * 4], image[i * 4 + 1], image[i * 4 + 2], 255];
    let mut out: Vec<u8> = Vec::with_capacity(flat.rgba.len());
    for (i, &id) in pieces.ids.iter().enumerate() {
        let is_piece = id != 0;
        let colour = if is_piece {
            // A lifted piece leaves the liner where it was.
            if lifted { liner } else { at(&flat.rgba, i) }
        } else if weeding.remove_waste {
            liner
        } else {
            at(&flat.rgba, i)
        };
        out.extend_from_slice(&colour);
    }
    let offset = pixel_offset(weeding, pieces.scale);
    if lifted {
        let (dx, dy) = weeding.direction;
        let len = dx.hypot(dy).max(f64::MIN_POSITIVE);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a shadow a few pixels long"
        )]
        let reach = |v: f64| (v / len * SHADOW_REACH_MM * pieces.scale).round() as isize;
        let shadow_by = (offset.0 + reach(dx), offset.1 + reach(dy));
        let mut covered = vec![false; width * height];
        for (i, &id) in pieces.ids.iter().enumerate() {
            if id != 0
                && let Some(to) = shifted(i, width, height, offset)
            {
                covered[to] = true;
            }
        }
        for (i, &id) in pieces.ids.iter().enumerate() {
            if id != 0
                && let Some(to) = shifted(i, width, height, shadow_by)
                && !covered[to]
            {
                for channel in 0..3 {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a darkened byte stays within 0..=255"
                    )]
                    {
                        out[to * 4 + channel] =
                            (f32::from(out[to * 4 + channel]) * SHADOW).round() as u8;
                    }
                }
            }
        }
    }
    for (i, &id) in pieces.ids.iter().enumerate() {
        if id != 0
            && let Some(to) = shifted(i, width, height, offset)
        {
            out[to * 4..to * 4 + 4].copy_from_slice(&at(&flat.rgba, i));
        }
    }
    Image {
        width: flat.width,
        height: flat.height,
        rgba: out,
    }
}

#[cfg(test)]
mod tests;
