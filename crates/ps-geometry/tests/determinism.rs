//! Regression golden for the compile path.
//!
//! This is NOT ground truth: the constant below is the hash of what this code produced when it
//! was recorded. It exists so that any operating system or toolchain that produces different
//! bytes fails loudly. CI runs it on every platform in its matrix; if a deliberate algorithm
//! change alters the output, re-record the constant in the same commit and say why.
//!
//! What it covers: everything that feeds G-code coordinates (flattening, integer booleans,
//! integer offsets, fills, medial axes, arc-exact offsets, curve fits). It cannot cover the
//! platform maths inside `i_overlay` (round joins) and `cavalier_contours` (arcs); those are
//! quantised to whole µm, and this hash is how a one-µm difference would be noticed.

use ps_core::{ContentHash, PtUm, StableHasher};
use ps_geometry::{
    BoolOp, Cap, FillRule, FillSpec, Join, Path, Pattern, Point, Ring, Shape, boolean,
    cubic_to_arcs, fill, flatten_to_grid, generate, medial_axis, normalize, offset_arcs,
    offset_shapes, refit_ring, stroke_outline, thin_parts,
};

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn ring(x0: i64, y0: i64, x1: i64, y1: i64) -> Ring {
    vec![
        PtUm::new(x0, y0),
        PtUm::new(x1, y0),
        PtUm::new(x1, y1),
        PtUm::new(x0, y1),
    ]
}

fn feed_points(h: &mut StableHasher, label: &str, points: &[PtUm]) {
    h.write_str(label);
    h.write_u64(points.len() as u64);
    for p in points {
        h.write_i64(p.x);
        h.write_i64(p.y);
    }
}

fn feed_shapes(h: &mut StableHasher, label: &str, shapes: &[Shape]) {
    h.write_str(label);
    for shape in shapes {
        for r in shape {
            feed_points(h, "ring", r);
        }
    }
}

fn feed_paths(h: &mut StableHasher, label: &str, paths: &[Path]) {
    h.write_str(label);
    for p in paths {
        h.write_f64(p.start.x);
        h.write_f64(p.start.y);
        for (_, seg) in p.segments() {
            let to = seg.to();
            h.write_i64(ps_core::um_from_mm(to.x).unwrap());
            h.write_i64(ps_core::um_from_mm(to.y).unwrap());
        }
    }
}

fn recorded_hash() -> ContentHash {
    let mut h = StableHasher::new();

    // Flattening: arcs and cubics to the µm grid.
    let circle = Path::circle(pt(5.0, -3.0), 12.345);
    feed_points(&mut h, "circle", &flatten_to_grid(&circle, 0.002).unwrap());
    let curve = Path::new(pt(0.0, 0.0))
        .cubic_to(pt(0.0, 5.0), pt(10.0, -5.0), pt(10.0, 0.0))
        .line_to(pt(3.0, 4.0));
    feed_points(&mut h, "curve", &flatten_to_grid(&curve, 0.002).unwrap());

    // Integer booleans.
    let a = vec![ring(0, 0, 4_000, 4_000)];
    let b = vec![ring(2_000, 2_000, 6_000, 6_000)];
    for op in [
        BoolOp::Union,
        BoolOp::Intersect,
        BoolOp::Difference,
        BoolOp::Xor,
    ] {
        feed_shapes(&mut h, "boolean", &boolean(&a, &b, op, FillRule::NonZero));
    }

    // Offsets and strokes on the grid.
    let square = vec![vec![ring(0, 0, 10_000, 10_000)]];
    for join in [Join::Round, Join::Bevel, Join::Miter { limit: 4.0 }] {
        feed_shapes(&mut h, "grow", &offset_shapes(&square, 1_000, join));
        feed_shapes(&mut h, "shrink", &offset_shapes(&square, -1_000, join));
    }
    let line = [
        PtUm::new(0, 0),
        PtUm::new(10_000, 3_000),
        PtUm::new(20_000, 0),
    ];
    feed_shapes(
        &mut h,
        "stroke",
        &stroke_outline(&line, 800, false, Cap::Round, Join::Round),
    );

    // Fills at an angle that exercises the float rotation.
    let region = normalize(&[ring(0, 0, 10_000, 10_000)], FillRule::NonZero);
    for pattern in [
        Pattern::Parallel,
        Pattern::Cross,
        Pattern::ZigZag,
        Pattern::Concentric,
    ] {
        let spec = FillSpec {
            pattern,
            angle_deg: 37.0,
            overlap: 0.15,
            outline_pass: true,
        };
        for stroke in generate(&region, 0.4, &spec) {
            feed_points(&mut h, "fill", &stroke);
        }
    }

    // Thin parts and the medial axis.
    let tail = normalize(
        &[
            ring(0, 0, 10_000, 10_000),
            ring(10_000, 4_850, 13_000, 5_150),
        ],
        FillRule::NonZero,
    );
    for stroke in thin_parts(&tail, 0.4).strokes {
        feed_points(&mut h, "thin", &stroke);
    }
    let spec = FillSpec {
        pattern: Pattern::Parallel,
        angle_deg: 0.0,
        overlap: 0.15,
        outline_pass: false,
    };
    h.write_u64(fill(&tail, 0.4, &spec).strokes.len() as u64);
    let axis = medial_axis(&[ring(0, 0, 10_000, 300)], 2.0).unwrap();
    for chain in axis.chains {
        feed_points(&mut h, "axis", &chain);
    }

    // Arc-exact offsets and curve fitting.
    feed_paths(
        &mut h,
        "offset-circle",
        &offset_arcs(&Path::circle(pt(0.0, 0.0), 10.0), 1.0, 0.002),
    );
    feed_paths(
        &mut h,
        "offset-rect",
        &offset_arcs(&Path::rect(0.0, 0.0, 10.0, 10.0), 1.0, 0.002),
    );
    let segs = cubic_to_arcs(
        pt(0.0, 0.0),
        pt(0.0, 5.0),
        pt(10.0, -5.0),
        pt(10.0, 0.0),
        0.002,
    );
    let fitted = segs.iter().fold(Path::new(pt(0.0, 0.0)), |p, s| match *s {
        ps_geometry::Seg::Line { to } => p.line_to(to),
        ps_geometry::Seg::Arc { to, center, ccw } => p.arc_to(to, center, ccw),
        ps_geometry::Seg::Cubic { c1, c2, to } => p.cubic_to(c1, c2, to),
    });
    feed_paths(&mut h, "biarc", &[fitted]);
    let ring_pts = ps_geometry::flatten(&Path::circle(pt(0.0, 0.0), 10.0), 0.002);
    feed_paths(&mut h, "refit", &[refit_ring(&ring_pts, 0.002)]);

    h.finish()
}

/// Recorded on Windows with Rust 1.100.0 on 2026-11-17, re-recorded 2026-11-19 when hatch rows became evenly spaced edge to edge (regression golden, see module docs).
const GOLDEN: &str = "d24c8d2d67815a5ac663ab775a2c77299ebcd5646c6cc8b337a0fd791e200409";

#[test]
fn compile_path_output_matches_the_recorded_golden() {
    let hash = recorded_hash();
    assert_eq!(
        hash.to_string(),
        GOLDEN,
        "the compile path now produces different bytes"
    );
}

#[test]
fn compile_path_output_is_repeatable_in_one_process() {
    assert_eq!(recorded_hash(), recorded_hash());
}
