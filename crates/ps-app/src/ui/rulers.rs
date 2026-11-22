//! The canvas rulers: where the tick marks go along the top and left edges, and the
//! drawing of rulers, guides and the snap marker over the canvas.

use ps_geometry::Point;
use ps_model::GuideAxis;

use super::App;
use crate::state::AppState;

/// Fewest pixels between neighbouring ticks.
pub(super) const MIN_TICK_PX: f64 = 6.0;
/// The most ticks one ruler draws.
pub(super) const MAX_TICKS: usize = 2_000;

/// One tick on a ruler.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Tick {
    /// Its position in millimetres.
    pub(super) mm: f64,
    /// True for the labelled, longer ticks.
    pub(super) major: bool,
}

/// The tick spacing in millimetres for `zoom` pixels per millimetre: 1, 2 or 5 times a power of ten,
/// the finest that keeps ticks at least [`MIN_TICK_PX`] apart.
pub(super) fn tick_step_mm(zoom: f64) -> f64 {
    if !(zoom.is_finite() && zoom > 0.0) {
        return 10.0;
    }
    let wanted = MIN_TICK_PX / zoom;
    let decade = 10f64.powf(wanted.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if decade * m >= wanted - 1e-12 {
            return decade * m;
        }
    }
    decade * 10.0
}

/// The ticks between `from_mm` and `to_mm` at `zoom` pixels per millimetre.
pub(super) fn ruler_ticks(from_mm: f64, to_mm: f64, zoom: f64) -> Vec<Tick> {
    if !(from_mm.is_finite()
        && to_mm.is_finite()
        && to_mm > from_mm
        && zoom.is_finite()
        && zoom > 0.0)
    {
        return Vec::new();
    }
    let step = tick_step_mm(zoom);
    let first = (from_mm / step).ceil();
    let mut out = Vec::new();
    let mut index = first;
    while index * step <= to_mm && out.len() < MAX_TICKS {
        out.push(Tick {
            mm: index * step,
            major: (index % 5.0).abs() < 1e-9,
        });
        index += 1.0;
    }
    out
}

/// Draws the guide lines alone, for screens that have no rulers.
pub(super) fn draw_guides(painter: &egui::Painter, app: &App, rect: egui::Rect) {
    draw_with(painter, app, rect, true, false);
}

/// Draws guide lines (when the GPU scene is not doing it), the rulers and the snap marker.
pub(super) fn draw(painter: &egui::Painter, app: &App, rect: egui::Rect, draw_guides: bool) {
    draw_with(painter, app, rect, draw_guides, true);
}

fn draw_with(
    painter: &egui::Painter,
    app: &App,
    rect: egui::Rect,
    draw_guides: bool,
    with_rulers: bool,
) {
    let state = &app.state;
    let mat = state.current_mat();
    let zoom = state.viewport().zoom();
    let origin = rect.min;
    if draw_guides {
        let colour = egui::Color32::from_rgb(0x00, 0xA8, 0xC8);
        for guide in mat.guides.iter() {
            let (a, b) = match guide.axis {
                GuideAxis::Horizontal => (
                    Point::new(-1.0e5, guide.at_mm),
                    Point::new(1.0e5, guide.at_mm),
                ),
                GuideAxis::Vertical => (
                    Point::new(guide.at_mm, -1.0e5),
                    Point::new(guide.at_mm, 1.0e5),
                ),
            };
            painter.line_segment(
                [
                    super::to_screen(state, origin, a),
                    super::to_screen(state, origin, b),
                ],
                egui::Stroke::new(1.0_f32, colour),
            );
        }
    }
    if with_rulers {
        rulers(painter, state, rect, zoom);
    }
    // The grid snaps quietly; only a shape's own points are worth marking.
    if let Some((point, kind)) = state.last_snap()
        && kind != ps_render::SnapKind::Grid
    {
        let at = super::to_screen(state, origin, point);
        painter.circle_stroke(
            at,
            5.0,
            egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(0xE0, 0x3E, 0x52)),
        );
    }
}

/// The two ruler bars of the design: white 22 px strips with rounded corners, inset 8 px from the
/// top and left edges of the canvas, ticks along the edge that faces the work.
fn rulers(painter: &egui::Painter, state: &AppState, rect: egui::Rect, zoom: f64) {
    const BAND: f32 = 22.0;
    const INSET: f32 = 8.0;
    let ctx = painter.ctx().clone();
    let pal = crate::theme::Palette::of(ctx.theme());
    let top = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + INSET + BAND + 4.0, rect.min.y + INSET),
        egui::pos2(rect.max.x - INSET, rect.min.y + INSET + BAND),
    );
    let left = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + INSET, rect.min.y + INSET + BAND + 4.0),
        egui::pos2(rect.min.x + INSET + BAND, rect.max.y - INSET),
    );
    for bar in [top, left] {
        painter.rect_filled(bar, 4.0, pal.panel);
    }
    let (min, max) = (
        state.screen_to_world(Point::new(0.0, 0.0)),
        state.screen_to_world(Point::new(
            f64::from(rect.width()),
            f64::from(rect.height()),
        )),
    );
    let stroke = egui::Stroke::new(1.0_f32, pal.outline);
    let font = super::zest::font(&ctx, 9.0, super::zest::Weight::Regular);
    let top_painter = painter.with_clip_rect(top);
    for tick in ruler_ticks(min.x, max.x, zoom) {
        let x = super::to_screen(state, rect.min, Point::new(tick.mm, 0.0)).x;
        let len = if tick.major { 7.0 } else { 3.5 };
        top_painter.line_segment(
            [
                egui::pos2(x, top.bottom() - len),
                egui::pos2(x, top.bottom()),
            ],
            stroke,
        );
        if tick.major {
            top_painter.text(
                egui::pos2(x + 3.0, top.top() + 3.0),
                egui::Align2::LEFT_TOP,
                format!("{}", tick.mm),
                font.clone(),
                pal.muted,
            );
        }
    }
    let left_painter = painter.with_clip_rect(left);
    for tick in ruler_ticks(min.y, max.y, zoom) {
        let y = super::to_screen(state, rect.min, Point::new(0.0, tick.mm)).y;
        let len = if tick.major { 7.0 } else { 3.5 };
        left_painter.line_segment(
            [
                egui::pos2(left.right() - len, y),
                egui::pos2(left.right(), y),
            ],
            stroke,
        );
        if tick.major {
            left_painter.text(
                egui::pos2(left.left() + 3.0, y + 2.0),
                egui::Align2::LEFT_TOP,
                format!("{}", tick.mm),
                font.clone(),
                pal.muted,
            );
        }
    }
}

#[cfg(test)]
mod tests;
