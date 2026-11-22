//! Ground truth for level of detail: flattening for display uses a tolerance of 0.25
//! screen px, meshes are re-tessellated when the zoom changes by more than 2x (until then the
//! cached mesh is scaled), and a path smaller than 1 px collapses to a dot.

use super::*;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-12
}

#[test]
fn the_flatten_tolerance_is_a_quarter_of_a_screen_pixel() {
    assert!(close(flatten_tolerance_mm(1.0), 0.25));
    assert!(close(flatten_tolerance_mm(4.0), 0.0625));
    assert!(close(flatten_tolerance_mm(0.5), 0.5));
}

#[test]
fn a_degenerate_zoom_gives_a_finite_tolerance() {
    assert!(flatten_tolerance_mm(0.0).is_finite());
    assert!(flatten_tolerance_mm(-3.0).is_finite());
}

#[test]
fn a_cached_mesh_is_kept_until_the_zoom_changes_by_more_than_two() {
    assert!(!needs_retessellation(10.0, 10.0));
    assert!(
        !needs_retessellation(10.0, 20.0),
        "exactly 2x is still fine"
    );
    assert!(
        !needs_retessellation(10.0, 5.0),
        "exactly 0.5x is still fine"
    );
    assert!(needs_retessellation(10.0, 20.1));
    assert!(needs_retessellation(10.0, 4.9));
}

#[test]
fn the_cached_mesh_is_scaled_by_the_zoom_ratio() {
    assert!(close(cached_mesh_scale(10.0, 15.0), 1.5));
    assert!(close(cached_mesh_scale(10.0, 10.0), 1.0));
}

#[test]
fn a_path_under_one_pixel_is_a_dot() {
    // 0.4 mm at 2 px/mm is 0.8 px.
    assert!(is_dot(0.4, 0.3, 2.0));
    // 0.6 mm at 2 px/mm is 1.2 px.
    assert!(!is_dot(0.6, 0.1, 2.0));
    // The larger side decides.
    assert!(!is_dot(0.1, 0.6, 2.0));
}

#[test]
fn exactly_one_pixel_is_not_yet_a_dot() {
    assert!(!is_dot(0.5, 0.5, 2.0));
}

#[test]
fn a_dot_sits_at_the_centre_of_the_bounds() {
    let c = dot_centre(10.0, 20.0, 10.4, 20.2);
    assert!(close(c.0, 10.2) && close(c.1, 20.1));
}

#[test]
fn the_dash_period_is_constant_in_screen_pixels() {
    // A 6 px dash pattern at 4 px/mm is 1.5 mm in the world.
    assert!(close(px_to_mm(6.0, 4.0), 1.5));
}
