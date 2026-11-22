//! Ground truth for the GPU renderer: the scene is drawn offscreen and the pixels
//! are read back, so the checks are on what a person would see. Positions are millimetres, the
//! viewport is pixels per millimetre, and y points down. These run against whatever device the
//! machine has (a software one in CI); a machine with none is a failed test, not a
//! skipped one, because the renderer cannot be checked without one.

use ps_geometry::{Affine, FillRule, Path, Point};

use super::*;
use crate::mesh::{CapStyle, Mesh, fill_mesh, stroke_mesh};
use crate::viewport::Viewport;

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// One test at a time opens the device: several at once have been seen to stall a software
/// adapter.
static DEVICE: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Gpu {
    _turn: std::sync::MutexGuard<'static, ()>,
    context: GpuContext,
}

impl std::ops::Deref for Gpu {
    type Target = GpuContext;
    fn deref(&self) -> &GpuContext {
        &self.context
    }
}

fn gpu() -> Gpu {
    let turn = DEVICE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Gpu {
        _turn: turn,
        context: GpuContext::headless().expect("a graphics device (a software one is fine)"),
    }
}

/// 200 x 200 px, 10 px per mm, with the world point (10, 10) at the centre.
fn view() -> Viewport {
    Viewport::new(200.0, 200.0)
        .with_center(Point::new(10.0, 10.0))
        .with_zoom(10.0)
}

fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * image.width + x) * 4) as usize;
    [
        image.rgba[i],
        image.rgba[i + 1],
        image.rgba[i + 2],
        image.rgba[i + 3],
    ]
}

fn close(a: [u8; 4], b: [u8; 4], tolerance: u8) -> bool {
    a.iter().zip(&b).all(|(x, y)| x.abs_diff(*y) <= tolerance)
}

fn square(x: f64, y: f64, side: f64) -> Mesh {
    fill_mesh(&[Path::rect(x, y, side, side)], FillRule::NonZero, 0.01)
}

#[test]
fn an_empty_scene_is_the_clear_colour() {
    let g = gpu();
    let image = g.render(&Scene::new(view(), WHITE), 200, 200);
    assert_eq!((image.width, image.height), (200, 200));
    assert_eq!(image.rgba.len(), 200 * 200 * 4);
    assert!(close(pixel(&image, 0, 0), [255, 255, 255, 255], 1));
    assert!(close(pixel(&image, 100, 100), [255, 255, 255, 255], 1));
}

#[test]
fn a_filled_square_covers_exactly_its_millimetres() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    // A 10 mm square from (5, 5) to (15, 15): pixels 50..150 on a 10 px/mm screen.
    scene.fill(&square(5.0, 5.0, 10.0), Affine::IDENTITY, RED);
    let image = g.render(&scene, 200, 200);
    assert!(
        close(pixel(&image, 100, 100), [255, 0, 0, 255], 1),
        "the centre"
    );
    assert!(
        close(pixel(&image, 52, 52), [255, 0, 0, 255], 1),
        "just inside the corner"
    );
    assert!(
        close(pixel(&image, 147, 147), [255, 0, 0, 255], 1),
        "just inside the far corner"
    );
    assert!(
        close(pixel(&image, 47, 100), [255, 255, 255, 255], 1),
        "outside the left edge"
    );
    assert!(
        close(pixel(&image, 153, 100), [255, 255, 255, 255], 1),
        "outside the right edge"
    );
    assert!(
        close(pixel(&image, 100, 47), [255, 255, 255, 255], 1),
        "above"
    );
    assert!(
        close(pixel(&image, 100, 153), [255, 255, 255, 255], 1),
        "below"
    );
}

#[test]
fn an_objects_transform_places_its_mesh() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    // The mesh sits at the origin; the transform moves it to (5, 5).
    scene.fill(&square(0.0, 0.0, 10.0), Affine::translate((5.0, 5.0)), RED);
    let image = g.render(&scene, 200, 200);
    assert!(close(pixel(&image, 100, 100), [255, 0, 0, 255], 1));
    assert!(close(pixel(&image, 47, 100), [255, 255, 255, 255], 1));
}

#[test]
fn scaling_the_view_scales_the_picture() {
    let g = gpu();
    let mut scene = Scene::new(view().with_zoom(5.0), WHITE);
    scene.fill(&square(5.0, 5.0, 10.0), Affine::IDENTITY, RED);
    let image = g.render(&scene, 200, 200);
    // At 5 px/mm the square is 50 px wide, centred: pixels 75..125.
    assert!(close(pixel(&image, 100, 100), [255, 0, 0, 255], 1));
    assert!(close(pixel(&image, 72, 100), [255, 255, 255, 255], 1));
    assert!(close(pixel(&image, 128, 100), [255, 255, 255, 255], 1));
}

#[test]
fn later_draws_cover_earlier_ones() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    scene.fill(&square(5.0, 5.0, 10.0), Affine::IDENTITY, RED);
    scene.fill(
        &square(8.0, 8.0, 4.0),
        Affine::IDENTITY,
        [0.0, 0.0, 1.0, 1.0],
    );
    let image = g.render(&scene, 200, 200);
    assert!(
        close(pixel(&image, 100, 100), [0, 0, 255, 255], 1),
        "blue on top"
    );
    assert!(
        close(pixel(&image, 60, 100), [255, 0, 0, 255], 1),
        "red around it"
    );
}

#[test]
fn a_translucent_fill_blends_with_what_is_below() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    scene.fill(
        &square(5.0, 5.0, 10.0),
        Affine::IDENTITY,
        [1.0, 0.0, 0.0, 0.5],
    );
    let image = g.render(&scene, 200, 200);
    // Half red over white is a light red.
    let p = pixel(&image, 100, 100);
    assert!(close(p, [255, 128, 128, 255], 3), "{p:?}");
}

#[test]
fn a_stroke_draws_a_line_of_the_given_width() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    let line = Path::polyline(&[Point::new(2.0, 10.0), Point::new(18.0, 10.0)], false);
    // 2 mm wide: 20 px, from y = 90 to y = 110.
    scene.fill(
        &stroke_mesh(&line, 2.0, CapStyle::Butt, 0.01),
        Affine::IDENTITY,
        RED,
    );
    let image = g.render(&scene, 200, 200);
    assert!(close(pixel(&image, 100, 100), [255, 0, 0, 255], 1));
    assert!(close(pixel(&image, 100, 92), [255, 0, 0, 255], 1));
    assert!(close(pixel(&image, 100, 88), [255, 255, 255, 255], 1));
    assert!(close(pixel(&image, 100, 112), [255, 255, 255, 255], 1));
}

#[test]
fn an_empty_mesh_draws_nothing_and_does_no_harm() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    scene.fill(&Mesh::default(), Affine::IDENTITY, RED);
    let image = g.render(&scene, 200, 200);
    assert!(close(pixel(&image, 100, 100), [255, 255, 255, 255], 1));
}

#[test]
fn the_output_size_does_not_have_to_be_a_multiple_of_sixty_four() {
    let g = gpu();
    let image = g.render(&Scene::new(Viewport::new(37.0, 21.0), WHITE), 37, 21);
    assert_eq!(image.rgba.len(), 37 * 21 * 4);
    assert!(close(pixel(&image, 36, 20), [255, 255, 255, 255], 1));
}

#[test]
fn many_draws_in_one_scene_all_appear() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    let tile = square(0.0, 0.0, 1.0);
    for i in 0..10 {
        let x = f64::from(i) * 2.0;
        scene.fill(&tile, Affine::translate((x, 9.5)), RED);
    }
    let image = g.render(&scene, 200, 200);
    for i in 0..10_u32 {
        // Tile i spans x = 2i..2i+1 mm, so pixels 20i..20i+10; the view's left edge is at 0 mm.
        let px = i * 20 + 5;
        assert!(
            close(pixel(&image, px, 100), [255, 0, 0, 255], 1),
            "tile {i}"
        );
        assert!(
            close(pixel(&image, px + 10, 100), [255, 255, 255, 255], 1),
            "gap {i}"
        );
    }
}

#[test]
fn rendering_twice_gives_the_same_pixels() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    scene.fill(&square(5.0, 5.0, 10.0), Affine::IDENTITY, RED);
    assert_eq!(g.render(&scene, 200, 200), g.render(&scene, 200, 200));
}

#[test]
fn a_zero_sized_image_is_refused_rather_than_crashing() {
    let g = gpu();
    let image = g.render(&Scene::new(view(), WHITE), 0, 0);
    assert_eq!(image.rgba.len(), 0);
}

#[test]
fn colours_are_converted_for_an_srgb_target() {
    // 0.5 in sRGB is about 0.214 in linear light; the ends stay put.
    assert!((srgb_to_linear(0.5) - 0.214_041).abs() < 1e-5);
    assert!(srgb_to_linear(0.0).abs() < 1e-9);
    assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
    // The linear toe is a straight line.
    assert!((srgb_to_linear(0.04) - 0.04 / 12.92).abs() < 1e-6);
}

#[test]
fn an_srgb_scene_stores_converted_colours() {
    let mut scene = Scene::new(view(), [0.5, 0.5, 0.5, 1.0]).for_srgb_target();
    scene.fill(
        &square(5.0, 5.0, 10.0),
        Affine::IDENTITY,
        [0.5, 0.5, 0.5, 0.5],
    );
    assert!((scene.clear[0] - 0.214_041).abs() < 1e-5);
    assert!((scene.draws[0].colour[0] - 0.214_041).abs() < 1e-5);
    assert!(
        (scene.draws[0].colour[3] - 0.5).abs() < 1e-9,
        "alpha is not a colour"
    );
}

// ---- instanced line segments ----

fn seg(x0: f32, y0: f32, x1: f32, y1: f32) -> [f32; 4] {
    [x0, y0, x1, y1]
}

#[test]
fn a_line_segment_covers_its_width_with_soft_edges() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    // 2 mm wide at 10 px/mm is 20 px: y from 90 to 110 on the screen.
    scene.lines(&[seg(5.0, 10.0, 15.0, 10.0)], 2.0, RED);
    let image = g.render(&scene, 200, 200);
    assert!(close(pixel(&image, 100, 100), [255, 0, 0, 255], 1));
    assert!(close(pixel(&image, 100, 92), [255, 0, 0, 255], 2));
    assert!(close(pixel(&image, 100, 88), [255, 255, 255, 255], 2));
    assert!(close(pixel(&image, 100, 112), [255, 255, 255, 255], 2));
}

#[test]
fn the_ends_are_round() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    scene.lines(&[seg(5.0, 10.0, 15.0, 10.0)], 2.0, RED);
    let image = g.render(&scene, 200, 200);
    // The line runs from x = 50 to 150 px; a round cap of radius 10 px reaches 40 and 160.
    assert!(
        close(pixel(&image, 43, 100), [255, 0, 0, 255], 2),
        "inside the cap"
    );
    assert!(
        close(pixel(&image, 37, 100), [255, 255, 255, 255], 2),
        "past the cap"
    );
    // At the corner of the square the cap would have, nothing is drawn.
    assert!(
        close(pixel(&image, 43, 91), [255, 255, 255, 255], 30),
        "the cap is round, not square"
    );
}

#[test]
fn a_hairline_is_never_thinner_than_a_pixel() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    scene.lines(&[seg(5.0, 10.0, 15.0, 10.0)], 0.0001, [0.0, 0.0, 0.0, 1.0]);
    let image = g.render(&scene, 200, 200);
    let rows: Vec<u8> = (96..104).map(|y| pixel(&image, 100, y)[0]).collect();
    assert!(
        rows.iter().any(|v| *v < 200),
        "something dark within a pixel of the line: {rows:?}"
    );
}

#[test]
fn a_diagonal_segment_is_as_wide_across_as_asked() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    scene.lines(&[seg(5.0, 5.0, 15.0, 15.0)], 2.0, RED);
    let image = g.render(&scene, 200, 200);
    assert!(close(pixel(&image, 100, 100), [255, 0, 0, 255], 1));
    // The half width is 10 px across the line, which is 14.1 px straight down from the centre line:
    // 6 px down is inside, 18 px down is outside.
    assert!(close(pixel(&image, 100, 106), [255, 0, 0, 255], 2));
    assert!(close(pixel(&image, 100, 118), [255, 255, 255, 255], 2));
}

#[test]
fn many_segments_in_one_draw_all_appear() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    // Ten vertical lines 1.8 mm apart starting at x = 1 mm: pixels 10, 28, 46 ...
    let segments: Vec<[f32; 4]> = (0..10_u8)
        .map(|i| {
            let x = 1.0 + 1.8 * f32::from(i);
            seg(x, 4.0, x, 16.0)
        })
        .collect();
    scene.lines(&segments, 0.5, RED);
    let image = g.render(&scene, 200, 200);
    for i in 0..10_u32 {
        assert!(
            close(pixel(&image, 10 + 18 * i, 100), [255, 0, 0, 255], 2),
            "segment {i}"
        );
        assert!(
            close(pixel(&image, 19 + 18 * i, 100), [255, 255, 255, 255], 2),
            "gap {i}"
        );
    }
}

#[test]
fn a_translucent_line_blends_and_later_draws_cover_earlier_ones() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    scene.lines(&[seg(5.0, 10.0, 15.0, 10.0)], 2.0, [1.0, 0.0, 0.0, 0.5]);
    scene.fill(
        &square(8.0, 8.0, 4.0),
        Affine::IDENTITY,
        [0.0, 0.0, 1.0, 1.0],
    );
    let image = g.render(&scene, 200, 200);
    assert!(
        close(pixel(&image, 60, 100), [255, 128, 128, 255], 3),
        "{:?}",
        pixel(&image, 60, 100)
    );
    assert!(
        close(pixel(&image, 100, 100), [0, 0, 255, 255], 1),
        "the fill is on top"
    );
}

#[test]
fn zero_length_segments_draw_a_round_dot_and_no_segments_draw_nothing() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    scene.lines(&[seg(10.0, 10.0, 10.0, 10.0)], 2.0, RED);
    scene.lines(&[], 2.0, RED);
    let image = g.render(&scene, 200, 200);
    assert!(close(pixel(&image, 100, 100), [255, 0, 0, 255], 1));
    assert!(close(pixel(&image, 100, 115), [255, 255, 255, 255], 2));
}

#[test]
fn line_scenes_render_the_same_twice() {
    let g = gpu();
    let mut scene = Scene::new(view(), WHITE);
    scene.lines(&[seg(5.0, 5.0, 15.0, 12.0)], 1.0, RED);
    assert_eq!(g.render(&scene, 200, 200), g.render(&scene, 200, 200));
}

#[test]
fn the_scene_counts_a_line_batch_as_one_draw() {
    let mut scene = Scene::new(view(), WHITE);
    scene.lines(
        &[seg(0.0, 0.0, 1.0, 1.0), seg(2.0, 2.0, 3.0, 3.0)],
        1.0,
        RED,
    );
    assert_eq!(scene.len(), 1);
    scene.lines(&[], 1.0, RED);
    assert_eq!(scene.len(), 1, "an empty batch adds nothing");
}
