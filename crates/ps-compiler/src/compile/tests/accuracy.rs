//! Accuracy and registration: processing adds at most 0.01 mm of positional error
//! end to end, and a calibrated holder offset moves the geometry by exactly that offset. The
//! program is read back and compared with the exact geometry it came from.

use ps_core::PtUm;
use ps_geometry::arc::dist;
use ps_geometry::{Affine, Path, Point, flatten};
use ps_job::Step;

use super::*;

/// The most processing may move a point from where the artwork put it.
const BUDGET_MM: f64 = 0.01;

fn pen_job(geom: Path, cal: &Calibration) -> Job {
    let d = doc(vec![(
        mat(1),
        vec![object(
            10,
            vec![path(geom, "p", vec![op(PEN, OpKind::Draw, 1, 100)])],
        )],
    )]);
    run_with(&d, cal)
}

/// Every drawn point of the program in millimetres, with arcs sampled along their true circle.
fn drawn_points(job: &Job) -> Vec<Point> {
    let mut at: Option<PtUm> = None;
    let mut out = Vec::new();
    let mm = |p: PtUm| {
        let (x, y) = p.to_mm();
        Point::new(x, y)
    };
    for step in &job.mats[0].steps {
        match step {
            Step::Travel { to } => at = Some(*to),
            Step::Line { to, .. } => {
                out.push(mm(*to));
                at = Some(*to);
            }
            Step::Arc {
                to, center, ccw, ..
            } => {
                if let Some(from) = at {
                    let arc = Path::new(mm(from)).arc_to(mm(*to), mm(*center), *ccw);
                    out.extend(flatten(&arc, 0.001));
                }
                out.push(mm(*to));
                at = Some(*to);
            }
            _ => {}
        }
    }
    out
}

fn distance_to_circle(p: Point, centre: Point, radius: f64) -> f64 {
    (dist(p, centre) - radius).abs()
}

#[test]
fn a_circle_is_drawn_within_the_error_budget() {
    let centre = Point::new(120.0, 90.0);
    let job = pen_job(Path::circle(centre, 30.0), &Calibration::default());
    let points = drawn_points(&job);
    assert!(points.len() > 8);
    let worst = points
        .iter()
        .map(|p| distance_to_circle(*p, centre, 30.0))
        .fold(0.0, f64::max);
    assert!(worst <= BUDGET_MM, "worst error {worst} mm");
}

#[test]
fn a_cubic_curve_is_drawn_within_the_error_budget() {
    let curve = Path::new(Point::new(20.0, 20.0)).cubic_to(
        Point::new(60.0, 120.0),
        Point::new(140.0, 120.0),
        Point::new(180.0, 20.0),
    );
    let job = pen_job(curve.clone(), &Calibration::default());
    let reference = flatten(&curve, 0.0005);
    let worst = drawn_points(&job)
        .iter()
        .map(|p| {
            reference
                .windows(2)
                .map(|w| ps_render_distance(*p, w[0], w[1]))
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max);
    assert!(worst <= BUDGET_MM, "worst error {worst} mm");
}

/// The distance from a point to a segment.
fn ps_render_distance(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    };
    dist(p, Point::new(a.x + t * dx, a.y + t * dy))
}

#[test]
fn straight_lines_land_on_the_micrometre_grid_exactly() {
    let line = Path::polyline(
        &[Point::new(12.3456, 7.8912), Point::new(99.9999, 50.0001)],
        false,
    );
    let job = pen_job(line, &Calibration::default());
    let pts = drawn_points(&job);
    let last = pts.last().unwrap();
    assert!(
        (last.x - 99.9999).abs() <= 0.0006 && (last.y - 50.0001).abs() <= 0.0006,
        "{last:?}"
    );
}

#[test]
fn a_transformed_object_is_placed_within_the_budget() {
    let mut o = object(
        10,
        vec![path(
            Path::rect(0.0, 0.0, 40.0, 20.0),
            "p",
            vec![op(PEN, OpKind::Draw, 1, 100)],
        )],
    );
    o.transform = Affine::translate((30.123_456, 40.654_321))
        * Affine::new([
            libm::cos(0.3),
            libm::sin(0.3),
            -libm::sin(0.3),
            libm::cos(0.3),
            0.0,
            0.0,
        ]);
    let d = doc(vec![(mat(1), vec![o.clone()])]);
    let job = run_with(&d, &Calibration::default());
    let expected: Vec<Point> = flatten(
        &Path::rect(0.0, 0.0, 40.0, 20.0).transformed(o.transform),
        0.001,
    );
    let got = drawn_points(&job);
    for e in &expected {
        let nearest = got
            .iter()
            .map(|g| dist(*g, *e))
            .fold(f64::INFINITY, f64::min);
        assert!(
            nearest <= BUDGET_MM,
            "corner {e:?} is {nearest} mm from the nearest drawn point"
        );
    }
}

#[test]
fn a_calibrated_holder_offset_moves_the_geometry_by_exactly_that_offset() {
    let geom = Path::rect(50.0, 50.0, 30.0, 30.0);
    let plain = drawn_points(&pen_job(geom.clone(), &Calibration::default()));
    let mut cal = Calibration::default();
    cal.holder_offsets_mm.insert(
        "generic.holder-a".to_owned(),
        Xy {
            x_mm: 0.35,
            y_mm: -0.2,
        },
    );
    let shifted = drawn_points(&pen_job(geom, &cal));
    assert_eq!(plain.len(), shifted.len());
    for (a, b) in plain.iter().zip(&shifted) {
        assert!(((b.x - a.x) - 0.35).abs() <= 0.002, "{a:?} {b:?}");
        assert!(((b.y - a.y) + 0.2).abs() <= 0.002, "{a:?} {b:?}");
    }
}

#[test]
fn the_registration_between_two_tools_is_the_difference_of_their_offsets() {
    // A pen in clamp A and a blade in clamp B cut the same square; calibrating only the blade's
    // holder moves the cut and leaves the drawing where it was.
    let square = || Path::rect(60.0, 60.0, 25.0, 25.0);
    let d = doc(vec![(
        mat(1),
        vec![object(
            10,
            vec![path(
                square(),
                "p",
                vec![
                    op(PEN, OpKind::Draw, 1, 100),
                    op(BLADE, OpKind::Cut, 2, 101),
                ],
            )],
        )],
    )]);
    let base = run_with(&d, &Calibration::default());
    let mut cal = Calibration::default();
    cal.holder_offsets_mm.insert(
        "generic.holder-b".to_owned(),
        Xy {
            x_mm: 0.5,
            y_mm: 0.0,
        },
    );
    let moved = run_with(&d, &cal);
    // The pen's first travel is unchanged.
    let first_travel = |j: &Job| {
        j.mats[0]
            .steps
            .iter()
            .find_map(|s| match s {
                Step::Travel { to } => Some(*to),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(
        first_travel(&base),
        first_travel(&moved),
        "the pen did not move"
    );
    assert_ne!(
        base.mats[0].steps, moved.mats[0].steps,
        "the blade's cut moved"
    );
}
