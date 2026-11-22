//! Fuzz entry points: one function per parser that reads untrusted files, taking
//! arbitrary bytes. The libFuzzer targets in the repository's `fuzz/` folder call [`run`]; the
//! tests here sweep truncated and corrupted seeds through the same entries on every build.
//!
//! An entry must return for any input. It reports whether the input was accepted only so tests
//! can check the seeds; the fuzzer looks for panics, hangs and out-of-memory.

use ps_import::{PdfOptions, SvgOptions, import_dxf, import_pdf, import_svg_auto};

/// A parser the fuzzer exercises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// SVG and SVGZ.
    Svg,
    /// DXF.
    Dxf,
    /// PDF and AI.
    Pdf,
    /// A `.pstudio` project (zip with `project.json`).
    Project,
    /// A material package zip.
    MaterialPackage,
    /// A library JSON file (tool, material or profile).
    LibraryJson,
}

impl Target {
    /// Every target.
    pub const ALL: [Self; 6] = [
        Self::Svg,
        Self::Dxf,
        Self::Pdf,
        Self::Project,
        Self::MaterialPackage,
        Self::LibraryJson,
    ];

    /// The name used for the libFuzzer target.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Dxf => "dxf",
            Self::Pdf => "pdf",
            Self::Project => "project",
            Self::MaterialPackage => "material_package",
            Self::LibraryJson => "library_json",
        }
    }

    /// The target with this name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }
}

/// Feeds `data` to the parser for `target` and reports whether it was accepted.
#[must_use]
pub fn accepts(target: Target, data: &[u8]) -> bool {
    match target {
        Target::Svg => import_svg_auto(data, &SvgOptions::default()).is_ok(),
        Target::Dxf => import_dxf(data).is_ok(),
        Target::Pdf => import_pdf(data, &PdfOptions::default()).is_ok(),
        Target::Project => {
            ps_model::Project::from_bytes(data, &semver::Version::new(0, 1, 0)).is_ok()
        }
        Target::MaterialPackage => ps_library::read_package(data).is_ok(),
        Target::LibraryJson => {
            let Ok(text) = std::str::from_utf8(data) else {
                return false;
            };
            [
                ps_library::LibraryKind::Tool,
                ps_library::LibraryKind::Material,
                ps_library::LibraryKind::Profile,
            ]
            .into_iter()
            .any(|kind| ps_library::Entry::from_json(kind, text).is_ok())
        }
    }
}

/// Feeds `data` to the parser for `target`; this is what a fuzz target calls.
pub fn run(target: Target, data: &[u8]) {
    let _ = accepts(target, data);
}

#[cfg(test)]
mod tests;
