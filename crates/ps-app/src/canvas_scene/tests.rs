//! Ground truth for the canvas scene: the mat, grid, objects and their operation
//! colours and line styles as GPU draws. Checked by rendering offscreen and sampling pixels at
//! positions found through the viewport, so the expectations are in millimetres.

use std::sync::Mutex;

use ps_geometry::Point;
use ps_library::Settings;
use ps_model::OpKind;
use ps_render::{GpuContext, Image, colour_of};

use super::*;
use crate::commands::Mods;
use crate::state::AppState;
use crate::tools::CanvasEvent;

const NONE: Mods = Mods {
    command: false,
    shift: false,
    alt: false,
};

static DEVICE: Mutex<()> = Mutex::new(());

fn render(state: &AppState, w: u32, h: u32) -> Image {
    let _turn = DEVICE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let gpu = GpuContext::headless().expect("a graphics device");
    gpu.render(&build_scene(state, false), w, h)
}

fn sized_state() -> AppState {
    let mut s = AppState::new(Settings::default());
    s.resize_canvas(400.0, 300.0);
    s.run_command("view.zoom-fit", 0);
    s
}

fn at(state: &AppState, image: &Image, x_mm: f64, y_mm: f64) -> [u8; 4] {
    let p = state.viewport().world_to_screen(Point::new(x_mm, y_mm));
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a rounded, non-negative pixel position inside a small test image"
    )]
    let (x, y) = (p.x.round() as u32, p.y.round() as u32);
    let i = ((y.min(image.height - 1) * image.width + x.min(image.width - 1)) * 4) as usize;
    [
        image.rgba[i],
        image.rgba[i + 1],
        image.rgba[i + 2],
        image.rgba[i + 3],
    ]
}

fn near(a: [u8; 4], b: [u8; 3], tol: u8) -> bool {
    a[..3].iter().zip(&b).all(|(x, y)| x.abs_diff(*y) <= tol)
}

/// Clicks empty mat with the select tool.
fn deselect(s: &mut AppState) {
    s.run_command("tool.select", 0);
    s.canvas_event(CanvasEvent::Press(Point::new(290.0, 290.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(Point::new(290.0, 290.0)), NONE, 0);
}

fn draw_rect(s: &mut AppState, from: (f64, f64), to: (f64, f64)) {
    s.run_command("tool.rectangle", 0);
    s.canvas_event(CanvasEvent::Press(Point::new(from.0, from.1)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(Point::new(to.0, to.1)), NONE, 0);
}

#[test]
fn the_mat_is_drawn_on_the_background() {
    let s = sized_state();
    let image = render(&s, 400, 300);
    // Inside the 12 x 12 inch mat: the mat colour. The window is wider than the mat, so the
    // left edge of the picture is background.
    assert!(
        near(at(&s, &image, 150.0, 150.0), MAT_COLOUR, 2),
        "{:?}",
        at(&s, &image, 150.0, 150.0)
    );
    let outside = image.rgba[0..4].to_vec();
    assert!(
        !near([outside[0], outside[1], outside[2], 255], MAT_COLOUR, 2),
        "background differs from the mat"
    );
}

#[test]
fn the_dark_theme_has_a_dark_background() {
    let _turn = DEVICE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = sized_state();
    let gpu = GpuContext::headless().unwrap();
    let dark = gpu.render(&build_scene(&s, true), 400, 300);
    let light = gpu.render(&build_scene(&s, false), 400, 300);
    assert!(dark.rgba[0] < light.rgba[0], "the corner pixel is darker");
}

#[test]
fn an_object_is_outlined_in_its_operations_colour() {
    let mut s = sized_state();
    draw_rect(&mut s, (50.0, 50.0), (150.0, 100.0));
    s.assign_operation(OpKind::Cut, "generic.drag-blade@1.0.0", 0);
    deselect(&mut s);
    let image = render(&s, 400, 300);
    // The top edge of the rectangle at y = 50 mm, halfway along. The line is 1.25 px wide with soft
    // edges, so no pixel need be the full colour; the pixels along it are plainly the operation's
    // colour rather than the mat's.
    let zoom = s.viewport().zoom();
    let cut = colour_of(OpKind::Cut);
    let distance = |p: [u8; 4], c: [u8; 3]| -> i32 {
        (0..3)
            .map(|i| (i32::from(p[i]) - i32::from(c[i])).abs())
            .sum()
    };
    let best = [-0.5, 0.0, 0.5]
        .iter()
        .map(|d| at(&s, &image, 100.0, 50.0 + d / zoom))
        .min_by_key(|p| distance(*p, cut))
        .unwrap();
    assert!(
        distance(best, cut) < distance(best, MAT_COLOUR),
        "{best:?} is nearer the cut colour than the mat"
    );
}

#[test]
fn a_filled_shape_has_a_faint_tint_inside() {
    let mut s = sized_state();
    draw_rect(&mut s, (50.0, 50.0), (150.0, 100.0));
    s.assign_operation(OpKind::Cut, "generic.drag-blade@1.0.0", 0);
    deselect(&mut s);
    let image = render(&s, 400, 300);
    let inside = at(&s, &image, 100.0, 75.0);
    assert!(!near(inside, MAT_COLOUR, 1), "tinted: {inside:?}");
    assert!(near(inside, MAT_COLOUR, 40), "but only faintly: {inside:?}");
}

#[test]
fn the_selection_is_drawn_in_the_accent_colour() {
    let mut s = sized_state();
    draw_rect(&mut s, (50.0, 50.0), (150.0, 100.0)); // selected after drawing
    let image = render(&s, 400, 300);
    let edge = at(&s, &image, 100.0, 50.0);
    assert!(near(edge, ACCENT, 8), "{edge:?}");
}

#[test]
fn a_score_line_is_dashed_so_it_has_gaps() {
    let mut s = sized_state();
    s.run_command("tool.line", 0);
    s.canvas_event(CanvasEvent::Press(Point::new(20.0, 200.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(Point::new(260.0, 200.0)), NONE, 0);
    s.assign_operation(OpKind::Score, "generic.scoring-stylus@1.0.0", 0);
    deselect(&mut s);
    let image = render(&s, 400, 300);
    let score = colour_of(OpKind::Score);
    let coloured = (0..200)
        .filter(|i| near(at(&s, &image, 20.0 + f64::from(*i) * 1.2, 200.0), score, 20))
        .count();
    assert!(
        coloured > 40 && coloured < 180,
        "some of the line is drawn and some is gap: {coloured} of 200"
    );
}

#[test]
fn the_scene_grows_with_the_content() {
    let mut s = sized_state();
    let empty = build_scene(&s, false).len();
    draw_rect(&mut s, (50.0, 50.0), (150.0, 100.0));
    assert!(build_scene(&s, false).len() > empty);
}

#[test]
fn a_guide_is_drawn_as_a_thin_line_across_the_mat() {
    let mut s = sized_state();
    s.add_guide(ps_model::GuideAxis::Horizontal, 150.0, 0);
    s.add_guide(ps_model::GuideAxis::Vertical, 100.0, 0);
    let image = render(&s, 400, 300);
    let h = at(&s, &image, 40.0, 150.0);
    let v = at(&s, &image, 100.0, 60.0);
    assert!(near(h, GUIDE, 12), "{h:?}");
    assert!(near(v, GUIDE, 12), "{v:?}");
    // Away from the guides the mat is untouched.
    assert!(near(at(&s, &image, 40.0, 60.0), MAT_COLOUR, 2));
}

#[test]
fn a_shape_whose_operations_are_switched_off_is_not_drawn() {
    let mut s = sized_state();
    draw_rect(&mut s, (50.0, 50.0), (150.0, 100.0));
    s.assign_operation(OpKind::Cut, "generic.drag-blade@1.0.0", 0);
    deselect(&mut s);
    let on = build_scene(&s, false).draw_count();
    let row = s.operation_rows()[0].id;
    assert!(s.set_operation_row_enabled(row, false, 1));
    let off = build_scene(&s, false).draw_count();
    assert!(
        off < on,
        "the outline is gone: {on} lines before, {off} after"
    );
}
