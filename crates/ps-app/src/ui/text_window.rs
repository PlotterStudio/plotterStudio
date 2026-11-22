//! The text window: type text, choose a font or a single-stroke pen font, and add
//! it to the mat as one object of paths.

use ps_import::{Align, StrokeFont};

use super::{App, selectable};
use crate::fonts::FontLibrary;
use crate::l10n::Arg;

const STROKE_FONTS: [(StrokeFont, &str); 5] = [
    (StrokeFont::RomanSimplex, "text-stroke-roman-simplex"),
    (StrokeFont::RomanDuplex, "text-stroke-roman-duplex"),
    (StrokeFont::RomanComplex, "text-stroke-roman-complex"),
    (StrokeFont::ScriptSimplex, "text-stroke-script-simplex"),
    (StrokeFont::ItalicComplex, "text-stroke-italic-complex"),
];

/// The text being typed, drawn where it will land and edited in a card that floats above it, so
/// there is no separate window.
pub(super) fn inline(ctx: &egui::Context, app: &mut App, canvas: egui::Rect, now_ms: u64) {
    if !app.state.text_open() {
        app.text_anchor = None;
        app.text_focused = false;
        return;
    }
    let anchor = *app
        .text_anchor
        .get_or_insert_with(|| app.state.viewport().center());
    let at = super::to_screen(&app.state, canvas.min, anchor);
    draw_preview(ctx, app, canvas, at);

    let p = super::zest::palette(ctx);
    let mut close = false;
    // Above the text's top-left corner, or below it when there is no room above.
    let above = at.y - canvas.top() > 260.0;
    let pivot = if above {
        egui::Align2::LEFT_BOTTOM
    } else {
        egui::Align2::LEFT_TOP
    };
    let pos = egui::pos2(
        at.x.clamp(
            canvas.left() + 8.0,
            (canvas.right() - 348.0).max(canvas.left() + 8.0),
        ),
        if above { at.y - 10.0 } else { at.y + 10.0 },
    );
    egui::Area::new(egui::Id::new("text_card"))
        .order(egui::Order::Foreground)
        .pivot(pivot)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(p.panel)
                .shadow(egui::epaint::Shadow {
                    offset: [0, 6],
                    blur: 20,
                    spread: 0,
                    color: egui::Color32::from_rgba_unmultiplied(0x17, 0x1a, 0x3a, 40),
                })
                .corner_radius(12.0)
                .stroke(egui::Stroke::new(1.0_f32, p.line))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width(316.0);
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    close = contents(ui, app, now_ms);
                });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if close {
        app.state.close_text();
    }
}

/// The outlines of the text as it would be added, drawn on the mat in the accent colour. They are
/// worked out again only when something about the text changes.
fn draw_preview(ctx: &egui::Context, app: &mut App, canvas: egui::Rect, at: egui::Pos2) {
    let params = app.text_params.clone();
    if app.text_preview.as_ref().is_none_or(|(p, _)| *p != params) {
        let _ = app.fonts();
        let lines: super::TextOutlines = app
            .fonts
            .as_ref()
            .and_then(|fonts| crate::text_object::text_paths(&params, fonts).ok())
            .map(|paths| {
                paths
                    .iter()
                    .map(|path| (ps_geometry::flatten(path, 0.05), path.closed))
                    .collect()
            })
            .unwrap_or_default();
        app.text_preview = Some((params, lines));
    }
    let Some((_, lines)) = &app.text_preview else {
        return;
    };
    let zoom = app.state.viewport().zoom();
    let accent = egui::Color32::from_rgb(0xB2, 0x47, 0x00);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("text_preview"),
    ));
    let painter = painter.with_clip_rect(canvas);
    for (points, closed) in lines {
        let screen: Vec<egui::Pos2> = points
            .iter()
            .map(|q| {
                egui::pos2(
                    at.x + super::f32_of(q.x * zoom),
                    at.y + super::f32_of(q.y * zoom),
                )
            })
            .collect();
        let stroke = egui::Stroke::new(1.5_f32, accent);
        if *closed {
            painter.add(egui::Shape::closed_line(screen, stroke));
        } else {
            painter.add(egui::Shape::line(screen, stroke));
        }
    }
}

/// The card's controls; true when the card should close.
fn contents(ui: &mut egui::Ui, app: &mut App, now_ms: u64) -> bool {
    let name = ui.label(app.loc.tr("text-content"));
    let edit = ui.add(
        egui::TextEdit::multiline(&mut app.text_params.text)
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );
    edit.clone().labelled_by(name.id);
    if !app.text_focused {
        edit.request_focus();
        app.text_focused = true;
    }

    let stroke = app.text_params.stroke.is_some();
    ui.horizontal(|ui| {
        if !stroke {
            font_picker(ui, app);
        }
        number(ui, app, "text-size", |p| &mut p.size_mm, 1.0..=500.0, 0.5);
    });
    if !stroke {
        ui.horizontal(|ui| {
            number(
                ui,
                app,
                "text-spacing",
                |p| &mut p.letter_spacing_mm,
                -20.0..=100.0,
                0.1,
            );
            number(
                ui,
                app,
                "text-line-height",
                |p| &mut p.line_height,
                0.5..=4.0,
                0.05,
            );
        });
    }
    ui.horizontal(|ui| {
        for (align, key) in [
            (Align::Left, "text-left"),
            (Align::Centre, "text-centre"),
            (Align::Right, "text-right"),
        ] {
            if selectable(ui, app.text_params.align == align, app.loc.tr(key)).clicked() {
                app.text_params.align = align;
            }
        }
    });

    let mut single = stroke;
    if ui
        .checkbox(&mut single, app.loc.tr("text-stroke"))
        .changed()
    {
        app.text_params.stroke = single.then_some(StrokeFont::RomanSimplex);
        app.text_params.svg_font = None;
    }
    if let Some(current) = app.text_params.stroke {
        let loaded = app.fonts().stroke_font_names();
        let chosen_svg = app.text_params.svg_font.clone();
        ui.horizontal_wrapped(|ui| {
            for (font, key) in STROKE_FONTS {
                if selectable(ui, chosen_svg.is_none() && current == font, app.loc.tr(key))
                    .clicked()
                {
                    app.text_params.stroke = Some(font);
                    app.text_params.svg_font = None;
                }
            }
            for name in loaded {
                if selectable(
                    ui,
                    chosen_svg.as_deref() == Some(name.as_str()),
                    name.clone(),
                )
                .clicked()
                {
                    app.text_params.svg_font = Some(name);
                }
            }
        });
    }

    ui.separator();
    let mut close = false;
    ui.horizontal(|ui| {
        let typed_add = edit.has_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
        if ui.button(app.loc.tr("text-add")).clicked() || typed_add {
            close = add(app, now_ms);
        }
        if ui.button(app.loc.tr("text-cancel")).clicked() {
            close = true;
        }
    });
    close
}

fn font_picker(ui: &mut egui::Ui, app: &mut App) {
    let families = app.fonts().families();
    let label = ui.label(app.loc.tr("text-font"));
    let mut chosen = None;
    let combo = egui::ComboBox::from_id_salt("text-font")
        .selected_text(app.text_params.family.clone())
        .show_ui(ui, |ui| {
            for family in &families {
                if ui
                    .selectable_label(*family == app.text_params.family, family.clone())
                    .clicked()
                {
                    chosen = Some(family.clone());
                }
            }
        });
    combo.response.labelled_by(label.id);
    if let Some(family) = chosen {
        app.text_params.family = family;
    }
}

fn number(
    ui: &mut egui::Ui,
    app: &mut App,
    key: &str,
    field: fn(&mut crate::text_object::TextParams) -> &mut f64,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
) {
    let label = ui.label(app.loc.tr(key));
    let drag = ui.add(
        egui::DragValue::new(field(&mut app.text_params))
            .range(range)
            .speed(speed),
    );
    drag.labelled_by(label.id);
}

/// Puts the text where its preview is and reports a problem in the status bar; true when it was
/// added.
fn add(app: &mut App, now_ms: u64) -> bool {
    let params = app.text_params.clone();
    let at = app
        .text_anchor
        .unwrap_or_else(|| app.state.viewport().center());
    // Load the fonts first so the borrows below are of separate fields.
    let _ = app.fonts();
    let Some(fonts) = app.fonts.as_ref() else {
        return false;
    };
    match app.state.add_text(&params, fonts, at, now_ms) {
        Ok(()) => {
            app.notice = None;
            true
        }
        Err(why) => {
            app.notice = Some(app.loc.tr_args("text-failed", &[("why", Arg::Text(&why))]));
            false
        }
    }
}

impl App {
    /// The fonts for the text tool; the system's fonts are read the first time they are needed.
    pub(crate) fn fonts(&mut self) -> &FontLibrary {
        self.fonts.get_or_insert_with(|| {
            let mut library = FontLibrary::system();
            // Single-stroke SVG fonts (EMS, Hershey) dropped into <data>/fonts are offered too.
            if let Some(root) = ps_library::default_root() {
                library.load_stroke_fonts_from(&root.join("fonts"));
            }
            library
        })
    }
}
