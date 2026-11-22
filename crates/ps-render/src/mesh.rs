//! Triangle meshes for fills and strokes: lyon tessellation in object space,
//! millimetres, cached per path by the app and instanced by transform.

use lyon_tessellation::geom::point;
use lyon_tessellation::path::Path as LyonPath;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule as LyonFillRule, FillTessellator, FillVertex, LineCap,
    LineJoin, StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers,
};
use ps_geometry::{FillRule, Path, Point, flatten};

/// How the ends of an open stroke look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapStyle {
    /// Cut off square at the end point.
    Butt,
    /// A half disc past the end point.
    Round,
}

/// Triangles with 2D positions in millimetres.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// Vertex positions.
    pub vertices: Vec<[f32; 2]>,
    /// Triangle corner indices, three per triangle.
    pub indices: Vec<u32>,
}

impl Mesh {
    /// The mesh scaled about the origin, for showing a cached mesh at a nearby zoom.
    #[must_use]
    pub fn scaled(&self, factor: f64) -> Self {
        let k = f32_of(factor);
        Self {
            vertices: self.vertices.iter().map(|v| [v[0] * k, v[1] * k]).collect(),
            indices: self.indices.clone(),
        }
    }

    /// The bounds as `(x0, y0, x1, y1)`; `None` for an empty mesh.
    #[must_use]
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let first = self.vertices.first()?;
        let mut b = (
            f64::from(first[0]),
            f64::from(first[1]),
            f64::from(first[0]),
            f64::from(first[1]),
        );
        for v in &self.vertices {
            let (x, y) = (f64::from(v[0]), f64::from(v[1]));
            b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
        }
        Some(b)
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "GPU meshes use f32; millimetre coordinates fit with room to spare"
)]
fn f32_of(v: f64) -> f32 {
    v as f32
}

/// A lyon path through the flattened points of `path`; `None` when there is nothing to draw.
fn lyon_path(paths: &[Path], tol: f64, force_close: bool) -> Option<LyonPath> {
    let mut builder = LyonPath::builder();
    let mut any = false;
    for path in paths {
        let points: Vec<Point> = flatten(path, tol);
        if points.len() < 2 {
            continue;
        }
        any = true;
        let mut iter = points.iter();
        if let Some(p) = iter.next() {
            builder.begin(point(f32_of(p.x), f32_of(p.y)));
        }
        for p in iter {
            builder.line_to(point(f32_of(p.x), f32_of(p.y)));
        }
        builder.end(force_close || path.closed);
    }
    any.then(|| builder.build())
}

fn mesh_of(buffers: VertexBuffers<[f32; 2], u32>) -> Mesh {
    Mesh {
        vertices: buffers.vertices,
        indices: buffers.indices,
    }
}

/// Fills the regions bounded by `paths`; open paths are closed. Flattening stays within `tol` mm.
#[must_use]
pub fn fill_mesh(paths: &[Path], rule: FillRule, tol: f64) -> Mesh {
    let Some(path) = lyon_path(paths, tol, true) else {
        return Mesh::default();
    };
    let options = FillOptions::tolerance(f32_of(tol)).with_fill_rule(match rule {
        FillRule::NonZero => LyonFillRule::NonZero,
        FillRule::EvenOdd => LyonFillRule::EvenOdd,
    });
    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let result = FillTessellator::new().tessellate_path(
        &path,
        &options,
        &mut BuffersBuilder::new(&mut buffers, |v: FillVertex<'_>| v.position().to_array()),
    );
    if result.is_err() {
        return Mesh::default();
    }
    mesh_of(buffers)
}

/// The outline of `path` drawn `width` mm wide, with mitred corners and the given end caps.
#[must_use]
pub fn stroke_mesh(path: &Path, width: f64, cap: CapStyle, tol: f64) -> Mesh {
    if width <= 0.0 {
        return Mesh::default();
    }
    let Some(lyon) = lyon_path(std::slice::from_ref(path), tol, false) else {
        return Mesh::default();
    };
    let line_cap = match cap {
        CapStyle::Butt => LineCap::Butt,
        CapStyle::Round => LineCap::Round,
    };
    let options = StrokeOptions::tolerance(f32_of(tol))
        .with_line_width(f32_of(width))
        .with_line_cap(line_cap)
        .with_line_join(LineJoin::Miter);
    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let result = StrokeTessellator::new().tessellate_path(
        &lyon,
        &options,
        &mut BuffersBuilder::new(&mut buffers, |v: StrokeVertex<'_, '_>| {
            v.position().to_array()
        }),
    );
    if result.is_err() {
        return Mesh::default();
    }
    mesh_of(buffers)
}

/// A filled disc of `diameter` mm at a point, for paths too small to see.
#[must_use]
pub fn dot_mesh(x: f64, y: f64, diameter: f64, tol: f64) -> Mesh {
    fill_mesh(
        &[Path::circle(Point::new(x, y), diameter / 2.0)],
        FillRule::NonZero,
        tol,
    )
}

#[cfg(test)]
mod tests;
