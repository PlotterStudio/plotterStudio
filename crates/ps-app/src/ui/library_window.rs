//! The library window: the tools, materials and machine profiles the app knows,
//! with the built-in ones marked, installed ones removable, and a way to install a material
//! package. Installing a package that is already there asks what to do before changing anything.

use egui::{Align, Color32, Layout, Margin, Pos2, Rect, Sense, Stroke, UiBuilder, vec2};
use ps_library::{Choice, LibraryKind};

use super::App;
use super::zest::{self, Icon, Weight, font, palette, text};
use crate::l10n::Arg;
use crate::state::Effect;

/// What the window says about the last install.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LibraryNote {
    /// The sentence shown.
    pub(crate) text: String,
    /// Ids already installed, waiting for the user to say replace, keep both or skip.
    pub(crate) conflicts: Vec<String>,
    /// The package those conflicts came from, kept so the answer can finish the install.
    pub(crate) package: Vec<u8>,
}

/// Which list the window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryTab {
    /// Entries of one kind in the library.
    Kind(LibraryKind),
    /// Packages the signed online index lists.
    Online,
}

const TABS: [(LibraryTab, &str); 4] = [
    (LibraryTab::Kind(LibraryKind::Material), "library-materials"),
    (LibraryTab::Kind(LibraryKind::Tool), "library-tools"),
    (LibraryTab::Kind(LibraryKind::Profile), "library-machines"),
    (LibraryTab::Online, "library-online"),
];

impl App {
    /// Opens (creating if needed) the on-disk library under `root` and compiles against it.
    ///
    /// # Errors
    ///
    /// A message when the library cannot be opened.
    pub fn open_library(&mut self, root: &std::path::Path) -> Result<(), String> {
        let manager = crate::library_manager::LibraryManager::open(root)?;
        let snapshot = manager.snapshot()?;
        self.library = Some(manager);
        self.state.replace_libraries(snapshot);
        Ok(())
    }

    /// Installs the materials in a package, or finishes an install the user answered a conflict
    /// question for.
    pub fn install_package(&mut self, bytes: &[u8], choice: Option<Choice>) {
        let Some(manager) = self.library.as_mut() else {
            self.notice = Some(self.loc.tr("library-unavailable"));
            return;
        };
        let report = manager.install_package(bytes, choice, false);
        let snapshot = manager.snapshot();
        if !report.installed.is_empty()
            && let Ok(snapshot) = snapshot
        {
            self.state.replace_libraries(snapshot);
        }
        let mut text = if report.installed.is_empty() {
            String::new()
        } else {
            let count = f64::from(u32::try_from(report.installed.len()).unwrap_or(u32::MAX));
            self.loc
                .tr_args("library-installed", &[("count", Arg::Number(count))])
        };
        if !report.problems.is_empty() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&self.loc.tr_args(
                "library-failed",
                &[("why", Arg::Text(&report.problems.join("; ")))],
            ));
        }
        self.library_note = Some(LibraryNote {
            text,
            package: if report.needs_answer.is_empty() {
                Vec::new()
            } else {
                bytes.to_vec()
            },
            conflicts: report.needs_answer,
        });
    }

    /// Installs a package the online tab finished downloading.
    pub(super) fn finish_download(&mut self) {
        if let Some((_, bytes)) = self.online.take_package() {
            self.install_package(&bytes, None);
        }
    }

    fn remove_from_library(&mut self, kind: LibraryKind, id: &str) {
        let Some(manager) = self.library.as_mut() else {
            return;
        };
        match manager.uninstall(kind, id) {
            Ok(()) => {
                if let Ok(snapshot) = manager.snapshot() {
                    self.state.replace_libraries(snapshot);
                }
            }
            Err(e) => self.notice = Some(e),
        }
    }
}

/// The library screen of the design: tabs and search, a grid of cards, and the details of the
/// chosen card on the right.
pub(super) fn library_screen(ctx: &egui::Context, app: &mut App, now_ms: u64) {
    let p = palette(ctx);
    super::screens::roomy_header(ctx, app, now_ms, true);
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(p.ground).inner_margin(Margin {
            left: 24,
            right: 24,
            top: 16,
            bottom: 24,
        }))
        .show(ctx, |ui| {
            let aside_w = 320.0;
            let total = ui.available_rect_before_wrap();
            let main = Rect::from_min_max(
                total.min,
                Pos2::new(total.right() - aside_w - 24.0, total.bottom()),
            );
            let aside = Rect::from_min_max(
                Pos2::new(total.right() - aside_w, total.top() + 68.0),
                total.max,
            );
            ui.scope_builder(UiBuilder::new().max_rect(main), |ui| {
                main_column(ui, app, p, now_ms);
            });
            ui.scope_builder(UiBuilder::new().max_rect(aside), |ui| {
                details(ui, app, p);
            });
        });
}

fn main_column(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette, _now_ms: u64) {
    ui.spacing_mut().item_spacing = vec2(12.0, 24.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        ui.add_sized(
            [0.0, 44.0],
            egui::Label::new(text(
                ui.ctx(),
                app.loc.tr("library-title"),
                28.0,
                Weight::Semibold,
                p.ink,
            )),
        );
        ui.add_space(8.0);
        tab_strip(ui, app, p);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let label = app.loc.tr("library-install");
            let (rect, button) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::click());
            zest::shadow(ui.painter(), rect, 10, false);
            ui.painter().rect_filled(rect, 10.0, p.panel);
            zest::icon(ui.painter(), rect.center(), Icon::Plus, 18.0, p.ink);
            let name = label.clone();
            button.widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone())
            });
            if button.on_hover_text(label).clicked() {
                app.effect = Effect::InstallPackage;
            }
            search_box(ui, app, p);
        });
    });
    note(ui, app);
    let LibraryTab::Kind(kind) = app.library_tab else {
        egui::ScrollArea::vertical().show(ui, |ui| online_tab(ui, app));
        return;
    };
    let Some(manager) = app.library.as_ref() else {
        ui.label(app.loc.tr("library-unavailable"));
        return;
    };
    let query = app.library_query.to_lowercase();
    let rows: Vec<_> = manager
        .rows(kind)
        .into_iter()
        .filter(|r| query.is_empty() || r.name.to_lowercase().contains(&query))
        .collect();
    if app.library_selected.is_none() {
        app.library_selected = rows.first().map(|r| r.id.clone());
    }
    let mut remove: Option<String> = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        let gap = 20.0;
        let width = ui.available_width();
        let (columns, card_w) = grid_columns(width, gap);
        ui.spacing_mut().item_spacing = vec2(gap, gap);
        for chunk in rows.chunks(columns) {
            ui.horizontal(|ui| {
                for row in chunk {
                    card(ui, app, p, kind, row, card_w, &mut remove);
                }
            });
        }
    });
    if let Some(id) = remove {
        app.remove_from_library(kind, &id);
    }
}

/// How many cards of at least 190 points fit across `width`, and how wide each then is.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "a handful of columns, counted from a screen width"
)]
fn grid_columns(width: f32, gap: f32) -> (usize, f32) {
    let columns = (((width + gap) / (190.0 + gap)).floor().max(1.0)) as usize;
    let card = (width - gap * (columns as f32 - 1.0)) / columns as f32;
    (columns, card)
}

fn tab_strip(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette) {
    egui::Frame::NONE
        .fill(p.panel)
        .corner_radius(10.0)
        .shadow(egui::epaint::Shadow {
            offset: [0, 6],
            blur: 20,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(0x17, 0x1a, 0x3a, 20),
        })
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (tab, key) in TABS {
                let selected = app.library_tab == tab;
                let label = app.loc.tr(key);
                let colour = if selected { p.accent } else { p.muted };
                let galley = ui.painter().layout_no_wrap(
                    label.clone(),
                    font(
                        ui.ctx(),
                        13.0,
                        if selected {
                            Weight::Semibold
                        } else {
                            Weight::Medium
                        },
                    ),
                    colour,
                );
                let (rect, response) =
                    ui.allocate_exact_size(vec2(galley.size().x + 28.0, 34.0), Sense::click());
                if selected {
                    ui.painter().rect_filled(rect, 6.0, p.tint);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 6.0, p.ground);
                }
                ui.painter().galley(
                    Pos2::new(rect.left() + 14.0, rect.center().y - galley.size().y / 2.0),
                    galley,
                    colour,
                );
                let name = label.clone();
                response.widget_info(move || {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        true,
                        selected,
                        name.clone(),
                    )
                });
                if response.clicked() {
                    app.library_tab = tab;
                    app.library_selected = None;
                }
            }
        });
}

fn search_box(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette) {
    egui::Frame::NONE
        .fill(p.panel)
        .corner_radius(10.0)
        .shadow(egui::epaint::Shadow {
            offset: [0, 6],
            blur: 20,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(0x17, 0x1a, 0x3a, 20),
        })
        .inner_margin(Margin::symmetric(14, 0))
        .show(ui, |ui| {
            ui.set_height(44.0);
            ui.set_width(232.0);
            ui.horizontal_centered(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                zest::icon(ui.painter(), rect.center(), Icon::Search, 16.0, p.muted);
                let hint = app.loc.tr("library-search");
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut app.library_query)
                        .hint_text(hint.clone())
                        .frame(false)
                        .desired_width(f32::INFINITY),
                );
                edit.widget_info(move || {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, hint.clone())
                });
            });
        });
}

fn card(
    ui: &mut egui::Ui,
    app: &mut App,
    p: crate::theme::Palette,
    kind: LibraryKind,
    row: &crate::library_manager::LibraryRow,
    width: f32,
    remove: &mut Option<String>,
) {
    let selected = app.library_selected.as_deref() == Some(row.id.as_str());
    let height = 10.0 + 128.0 + 12.0 + 20.0 + 14.0 + if row.builtin { 0.0 } else { 20.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    if selected {
        zest::shadow(ui.painter(), rect, 16, true);
    } else {
        zest::shadow(ui.painter(), rect, 16, false);
    }
    ui.painter().rect_filled(rect, 16.0, p.panel);
    if selected {
        ui.painter().rect_stroke(
            rect,
            16.0,
            Stroke::new(2.0_f32, p.orange),
            egui::StrokeKind::Inside,
        );
    }
    let swatch = Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(width - 20.0, 128.0));
    paint_swatch(ui, app, p, kind, &row.id, &row.name, swatch);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, swatch.bottom() + 22.0),
        egui::Align2::LEFT_CENTER,
        &row.name,
        font(ui.ctx(), 14.0, Weight::Semibold),
        p.ink,
    );
    if row.builtin {
        let label = app.loc.tr("library-builtin");
        ui.painter().text(
            Pos2::new(rect.right() - 14.0, swatch.bottom() + 22.0),
            egui::Align2::RIGHT_CENTER,
            &label,
            font(ui.ctx(), 11.0, Weight::Regular),
            p.muted,
        );
        let badge = Rect::from_center_size(
            Pos2::new(rect.right() - 40.0, swatch.bottom() + 22.0),
            vec2(56.0, 16.0),
        );
        ui.interact(badge, response.id.with("builtin"), Sense::hover())
            .widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.clone())
            });
    }
    let name = row.name.clone();
    response.widget_info(move || {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, name.clone())
    });
    if response.clicked() {
        app.library_selected = Some(row.id.clone());
    }
    if !row.builtin {
        let remove_rect = Rect::from_min_size(
            Pos2::new(rect.left() + 10.0, rect.bottom() - 28.0),
            vec2(80.0, 22.0),
        );
        let button = ui.put(
            remove_rect,
            egui::Button::new(text(
                ui.ctx(),
                app.loc.tr("library-remove"),
                12.0,
                Weight::Medium,
                p.danger,
            ))
            .frame(false),
        );
        if button.clicked() {
            *remove = Some(row.id.clone());
        }
    }
}

/// The picture on a card: a material's look, or a tinted tile with the name's initial.
fn paint_swatch(
    ui: &mut egui::Ui,
    app: &App,
    p: crate::theme::Palette,
    kind: LibraryKind,
    id: &str,
    name: &str,
    rect: Rect,
) {
    let base = if kind == LibraryKind::Material {
        app.state
            .libraries()
            .materials()
            .iter()
            .find(|m| m.id == id)
            .map(|m| crate::state::material_swatch(&m.category))
    } else {
        None
    };
    let fill = base.map_or(p.ground, |[r, g, b]| Color32::from_rgb(r, g, b));
    ui.painter().rect_filled(rect, 10.0, fill);
    if base.is_none() {
        let initial = name.chars().next().map(String::from).unwrap_or_default();
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            initial,
            font(ui.ctx(), 40.0, Weight::Semibold),
            p.outline,
        );
    } else {
        ui.painter().rect_stroke(
            rect,
            10.0,
            Stroke::new(1.0_f32, Color32::from_black_alpha(14)),
            egui::StrokeKind::Inside,
        );
    }
}

/// The chosen card's details: its name, what it is for, and its settings.
fn details(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette) {
    let LibraryTab::Kind(kind) = app.library_tab else {
        return;
    };
    let Some(id) = app.library_selected.clone() else {
        return;
    };
    let Some(row) = app
        .library
        .as_ref()
        .and_then(|m| m.rows(kind).into_iter().find(|r| r.id == id))
    else {
        return;
    };
    let lines = detail_lines(app, kind, &id);
    egui::Frame::NONE
        .fill(p.panel)
        .corner_radius(20.0)
        .shadow(egui::epaint::Shadow {
            offset: [0, 6],
            blur: 20,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(0x17, 0x1a, 0x3a, 20),
        })
        .inner_margin(Margin::same(20))
        .show(ui, |ui| {
            ui.set_width(280.0);
            ui.spacing_mut().item_spacing.y = 20.0;
            let (swatch, _) = ui.allocate_exact_size(vec2(280.0, 140.0), Sense::hover());
            paint_swatch(ui, app, p, kind, &id, &row.name, swatch);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(text(ui.ctx(), &row.name, 20.0, Weight::Semibold, p.ink));
                ui.label(text(
                    ui.ctx(),
                    format!("{} · {}", row.id, row.version),
                    13.0,
                    Weight::Regular,
                    p.muted,
                ));
            });
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, line) in lines.iter().enumerate() {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(280.0, 44.0), Sense::hover());
                    if index + 1 < lines.len() {
                        zest::hairline(
                            ui.painter(),
                            Pos2::new(rect.left(), rect.bottom() - 0.5),
                            Pos2::new(rect.right(), rect.bottom() - 0.5),
                            p.line,
                        );
                    }
                    let mut x = rect.left();
                    if let Some((colour, style)) = line.swatch {
                        zest::line_swatch(
                            ui.painter(),
                            Pos2::new(x, rect.center().y),
                            18.0,
                            colour,
                            style,
                            2.5,
                        );
                        x += 30.0;
                    }
                    ui.painter().text(
                        Pos2::new(x, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        &line.label,
                        font(ui.ctx(), 14.0, Weight::Regular),
                        p.ink,
                    );
                    ui.painter().text(
                        Pos2::new(rect.right(), rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        &line.value,
                        font(ui.ctx(), 14.0, Weight::Semibold),
                        p.ink,
                    );
                    let said = format!("{}: {}", line.label, line.value);
                    response.widget_info(move || {
                        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, said.clone())
                    });
                }
            });
        });
}

/// One row of the details: a label, a value, and for operations a line swatch.
struct DetailLine {
    label: String,
    value: String,
    swatch: Option<(Color32, ps_model::LineStyle)>,
}

fn detail_lines(app: &App, kind: LibraryKind, id: &str) -> Vec<DetailLine> {
    let libs = app.state.libraries();
    match kind {
        LibraryKind::Material => libs
            .materials()
            .iter()
            .find(|m| m.id == id)
            .map(|m| {
                let machine = app.state.current_machine_id().unwrap_or_default();
                m.presets
                    .iter()
                    .filter_map(|preset| {
                        let op = ps_model::OpKind::ALL
                            .into_iter()
                            .find(|k| k.as_str() == preset.operation)?;
                        let force = preset.params_for(&machine).get("force").copied()?;
                        Some(DetailLine {
                            label: app.loc.tr(&format!("op-{}", op.as_str())),
                            value: format!("{force:.0} gf"),
                            swatch: Some((super::side::op_colour(op), ps_render::style_of(op))),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        LibraryKind::Tool => libs
            .tools()
            .iter()
            .find(|t| t.id == id)
            .map(|t| {
                t.parameters
                    .iter()
                    .map(|(name, spec)| DetailLine {
                        label: name.replace(['_', '-'], " "),
                        value: format!(
                            "{} {}",
                            spec.default,
                            spec.unit.clone().unwrap_or_default()
                        )
                        .trim()
                        .to_owned(),
                        swatch: None,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        LibraryKind::Profile => libs
            .profiles()
            .iter()
            .find(|m| m.id == id)
            .map(|m| {
                vec![
                    DetailLine {
                        label: app.loc.tr("library-work-area"),
                        value: format!(
                            "{:.0} × {:.0} mm",
                            m.work_area.width_mm, m.work_area.height_mm
                        ),
                        swatch: None,
                    },
                    DetailLine {
                        label: app.loc.tr("library-top-speed"),
                        value: format!("{:.0} mm/s", m.max_speed_mm_s),
                        swatch: None,
                    },
                ]
            })
            .unwrap_or_default(),
    }
}

/// The packages the signed index lists, each with an install button.
fn online_tab(ui: &mut egui::Ui, app: &mut App) {
    if !app.state.settings().network.material_index {
        ui.label(app.loc.tr("library-online-off"));
        return;
    }
    ui.horizontal(|ui| {
        let busy = app.online.refreshing_index();
        if ui
            .add_enabled(!busy, egui::Button::new(app.loc.tr("library-refresh")))
            .clicked()
        {
            let _ = app.online.refresh_material_index();
        }
        if app.online.downloading() {
            ui.label(app.loc.tr("library-downloading"));
        }
    });
    if let Some(error) = app.online.last_error() {
        ui.label(error);
    }
    let Some(index) = app.online.index() else {
        ui.label(app.loc.tr("library-online-empty"));
        return;
    };
    let packages = index.packages.clone();
    let mut install = None;
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .show(ui, |ui| {
            for package in &packages {
                ui.horizontal(|ui| {
                    ui.label(&package.name);
                    ui.weak(package.version.to_string());
                    if ui.button(app.loc.tr("library-online-install")).clicked() {
                        install = Some(package.clone());
                    }
                });
            }
        });
    if let Some(entry) = install {
        let _ = app.online.download_package(&entry);
    }
}

fn note(ui: &mut egui::Ui, app: &mut App) {
    let Some(note) = app.library_note.clone() else {
        return;
    };
    if !note.text.is_empty() {
        ui.label(&note.text);
    }
    if note.conflicts.is_empty() {
        return;
    }
    for id in &note.conflicts {
        ui.label(
            app.loc
                .tr_args("library-conflict", &[("id", Arg::Text(id))]),
        );
    }
    ui.horizontal(|ui| {
        for (key, choice) in [
            ("library-replace", Choice::Replace),
            ("library-keep-both", Choice::KeepBoth),
            ("library-skip", Choice::Skip),
        ] {
            if ui.button(app.loc.tr(key)).clicked() {
                app.library_note = None;
                app.install_package(&note.package, Some(choice));
            }
        }
    });
}
