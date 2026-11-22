//! Unit handling.
//!
//! Drawings arrive in pixels, points or CAD units. The import dialog shows the physical size under
//! each plausible reading when they disagree. For SVG that is 96 against 72 px/in; for DXF the
//! header's `$INSUNITS` decides, and unitless files default to millimetres.

const MM_PER_INCH: f64 = 25.4;
/// How close a size must be to a paper size to count as that paper, in mm.
const PAPER_TOLERANCE_MM: f64 = 0.5;

/// A common paper size in millimetres (portrait).
const PAPERS: [(&str, f64, f64); 8] = [
    ("A3", 297.0, 420.0),
    ("A4", 210.0, 297.0),
    ("A5", 148.0, 210.0),
    ("A6", 105.0, 148.0),
    ("Letter", 215.9, 279.4),
    ("Legal", 215.9, 355.6),
    ("Tabloid", 279.4, 431.8),
    ("12x12in", 304.8, 304.8),
];

/// How the units of a drawing were interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitGuess {
    /// Pixels at 96 per inch (the CSS definition).
    Px96,
    /// Points or pixels at 72 per inch (print and Illustrator).
    Px72,
    /// Millimetres.
    Mm,
    /// Inches.
    In,
    /// A DXF `$INSUNITS` code.
    DxfInsUnits(i32),
}

/// The tool that wrote an SVG, which decides how pixel sizes are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Generator {
    /// Inkscape.
    Inkscape,
    /// Adobe Illustrator.
    Illustrator,
    /// Affinity Designer or Publisher.
    Affinity,
    /// Figma.
    Figma,
    /// Anything else.
    Unknown,
}

impl Generator {
    /// A stable key for remembering the user's choice per generator.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Inkscape => "inkscape",
            Self::Illustrator => "illustrator",
            Self::Affinity => "affinity",
            Self::Figma => "figma",
            Self::Unknown => "unknown",
        }
    }
}

/// Millimetres in one unit.
#[must_use]
pub fn mm_per_unit(unit: UnitGuess) -> f64 {
    match unit {
        UnitGuess::Px96 => MM_PER_INCH / 96.0,
        UnitGuess::Px72 => MM_PER_INCH / 72.0,
        UnitGuess::Mm => 1.0,
        UnitGuess::In => MM_PER_INCH,
        UnitGuess::DxfInsUnits(code) => match code {
            1 => MM_PER_INCH,
            2 => 12.0 * MM_PER_INCH,
            3 => 63_360.0 * MM_PER_INCH,
            5 => 10.0,
            6 => 1_000.0,
            7 => 1_000_000.0,
            8 => MM_PER_INCH / 1_000_000.0,
            9 => MM_PER_INCH / 1_000.0,
            10 => 36.0 * MM_PER_INCH,
            13 => 0.001,
            14 => 100.0,
            // 4 is millimetres; 0 (unitless) and unknown codes default to millimetres too.
            _ => 1.0,
        },
    }
}

/// The unit guess for a DXF `$INSUNITS` code.
#[must_use]
pub fn dxf_unit_guess(code: i32) -> UnitGuess {
    UnitGuess::DxfInsUnits(code)
}

/// True when a DXF code means "unitless" (the dialog should ask, defaulting to millimetres):
/// zero, or a code outside the table this importer knows.
#[must_use]
pub fn dxf_is_unitless(code: i32) -> bool {
    !matches!(code, 1..=10 | 13 | 14)
}

/// The physical size of a drawing under one reading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizeCandidate {
    /// The reading.
    pub unit: UnitGuess,
    /// Width in mm.
    pub width_mm: f64,
    /// Height in mm.
    pub height_mm: f64,
}

impl SizeCandidate {
    /// A drawing of `width` × `height` in `unit`.
    #[must_use]
    pub fn of(unit: UnitGuess, width: f64, height: f64) -> Self {
        let k = mm_per_unit(unit);
        Self {
            unit,
            width_mm: width * k,
            height_mm: height * k,
        }
    }
}

/// The name of a common paper size that `width_mm` × `height_mm` matches in either orientation.
#[must_use]
pub fn paper_name(width_mm: f64, height_mm: f64) -> Option<&'static str> {
    let close = |a: f64, b: f64| (a - b).abs() <= PAPER_TOLERANCE_MM;
    PAPERS.iter().find_map(|&(name, w, h)| {
        ((close(width_mm, w) && close(height_mm, h)) || (close(width_mm, h) && close(height_mm, w)))
            .then_some(name)
    })
}

/// The readings to offer for an SVG's size.
///
/// With real units (`unitless` false) there is one reading, in millimetres. Unitless sizes are
/// read at 96 px/in, and both 96 and 72 are offered when the file came from Illustrator or when
/// the 72 px/in reading is exactly a common paper size.
#[must_use]
pub fn svg_size_candidates(
    width: f64,
    height: f64,
    unitless: bool,
    generator: Generator,
) -> Vec<SizeCandidate> {
    if !unitless {
        return vec![SizeCandidate::of(UnitGuess::Mm, width, height)];
    }
    let at96 = SizeCandidate::of(UnitGuess::Px96, width, height);
    let at72 = SizeCandidate::of(UnitGuess::Px72, width, height);
    let paper_at_72 = paper_name(at72.width_mm, at72.height_mm).is_some();
    if generator == Generator::Illustrator || paper_at_72 {
        vec![at96, at72]
    } else {
        vec![at96]
    }
}

/// Guesses the generator from the start of the SVG text (root attributes, namespaces, comments).
#[must_use]
pub fn detect_generator(svg: &str) -> Generator {
    let head: String = svg.chars().take(4_096).collect();
    let lower = head.to_ascii_lowercase();
    if lower.contains("inkscape") {
        Generator::Inkscape
    } else if lower.contains("adobe illustrator") || lower.contains("ns.adobe.com/adobeillustrator")
    {
        Generator::Illustrator
    } else if lower.contains("serif.com") || lower.contains("xmlns:serif") {
        Generator::Affinity
    } else if lower.contains("figma") {
        Generator::Figma
    } else {
        Generator::Unknown
    }
}

#[cfg(test)]
mod tests;
