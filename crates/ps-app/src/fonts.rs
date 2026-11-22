//! The fonts the text tool can use: the system's installed fonts through `fontdb`, plus
//! one bundled face so text always works.

use std::collections::BTreeMap;

/// The family name of the bundled face.
pub const BUNDLED_FAMILY: &str = "Ubuntu Light (bundled)";

/// The fonts available to the text tool.
pub struct FontLibrary {
    db: fontdb::Database,
    /// Family name to the id of its first face.
    faces: BTreeMap<String, fontdb::ID>,
    /// Loaded SVG single-stroke fonts by family name.
    stroke_fonts: BTreeMap<String, ps_import::SvgStrokeFont>,
}

impl std::fmt::Debug for FontLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontLibrary")
            .field("families", &self.faces.len())
            .finish_non_exhaustive()
    }
}

impl FontLibrary {
    fn from_database(db: fontdb::Database) -> Self {
        let mut faces: BTreeMap<String, fontdb::ID> = BTreeMap::new();
        for face in db.faces() {
            if let Some((name, _)) = face.families.first() {
                faces.entry(name.clone()).or_insert(face.id);
            }
        }
        Self {
            db,
            faces,
            stroke_fonts: BTreeMap::new(),
        }
    }

    fn bundled_database() -> fontdb::Database {
        let mut db = fontdb::Database::new();
        db.load_font_data(epaint_default_fonts::UBUNTU_LIGHT.to_vec());
        db
    }

    /// Only the bundled face.
    #[must_use]
    pub fn bundled_only() -> Self {
        let mut library = Self::from_database(Self::bundled_database());
        library.rename_bundled();
        library
    }

    /// The system's fonts and the bundled face.
    #[must_use]
    pub fn system() -> Self {
        let mut db = Self::bundled_database();
        db.load_system_fonts();
        let mut library = Self::from_database(db);
        library.rename_bundled();
        library
    }

    /// The bundled face is listed under [`BUNDLED_FAMILY`] so it cannot be mistaken for, or
    /// hidden by, an installed Ubuntu font.
    fn rename_bundled(&mut self) {
        if let Some(id) = self.faces.remove("Ubuntu Light").or_else(|| {
            self.db
                .faces()
                .next()
                .and_then(|f| f.families.first().map(|(n, _)| n.clone()))
                .and_then(|n| self.faces.remove(&n))
        }) {
            self.faces.insert(BUNDLED_FAMILY.to_owned(), id);
        }
    }

    /// Adds an SVG single-stroke font and returns the name it is listed under.
    ///
    /// # Errors
    ///
    /// [`ps_import::SvgFontError`] when the file holds no readable font.
    pub fn add_stroke_font(&mut self, source: &str) -> Result<String, ps_import::SvgFontError> {
        let font = ps_import::SvgStrokeFont::parse(source)?;
        let name = if font.name().is_empty() {
            "Untitled single-stroke font".to_owned()
        } else {
            font.name().to_owned()
        };
        self.stroke_fonts.insert(name.clone(), font);
        Ok(name)
    }

    /// Loads every readable `.svg` font in a folder, skipping the rest; returns how many loaded.
    pub fn load_stroke_fonts_from(&mut self, dir: &std::path::Path) -> usize {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return 0;
        };
        let mut paths: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        paths.sort();
        paths
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg")))
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .filter(|text| self.add_stroke_font(text).is_ok())
            .count()
    }

    /// The loaded single-stroke font names, sorted.
    #[must_use]
    pub fn stroke_font_names(&self) -> Vec<String> {
        self.stroke_fonts.keys().cloned().collect()
    }

    /// A loaded single-stroke font by name.
    #[must_use]
    pub fn stroke_font(&self, name: &str) -> Option<&ps_import::SvgStrokeFont> {
        self.stroke_fonts.get(name)
    }

    /// The family names, sorted without regard to case.
    #[must_use]
    pub fn families(&self) -> Vec<String> {
        let mut names: Vec<String> = self.faces.keys().cloned().collect();
        names.sort_by_key(|n| n.to_lowercase());
        names
    }

    /// The font file data for a family.
    #[must_use]
    pub fn data_for(&self, family: &str) -> Option<Vec<u8>> {
        let id = *self.faces.get(family)?;
        self.db.with_face_data(id, |data, _| data.to_vec())
    }

    /// The family and data to use for a requested family: the family itself when it is known,
    /// otherwise the bundled face.
    #[must_use]
    pub fn resolve(&self, family: &str) -> (String, Vec<u8>) {
        if let Some(data) = self.data_for(family) {
            return (family.to_owned(), data);
        }
        (
            BUNDLED_FAMILY.to_owned(),
            self.data_for(BUNDLED_FAMILY)
                .unwrap_or_else(|| epaint_default_fonts::UBUNTU_LIGHT.to_vec()),
        )
    }
}

#[cfg(test)]
mod tests;
