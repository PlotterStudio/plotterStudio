//! Dropping the vectors of a stroke that add nothing the machine could draw.
//!
//! Some cutting plotters spend a fixed time on every vector they are given, however short (measured on
//! one such machine: a pen fill of 9,214 vectors took 321 s, and 74 % of those vectors disappear when the
//! path is simplified within 10 um, after which it took 220 s). A curve flattened to the chord
//! tolerance, or a link that follows a boundary, is full of points that lie within a few
//! micrometres of the line between their neighbours. They are removed with Douglas-Peucker,
//! keeping every point that is farther than the tolerance (the profile's chord tolerance, 10 um by
//! default, a tenth of a pen's width at least). Arcs are never touched, and they end a run.

use ps_core::PtUm;
use simplify_polyline::{Point, simplify};

use crate::compile::Emit;

/// The moves of a stroke that starts at `start`, with every run of straight moves replaced by the
/// fewest that stay within `tolerance_um` micrometres of it.
pub(crate) fn simplify_emits(start: PtUm, emits: Vec<Emit>, tolerance_um: f64) -> Vec<Emit> {
    let mut out = Vec::with_capacity(emits.len());
    let mut run = vec![point(start)];
    for emit in emits {
        match emit {
            Emit::Line(to) => run.push(point(to)),
            Emit::Arc { to, center, ccw } => {
                flush(&mut run, tolerance_um, &mut out);
                out.push(Emit::Arc { to, center, ccw });
                // The next run starts where the arc ends.
                run.clear();
                run.push(point(to));
            }
        }
    }
    flush(&mut run, tolerance_um, &mut out);
    out
}

#[expect(
    clippy::cast_precision_loss,
    reason = "micrometre coordinates on a mat are far below 2^53, where i64 -> f64 is exact"
)]
fn point(p: PtUm) -> Point<2, f64> {
    Point {
        vec: [p.x as f64, p.y as f64],
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the values are whole micrometres that came from an i64 and are returned unchanged"
)]
fn grid(p: &Point<2, f64>) -> PtUm {
    PtUm::new(p.vec[0] as i64, p.vec[1] as i64)
}

/// Writes the pending run (whose first point is where the last move ended) and starts the next
/// one from its end.
fn flush(run: &mut Vec<Point<2, f64>>, tolerance_um: f64, out: &mut Vec<Emit>) {
    if run.len() > 1 {
        for p in simplify(run, tolerance_um, true).iter().skip(1) {
            out.push(Emit::Line(grid(p)));
        }
    }
    let last = run.last().copied();
    run.clear();
    run.extend(last);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i64, y: i64) -> PtUm {
        PtUm::new(x, y)
    }

    fn lines(points: &[(i64, i64)]) -> Vec<Emit> {
        points.iter().map(|&(x, y)| Emit::Line(p(x, y))).collect()
    }

    /// The end points of the moves, for comparing.
    fn ends(emits: &[Emit]) -> Vec<(i64, i64)> {
        emits
            .iter()
            .map(|e| match e {
                Emit::Line(to) | Emit::Arc { to, .. } => (to.x, to.y),
            })
            .collect()
    }

    #[test]
    fn points_on_the_line_between_their_neighbours_are_dropped() {
        let out = simplify_emits(p(0, 0), lines(&[(5_000, 0), (10_000, 0)]), 10.0);
        assert_eq!(ends(&out), vec![(10_000, 0)]);
    }

    #[test]
    fn a_wobble_inside_the_tolerance_goes_and_one_outside_stays() {
        // 2 um off the line is inside 10 um, 20 um off is not.
        let near = simplify_emits(p(0, 0), lines(&[(5_000, 2), (10_000, 0)]), 10.0);
        assert_eq!(ends(&near), vec![(10_000, 0)]);
        let far = simplify_emits(p(0, 0), lines(&[(5_000, 20), (10_000, 0)]), 10.0);
        assert_eq!(ends(&far), vec![(5_000, 20), (10_000, 0)]);
    }

    #[test]
    fn a_corner_is_always_kept() {
        let out = simplify_emits(p(0, 0), lines(&[(5_000, 0), (5_000, 5_000)]), 10.0);
        assert_eq!(ends(&out), vec![(5_000, 0), (5_000, 5_000)]);
        // Even a small one, if it is bigger than the tolerance: a 0.8 mm step between rows.
        let rows = simplify_emits(
            p(0, 0),
            lines(&[(30_000, 0), (30_000, 800), (0, 800)]),
            10.0,
        );
        assert_eq!(ends(&rows), vec![(30_000, 0), (30_000, 800), (0, 800)]);
    }

    #[test]
    fn an_arc_is_kept_exactly_and_ends_a_run() {
        let emits = vec![
            Emit::Line(p(5_000, 0)),
            Emit::Line(p(10_000, 0)),
            Emit::Arc {
                to: p(15_000, 5_000),
                center: p(10_000, 5_000),
                ccw: true,
            },
            Emit::Line(p(20_000, 5_000)),
            Emit::Line(p(25_000, 5_000)),
        ];
        let out = simplify_emits(p(0, 0), emits, 10.0);
        assert_eq!(out.len(), 3);
        assert!(matches!(out[0], Emit::Line(to) if to == p(10_000, 0)));
        assert!(matches!(
            out[1],
            Emit::Arc { to, center, ccw: true } if to == p(15_000, 5_000) && center == p(10_000, 5_000)
        ));
        assert!(matches!(out[2], Emit::Line(to) if to == p(25_000, 5_000)));
    }

    #[test]
    fn a_closed_path_keeps_its_corners_and_closes() {
        // A 10 mm square with a point in the middle of every edge, ending where it began.
        let out = simplify_emits(
            p(0, 0),
            lines(&[
                (5_000, 0),
                (10_000, 0),
                (10_000, 5_000),
                (10_000, 10_000),
                (5_000, 10_000),
                (0, 10_000),
                (0, 5_000),
                (0, 0),
            ]),
            10.0,
        );
        assert_eq!(
            ends(&out),
            vec![(10_000, 0), (10_000, 10_000), (0, 10_000), (0, 0)]
        );
    }

    #[test]
    fn short_paths_are_left_alone() {
        assert!(simplify_emits(p(0, 0), vec![], 10.0).is_empty());
        let one = simplify_emits(p(0, 0), lines(&[(1_000, 1_000)]), 10.0);
        assert_eq!(ends(&one), vec![(1_000, 1_000)]);
    }

    #[test]
    fn the_last_point_is_always_the_last_point() {
        let out = simplify_emits(
            p(0, 0),
            lines(&[(3_000, 1), (6_000, 0), (9_000, 2), (10_001, 0)]),
            10.0,
        );
        assert_eq!(ends(&out).last(), Some(&(10_001, 0)));
    }
}
