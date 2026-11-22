//! Number formatting for the UI.
//!
//! Our own formatter, so the decimal separator and unit come from the user's settings and not from
//! the UI language. The document always stores millimetres; only display and typed input convert.

use ps_library::{DecimalSeparator, DisplayUnits};

const MM_PER_INCH: f64 = 25.4;
const MM_DECIMALS: usize = 3;
const INCH_DECIMALS: usize = 4;

/// Formats and parses lengths for display units and a decimal separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberFormat {
    units: DisplayUnits,
    separator: char,
}

impl NumberFormat {
    /// A formatter for `units`; `separator` is resolved against `locale` when it is automatic.
    #[must_use]
    pub fn new(units: DisplayUnits, separator: DecimalSeparator, locale: &str) -> Self {
        Self {
            units,
            separator: separator.resolve(locale),
        }
    }

    /// The label of the display unit.
    #[must_use]
    pub fn unit_label(&self) -> &'static str {
        match self.units {
            DisplayUnits::Millimetres => "mm",
            DisplayUnits::Inches => "in",
        }
    }

    fn decimals(self) -> usize {
        match self.units {
            DisplayUnits::Millimetres => MM_DECIMALS,
            DisplayUnits::Inches => INCH_DECIMALS,
        }
    }

    fn in_display_units(self, mm: f64) -> f64 {
        match self.units {
            DisplayUnits::Millimetres => mm,
            DisplayUnits::Inches => mm / MM_PER_INCH,
        }
    }

    /// A length in millimetres as a plain number in the display unit.
    #[must_use]
    pub fn display_from_mm(self, mm: f64) -> f64 {
        self.in_display_units(mm)
    }

    /// A plain number in the display unit as millimetres.
    #[must_use]
    pub fn mm_from_display(self, value: f64) -> f64 {
        match self.units {
            DisplayUnits::Millimetres => value,
            DisplayUnits::Inches => value * MM_PER_INCH,
        }
    }

    /// A length (given in millimetres) as a number in the display unit, without the unit.
    #[must_use]
    pub fn value(&self, mm: f64) -> String {
        if !mm.is_finite() {
            return "–".to_owned();
        }
        let decimals = self.decimals();
        let text = format!("{:.decimals$}", self.in_display_units(mm));
        let trimmed = if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.')
        } else {
            &text
        };
        let trimmed = if trimmed == "-0" { "0" } else { trimmed };
        trimmed.replace('.', &self.separator.to_string())
    }

    /// A length (given in millimetres) with its unit, such as `12.5 mm`.
    #[must_use]
    pub fn length(&self, mm: f64) -> String {
        let value = self.value(mm);
        if value == "–" {
            value
        } else {
            format!("{value} {}", self.unit_label())
        }
    }

    /// Reads typed text as a length in millimetres. A unit suffix (`mm`, `cm`, `in`, `"`)
    /// overrides the display unit; both `.` and `,` are accepted as the separator when only one
    /// is present.
    #[must_use]
    pub fn parse_length(&self, text: &str) -> Option<f64> {
        let t = text.trim();
        let lower = t.to_ascii_lowercase();
        let (number, factor) = if let Some(n) = lower.strip_suffix("mm") {
            (n, 1.0)
        } else if let Some(n) = lower.strip_suffix("cm") {
            (n, 10.0)
        } else if let Some(n) = lower.strip_suffix("in") {
            (n, MM_PER_INCH)
        } else if let Some(n) = lower.strip_suffix('"') {
            (n, MM_PER_INCH)
        } else {
            (
                lower.as_str(),
                match self.units {
                    DisplayUnits::Millimetres => 1.0,
                    DisplayUnits::Inches => MM_PER_INCH,
                },
            )
        };
        let number = number.trim();
        if number.contains('.') && number.contains(',') {
            return None;
        }
        let normalised = number.replace(',', ".");
        let digits = normalised.trim_start_matches(['+', '-']);
        if digits.is_empty()
            || normalised.len() - digits.len() > 1
            || !digits.chars().all(|c| c.is_ascii_digit() || c == '.')
            || digits.matches('.').count() > 1
        {
            return None;
        }
        let value: f64 = normalised.parse().ok()?;
        let mm = value * factor;
        mm.is_finite().then_some(mm)
    }
}

/// A duration as `12 s`, `3 min 05 s` or `1 h 02 min`.
#[must_use]
pub fn format_duration_ms(ms: u64) -> String {
    let total = ms / 1000;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h} h {m:02} min")
    } else if m > 0 {
        format!("{m} min {s:02} s")
    } else {
        format!("{s} s")
    }
}

/// A fraction from 0 to 1 as a whole percentage, clamped.
#[must_use]
pub fn format_percent(fraction: f64) -> String {
    let clamped = fraction.clamp(0.0, 1.0);
    format!("{:.0}%", clamped * 100.0)
}

#[cfg(test)]
mod tests;
