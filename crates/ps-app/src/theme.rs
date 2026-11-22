//! The "Zest" look from the design: Poppins type, warm orange accent, white panels on a pale
//! blue-grey ground, small radii and 12 px body text. Applied to egui's light and dark themes.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Theme,
    Visuals, epaint::Shadow,
};

/// The colours of one theme, named after their job in the design.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    /// The window ground behind panels.
    pub(crate) ground: Color32,
    /// Panels, bars and inputs.
    pub(crate) panel: Color32,
    /// The work area around the mat.
    pub(crate) canvas: Color32,
    /// Hairlines between panels.
    pub(crate) line: Color32,
    /// Stronger outlines of inputs and groups.
    pub(crate) outline: Color32,
    /// Body text.
    pub(crate) ink: Color32,
    /// Secondary text and idle icons.
    pub(crate) muted: Color32,
    /// The accent used for selection, text on tint and focus.
    pub(crate) accent: Color32,
    /// A pale fill behind selected items.
    pub(crate) tint: Color32,
    /// The bright orange of primary buttons and the active mode.
    pub(crate) orange: Color32,
    /// Errors and warnings.
    pub(crate) danger: Color32,
    /// Ready and saved.
    pub(crate) ok: Color32,
    /// Hover fill of quiet buttons.
    pub(crate) hover: Color32,
}

impl Palette {
    /// The palette of `theme`.
    #[must_use]
    pub(crate) fn of(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self {
                ground: Color32::from_rgb(0xf4, 0xf5, 0xfa),
                panel: Color32::from_rgb(0xff, 0xff, 0xff),
                canvas: Color32::from_rgb(0xec, 0xee, 0xf4),
                line: Color32::from_rgb(0xe8, 0xea, 0xf1),
                outline: Color32::from_rgb(0xd3, 0xd7, 0xe4),
                ink: Color32::from_rgb(0x17, 0x1a, 0x3a),
                muted: Color32::from_rgb(0x5f, 0x65, 0x80),
                accent: Color32::from_rgb(0xb2, 0x47, 0x00),
                tint: Color32::from_rgb(0xff, 0xeb, 0xd6),
                orange: Color32::from_rgb(0xff, 0x8a, 0x1f),
                danger: Color32::from_rgb(0xc4, 0x2b, 0x2b),
                ok: Color32::from_rgb(0x2b, 0xc4, 0x8a),
                hover: Color32::from_rgb(0xee, 0xf0, 0xf5),
            },
            Theme::Dark => Self {
                ground: Color32::from_rgb(0x12, 0x14, 0x2b),
                panel: Color32::from_rgb(0x1b, 0x1e, 0x3a),
                canvas: Color32::from_rgb(0x15, 0x17, 0x2f),
                line: Color32::from_rgb(0x2c, 0x30, 0x52),
                outline: Color32::from_rgb(0x40, 0x46, 0x6b),
                ink: Color32::from_rgb(0xec, 0xee, 0xfa),
                muted: Color32::from_rgb(0x9a, 0xa0, 0xc0),
                accent: Color32::from_rgb(0xff, 0xa9, 0x5c),
                tint: Color32::from_rgb(0x3b, 0x2a, 0x1c),
                orange: Color32::from_rgb(0xff, 0x8a, 0x1f),
                danger: Color32::from_rgb(0xff, 0x7b, 0x7b),
                ok: Color32::from_rgb(0x2b, 0xc4, 0x8a),
                hover: Color32::from_rgb(0x26, 0x2a, 0x4a),
            },
        }
    }
}

/// Installs the Poppins faces and the type scale, and styles both themes.
pub(crate) fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    for theme in [Theme::Light, Theme::Dark] {
        ctx.set_visuals_of(theme, visuals(theme));
        ctx.style_mut_of(theme, |style| {
            style.text_styles = [
                (
                    TextStyle::Small,
                    FontId::new(10.0, FontFamily::Proportional),
                ),
                (TextStyle::Body, FontId::new(12.0, FontFamily::Proportional)),
                (
                    TextStyle::Button,
                    FontId::new(12.0, FontFamily::Proportional),
                ),
                (
                    TextStyle::Heading,
                    FontId::new(15.0, FontFamily::Proportional),
                ),
                (
                    TextStyle::Monospace,
                    FontId::new(12.0, FontFamily::Monospace),
                ),
            ]
            .into();
            style.spacing.item_spacing = egui::vec2(6.0, 4.0);
            style.spacing.button_padding = egui::vec2(8.0, 2.0);
            style.spacing.interact_size = egui::vec2(24.0, 20.0);
            style.spacing.window_margin = egui::Margin::same(12);
            style.spacing.menu_margin = egui::Margin::same(6);
            style.spacing.indent = 14.0;
            style.spacing.slider_width = 120.0;
            style.interaction.tooltip_delay = 0.4;
        });
    }
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        (
            "poppins",
            &include_bytes!("../assets/fonts/Poppins-Regular.ttf")[..],
        ),
        (
            "poppins-medium",
            &include_bytes!("../assets/fonts/Poppins-Medium.ttf")[..],
        ),
        (
            "poppins-semibold",
            &include_bytes!("../assets/fonts/Poppins-SemiBold.ttf")[..],
        ),
    ] {
        fonts.font_data.insert(
            name.to_owned(),
            std::sync::Arc::new(FontData::from_static(bytes)),
        );
    }
    // Poppins first; egui's built-in faces stay behind it for glyphs Poppins lacks.
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "poppins".to_owned());
    for (family, face) in [
        ("medium", "poppins-medium"),
        ("semibold", "poppins-semibold"),
    ] {
        let mut chain = vec![face.to_owned()];
        chain.extend(
            fonts
                .families
                .get(&FontFamily::Proportional)
                .cloned()
                .unwrap_or_default(),
        );
        fonts
            .families
            .insert(FontFamily::Name(family.into()), chain);
    }
    fonts
}

/// The egui visuals for `theme`.
#[must_use]
pub(crate) fn visuals(theme: Theme) -> Visuals {
    let p = Palette::of(theme);
    let mut v = if theme == Theme::Dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let small = CornerRadius::same(4);
    v.override_text_color = Some(p.ink);
    v.panel_fill = p.panel;
    v.window_fill = p.panel;
    v.extreme_bg_color = p.ground;
    v.faint_bg_color = p.ground;
    v.code_bg_color = p.ground;
    v.window_stroke = Stroke::new(1.0_f32, p.line);
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow {
        offset: [0, 10],
        blur: 30,
        spread: 0,
        color: Color32::from_black_alpha(40),
    };
    v.popup_shadow = v.window_shadow;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.accent;
    v.error_fg_color = p.danger;
    v.selection.bg_fill = p.tint;
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);

    v.widgets.noninteractive.bg_fill = p.panel;
    v.widgets.noninteractive.weak_bg_fill = p.panel;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.line);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.ink);
    v.widgets.noninteractive.corner_radius = small;

    v.widgets.inactive.bg_fill = p.ground;
    v.widgets.inactive.weak_bg_fill = p.panel;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.outline);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.ink);
    v.widgets.inactive.corner_radius = small;

    v.widgets.hovered.bg_fill = p.hover;
    v.widgets.hovered.weak_bg_fill = p.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.outline);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.ink);
    v.widgets.hovered.corner_radius = small;

    v.widgets.active.bg_fill = p.tint;
    v.widgets.active.weak_bg_fill = p.tint;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, p.orange);
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.accent);
    v.widgets.active.corner_radius = small;

    v.widgets.open = v.widgets.hovered;
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_palette_is_the_design_system() {
        let p = Palette::of(Theme::Light);
        assert_eq!(p.ground, Color32::from_rgb(0xf4, 0xf5, 0xfa));
        assert_eq!(p.orange, Color32::from_rgb(0xff, 0x8a, 0x1f));
        assert_eq!(p.ink, Color32::from_rgb(0x17, 0x1a, 0x3a));
    }

    #[test]
    fn text_has_enough_contrast_on_panels_in_both_themes() {
        for theme in [Theme::Light, Theme::Dark] {
            let p = Palette::of(theme);
            for fg in [p.ink, p.muted, p.accent] {
                assert!(contrast(fg, p.panel) >= 4.5, "{theme:?} {fg:?}");
            }
        }
    }

    fn lum(c: Color32) -> f64 {
        let f = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }
}
