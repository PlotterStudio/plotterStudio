//! Dashing: cutting a path into the pieces a dash pattern keeps, so dashed score lines
//! and travel moves can be drawn as solid meshes.

use ps_geometry::Path;

/// The most dashes made from one path, so a tiny pattern on a long path cannot run away.
pub const MAX_DASHES: usize = 20_000;

/// A pixel dash pattern as millimetres at `zoom` pixels per millimetre.
#[must_use]
pub fn dash_for_zoom(pattern_px: &[f64], zoom: f64) -> Vec<f64> {
    pattern_px.iter().map(|px| px / zoom).collect()
}

/// The pieces of `path` a dash `pattern` (on, off, on, off … in millimetres) keeps. An odd
/// pattern is repeated to make pairs; an empty or unusable pattern keeps the path whole.
#[must_use]
pub fn dash(path: &Path, pattern: &[f64]) -> Vec<Path> {
    let mut steps: Vec<f64> = pattern.to_vec();
    if steps.len() % 2 == 1 {
        steps.extend_from_slice(pattern);
    }
    let usable = !steps.is_empty()
        && steps.iter().all(|s| s.is_finite() && *s >= 0.0)
        && steps.iter().sum::<f64>() > 1e-9;
    if !usable {
        return vec![path.clone()];
    }
    let total = path.length();
    let mut cuts = Vec::new();
    let mut at = 0.0;
    'walk: loop {
        for step in &steps {
            at += step;
            if at >= total || cuts.len() > MAX_DASHES * 2 {
                break 'walk;
            }
            cuts.push(at);
        }
    }
    path.split_at_lengths(&cuts)
        .into_iter()
        .enumerate()
        .filter(|(i, _)| i % 2 == 0)
        .map(|(_, piece)| piece)
        .take(MAX_DASHES + 1)
        .collect()
}

#[cfg(test)]
mod tests;
