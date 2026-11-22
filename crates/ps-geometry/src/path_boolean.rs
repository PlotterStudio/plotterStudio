//! Edit-time booleans on paths: weld, subtract, intersect, exclude.
//!
//! Everything sits behind one [`PathBoolean`] trait so engines can be swapped without touching
//! callers. [`Sweep`] (linesweeper) keeps Béziers editable; [`Grid`] (`i_overlay` on the µm grid)
//! is robust and is the fallback; [`Auto`] tries the first and falls back to the second.
//!
//! Results are closed contours: outers wind counter-clockwise and holes clockwise.

use kurbo::{BezPath, PathEl, Point};
use ps_core::GridError;

use crate::arc;
use crate::boolean::{self, BoolOp, FillRule, Ring};
use crate::flatten::flatten_to_grid;
use crate::path::{Path, Seg};
use crate::refit::refit_ring;

/// Flattening tolerance of the grid engine, in mm.
const GRID_FLATTEN_TOL: f64 = 0.002;
/// Tolerance of the curve refit after the grid engine, in mm.
const REFIT_TOL: f64 = 0.002;

/// Why a boolean could not be computed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BooleanError {
    /// The engine rejected or failed on the input.
    #[error("boolean engine failed: {0}")]
    Engine(String),
    /// An input coordinate was not finite or out of range.
    #[error("input coordinate cannot be placed on the grid: {0}")]
    Grid(#[from] GridError),
}

/// A boolean engine over paths.
pub trait PathBoolean: Send + Sync {
    /// Combines two sets of paths; every path is treated as closed.
    ///
    /// # Errors
    /// [`BooleanError`] when the input is invalid or the engine fails.
    fn apply(
        &self,
        subject: &[Path],
        clip: &[Path],
        op: BoolOp,
        rule: FillRule,
    ) -> Result<Vec<Path>, BooleanError>;
}

fn check_finite(paths: &[Path]) -> Result<(), BooleanError> {
    let finite = |p: Point| p.x.is_finite() && p.y.is_finite();
    let ok = paths.iter().all(|path| {
        finite(path.start)
            && path.segments().all(|(_, seg)| match *seg {
                Seg::Line { to } => finite(to),
                Seg::Arc { to, center, .. } => finite(to) && finite(center),
                Seg::Cubic { c1, c2, to } => finite(c1) && finite(c2) && finite(to),
            })
    });
    if ok {
        Ok(())
    } else {
        Err(BooleanError::Engine(
            "input contains a non-finite coordinate".into(),
        ))
    }
}

/// The linesweeper engine: curves stay curves.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sweep;

fn to_bez(paths: &[Path]) -> BezPath {
    let mut bez = BezPath::new();
    for path in paths {
        bez.move_to(path.start);
        for (from, seg) in path.segments() {
            match *seg {
                Seg::Line { to } => bez.line_to(to),
                Seg::Cubic { c1, c2, to } => bez.curve_to(c1, c2, to),
                Seg::Arc { to, center, ccw } => {
                    for [c1, c2, end] in arc::to_cubics(from, to, center, ccw) {
                        bez.curve_to(c1, c2, end);
                    }
                }
            }
        }
        bez.close_path();
    }
    bez
}

fn from_bez(bez: &BezPath) -> Path {
    let mut path: Option<Path> = None;
    let mut last = Point::ORIGIN;
    for el in bez.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                path = Some(Path::new(p));
                last = p;
            }
            PathEl::LineTo(p) => {
                path = path.take().map(|x| x.line_to(p));
                last = p;
            }
            PathEl::QuadTo(c, p) => {
                // Raise to a cubic: exact.
                let c1 = last + (c - last) * (2.0 / 3.0);
                let c2 = p + (c - p) * (2.0 / 3.0);
                path = path.take().map(|x| x.cubic_to(c1, c2, p));
                last = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                path = path.take().map(|x| x.cubic_to(c1, c2, p));
                last = p;
            }
            PathEl::ClosePath => {}
        }
    }
    path.map_or_else(|| Path::new(Point::ORIGIN), Path::closed)
}

impl PathBoolean for Sweep {
    fn apply(
        &self,
        subject: &[Path],
        clip: &[Path],
        op: BoolOp,
        rule: FillRule,
    ) -> Result<Vec<Path>, BooleanError> {
        check_finite(subject)?;
        check_finite(clip)?;
        let fill = match rule {
            FillRule::NonZero => linesweeper::FillRule::NonZero,
            FillRule::EvenOdd => linesweeper::FillRule::EvenOdd,
        };
        let op = match op {
            BoolOp::Union => linesweeper::BinaryOp::Union,
            BoolOp::Intersect => linesweeper::BinaryOp::Intersection,
            BoolOp::Difference => linesweeper::BinaryOp::Difference,
            BoolOp::Xor => linesweeper::BinaryOp::Xor,
        };
        let contours = linesweeper::binary_op(&to_bez(subject), &to_bez(clip), fill, op)
            .map_err(|e| BooleanError::Engine(e.to_string()))?;

        // Depth in the containment tree decides the winding: even = outer, odd = hole.
        let all: Vec<_> = contours.contours().collect();
        let depth = |mut idx: usize| {
            let mut d = 0;
            while let Some(parent) = all[idx].parent {
                idx = parent.0;
                d += 1;
            }
            d
        };
        Ok(all
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let p = from_bez(&c.path);
                let want_ccw = depth(i) % 2 == 0;
                if (p.signed_area() > 0.0) == want_ccw {
                    p
                } else {
                    p.reversed()
                }
            })
            .collect())
    }
}

/// The µm-grid engine: robust, returns polylines within the flattening tolerance.
#[derive(Debug, Clone, Copy, Default)]
pub struct Grid;

fn to_rings(paths: &[Path]) -> Result<Vec<Ring>, BooleanError> {
    check_finite(paths)?;
    paths
        .iter()
        .map(|p| flatten_to_grid(p, GRID_FLATTEN_TOL).map_err(BooleanError::from))
        .filter(|r| r.as_ref().map_or(true, |ring| ring.len() >= 3))
        .collect()
}

impl PathBoolean for Grid {
    fn apply(
        &self,
        subject: &[Path],
        clip: &[Path],
        op: BoolOp,
        rule: FillRule,
    ) -> Result<Vec<Path>, BooleanError> {
        let shapes = boolean::boolean(&to_rings(subject)?, &to_rings(clip)?, op, rule);
        Ok(shapes
            .iter()
            .flatten()
            .map(|ring| {
                let pts: Vec<Point> = ring
                    .iter()
                    .map(|p| {
                        let (x, y) = p.to_mm();
                        Point::new(x, y)
                    })
                    .collect();
                Path::polyline(&pts, true)
            })
            .collect())
    }
}

/// The µm-grid engine followed by a curve refit: robust like [`Grid`], but the result is made of
/// lines and cubics again (corners stay sharp), so it remains editable.
#[derive(Debug, Clone, Copy, Default)]
pub struct RefitGrid;

impl PathBoolean for RefitGrid {
    fn apply(
        &self,
        subject: &[Path],
        clip: &[Path],
        op: BoolOp,
        rule: FillRule,
    ) -> Result<Vec<Path>, BooleanError> {
        Ok(Grid
            .apply(subject, clip, op, rule)?
            .iter()
            .map(|polygon| {
                let vertices: Vec<Point> = std::iter::once(polygon.start)
                    .chain(polygon.segs.iter().map(Seg::to))
                    .collect();
                refit_ring(&vertices, REFIT_TOL)
            })
            .collect())
    }
}

/// Tries `primary`; if it fails, runs `fallback` and returns its result.
#[derive(Debug, Clone, Copy)]
pub struct Fallback<A, B> {
    primary: A,
    fallback: B,
}

impl<A, B> Fallback<A, B> {
    /// Chains two engines.
    #[must_use]
    pub const fn new(primary: A, fallback: B) -> Self {
        Self { primary, fallback }
    }
}

impl<A: PathBoolean, B: PathBoolean> PathBoolean for Fallback<A, B> {
    fn apply(
        &self,
        subject: &[Path],
        clip: &[Path],
        op: BoolOp,
        rule: FillRule,
    ) -> Result<Vec<Path>, BooleanError> {
        self.primary
            .apply(subject, clip, op, rule)
            .or_else(|_| self.fallback.apply(subject, clip, op, rule))
    }
}

/// The default engine: [`Sweep`], falling back to [`RefitGrid`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Auto;

impl PathBoolean for Auto {
    fn apply(
        &self,
        subject: &[Path],
        clip: &[Path],
        op: BoolOp,
        rule: FillRule,
    ) -> Result<Vec<Path>, BooleanError> {
        Fallback::new(Sweep, RefitGrid).apply(subject, clip, op, rule)
    }
}

#[cfg(test)]
mod tests;
