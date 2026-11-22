//! Physical-quantity newtypes and the tolerance constants of the precision budget.
//!
//! Newtypes stop millimetres, forces and speeds being mixed by accident. They serialise as bare
//! numbers so project and library files stay readable.

use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

use serde::{Deserialize, Serialize};

/// Millimetres per inch.
const MM_PER_INCH: f64 = 25.4;

macro_rules! quantity {
    ($(#[$doc:meta])* $name:ident, $suffix:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub f64);

        impl $name {
            /// Returns the raw number.
            #[must_use]
            pub const fn get(self) -> f64 {
                self.0
            }
        }

        impl Add for $name {
            type Output = Self;
            fn add(self, o: Self) -> Self {
                Self(self.0 + o.0)
            }
        }

        impl Sub for $name {
            type Output = Self;
            fn sub(self, o: Self) -> Self {
                Self(self.0 - o.0)
            }
        }

        impl Neg for $name {
            type Output = Self;
            fn neg(self) -> Self {
                Self(-self.0)
            }
        }

        impl Mul<f64> for $name {
            type Output = Self;
            fn mul(self, k: f64) -> Self {
                Self(self.0 * k)
            }
        }

        impl Div<f64> for $name {
            type Output = Self;
            fn div(self, k: f64) -> Self {
                Self(self.0 / k)
            }
        }

        /// The ratio of two quantities of the same kind is a plain number.
        impl Div for $name {
            type Output = f64;
            fn div(self, o: Self) -> f64 {
                self.0 / o.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}{}", self.0, $suffix)
            }
        }
    };
}

quantity!(
    /// A length in millimetres.
    Mm,
    " mm"
);
quantity!(
    /// A force in grams-force.
    GramsForce,
    " gf"
);
quantity!(
    /// A speed in millimetres per second.
    MmPerSec,
    " mm/s"
);
quantity!(
    /// An acceleration in millimetres per second squared.
    MmPerSec2,
    " mm/s²"
);
quantity!(
    /// A temperature in degrees Celsius.
    Celsius,
    " °C"
);
quantity!(
    /// An angle in degrees.
    Degrees,
    "°"
);

/// A duration in whole milliseconds.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Millis(pub u64);

impl fmt::Display for Millis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ms", self.0)
    }
}

impl Mm {
    /// Converts inches to millimetres.
    #[must_use]
    pub fn from_inches(inches: f64) -> Self {
        Self(inches * MM_PER_INCH)
    }

    /// Returns the length in inches.
    #[must_use]
    pub fn to_inches(self) -> f64 {
        self.0 / MM_PER_INCH
    }
}

impl MmPerSec {
    /// Converts to the millimetres-per-minute feed rate that G-code `F` words use.
    #[must_use]
    pub fn to_mm_per_min(self) -> f64 {
        self.0 * 60.0
    }
}

impl Degrees {
    /// Converts to radians.
    #[must_use]
    pub fn to_radians(self) -> f64 {
        self.0.to_radians()
    }

    /// Converts from radians.
    #[must_use]
    pub fn from_radians(rad: f64) -> Self {
        Self(rad.to_degrees())
    }

    /// Wraps the angle into the half-open range (-180, 180].
    #[must_use]
    pub fn wrapped(self) -> Self {
        let r = self.0.rem_euclid(360.0);
        Self(if r > 180.0 { r - 360.0 } else { r })
    }

    /// Returns the equivalent angle closest to `prev`, so a sequence of angles stays continuous
    /// and a tangential tool never spins a full turn to cross ±180°.
    #[must_use]
    pub fn unwrap_near(self, prev: Self) -> Self {
        let turns = ((prev.0 - self.0) / 360.0).round();
        Self(self.0 + 360.0 * turns)
    }
}

/// The precision budget and fixed edit-time constants.
pub mod tolerance {
    use super::Mm;

    /// Bézier to arc (biarc) fitting for output.
    pub const BEZIER_TO_ARC: Mm = Mm(0.002);
    /// Flattening to polylines at compile time.
    pub const FLATTEN: Mm = Mm(0.002);
    /// Offsets (blade compensation, fill inset).
    pub const OFFSET: Mm = Mm(0.002);
    /// Snapping to the µm grid and G-code formatting.
    pub const GRID_SNAP: Mm = Mm(0.0005);
    /// Reserve for transforms and calibration offsets.
    pub const RESERVE: Mm = Mm(0.0035);
    /// The cumulative processing error; the five stages above sum to this.
    pub const PROCESSING_BUDGET: Mm = Mm(0.010);

    /// Gap below which a nearly closed path is closed.
    pub const CLOSE_PATH: Mm = Mm(0.05);
    /// Distance below which two paths count as duplicates.
    pub const DUPLICATE: Mm = Mm(0.001);
    /// Segments shorter than this are removed as zero length.
    pub const ZERO_LENGTH_SEGMENT: Mm = Mm(0.001);
    /// Chord tolerance for output on profiles without arc support.
    pub const CHORD: Mm = Mm(0.01);

    /// Hit-test radius in screen pixels.
    pub const HIT_TEST_SCREEN_PX: f64 = 4.0;
    /// Snapping radius in screen pixels.
    pub const SNAP_SCREEN_PX: f64 = 8.0;

    /// Converts a screen-pixel distance to millimetres at `px_per_mm` zoom.
    #[must_use]
    pub fn px_to_mm(px: f64, px_per_mm: f64) -> Mm {
        Mm(px / px_per_mm)
    }
}

#[cfg(test)]
mod tests;
