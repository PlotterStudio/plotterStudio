//! The canvas as GPU draws: the mat, grid, and every object with its operation's colour
//! and line style. Fills are lyon meshes in object space, strokes are batches of line segments (the dashed
//! pieces) drawn by the line shader; the selection and tool previews are drawn over this by egui's painter.

use ps_geometry::{Affine, FillRule, Path, flatten};
use ps_render::{
    Mesh, Scene, colour_of, dash, dash_for_zoom, dash_pattern_px, fill_mesh, flatten_tolerance_mm,
    style_of,
};

use crate::state::AppState;

/// The mat's colour.
pub const MAT_COLOUR: [u8; 3] = [0xFF, 0xFF, 0xFF];
/// The selection colour.
pub const ACCENT: [u8; 3] = [0xB2, 0x47, 0x00];
/// The guide line colour.
pub const GUIDE: [u8; 3] = [0x00, 0xA8, 0xC8];
const NEUTRAL: [u8; 3] = [60, 60, 60];
/// The frame around the mat.
const FRAME_COLOUR: [u8; 3] = [0xDF, 0xE3, 0xEC];
const FRAME_PX: f64 = 12.0;
const GRID_MM: f64 = 10.0;
const MIN_GRID_PX: f64 = 8.0;
const STROKE_PX: f64 = 1.25;
const SELECTED_STROKE_PX: f64 = 2.0;

#[expect(clippy::cast_possible_truncation, reason = "the GPU works in f32")]
fn f32_of(v: f64) -> f32 {
    v as f32
}

fn rgba(c: [u8; 3], alpha: f32) -> [f32; 4] {
    let f = |v: u8| f32::from(v) / 255.0;
    [f(c[0]), f(c[1]), f(c[2]), alpha]
}

fn rect_mesh(x0: f64, y0: f64, x1: f64, y1: f64) -> Mesh {
    fill_mesh(
        &[Path::rect(x0, y0, x1 - x0, y1 - y0)],
        FillRule::NonZero,
        0.01,
    )
}

/// Draws the mat's guide lines as one-pixel lines across the whole view.
fn add_guides(scene: &mut Scene, mat: &ps_model::Mat, zoom: f64) {
    let reach = 100_000.0;
    for guide in mat.guides.iter() {
        let half = 0.5 / zoom;
        let (x0, y0, x1, y1) = match guide.axis {
            ps_model::GuideAxis::Horizontal => {
                (-reach, guide.at_mm - half, reach, guide.at_mm + half)
            }
            ps_model::GuideAxis::Vertical => {
                (guide.at_mm - half, -reach, guide.at_mm + half, reach)
            }
        };
        scene.fill(
            &rect_mesh(x0, y0, x1, y1),
            Affine::IDENTITY,
            rgba(GUIDE, 1.0),
        );
    }
}

/// How faint everything outside the highlight is drawn.
const DIMMED: f32 = 0.2;

/// Draws every visible object: a faint fill for closed shapes, and strokes batched by colour and
/// width in the operation's line style, with the selection in the accent colour. Objects a gesture
/// is moving are drawn where the gesture has put them; the paths of a highlighted operation or
/// layers row are drawn heavier and everything else steps back.
fn add_objects(scene: &mut Scene, state: &AppState, mat: &ps_model::Mat, zoom: f64) {
    let selected = state.selection().objects();
    let live = state.live_transform();
    let row = state.highlight_row();
    let layer = state.highlight_layer();
    let highlighting = row.is_some() || layer.is_some();
    let tol = flatten_tolerance_mm(zoom);
    let mut batches: std::collections::BTreeMap<([u8; 3], u32, u32), Vec<[f32; 4]>> =
        std::collections::BTreeMap::new();
    for object in mat.objects.values().filter(|o| !o.hidden) {
        let object_selected = selected.contains(&object.id);
        let transform = match &live {
            Some((ids, delta)) if ids.contains(&object.id) => *delta * object.transform,
            _ => object.transform,
        };
        // Closed paths of one object fill together, so a counter cuts a hole in its letter instead
        // of being filled on top of it.
        let mut fills: Vec<([u8; 3], FillRule, f32, Vec<Path>)> = Vec::new();
        for path in object.paths.iter() {
            let is_selected = state.selection().includes_path(object.id, &path.key);
            let kind = path.ops.first().map(|op| op.kind);
            let base = if is_selected {
                ACCENT
            } else {
                kind.map_or(NEUTRAL, colour_of)
            };
            let in_layer = layer.is_some_and(|(id, key)| {
                *id == object.id && key.as_ref().is_none_or(|key| *key == path.key)
            });
            let in_highlight =
                in_layer || row.is_some_and(|row| path.ops.iter().any(|op| op.row == row));
            let all_off = !path.ops.is_empty() && path.ops.iter().all(|op| !op.enabled);
            // The eye hides a shape whose every operation is off, unless its object is selected
            // or it is pointed at in the layers panel, so that it can still be seen and picked.
            if all_off && !object_selected && !in_layer {
                continue;
            }
            let alpha = if highlighting && !in_highlight {
                DIMMED
            } else {
                1.0
            };
            if path.geom.closed {
                if let Some(group) = fills.iter_mut().find(|(c, r, a, _)| {
                    *c == base && *r == path.fill_rule && (*a - alpha).abs() < f32::EPSILON
                }) {
                    group.3.push(path.geom.clone());
                } else {
                    fills.push((base, path.fill_rule, alpha, vec![path.geom.clone()]));
                }
            }
            let width = f32_of(
                (if is_selected {
                    SELECTED_STROKE_PX
                } else {
                    STROKE_PX
                }) / zoom,
            );
            let world = path.geom.transformed(transform);
            let pattern = kind.map_or_else(Vec::new, |k| dash_pattern_px(style_of(k)));
            let mut add = |width: f32, alpha: f32| {
                let batch = batches
                    .entry((base, width.to_bits(), alpha.to_bits()))
                    .or_default();
                for piece in dash(&world, &dash_for_zoom(&pattern, zoom)) {
                    let points = flatten(&piece, tol);
                    for pair in points.windows(2) {
                        batch.push([
                            f32_of(pair[0].x),
                            f32_of(pair[0].y),
                            f32_of(pair[1].x),
                            f32_of(pair[1].y),
                        ]);
                    }
                    if piece.closed && points.len() > 2 {
                        let (last, first) = (points[points.len() - 1], points[0]);
                        batch.push([
                            f32_of(last.x),
                            f32_of(last.y),
                            f32_of(first.x),
                            f32_of(first.y),
                        ]);
                    }
                }
            };
            // A highlighted path is drawn as a crisp, heavier line, not a glow.
            add(if in_highlight { width * 2.2 } else { width }, alpha);
        }
        for (base, rule, alpha, paths) in fills {
            let mesh = fill_mesh(&paths, rule, tol);
            scene.fill(&mesh, transform, rgba(base, 0.15 * alpha));
        }
    }
    // One instanced draw per stroke colour, width and strength, with round ends from the line shader.
    for ((colour, width_bits, alpha_bits), segments) in &batches {
        scene.lines(
            segments,
            f32::from_bits(*width_bits),
            rgba(*colour, f32::from_bits(*alpha_bits)),
        );
    }
}

/// Builds the scene for the current state; `dark` picks the background.
#[must_use]
pub fn build_scene(state: &AppState, dark: bool) -> Scene {
    build_scene_at(state, dark, 1.0)
}

/// Like [`build_scene`] for a target with `scale` physical pixels per logical pixel.
#[must_use]
pub fn build_scene_at(state: &AppState, dark: bool, scale: f64) -> Scene {
    let background = if dark {
        [0.082, 0.090, 0.184, 1.0]
    } else {
        [0.925, 0.933, 0.957, 1.0]
    };
    let viewport = state.viewport().with_zoom(state.viewport().zoom() * scale);
    let zoom = viewport.zoom();
    let mut scene = Scene::new(viewport, background);

    let mat = state.current_mat();
    let (w, h) = (mat.size.width.get(), mat.size.height.get());
    // The pale frame the mat sits in: 12 screen pixels all round.
    let frame = FRAME_PX / zoom;
    scene.fill(
        &rect_mesh(-frame, -frame, w + frame, h + frame),
        Affine::IDENTITY,
        rgba(FRAME_COLOUR, 1.0),
    );
    scene.fill(
        &rect_mesh(0.0, 0.0, w, h),
        Affine::IDENTITY,
        rgba(MAT_COLOUR, 1.0),
    );

    if GRID_MM * zoom >= MIN_GRID_PX {
        let line = 1.0 / zoom;
        let mut x = 0.0;
        while x <= w {
            scene.fill(
                &rect_mesh(x - line / 2.0, 0.0, x + line / 2.0, h),
                Affine::IDENTITY,
                [0.09, 0.10, 0.23, 0.09],
            );
            x += GRID_MM;
        }
        let mut y = 0.0;
        while y <= h {
            scene.fill(
                &rect_mesh(0.0, y - line / 2.0, w, y + line / 2.0),
                Affine::IDENTITY,
                [0.09, 0.10, 0.23, 0.09],
            );
            y += GRID_MM;
        }
    }
    add_guides(&mut scene, &mat, zoom);

    add_objects(&mut scene, state, &mat, zoom);
    scene
}

#[cfg(test)]
mod tests;
