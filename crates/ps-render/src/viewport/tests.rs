//! Ground truth for the canvas viewport: pan and zoom about the cursor, fit to a
//! rectangle, and true-scale ("100%" is the monitor's physical size, with a user calibration).
//! Positions are millimetres in the world, pixels on the screen, y down in both.

use ps_geometry::{Point, Rect};

use super::*;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn vp() -> Viewport {
    // 1000 x 800 px, 2 px per mm, world point (100, 50) at the centre.
    Viewport::new(1000.0, 800.0)
        .with_center(Point::new(100.0, 50.0))
        .with_zoom(2.0)
}

#[test]
fn the_centre_of_the_screen_shows_the_centre_point() {
    let v = vp();
    let s = v.world_to_screen(Point::new(100.0, 50.0));
    assert!(close(s.x, 500.0) && close(s.y, 400.0), "{s:?}");
}

#[test]
fn screen_distance_is_zoom_times_world_distance() {
    let v = vp();
    let s = v.world_to_screen(Point::new(110.0, 40.0));
    assert!(close(s.x, 520.0) && close(s.y, 380.0), "{s:?}");
}

#[test]
fn screen_and_world_convert_back_and_forth() {
    let v = vp();
    for p in [
        Point::new(0.0, 0.0),
        Point::new(-33.3, 71.2),
        Point::new(300.0, -10.0),
    ] {
        let back = v.screen_to_world(v.world_to_screen(p));
        assert!(
            close(back.x, p.x) && close(back.y, p.y),
            "{p:?} -> {back:?}"
        );
    }
}

#[test]
fn zooming_keeps_the_point_under_the_cursor_fixed() {
    let mut v = vp();
    let cursor = Point::new(700.0, 200.0);
    let before = v.screen_to_world(cursor);
    v.zoom_at(cursor, 1.5);
    assert!(close(v.zoom(), 3.0));
    let after = v.screen_to_world(cursor);
    assert!(
        close(before.x, after.x) && close(before.y, after.y),
        "{before:?} {after:?}"
    );
    v.zoom_at(cursor, 0.25);
    let again = v.screen_to_world(cursor);
    assert!(close(before.x, again.x) && close(before.y, again.y));
}

#[test]
fn zoom_is_limited_at_both_ends() {
    let mut v = vp();
    v.zoom_at(Point::new(500.0, 400.0), 1e9);
    assert!(close(v.zoom(), MAX_ZOOM));
    v.zoom_at(Point::new(500.0, 400.0), 1e-12);
    assert!(close(v.zoom(), MIN_ZOOM));
}

#[test]
fn a_clamped_zoom_still_keeps_the_cursor_point_fixed() {
    let mut v = vp();
    let cursor = Point::new(123.0, 456.0);
    let before = v.screen_to_world(cursor);
    v.zoom_at(cursor, 1e9);
    let after = v.screen_to_world(cursor);
    assert!(close(before.x, after.x) && close(before.y, after.y));
}

#[test]
fn panning_by_screen_pixels_moves_the_world_the_other_way() {
    let mut v = vp();
    v.pan_by_screen(40.0, -20.0);
    // Dragging the content right and up by (40, -20) px means the centre moves left and down in
    // the world by (20, -10) mm... content right = centre left.
    assert!(
        close(v.center().x, 80.0) && close(v.center().y, 60.0),
        "{:?}",
        v.center()
    );
}

#[test]
fn fitting_a_rectangle_centres_it_and_leaves_the_margin() {
    let mut v = Viewport::new(1000.0, 800.0);
    v.fit(Rect::new(0.0, 0.0, 300.0, 200.0), 50.0);
    // Available 900 x 700 px: width limits (900/300 = 3 px/mm; 700/200 = 3.5).
    assert!(close(v.zoom(), 3.0), "{}", v.zoom());
    assert!(close(v.center().x, 150.0) && close(v.center().y, 100.0));
    let tl = v.world_to_screen(Point::new(0.0, 0.0));
    assert!(close(tl.x, 50.0), "{tl:?}");
}

#[test]
fn fitting_an_empty_rectangle_does_nothing_odd() {
    let mut v = vp();
    v.fit(Rect::new(5.0, 5.0, 5.0, 5.0), 10.0);
    assert!(v.zoom().is_finite() && v.zoom() >= MIN_ZOOM && v.zoom() <= MAX_ZOOM);
    assert!(close(v.center().x, 5.0) && close(v.center().y, 5.0));
}

#[test]
fn the_visible_rectangle_is_what_the_screen_shows() {
    let v = vp();
    let r = v.visible_rect();
    assert!(
        close(r.x0, 100.0 - 250.0) && close(r.x1, 100.0 + 250.0),
        "{r:?}"
    );
    assert!(
        close(r.y0, 50.0 - 200.0) && close(r.y1, 50.0 + 200.0),
        "{r:?}"
    );
}

#[test]
fn a_millimetre_per_pixel_is_the_inverse_of_zoom() {
    assert!(close(vp().mm_per_px(), 0.5));
}

#[test]
fn true_scale_follows_the_monitors_dpi() {
    // 96 dpi: 96 px per 25.4 mm.
    let mut v = vp();
    v.set_true_scale(96.0, 1.0);
    assert!(close(v.zoom(), 96.0 / 25.4));
    // A user who measured the ruler 5% too long on screen calibrates with 1/1.05.
    v.set_true_scale(96.0, 1.0 / 1.05);
    assert!(close(v.zoom(), 96.0 / 25.4 / 1.05));
}

#[test]
fn true_scale_keeps_the_centre() {
    let mut v = vp();
    v.set_true_scale(144.0, 1.0);
    assert!(close(v.center().x, 100.0) && close(v.center().y, 50.0));
}

#[test]
fn resizing_the_window_keeps_the_centre_point() {
    let mut v = vp();
    v.resize(500.0, 400.0);
    let s = v.world_to_screen(Point::new(100.0, 50.0));
    assert!(close(s.x, 250.0) && close(s.y, 200.0));
}

#[test]
fn a_zero_sized_window_does_not_divide_by_zero() {
    let mut v = Viewport::new(0.0, 0.0);
    v.fit(Rect::new(0.0, 0.0, 10.0, 10.0), 5.0);
    assert!(v.zoom().is_finite());
    let _ = v.visible_rect();
}

#[test]
fn zoom_percent_reads_relative_to_true_scale() {
    let mut v = vp();
    v.set_true_scale(96.0, 1.0);
    assert!(close(v.percent_of_true_scale(96.0, 1.0), 100.0));
    v.zoom_at(Point::new(500.0, 400.0), 2.0);
    assert!(close(v.percent_of_true_scale(96.0, 1.0), 200.0));
}
