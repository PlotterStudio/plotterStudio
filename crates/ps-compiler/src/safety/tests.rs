//! Ground truth for safety checks. Release safety: for every through-cut contour, any
//! other block whose paths lie inside (or cross into) the region it frees must run before it, so
//! nothing is drawn on a piece that has already come loose. Machine limits: every move inside the
//! work area and outside keep-outs, force and speed within the profile range.

use ps_core::PtUm;

use super::*;

const MM: i64 = 1_000;

fn p(x: i64, y: i64) -> PtUm {
    PtUm::new(x * MM, y * MM)
}

fn square(x0: i64, y0: i64, x1: i64, y1: i64) -> Vec<PtUm> {
    vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)]
}

#[test]
fn segments_cross_properly() {
    assert!(segments_cross(p(0, 0), p(10, 10), p(0, 10), p(10, 0)));
    assert!(!segments_cross(p(0, 0), p(10, 0), p(0, 5), p(10, 5)));
    assert!(
        !segments_cross(p(0, 0), p(4, 0), p(6, 0), p(10, 0)),
        "collinear but apart"
    );
    assert!(
        segments_cross(p(0, 0), p(10, 0), p(5, 0), p(15, 0)),
        "collinear overlap"
    );
    assert!(
        segments_cross(p(0, 0), p(10, 0), p(10, 0), p(10, 10)),
        "touching at an end"
    );
}

#[test]
fn a_path_inside_a_cut_must_run_before_it() {
    let cut = Cutter {
        block: 0,
        rings: vec![square(0, 0, 100, 100)],
    };
    let drawn = Drawn {
        block: 1,
        paths: vec![vec![p(10, 10), p(20, 20)]],
    };
    assert_eq!(release_edges(&[cut], &[drawn]), vec![Edge::safety(1, 0)]);
}

#[test]
fn a_path_outside_a_cut_has_no_constraint() {
    let cut = Cutter {
        block: 0,
        rings: vec![square(0, 0, 100, 100)],
    };
    let drawn = Drawn {
        block: 1,
        paths: vec![vec![p(200, 200), p(220, 220)]],
    };
    assert_eq!(release_edges(&[cut], &[drawn]).len(), 0);
}

#[test]
fn a_path_crossing_the_cut_line_is_constrained() {
    let cut = Cutter {
        block: 0,
        rings: vec![square(0, 0, 100, 100)],
    };
    let drawn = Drawn {
        block: 1,
        paths: vec![vec![p(-50, 50), p(150, 50)]],
    };
    assert_eq!(release_edges(&[cut], &[drawn]), vec![Edge::safety(1, 0)]);
}

#[test]
fn a_cut_never_constrains_its_own_block() {
    let cut = Cutter {
        block: 0,
        rings: vec![square(0, 0, 100, 100), square(20, 20, 40, 40)],
    };
    assert_eq!(
        release_edges(
            &[cut],
            &[Drawn {
                block: 0,
                paths: vec![vec![p(25, 25), p(30, 30)]]
            }]
        )
        .len(),
        0
    );
}

#[test]
fn an_inner_cut_of_another_block_runs_before_the_outer_cut() {
    let outer = Cutter {
        block: 0,
        rings: vec![square(0, 0, 100, 100)],
    };
    let inner = Cutter {
        block: 1,
        rings: vec![square(20, 20, 40, 40)],
    };
    let drawn = Drawn {
        block: 1,
        paths: vec![square(20, 20, 40, 40)],
    };
    assert_eq!(
        release_edges(&[outer, inner], &[drawn]),
        vec![Edge::safety(1, 0)]
    );
}

#[test]
fn duplicate_pairs_are_reported_once_and_sorted() {
    let cut = Cutter {
        block: 3,
        rings: vec![square(0, 0, 100, 100), square(200, 0, 300, 100)],
    };
    let a = Drawn {
        block: 2,
        paths: vec![vec![p(10, 10), p(20, 10)], vec![p(210, 10), p(220, 10)]],
    };
    let b = Drawn {
        block: 1,
        paths: vec![vec![p(50, 50), p(60, 60)]],
    };
    assert_eq!(
        release_edges(&[cut], &[a, b]),
        vec![Edge::safety(1, 3), Edge::safety(2, 3)]
    );
}

#[test]
fn a_single_point_path_inside_is_constrained() {
    let cut = Cutter {
        block: 0,
        rings: vec![square(0, 0, 100, 100)],
    };
    let dot = Drawn {
        block: 1,
        paths: vec![vec![p(50, 50)]],
    };
    assert_eq!(release_edges(&[cut], &[dot]), vec![Edge::safety(1, 0)]);
}

// ---- machine limits ----

fn limits() -> MachineLimits {
    MachineLimits {
        work_w_um: 300 * MM,
        work_h_um: 200 * MM,
        keep_outs: vec![Rect {
            x0: 100 * MM,
            y0: 100 * MM,
            x1: 150 * MM,
            y1: 150 * MM,
        }],
        force_min_gf: 50,
        force_max_gf: 400,
    }
}

#[test]
fn moves_inside_the_work_area_are_fine() {
    assert_eq!(check_point(p(10, 10), &limits()), None);
    assert_eq!(
        check_point(p(300, 200), &limits()),
        None,
        "the far corner is inside"
    );
    assert_eq!(check_point(p(0, 0), &limits()), None);
}

#[test]
fn moves_outside_the_work_area_are_reported() {
    assert_eq!(
        check_point(p(-1, 10), &limits()),
        Some(Violation::OutsideWorkArea)
    );
    assert_eq!(
        check_point(p(301, 10), &limits()),
        Some(Violation::OutsideWorkArea)
    );
    assert_eq!(
        check_point(p(10, 201), &limits()),
        Some(Violation::OutsideWorkArea)
    );
}

#[test]
fn moves_into_a_keep_out_are_reported() {
    assert_eq!(
        check_point(p(120, 120), &limits()),
        Some(Violation::InKeepOut)
    );
    assert_eq!(
        check_point(p(100, 100), &limits()),
        None,
        "touching the edge is allowed"
    );
}

#[test]
fn a_move_that_passes_through_a_keep_out_is_reported() {
    assert_eq!(
        check_segment(p(50, 125), p(200, 125), &limits()),
        Some(Violation::InKeepOut)
    );
    assert_eq!(check_segment(p(50, 50), p(200, 50), &limits()), None);
}

#[test]
fn force_is_clamped_into_the_profile_range_and_reported() {
    assert_eq!(clamp_force(200, &limits()), (200, false));
    assert_eq!(clamp_force(10, &limits()), (50, true));
    assert_eq!(clamp_force(900, &limits()), (400, true));
}
