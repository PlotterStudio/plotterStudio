//! Frame-time benchmark: the canvas with 10,000 paths at 1080p and 4K, drawn offscreen
//! with the line shader for strokes and meshes for fills. The nightly run compares each result with
//! the last. The budget is 60 fps (16.7 ms) for the whole frame on an integrated GPU; a software
//! adapter is far slower and is only a regression yardstick.

use criterion::{Criterion, criterion_group, criterion_main};
use ps_geometry::{Affine, FillRule, Path, Point};
use ps_render::{GpuContext, Scene, Viewport, fill_mesh};

fn scene(width: f64, height: f64) -> Scene {
    // A 300 x 300 mm mat with 10,000 small rectangles on a 100 x 100 grid.
    let view = Viewport::new(width, height)
        .with_center(Point::new(150.0, 150.0))
        .with_zoom((height / 300.0).min(width / 300.0));
    let mut scene = Scene::new(view, [0.93, 0.93, 0.94, 1.0]);
    let tile = fill_mesh(&[Path::rect(0.0, 0.0, 2.0, 2.0)], FillRule::NonZero, 0.01);
    let mut segments = Vec::with_capacity(40_000);
    for i in 0..10_000_u32 {
        // Whole millimetres, so the conversion to the GPU's f32 is exact.
        let (col, row) = (
            f32::from(u16::try_from(i % 100).unwrap_or(0)) * 3.0,
            f32::from(u16::try_from(i / 100).unwrap_or(0)) * 3.0,
        );
        let (x0, y0, x1, y1) = (col, row, col + 2.0, row + 2.0);
        segments.extend([
            [x0, y0, x1, y0],
            [x1, y0, x1, y1],
            [x1, y1, x0, y1],
            [x0, y1, x0, y0],
        ]);
    }
    // One mesh draw per path would measure buffer creation, not drawing: the app batches fills
    // by colour, so a few hundred tiles stand in for the fills.
    for i in 0..200_u32 {
        let (col, row) = (f64::from(i % 20) * 15.0, f64::from(i / 20) * 15.0);
        scene.fill(&tile, Affine::translate((col, row)), [0.8, 0.2, 0.2, 0.15]);
    }
    scene.lines(&segments, 0.2, [0.8, 0.2, 0.2, 1.0]);
    scene
}

fn frames(c: &mut Criterion) {
    let Ok(gpu) = GpuContext::headless() else {
        return;
    };
    let mut group = c.benchmark_group("canvas_frame");
    group.sample_size(10);
    for (name, w, h) in [("1080p", 1920_u32, 1080_u32), ("4k", 3840, 2160)] {
        let scene = scene(f64::from(w), f64::from(h));
        group.bench_function(format!("10000_paths_{name}"), |b| {
            b.iter(|| gpu.render(&scene, w, h));
        });
    }
    group.finish();
}

criterion_group!(benches, frames);
criterion_main!(benches);
