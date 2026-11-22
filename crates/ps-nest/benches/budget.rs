//! The nesting budget: fifty objects nest in under ten seconds. The objects are
//! rectangles, squares and hexagons of assorted sizes in a 12 x 12 inch bin, with quarter-turn
//! rotations and a few alternative orders tried after the greedy one.

use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use ps_core::PtUm;
use ps_nest::{NestBin, NestItem, NestParams, Rotations, nest};

const MM: i64 = 1_000;

fn rect(w: i64, h: i64) -> Vec<PtUm> {
    vec![
        PtUm::new(0, 0),
        PtUm::new(w * MM, 0),
        PtUm::new(w * MM, h * MM),
        PtUm::new(0, h * MM),
    ]
}

fn hexagon(r: i64) -> Vec<PtUm> {
    // A regular hexagon with circumradius r mm, counter-clockwise, from integer-friendly points.
    let (a, b) = (r * MM, r * MM * 866 / 1000);
    vec![
        PtUm::new(a, 0),
        PtUm::new(a / 2, b),
        PtUm::new(-a / 2, b),
        PtUm::new(-a, 0),
        PtUm::new(-a / 2, -b),
        PtUm::new(a / 2, -b),
    ]
}

fn items(count: u32) -> Vec<NestItem> {
    (0..count)
        .map(|i| {
            let size = 15 + i64::from(i % 7) * 4;
            let outline = match i % 3 {
                0 => rect(size, size + 8),
                1 => rect(size, size),
                _ => hexagon(size / 2 + 4),
            };
            NestItem {
                id: i,
                outline: vec![outline],
                rotations: Rotations::Quarter,
            }
        })
        .collect()
}

fn nesting(c: &mut Criterion) {
    let side = 304_800;
    let bin = NestBin {
        outline: vec![
            PtUm::new(0, 0),
            PtUm::new(side, 0),
            PtUm::new(side, side),
            PtUm::new(0, side),
        ],
        keep_outs: Vec::new(),
    };
    let items = items(50);
    let params = NestParams {
        spacing_um: 2_000,
        seed: 1,
        iterations: 4,
    };
    let mut group = c.benchmark_group("nesting");
    group
        .sample_size(10)
        .measurement_time(Duration::from_secs(30));
    group.bench_function("nest_50_objects (budget 10 s)", |b| {
        b.iter(|| {
            let result = nest(std::slice::from_ref(&bin), &items, &params, &|| false);
            assert!(
                result.placements.len() > 25,
                "{} placed",
                result.placements.len()
            );
            result
        });
    });
    group.finish();
}

criterion_group!(benches, nesting);
criterion_main!(benches);
