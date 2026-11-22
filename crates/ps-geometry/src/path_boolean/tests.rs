//! Ground truth for edit-time booleans on paths: curves stay curves when the sweep
//! engine succeeds, and the grid engine is the safety net.

use std::f64::consts::PI;

use super::*;

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Path {
    Path::rect(x0, y0, x1 - x0, y1 - y0)
}

/// Sum of the absolute areas of the result contours (no holes in these cases).
fn total_area(paths: &[Path]) -> f64 {
    paths.iter().map(|p| p.signed_area().abs()).sum()
}

fn engines() -> Vec<(&'static str, Box<dyn PathBoolean>)> {
    vec![
        ("sweep", Box::new(Sweep)),
        ("grid", Box::new(Grid)),
        ("auto", Box::new(Auto)),
    ]
}

#[test]
fn every_engine_unions_overlapping_squares() {
    for (name, e) in engines() {
        let out = e
            .apply(
                &[rect(0.0, 0.0, 4.0, 4.0)],
                &[rect(2.0, 2.0, 6.0, 6.0)],
                BoolOp::Union,
                FillRule::NonZero,
            )
            .unwrap();
        assert_eq!(out.len(), 1, "{name}");
        assert!(
            (total_area(&out) - 28.0).abs() < 1e-3,
            "{name}: {}",
            total_area(&out)
        );
        assert!(out[0].closed, "{name}");
    }
}

#[test]
fn every_engine_intersects_difference_and_xors() {
    for (name, e) in engines() {
        let a = [rect(0.0, 0.0, 4.0, 4.0)];
        let b = [rect(2.0, 2.0, 6.0, 6.0)];
        let run = |op| total_area(&e.apply(&a, &b, op, FillRule::NonZero).unwrap());
        assert!(
            (run(BoolOp::Intersect) - 4.0).abs() < 1e-3,
            "{name} intersect"
        );
        assert!(
            (run(BoolOp::Difference) - 12.0).abs() < 1e-3,
            "{name} difference"
        );
        assert!((run(BoolOp::Xor) - 24.0).abs() < 1e-3, "{name} xor");
    }
}

#[test]
fn disjoint_inputs_intersect_to_nothing() {
    for (name, e) in engines() {
        let out = e
            .apply(
                &[rect(0.0, 0.0, 1.0, 1.0)],
                &[rect(5.0, 5.0, 6.0, 6.0)],
                BoolOp::Intersect,
                FillRule::NonZero,
            )
            .unwrap();
        assert!(out.is_empty(), "{name}");
    }
}

#[test]
fn sweep_keeps_curves_editable() {
    // A unit circle clipped by a large square is still the circle: curved segments survive.
    let circle = Path::circle(pt(0.0, 0.0), 1.0);
    let out = Sweep
        .apply(
            &[circle],
            &[rect(-5.0, -5.0, 5.0, 5.0)],
            BoolOp::Intersect,
            FillRule::NonZero,
        )
        .unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].segs.iter().any(|s| matches!(s, Seg::Cubic { .. })));
    assert!((total_area(&out) - PI).abs() < 2e-3);
}

#[test]
fn grid_engine_returns_polylines_within_tolerance() {
    let circle = Path::circle(pt(0.0, 0.0), 10.0);
    let out = Grid
        .apply(
            &[circle],
            &[rect(-50.0, -50.0, 50.0, 50.0)],
            BoolOp::Intersect,
            FillRule::NonZero,
        )
        .unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].segs.iter().all(|s| matches!(s, Seg::Line { .. })));
    // Chord error at 0.002 mm loses at most 2*pi*r*tol of area.
    assert!((total_area(&out) - 100.0 * PI).abs() < 2.0 * PI * 10.0 * 0.002 + 1e-3);
}

#[test]
fn a_hole_comes_back_as_a_second_contour() {
    for (name, e) in engines() {
        let out = e
            .apply(
                &[rect(0.0, 0.0, 10.0, 10.0)],
                &[rect(4.0, 4.0, 6.0, 6.0)],
                BoolOp::Difference,
                FillRule::NonZero,
            )
            .unwrap();
        assert_eq!(out.len(), 2, "{name}");
        let net: f64 = out.iter().map(|p| p.signed_area().abs()).sum();
        // Outer 100 plus hole 4 when absolute areas are summed.
        assert!((net - 104.0).abs() < 1e-3, "{name}");
        let signed: f64 = out.iter().map(Path::signed_area).sum::<f64>().abs();
        assert!(
            (signed - 96.0).abs() < 1e-3,
            "{name}: holes wind the other way"
        );
    }
}

#[test]
fn non_finite_input_is_an_error_for_every_engine() {
    let bad = Path::polyline(&[pt(0.0, 0.0), pt(f64::NAN, 0.0), pt(1.0, 1.0)], true);
    for (name, e) in engines() {
        let r = e.apply(
            std::slice::from_ref(&bad),
            &[rect(0.0, 0.0, 1.0, 1.0)],
            BoolOp::Union,
            FillRule::NonZero,
        );
        assert!(r.is_err(), "{name}");
    }
}

#[test]
fn auto_falls_back_when_the_sweep_engine_fails() {
    struct Failing;
    impl PathBoolean for Failing {
        fn apply(
            &self,
            _: &[Path],
            _: &[Path],
            _: BoolOp,
            _: FillRule,
        ) -> Result<Vec<Path>, BooleanError> {
            Err(BooleanError::Engine("simulated degenerate input".into()))
        }
    }
    let chain = Fallback::new(Failing, Grid);
    let out = chain
        .apply(
            &[rect(0.0, 0.0, 4.0, 4.0)],
            &[rect(2.0, 2.0, 6.0, 6.0)],
            BoolOp::Union,
            FillRule::NonZero,
        )
        .unwrap();
    assert!((total_area(&out) - 28.0).abs() < 1e-3);
}

#[test]
fn fallback_reports_the_last_error_when_both_fail() {
    struct Failing(&'static str);
    impl PathBoolean for Failing {
        fn apply(
            &self,
            _: &[Path],
            _: &[Path],
            _: BoolOp,
            _: FillRule,
        ) -> Result<Vec<Path>, BooleanError> {
            Err(BooleanError::Engine(self.0.into()))
        }
    }
    let chain = Fallback::new(Failing("first"), Failing("second"));
    let err = chain
        .apply(&[], &[], BoolOp::Union, FillRule::NonZero)
        .unwrap_err();
    assert_eq!(err, BooleanError::Engine("second".into()));
}
