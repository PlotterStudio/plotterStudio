//! The integer 1 µm grid used for robust booleans and G-code output.
//!
//! Final coordinates are converted to integer micrometres exactly once, rounding half away
//! from zero. Text output is produced from those integers, so float formatting can never
//! change the bytes of a G-code file.

use std::ops::{Add, Neg, Sub};

use serde::{Deserialize, Serialize};

/// Largest magnitude (in grid units) that an `f64` holds exactly: 2^53.
const MAX_EXACT: f64 = 9_007_199_254_740_992.0;

/// Why a floating-point value could not be placed on the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GridError {
    /// The value was NaN or infinite.
    #[error("value is not finite")]
    NotFinite,
    /// The value is too large to be held exactly on the grid.
    #[error("value is outside the exact µm grid range")]
    OutOfRange,
}

/// Rounds a value already expressed in grid units, half away from zero, into an `i64`.
fn round_to_i64(scaled: f64) -> Result<i64, GridError> {
    if !scaled.is_finite() {
        return Err(GridError::NotFinite);
    }
    let rounded = scaled.round();
    if rounded.abs() > MAX_EXACT {
        return Err(GridError::OutOfRange);
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "range-checked against 2^53 above; the value is already integral"
    )]
    let v = rounded as i64;
    Ok(v)
}

/// Converts millimetres to integer micrometres, rounding half away from zero.
///
/// # Errors
/// [`GridError::NotFinite`] for NaN or infinity, [`GridError::OutOfRange`] beyond 2^53 µm.
pub fn um_from_mm(mm: f64) -> Result<i64, GridError> {
    round_to_i64(mm * 1_000.0)
}

/// Converts integer micrometres to millimetres.
#[must_use]
pub fn mm_from_um(um: i64) -> f64 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "document coordinates are far below 2^53 µm, where i64 -> f64 is exact"
    )]
    let v = um as f64;
    v / 1_000.0
}

/// Converts degrees to integer milli-degrees, rounding half away from zero.
///
/// # Errors
/// As for [`um_from_mm`].
pub fn mdeg_from_degrees(deg: f64) -> Result<i64, GridError> {
    round_to_i64(deg * 1_000.0)
}

/// Formats a quantity held in thousandths as a decimal with exactly three fraction digits.
fn format_thousandths(v: i64) -> String {
    let sign = if v < 0 { "-" } else { "" };
    let abs = v.unsigned_abs();
    format!("{sign}{}.{:03}", abs / 1_000, abs % 1_000)
}

/// Formats integer micrometres as millimetres with exactly three decimals (`-12345` → `-12.345`).
#[must_use]
pub fn format_um(um: i64) -> String {
    format_thousandths(um)
}

/// Formats integer milli-degrees as degrees with exactly three decimals.
#[must_use]
pub fn format_mdeg(mdeg: i64) -> String {
    format_thousandths(mdeg)
}

/// A point on the µm grid.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct PtUm {
    /// X in micrometres.
    pub x: i64,
    /// Y in micrometres.
    pub y: i64,
}

impl PtUm {
    /// Creates a point from micrometres.
    #[must_use]
    pub const fn new(x: i64, y: i64) -> Self {
        Self { x, y }
    }

    /// Snaps a millimetre point onto the grid.
    ///
    /// # Errors
    /// As for [`um_from_mm`].
    pub fn from_mm(x: f64, y: f64) -> Result<Self, GridError> {
        Ok(Self::new(um_from_mm(x)?, um_from_mm(y)?))
    }

    /// Returns the point in millimetres.
    #[must_use]
    pub fn to_mm(self) -> (f64, f64) {
        (mm_from_um(self.x), mm_from_um(self.y))
    }

    /// Squared distance in µm², computed in 128 bits so it cannot overflow for real inputs.
    #[must_use]
    pub fn dist_sq(self, other: Self) -> i128 {
        let dx = i128::from(self.x) - i128::from(other.x);
        let dy = i128::from(self.y) - i128::from(other.y);
        dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
    }
}

impl Add for PtUm {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x.saturating_add(o.x), self.y.saturating_add(o.y))
    }
}

impl Sub for PtUm {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x.saturating_sub(o.x), self.y.saturating_sub(o.y))
    }
}

impl Neg for PtUm {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(self.x.saturating_neg(), self.y.saturating_neg())
    }
}

#[cfg(test)]
mod tests;
