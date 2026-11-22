//! The canvas viewport: which part of the mat is on screen.
//!
//! World positions are millimetres, screen positions are pixels, and y points down in both.

use ps_geometry::{Point, Rect};

/// The smallest zoom, in pixels per millimetre.
pub const MIN_ZOOM: f64 = 0.01;
/// The largest zoom, in pixels per millimetre.
pub const MAX_ZOOM: f64 = 2_000.0;

const MM_PER_INCH: f64 = 25.4;

/// A pan and zoom over the world, for a window of a given size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    centre: Point,
    zoom: f64,
    width_px: f64,
    height_px: f64,
}

impl Viewport {
    /// A viewport over a window of this size, 1 px per mm, centred on the origin.
    #[must_use]
    pub fn new(width_px: f64, height_px: f64) -> Self {
        Self {
            centre: Point::ZERO,
            zoom: 1.0,
            width_px,
            height_px,
        }
    }

    /// The same viewport centred on a world point.
    #[must_use]
    pub fn with_center(mut self, centre: Point) -> Self {
        self.centre = centre;
        self
    }

    /// The same viewport at a zoom (clamped to the limits).
    #[must_use]
    pub fn with_zoom(mut self, zoom: f64) -> Self {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self
    }

    /// The world point at the middle of the window.
    #[must_use]
    pub fn center(&self) -> Point {
        self.centre
    }

    /// Pixels per millimetre.
    #[must_use]
    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    /// Millimetres per pixel.
    #[must_use]
    pub fn mm_per_px(&self) -> f64 {
        1.0 / self.zoom
    }

    /// The screen position of a world point.
    #[must_use]
    pub fn world_to_screen(&self, p: Point) -> Point {
        Point::new(
            (p.x - self.centre.x) * self.zoom + self.width_px / 2.0,
            (p.y - self.centre.y) * self.zoom + self.height_px / 2.0,
        )
    }

    /// The world point under a screen position.
    #[must_use]
    pub fn screen_to_world(&self, s: Point) -> Point {
        Point::new(
            (s.x - self.width_px / 2.0) / self.zoom + self.centre.x,
            (s.y - self.height_px / 2.0) / self.zoom + self.centre.y,
        )
    }

    /// Zooms by `factor` keeping the world point under `cursor` where it is.
    pub fn zoom_at(&mut self, cursor: Point, factor: f64) {
        let anchor = self.screen_to_world(cursor);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let moved = self.screen_to_world(cursor);
        self.centre = Point::new(
            self.centre.x + anchor.x - moved.x,
            self.centre.y + anchor.y - moved.y,
        );
    }

    /// Drags the content by a screen offset.
    pub fn pan_by_screen(&mut self, across_px: f64, down_px: f64) {
        self.centre = Point::new(
            self.centre.x - across_px / self.zoom,
            self.centre.y - down_px / self.zoom,
        );
    }

    /// Shows `rect` (millimetres) as large as fits, centred, with `margin_px` around it.
    pub fn fit(&mut self, rect: Rect, margin_px: f64) {
        self.centre = Point::new(rect.x0.midpoint(rect.x1), rect.y0.midpoint(rect.y1));
        let avail_w = (self.width_px - 2.0 * margin_px).max(1.0);
        let avail_h = (self.height_px - 2.0 * margin_px).max(1.0);
        let w = rect.width();
        let h = rect.height();
        let zoom = if w <= 0.0 && h <= 0.0 {
            self.zoom
        } else {
            let zw = if w > 0.0 { avail_w / w } else { f64::INFINITY };
            let zh = if h > 0.0 { avail_h / h } else { f64::INFINITY };
            zw.min(zh)
        };
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// The world rectangle the window shows.
    #[must_use]
    pub fn visible_rect(&self) -> Rect {
        let tl = self.screen_to_world(Point::ZERO);
        let br = self.screen_to_world(Point::new(self.width_px, self.height_px));
        Rect::new(tl.x, tl.y, br.x, br.y)
    }

    /// Shows the world at its physical size on a display of `dpi`, times a user calibration
    /// factor (1.0 when the display reports its size correctly).
    pub fn set_true_scale(&mut self, dpi: f64, calibration: f64) {
        self.zoom = (dpi / MM_PER_INCH * calibration).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// The zoom as a percentage of true scale.
    #[must_use]
    pub fn percent_of_true_scale(&self, dpi: f64, calibration: f64) -> f64 {
        self.zoom / (dpi / MM_PER_INCH * calibration) * 100.0
    }

    /// Changes the window size, keeping the centre point.
    pub fn resize(&mut self, width_px: f64, height_px: f64) {
        self.width_px = width_px;
        self.height_px = height_px;
    }
}

#[cfg(test)]
mod tests;
