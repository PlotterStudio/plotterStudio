//! Ground truth for tessellation: fills and strokes become triangle meshes in object
//! space (millimetres), cached per path. Areas are hand-computed: a 10 x 10 square is 100 mm2, a
//! circle is pi r2 within the flatten tolerance.

use ps_geometry::{FillRule, Path, Point};

use super::*;

fn area(mesh: &Mesh) -> f64 {
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let p = |i: u32| {
                let v = mesh.vertices[usize::try_from(i).unwrap()];
                (f64::from(v[0]), f64::from(v[1]))
            };
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)).abs() / 2.0
        })
        .sum()
}

fn within(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn a_square_fill_is_its_area() {
    let m = fill_mesh(&[Path::rect(0.0, 0.0, 10.0, 10.0)], FillRule::NonZero, 0.01);
    assert!(within(area(&m), 100.0, 1e-4), "{}", area(&m));
    assert_eq!(m.indices.len() % 3, 0);
}

#[test]
fn every_index_points_at_a_vertex() {
    let m = fill_mesh(
        &[Path::circle(Point::new(5.0, 5.0), 4.0)],
        FillRule::NonZero,
        0.01,
    );
    let n = u32::try_from(m.vertices.len()).unwrap();
    assert!(m.indices.iter().all(|i| *i < n));
}

#[test]
fn a_circle_fill_is_pi_r_squared_within_the_tolerance() {
    let r = 10.0;
    let m = fill_mesh(
        &[Path::circle(Point::new(0.0, 0.0), r)],
        FillRule::NonZero,
        0.01,
    );
    let exact = std::f64::consts::PI * r * r;
    // A polygon inscribed with sagitta <= 0.01 loses at most circumference * 0.01 * 2/3 of area.
    assert!(
        within(area(&m), exact, 2.0 * std::f64::consts::PI * r * 0.01),
        "{} vs {exact}",
        area(&m)
    );
    assert!(
        area(&m) <= exact + 1e-6,
        "an inscribed polygon cannot be larger"
    );
}

#[test]
fn a_looser_tolerance_makes_a_lighter_mesh() {
    let c = [Path::circle(Point::new(0.0, 0.0), 10.0)];
    let fine = fill_mesh(&c, FillRule::NonZero, 0.005);
    let coarse = fill_mesh(&c, FillRule::NonZero, 0.5);
    assert!(coarse.indices.len() < fine.indices.len());
}

#[test]
fn even_odd_leaves_a_hole_and_nonzero_with_opposite_winding_does_too() {
    let outer = Path::rect(0.0, 0.0, 10.0, 10.0);
    let inner = Path::rect(2.0, 2.0, 5.0, 5.0);
    let even_odd = fill_mesh(&[outer.clone(), inner.clone()], FillRule::EvenOdd, 0.01);
    assert!(within(area(&even_odd), 75.0, 1e-4), "{}", area(&even_odd));

    let reversed = inner.reversed();
    let non_zero = fill_mesh(&[outer, reversed], FillRule::NonZero, 0.01);
    assert!(within(area(&non_zero), 75.0, 1e-4), "{}", area(&non_zero));
}

#[test]
fn an_open_path_still_fills_as_if_closed() {
    let open = Path::polyline(
        &[
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ],
        false,
    );
    let m = fill_mesh(&[open], FillRule::NonZero, 0.01);
    assert!(within(area(&m), 100.0, 1e-4));
}

#[test]
fn nothing_to_fill_gives_an_empty_mesh() {
    let m = fill_mesh(&[], FillRule::NonZero, 0.01);
    assert!(m.vertices.is_empty() && m.indices.is_empty());
    let line = Path::polyline(&[Point::new(0.0, 0.0), Point::new(10.0, 0.0)], false);
    let degenerate = fill_mesh(&[line], FillRule::NonZero, 0.01);
    assert!(area(&degenerate) < 1e-9);
}

#[test]
fn a_butt_capped_line_is_length_times_width() {
    let line = Path::polyline(&[Point::new(0.0, 0.0), Point::new(10.0, 0.0)], false);
    let m = stroke_mesh(&line, 2.0, CapStyle::Butt, 0.01);
    assert!(within(area(&m), 20.0, 1e-4), "{}", area(&m));
}

#[test]
fn round_caps_add_a_disc() {
    let line = Path::polyline(&[Point::new(0.0, 0.0), Point::new(10.0, 0.0)], false);
    let m = stroke_mesh(&line, 2.0, CapStyle::Round, 0.005);
    // The two half-disc caps are inscribed polygons: each loses up to 2 pi r tol 2/3 (about 0.021
    // for r = 1, tol = 0.005), so the bound is 0.03.
    let exact = 20.0 + std::f64::consts::PI;
    assert!(within(area(&m), exact, 0.03), "{} vs {exact}", area(&m));
}

#[test]
fn a_closed_stroke_is_a_ring() {
    let sq = Path::rect(0.0, 0.0, 10.0, 10.0);
    let m = stroke_mesh(&sq, 1.0, CapStyle::Butt, 0.01);
    // Mitred corners: outer 11 x 11 minus inner 9 x 9.
    assert!(within(area(&m), 121.0 - 81.0, 0.05), "{}", area(&m));
}

#[test]
fn a_zero_width_stroke_has_no_mesh() {
    let line = Path::polyline(&[Point::new(0.0, 0.0), Point::new(10.0, 0.0)], false);
    assert_eq!(
        stroke_mesh(&line, 0.0, CapStyle::Butt, 0.01).indices.len(),
        0
    );
}

#[test]
fn a_dot_is_a_small_disc_of_the_given_diameter() {
    let m = dot_mesh(5.0, 5.0, 1.0, 0.005);
    assert!(
        within(area(&m), std::f64::consts::PI * 0.25, 0.01),
        "{}",
        area(&m)
    );
}

#[test]
fn meshes_stay_where_the_path_was() {
    let m = fill_mesh(&[Path::rect(20.0, 30.0, 4.0, 4.0)], FillRule::NonZero, 0.01);
    for v in &m.vertices {
        assert!(
            v[0] >= 19.99 && v[0] <= 24.01 && v[1] >= 29.99 && v[1] <= 34.01,
            "{v:?}"
        );
    }
}

#[test]
fn scaling_a_mesh_scales_its_area() {
    let m = fill_mesh(&[Path::rect(0.0, 0.0, 10.0, 10.0)], FillRule::NonZero, 0.01);
    let s = m.scaled(1.5);
    assert!(within(area(&s), 225.0, 1e-3));
}

#[test]
fn a_mesh_reports_its_bounds() {
    let m = fill_mesh(&[Path::rect(1.0, 2.0, 3.0, 4.0)], FillRule::NonZero, 0.01);
    let b = m.bounds().expect("not empty");
    assert!(
        within(b.0, 1.0, 1e-6)
            && within(b.1, 2.0, 1e-6)
            && within(b.2, 4.0, 1e-6)
            && within(b.3, 6.0, 1e-6),
        "{b:?}"
    );
    assert_eq!(Mesh::default().bounds(), None);
}
