//! Calibration jobs.
//!
//! Each feature generates a small project that runs through the same compiler as production
//! work, so what the user tests is exactly what production uses. The projects use a 12 × 12 in
//! mat and stay well inside every supported work area.

use std::sync::Arc;

use ps_core::{LibRef, MatId, ObjectId, OpId, OpRowId, PathKey};
use ps_geometry::{Affine, FillRule, Path, Point};
use ps_model::{
    Document, Mat, MatSize, OpKind, Operation, ParamKey, ParamOverrides, ParamValue, PlacedObject,
    ProjectMeta, ShapePath, SourceStyle, ToolSlotRef,
};

// ---- a single-stroke font for labels ----

/// Glyph cell size in grid units: two wide, four tall, with a one-unit gap between glyphs.
const GLYPH_H: f64 = 4.0;
const ADVANCE: f64 = 3.0;

type Seg = [(f64, f64); 2];

const A: Seg = [(0.0, 4.0), (2.0, 4.0)];
const B: Seg = [(2.0, 4.0), (2.0, 2.0)];
const C: Seg = [(2.0, 2.0), (2.0, 0.0)];
const D: Seg = [(0.0, 0.0), (2.0, 0.0)];
const E: Seg = [(0.0, 2.0), (0.0, 0.0)];
const F: Seg = [(0.0, 4.0), (0.0, 2.0)];
const G: Seg = [(0.0, 2.0), (2.0, 2.0)];

/// The segments of a character in a seven-segment style, on a 2 × 4 grid; `None` when the font
/// has no such glyph.
#[must_use]
pub fn glyph(c: char) -> Option<Vec<[(f64, f64); 2]>> {
    Some(match c {
        '0' => vec![A, B, C, D, E, F],
        '1' => vec![B, C],
        '2' => vec![A, B, G, E, D],
        '3' => vec![A, B, G, C, D],
        '4' => vec![F, G, B, C],
        '5' => vec![A, F, G, C, D],
        '6' => vec![A, F, G, E, D, C],
        '7' => vec![A, B, C],
        '8' => vec![A, B, C, D, E, F, G],
        '9' => vec![A, B, C, D, F, G],
        '-' => vec![G],
        '+' => vec![G, [(1.0, 1.0), (1.0, 3.0)]],
        _ => return None,
    })
}

/// Lays text out left to right as one open stroke per glyph segment. `origin` is the bottom-left
/// corner (in mm) and `height_mm` the glyph height. Unknown characters take space but draw
/// nothing.
#[must_use]
pub fn text_strokes(text: &str, origin: Point, height_mm: f64) -> Vec<Path> {
    let scale = height_mm / GLYPH_H;
    let mut out = Vec::new();
    for (i, c) in text.chars().enumerate() {
        let Some(segments) = glyph(c) else {
            continue;
        };
        #[expect(
            clippy::cast_precision_loss,
            reason = "a label has a handful of characters"
        )]
        let left = origin.x + i as f64 * ADVANCE * scale;
        for [(x0, y0), (x1, y1)] in segments {
            out.push(Path::polyline(
                &[
                    Point::new(left + x0 * scale, origin.y + y0 * scale),
                    Point::new(left + x1 * scale, origin.y + y1 * scale),
                ],
                false,
            ));
        }
    }
    out
}

// ---- project building ----

/// A project under construction: one mat, objects added with their operations.
struct Builder {
    doc: Document,
    mat: Mat,
    counter: u32,
}

impl Builder {
    fn new(name: &str) -> Self {
        Self {
            doc: Document::new(ProjectMeta::new(name, "calibration", "ps-compiler")),
            mat: Mat::new(MatId::new(), name, MatSize::inches(12.0, 12.0)),
            counter: 0,
        }
    }

    fn row() -> OpRowId {
        OpRowId::new()
    }

    fn op(tool: &LibRef, kind: OpKind, row: OpRowId, force: Option<f64>) -> Operation {
        let mut overrides = ParamOverrides::default();
        if let Some(f) = force {
            overrides.set(ParamKey::new("force"), ParamValue::Number(f));
        }
        Operation {
            id: OpId::new(),
            kind,
            tool: ToolSlotRef {
                tool: tool.clone(),
                pen_colour: None,
            },
            overrides,
            fill: None,
            enabled: true,
            row,
        }
    }

    /// Adds a path with its operations as its own object.
    fn add(&mut self, geom: Path, ops: Vec<Operation>) {
        self.counter += 1;
        let path = ShapePath {
            key: PathKey::source(format!("cal-{}", self.counter)),
            geom,
            fill_rule: FillRule::NonZero,
            style: SourceStyle::default(),
            ops,
            start_locked: false,
        };
        let object = PlacedObject {
            id: ObjectId::new(),
            source: None,
            transform: Affine::IDENTITY,
            group: None,
            attach: None,
            paths: Arc::new(vec![Arc::new(path)]),
            locked: false,
            hidden: false,
        };
        Arc::make_mut(&mut self.mat.objects).insert(object.id, Arc::new(object));
    }

    fn finish(mut self) -> Document {
        let mut rows: Vec<OpRowId> = Vec::new();
        for o in self.mat.objects.values() {
            for p in o.paths.iter() {
                for op in &p.ops {
                    if !rows.contains(&op.row) {
                        rows.push(op.row);
                    }
                }
            }
        }
        self.mat.op_order = Arc::new(rows);
        Arc::make_mut(&mut self.doc.mats).insert(self.mat.id, Arc::new(self.mat));
        self.doc
    }
}

fn polygon(points: &[(f64, f64)]) -> Path {
    let pts: Vec<Point> = points.iter().map(|&(x, y)| Point::new(x, y)).collect();
    Path::polyline(&pts, true)
}

fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Path {
    Path::polyline(&[Point::new(x0, y0), Point::new(x1, y1)], false)
}

/// The test cut: a 10 mm square inside a triangle, both cut. The user rates the result per
/// preset.
#[must_use]
pub fn test_cut(blade: &LibRef) -> Document {
    let mut b = Builder::new("Test cut");
    let row = Builder::row();
    let side = 40.0;
    let height = side * 3.0_f64.sqrt() / 2.0;
    b.add(
        polygon(&[(20.0, 20.0), (20.0 + side, 20.0), (40.0, 20.0 + height)]),
        vec![Builder::op(blade, OpKind::Cut, row, None)],
    );
    b.add(
        Path::rect(35.0, 25.0, 10.0, 10.0),
        vec![Builder::op(blade, OpKind::Cut, row, None)],
    );
    b.finish()
}

/// The pressure ladder: six 15 mm squares with the force stepped from -30 % to +30 % of
/// `base_force_gf`. A drawing tool also labels each square with its percentage.
#[must_use]
pub fn pressure_ladder(tool: &LibRef, kind: OpKind, base_force_gf: f64) -> Document {
    let mut b = Builder::new("Pressure ladder");
    let shapes = Builder::row();
    let labels = Builder::row();
    for i in 0..6_u32 {
        let step = f64::from(i);
        let percent = -30.0 + 12.0 * step;
        let force = base_force_gf * (1.0 + percent / 100.0);
        let left = 15.0 + 25.0 * step;
        b.add(
            Path::rect(left, 20.0, 15.0, 15.0),
            vec![Builder::op(tool, kind, shapes, Some(force))],
        );
        if kind == OpKind::Draw {
            let text = format!("{percent:+.0}");
            for stroke in text_strokes(&text, Point::new(left, 40.0), 4.0) {
                b.add(stroke, vec![Builder::op(tool, kind, labels, None)]);
            }
        }
    }
    b.finish()
}

/// The tip width ladder: 19 pairs of 20 mm lines at 0.10 to 1.00 mm spacing in 0.05 mm steps.
#[must_use]
pub fn tip_width_ladder(pen: &LibRef) -> Document {
    let mut b = Builder::new("Tip width ladder");
    let row = Builder::row();
    for k in 0..19_u32 {
        let step = f64::from(k);
        let spacing = 0.10 + 0.05 * step;
        let y = 20.0 + 3.0 * step;
        b.add(
            line(20.0, y, 40.0, y),
            vec![Builder::op(pen, OpKind::Draw, row, None)],
        );
        b.add(
            line(20.0, y + spacing, 40.0, y + spacing),
            vec![Builder::op(pen, OpKind::Draw, row, None)],
        );
    }
    b.finish()
}

/// The blade offset job: a 20 mm circle and a 20 mm square, cut; the user measures the error.
#[must_use]
pub fn blade_offset(blade: &LibRef) -> Document {
    let mut b = Builder::new("Blade offset");
    let row = Builder::row();
    b.add(
        Path::circle(Point::new(40.0, 40.0), 10.0),
        vec![Builder::op(blade, OpKind::Cut, row, None)],
    );
    b.add(
        Path::rect(70.0, 30.0, 20.0, 20.0),
        vec![Builder::op(blade, OpKind::Cut, row, None)],
    );
    b.finish()
}

/// The pen-to-blade job: a crosshair drawn and a crosshair cut at the same place; the user
/// enters the measured offset between them.
#[must_use]
pub fn holder_offset(pen: &LibRef, blade: &LibRef) -> Document {
    let mut b = Builder::new("Holder offset");
    let draw = Builder::row();
    let cut = Builder::row();
    for (tool, kind, row) in [(pen, OpKind::Draw, draw), (blade, OpKind::Cut, cut)] {
        b.add(
            line(45.0, 50.0, 55.0, 50.0),
            vec![Builder::op(tool, kind, row, None)],
        );
        b.add(
            line(50.0, 45.0, 50.0, 55.0),
            vec![Builder::op(tool, kind, row, None)],
        );
    }
    b.finish()
}

/// The time reference job: a fixed drawing the user times, to fit the machine's `time_scale`.
#[must_use]
pub fn time_reference(pen: &LibRef) -> Document {
    let mut b = Builder::new("Time reference");
    let row = Builder::row();
    let draw = || vec![Builder::op(pen, OpKind::Draw, row, None)];
    b.add(Path::rect(20.0, 20.0, 100.0, 100.0), draw());
    b.add(line(20.0, 20.0, 120.0, 120.0), draw());
    b.add(line(120.0, 20.0, 20.0, 120.0), draw());
    b.add(Path::circle(Point::new(70.0, 70.0), 40.0), draw());
    b.finish()
}

#[cfg(test)]
mod tests;
