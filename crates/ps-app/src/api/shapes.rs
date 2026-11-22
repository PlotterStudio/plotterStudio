//! Turns a [`Shape`] into a path and the point on the mat the path's origin sits at.

use std::f64::consts::FRAC_PI_2;

use ps_geometry::{Path, Point};

use super::ApiError;
use super::types::{Pt, Shape};
use crate::tools::{ellipse_path, radial_points};

fn pt(p: Pt) -> Point {
    Point::new(p.x, p.y)
}

fn invalid(why: &str) -> ApiError {
    ApiError::new("invalid_shape", why)
}

fn finite(values: &[f64]) -> Result<(), ApiError> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(invalid("every number must be finite"))
    }
}

/// The path (relative to its origin) and the origin on the mat.
pub(super) fn build(shape: &Shape) -> Result<(Path, Point), ApiError> {
    match shape {
        Shape::Rectangle {
            x,
            y,
            width,
            height,
        } => {
            finite(&[*x, *y, *width, *height])?;
            if *width <= 0.0 || *height <= 0.0 {
                return Err(invalid("a rectangle needs a positive width and height"));
            }
            Ok((Path::rect(0.0, 0.0, *width, *height), Point::new(*x, *y)))
        }
        Shape::Ellipse {
            x,
            y,
            width,
            height,
        } => {
            finite(&[*x, *y, *width, *height])?;
            if *width <= 0.0 || *height <= 0.0 {
                return Err(invalid("an ellipse needs a positive width and height"));
            }
            Ok((ellipse_path(*width, *height), Point::new(*x, *y)))
        }
        Shape::Line { from, to } => {
            finite(&[from.x, from.y, to.x, to.y])?;
            let (a, b) = (pt(*from), pt(*to));
            if a == b {
                return Err(invalid("a line needs two different points"));
            }
            Ok((
                Path::polyline(&[Point::ZERO, Point::new(b.x - a.x, b.y - a.y)], false),
                a,
            ))
        }
        Shape::Polygon {
            centre,
            radius,
            sides,
        } => {
            finite(&[centre.x, centre.y, *radius])?;
            if *sides < 3 || *radius <= 0.0 {
                return Err(invalid(
                    "a polygon needs at least 3 sides and a positive radius",
                ));
            }
            let corners = radial_points(*sides, *radius, None, -FRAC_PI_2);
            Ok((Path::polyline(&corners, true), pt(*centre)))
        }
        Shape::Star {
            centre,
            radius,
            points,
            ratio,
        } => {
            finite(&[centre.x, centre.y, *radius, *ratio])?;
            if *points < 3 || *radius <= 0.0 || !(*ratio > 0.0 && *ratio < 1.0) {
                return Err(invalid(
                    "a star needs at least 3 points, a positive radius and a ratio between 0 and 1",
                ));
            }
            let corners = radial_points(*points, *radius, Some(radius * ratio), -FRAC_PI_2);
            Ok((Path::polyline(&corners, true), pt(*centre)))
        }
        Shape::Arc { from, through, to } => {
            finite(&[from.x, from.y, through.x, through.y, to.x, to.y])?;
            let (a, b, c) = (pt(*from), pt(*through), pt(*to));
            let Some((centre, ccw)) = ps_geometry::arc::from_three_points(a, b, c) else {
                return Err(invalid("the three points of an arc must not be in a line"));
            };
            let rel = |p: Point| Point::new(p.x - a.x, p.y - a.y);
            Ok((Path::new(Point::ZERO).arc_to(rel(c), rel(centre), ccw), a))
        }
        Shape::Path { points, closed } => {
            if points.len() < 2 {
                return Err(invalid("a path needs at least two points"));
            }
            if points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
                return Err(invalid("every number must be finite"));
            }
            let origin = pt(points[0]);
            let rel: Vec<Point> = points
                .iter()
                .map(|p| Point::new(p.x - origin.x, p.y - origin.y))
                .collect();
            Ok((Path::polyline(&rel, *closed), origin))
        }
    }
}
