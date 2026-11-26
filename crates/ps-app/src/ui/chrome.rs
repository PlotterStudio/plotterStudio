//! The frame of the Prepare screen from the design: the 36 px menu and mode bar, the 40 px
//! control bar with the selection's position, size and arrange tools, the tool strip, and the
//! 24 px status bar.

use egui::{Align, Color32, Layout, Margin, Pos2, Rect, Sense, Stroke, UiBuilder, vec2};

use super::zest::{self, Icon, Weight, font, hairline, palette, text};
use super::{App, machine_panel};
use crate::arrange::anchor_point;
use crate::state::Mode;
use crate::tools::ToolId;

const HEADER_H: f32 = 36.0;
const CONTROL_H: f32 = 40.0;
const STATUS_H: f32 = 24.0;
/// Where the control bar's content starts: clear of the 40 px tool strip.
const CONTROL_INSET: f32 = 50.0;

/// The menu bar groups: a title key and the command-id prefixes each collects.
const MENUS: [(&str, &[&str]); 9] = [
    ("menu-file", &["file.", "settings."]),
    ("menu-edit", &["edit.", "palette."]),
    ("menu-object", &["node.", "tool.start-point", "text."]),
    ("menu-arrange", &["arrange."]),
    ("menu-operations", &[]),
    ("menu-mat", &["mat."]),
    ("menu-view", &["view.", "focus.", "mode."]),
    ("menu-machine", &["machine."]),
    ("menu-help", &[]),
];

/// Commands that start a new group in their menu, besides where the kind of command changes.
const SEPARATE_BEFORE: [&str; 5] = [
    "file.save",
    "file.export",
    "edit.delete",
    "edit.select-all",
    "view.zoom-fit",
];

/// The compact header of the Prepare screen.
pub(super) fn prepare_header(ctx: &egui::Context, app: &mut App, now_ms: u64) {
    let p = palette(ctx);
    egui::TopBottomPanel::top("header")
        .show_separator_line(false)
        .exact_height(HEADER_H)
        .frame(egui::Frame::NONE.fill(p.panel))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            hairline(
                ui.painter(),
                Pos2::new(full.left(), full.bottom() - 0.5),
                Pos2::new(full.right(), full.bottom() - 0.5),
                p.line,
            );
            let centre_w = 250.0;
            let centre = Rect::from_center_size(full.center(), vec2(centre_w, HEADER_H));
            let left = Rect::from_min_max(
                Pos2::new(full.left() + 8.0, full.top()),
                Pos2::new(centre.left() - 8.0, full.bottom()),
            );
            let right = Rect::from_min_max(
                Pos2::new(centre.right() + 8.0, full.top()),
                Pos2::new(full.right() - 8.0, full.bottom()),
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(left)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| left_group(ui, app, now_ms),
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(centre)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| mode_switch(ui, app, now_ms),
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(right)
                    .layout(Layout::right_to_left(Align::Center)),
                |ui| right_group(ui, app, now_ms),
            );
        });
}

fn left_group(ui: &mut egui::Ui, app: &mut App, now_ms: u64) {
    let p = palette(ui.ctx());
    ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
    zest::logo(ui, 20.0, p.ink);
    ui.add_space(4.0);
    // A real menu bar: once one menu is open, moving over another opens that one instead.
    egui::MenuBar::new().ui(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
        menus(ui, app, p, now_ms);
    });
}

/// The menu bar on its own strip, for the screens that have no toolbar to hold it, so the menus
/// are there on every screen.
pub(super) fn menu_strip(ctx: &egui::Context, app: &mut App, now_ms: u64) {
    let p = palette(ctx);
    egui::TopBottomPanel::top("menu_strip")
        .show_separator_line(false)
        .exact_height(30.0)
        .frame(
            egui::Frame::NONE
                .fill(p.panel)
                .inner_margin(Margin::symmetric(12, 0)),
        )
        .show(ctx, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
                    menus(ui, app, p, now_ms);
                });
            });
        });
}

fn menus(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette, now_ms: u64) {
    for (key, prefixes) in MENUS {
        let title = app.loc.tr(key);
        let operations = key == "menu-operations";
        let help = key == "menu-help";
        let items = menu_items(app, prefixes);
        if items.is_empty() && !operations && !help {
            continue;
        }
        ui.scope(|ui| {
            let v = ui.visuals_mut();
            for w in [
                &mut v.widgets.inactive,
                &mut v.widgets.hovered,
                &mut v.widgets.active,
                &mut v.widgets.open,
            ] {
                w.bg_stroke = Stroke::NONE;
                w.corner_radius = egui::CornerRadius::same(3);
            }
            v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
            v.widgets.hovered.weak_bg_fill = p.hover;
            v.widgets.open.weak_bg_fill = p.hover;
            ui.spacing_mut().button_padding = vec2(8.0, 0.0);
            ui.spacing_mut().interact_size.y = 26.0;
            ui.menu_button(text(ui.ctx(), title, 12.0, Weight::Regular, p.ink), |ui| {
                ui.set_min_width(190.0);
                let mut previous: Option<&str> = None;
                for item in &items {
                    // A rule where the kind of command changes, as in the system's own menus.
                    let group = item.id.split('.').next().unwrap_or(item.id);
                    if previous.is_some_and(|before| before != group)
                        || SEPARATE_BEFORE.contains(&item.id)
                    {
                        ui.separator();
                    }
                    previous = Some(group);
                    let mut button = egui::Button::new(&item.label);
                    if let Some(shortcut) = &item.shortcut {
                        button = button.shortcut_text(shortcut.clone());
                    }
                    if ui.add_enabled(item.enabled, button).clicked() {
                        app.run(item.id, now_ms);
                        ui.close();
                    }
                }
                if operations {
                    super::extras::assign_menu(ui, app, now_ms);
                }
                if help {
                    if ui.button(app.loc.tr("menu-help-machine")).clicked() {
                        app.machine_open = true;
                        ui.close();
                    }
                    if ui.button(app.loc.tr("library-button")).clicked() {
                        app.library_open = true;
                        ui.close();
                    }
                }
            });
        });
    }
}

struct MenuItem {
    id: &'static str,
    label: String,
    shortcut: Option<String>,
    enabled: bool,
}

fn menu_items(app: &App, prefixes: &[&str]) -> Vec<MenuItem> {
    let context = app.state.context();
    app.state
        .registry()
        .all()
        .iter()
        .filter(|c| {
            prefixes.iter().any(|prefix| {
                if prefix.ends_with('.') {
                    c.id.starts_with(prefix)
                } else {
                    c.id == *prefix
                }
            })
        })
        .filter(|c| !c.id.starts_with("nudge."))
        .map(|c| MenuItem {
            id: c.id,
            label: app.loc.command_label(c.id),
            shortcut: app
                .state
                .registry()
                .shortcut_of(c.id)
                .map(|s| s.display(crate::commands::Platform::current())),
            enabled: app.state.registry().is_enabled(c.id, &context),
        })
        .collect()
}

/// The Prepare / Preview / Export segmented switch.
pub(super) fn mode_switch(ui: &mut egui::Ui, app: &mut App, now_ms: u64) {
    let p = palette(ui.ctx());
    let warnings = app.state.warning_count();
    let frame = egui::Frame::NONE
        .fill(p.ground)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(4.0)
        .inner_margin(Margin::same(2));
    frame.show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
        for (mode, key, id) in [
            (Mode::Prepare, "mode-prepare", "mode.prepare"),
            (Mode::Preview, "mode-preview", "mode.preview"),
            (Mode::Export, "mode-export", "mode.export"),
        ] {
            let selected = app.state.mode() == mode;
            let label = app.loc.tr(key);
            let badge = (mode == Mode::Export && warnings > 0).then_some(warnings);
            let response = segment(ui, p, &label, selected, badge, 24.0, 14.0);
            if mode == Mode::Prepare && app.focus_target == Some(super::FocusPanel::TopBar) {
                response.request_focus();
            }
            if response.clicked() {
                app.library_open = false;
                app.run(id, now_ms);
            }
        }
    });
}

/// One segment of a mode switch: gradient when selected, with an optional warning count.
pub(super) fn segment(
    ui: &mut egui::Ui,
    p: crate::theme::Palette,
    label: &str,
    selected: bool,
    badge: Option<usize>,
    height: f32,
    pad: f32,
) -> egui::Response {
    let size = if selected { 13.0 } else { 12.0 };
    let (weight, colour) = if selected {
        (Weight::Semibold, p.ink)
    } else {
        (Weight::Medium, p.muted)
    };
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), font(ui.ctx(), size, weight), colour);
    let badge_w = if badge.is_some() { 20.0 } else { 0.0 };
    let width = galley.size().x + pad * 2.0 + badge_w;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    if selected {
        zest::gradient_rect(ui.painter(), rect, 3.0);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 3.0, p.hover);
    }
    ui.painter().galley(
        Pos2::new(rect.left() + pad, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );
    if let Some(count) = badge {
        let badge_rect = Rect::from_center_size(
            Pos2::new(rect.right() - pad - 5.0, rect.center().y),
            vec2(14.0, 14.0),
        );
        ui.painter()
            .rect_filled(badge_rect, 7.0, Color32::from_rgb(0xff, 0xe4, 0xe4));
        ui.painter().text(
            badge_rect.center(),
            egui::Align2::CENTER_CENTER,
            count.to_string(),
            font(ui.ctx(), 9.0, Weight::Semibold),
            p.danger,
        );
    }
    let name = label.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, name.clone())
    });
    response
}

fn right_group(ui: &mut egui::Ui, app: &mut App, _now_ms: u64) {
    let p = palette(ui.ctx());
    ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
    machine_chip(ui, app, p);
    let library = zest::icon_button(
        ui,
        p,
        Icon::Book,
        vec2(26.0, 26.0),
        15.0,
        app.library_open,
        &app.loc.tr("library-button"),
    );
    if library.clicked() {
        app.library_open = !app.library_open;
    }
    let (rule, _) = ui.allocate_exact_size(vec2(1.0, 16.0), Sense::hover());
    ui.painter().rect_filled(rule, 0.0, p.line);
    let name = app.state.window_title();
    let name = name.trim_end_matches(" — Plotter Studio");
    let saved = if app.state.is_dirty() {
        app.loc.tr("status-unsaved")
    } else {
        app.loc.tr("status-saved")
    };
    ui.label(text(
        ui.ctx(),
        format!("{} · {saved}", name.trim_end_matches('*')),
        12.0,
        Weight::Regular,
        p.muted,
    ));
}

/// The machine chip: a status dot, the model and its state.
pub(super) fn machine_chip(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette) {
    let label = machine_panel::chip_text(app);
    let ready = app.machine.is_ready();
    let galley =
        ui.painter()
            .layout_no_wrap(label.clone(), font(ui.ctx(), 12.0, Weight::Medium), p.ink);
    let size = vec2(galley.size().x + 8.0 + 7.0 + 6.0 + 8.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    ui.painter().rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0_f32, p.line),
        egui::StrokeKind::Inside,
    );
    if response.hovered() {
        ui.painter().rect_filled(rect.shrink(1.0), 3.0, p.hover);
    }
    let dot = if ready {
        p.ok
    } else {
        Color32::from_rgb(0xff, 0x8a, 0x1f)
    };
    ui.painter()
        .circle_filled(Pos2::new(rect.left() + 11.5, rect.center().y), 3.5, dot);
    ui.painter().galley(
        Pos2::new(
            rect.left() + 8.0 + 7.0 + 6.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        p.ink,
    );
    let name = label.clone();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone())
    });
    if response.clicked() {
        app.machine_open = !app.machine_open;
    }
    machine_panel::machine_popup(&response, app, 0);
}

// ---- control bar ----

/// The control bar under the header: the selection's name, position, size and arrange tools.
pub(super) fn control_bar(ctx: &egui::Context, app: &mut App, now_ms: u64) {
    let p = palette(ctx);
    egui::TopBottomPanel::top("control_bar")
        .show_separator_line(false)
        .exact_height(CONTROL_H)
        .frame(egui::Frame::NONE.fill(p.panel))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            hairline(
                ui.painter(),
                Pos2::new(full.left(), full.bottom() - 0.5),
                Pos2::new(full.right(), full.bottom() - 0.5),
                p.line,
            );
            let inner = Rect::from_min_max(
                Pos2::new(full.left() + CONTROL_INSET, full.top() + 5.0),
                Pos2::new(full.right() - 10.0, full.bottom() - 5.0),
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(inner)
                    .layout(Layout::left_to_right(Align::Center).with_main_wrap(true)),
                |ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    control_contents(ui, app, now_ms);
                },
            );
        });
}

fn divider(ui: &mut egui::Ui) {
    let p = palette(ui.ctx());
    ui.add_space(3.0);
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, 22.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, p.line);
    ui.add_space(3.0);
}

/// How much is selected: paths, or shapes inside an object, and how to reach an object's shapes.
fn selection_counts(ui: &mut egui::Ui, app: &App, selected: usize) {
    let muted = palette(ui.ctx()).muted;
    if let Some(paths) = app.state.selected_path_count() {
        let count = crate::l10n::Arg::Number(super::count_f64(paths));
        let total = app.state.selected_object_path_count();
        let said = if app.state.has_shape_selection() {
            let total = crate::l10n::Arg::Number(super::count_f64(total));
            app.loc
                .tr_args("control-shapes", &[("count", count), ("total", total)])
        } else {
            app.loc.tr_args("control-paths", &[("count", count)])
        };
        ui.label(text(ui.ctx(), said, 12.0, Weight::Regular, muted));
        // Say how to get at the shapes of an object that has several.
        if !app.state.has_shape_selection() && selected == 1 && total > 1 {
            ui.label(text(
                ui.ctx(),
                app.loc.tr("control-pick-shape"),
                12.0,
                Weight::Regular,
                muted,
            ));
        }
    }
}

fn control_contents(ui: &mut egui::Ui, app: &mut App, now_ms: u64) {
    let p = palette(ui.ctx());
    let selected = app.state.selection().objects().len();
    let name = match selected {
        0 => app.loc.tr("inspector-nothing"),
        1 => {
            let n = app
                .state
                .current_mat()
                .objects
                .keys()
                .position(|id| app.state.selection().objects().contains(id))
                .map_or(1, |i| i + 1);
            app.loc.tr_args(
                "tree-object",
                &[("n", crate::l10n::Arg::Number(super::count_f64(n)))],
            )
        }
        many => app.loc.tr_args(
            "inspector-multi",
            &[("count", crate::l10n::Arg::Number(super::count_f64(many)))],
        ),
    };
    let heard = app.loc.tr_args(
        "control-selection",
        &[("name", crate::l10n::Arg::Text(&name))],
    );
    let shown = ui.label(text(ui.ctx(), name, 12.0, Weight::Semibold, p.ink));
    shown.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, heard.clone())
    });
    super::tool_options(ui, app);
    selection_counts(ui, app, selected);
    let Some(bounds) = app.state.selection_bounds() else {
        return;
    };
    let fmt = app.format();
    let at = anchor_point(bounds, app.anchor);
    let (mut pos_x, mut pos_y) = (fmt.display_from_mm(at.x), fmt.display_from_mm(at.y));
    let (mut size_w, mut size_h) = (
        fmt.display_from_mm(bounds.width()),
        fmt.display_from_mm(bounds.height()),
    );
    ui.add_space(4.0);
    let moved = [
        number_field(ui, "X", &app.loc.tr("arrange-x"), &mut pos_x, 66.0),
        number_field(ui, "Y", &app.loc.tr("arrange-y"), &mut pos_y, 66.0),
    ];
    if moved.contains(&true) {
        app.state.move_selection_to(
            app.anchor,
            fmt.mm_from_display(pos_x),
            fmt.mm_from_display(pos_y),
            now_ms,
        );
    }
    let width = number_field(ui, "W", &app.loc.tr("arrange-width"), &mut size_w, 66.0);
    let lock_label = app.loc.tr("arrange-keep-proportions");
    let lock = zest::icon_button(
        ui,
        p,
        Icon::Link,
        vec2(20.0, 24.0),
        13.0,
        app.keep_proportions,
        &lock_label,
    );
    if lock.clicked() {
        app.keep_proportions = !app.keep_proportions;
    }
    let height = number_field(ui, "H", &app.loc.tr("arrange-height"), &mut size_h, 66.0);
    if width || height {
        let (new_w, new_h) = (fmt.mm_from_display(size_w), fmt.mm_from_display(size_h));
        let (new_w, new_h, lock) = if app.keep_proportions && height && !width {
            (new_h * bounds.width() / bounds.height(), new_h, true)
        } else {
            (new_w, new_h, app.keep_proportions)
        };
        app.state
            .resize_selection(app.anchor, new_w, new_h, lock, now_ms);
    }
    anchor_picker(ui, app, p);
    divider(ui);
    arrange_groups(ui, app, p, selected > 1, now_ms);
}

/// A labelled numeric field: a 24 px tile with a 10 px caption and the value.
fn number_field(ui: &mut egui::Ui, caption: &str, name: &str, value: &mut f64, width: f32) -> bool {
    let p = palette(ui.ctx());
    let mut changed = false;
    egui::Frame::NONE
        .fill(p.ground)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(3.0)
        .inner_margin(Margin::symmetric(6, 0))
        .show(ui, |ui| {
            ui.set_width(width - 12.0);
            ui.set_height(22.0);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                ui.label(text(ui.ctx(), caption, 10.0, Weight::Regular, p.muted));
                ui.scope(|ui| {
                    let v = ui.visuals_mut();
                    for w in [
                        &mut v.widgets.inactive,
                        &mut v.widgets.hovered,
                        &mut v.widgets.active,
                    ] {
                        w.bg_fill = Color32::TRANSPARENT;
                        w.weak_bg_fill = Color32::TRANSPARENT;
                        w.bg_stroke = Stroke::NONE;
                    }
                    let drag = ui.add(egui::DragValue::new(value).speed(0.1).fixed_decimals(2));
                    changed = drag.changed();
                    let full = name.to_owned();
                    let current = *value;
                    drag.widget_info(move || {
                        let mut info = egui::WidgetInfo::labeled(
                            egui::WidgetType::DragValue,
                            true,
                            full.clone(),
                        );
                        info.value = Some(current);
                        info
                    });
                });
            });
        });
    changed
}

fn anchor_picker(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette) {
    let label = app.loc.tr("arrange-anchor");
    let response = zest::icon_button(
        ui,
        p,
        Icon::ChevronDown,
        vec2(20.0, 24.0),
        12.0,
        false,
        &label,
    );
    egui::Popup::menu(&response).show(|ui| {
        egui::Grid::new("anchors").show(ui, |ui| {
            for (row, chunk) in super::arrange_panel::ANCHOR_KEYS.chunks(3).enumerate() {
                for (anchor, key, glyph) in chunk {
                    let selected = app.anchor == *anchor;
                    let r = ui.selectable_label(selected, *glyph);
                    let name = app.loc.tr(key);
                    r.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            true,
                            selected,
                            name.clone(),
                        )
                    });
                    if r.clicked() {
                        app.anchor = *anchor;
                    }
                }
                let _ = row;
                ui.end_row();
            }
        });
    });
}

/// Align, distribute, mirror, rotate and array buttons, in the design's bordered groups.
fn arrange_groups(
    ui: &mut egui::Ui,
    app: &mut App,
    p: crate::theme::Palette,
    many: bool,
    now_ms: u64,
) {
    let groups: [&[(&str, Glyph)]; 4] = [
        &[
            ("arrange.align-left", Glyph::AlignLeft),
            ("arrange.align-centre-h", Glyph::AlignCentreH),
            ("arrange.align-right", Glyph::AlignRight),
        ],
        &[
            ("arrange.align-top", Glyph::AlignTop),
            ("arrange.align-centre-v", Glyph::AlignCentreV),
            ("arrange.align-bottom", Glyph::AlignBottom),
        ],
        &[
            ("arrange.distribute-h", Glyph::DistributeH),
            ("arrange.distribute-v", Glyph::DistributeV),
        ],
        &[
            ("arrange.mirror-h", Glyph::FlipH),
            ("arrange.mirror-v", Glyph::FlipV),
            ("arrange.rotate-ccw", Glyph::RotateCcw),
            ("arrange.rotate-cw", Glyph::RotateCw),
        ],
    ];
    for (index, group) in groups.iter().enumerate() {
        let aligning = index < 3;
        if index == 2 {
            divider(ui);
        }
        if index == 3 {
            divider(ui);
        }
        group_frame(ui, p, |ui| {
            for (i, (id, glyph)) in group.iter().enumerate() {
                let enabled = !aligning || many;
                if i > 0 {
                    let (rule, _) = ui.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
                    ui.painter().rect_filled(rule, 0.0, p.line);
                }
                let label = app.loc.command_label(id);
                if glyph_button(ui, p, *glyph, enabled, &label).clicked() {
                    app.run(id, now_ms);
                }
            }
        });
    }
    array_buttons(ui, app, p, now_ms);
}

fn group_frame(ui: &mut egui::Ui, p: crate::theme::Palette, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .stroke(Stroke::new(1.0_f32, p.outline))
        .corner_radius(3.0)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ui.horizontal(add);
        });
}

fn array_buttons(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette, now_ms: u64) {
    group_frame(ui, p, |ui| {
        let grid = glyph_button(ui, p, Glyph::Grid, true, &app.loc.tr("arrange-grid-title"));
        egui::Popup::menu(&grid).show(|ui| super::arrange_panel::grid_form(ui, app, now_ms));
        let (rule, _) = ui.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
        ui.painter().rect_filled(rule, 0.0, p.line);
        let circle = glyph_button(
            ui,
            p,
            Glyph::Circle,
            true,
            &app.loc.tr("arrange-circle-title"),
        );
        egui::Popup::menu(&circle).show(|ui| super::arrange_panel::circle_form(ui, app, now_ms));
    });
    if app.state.has_selected_node() {
        divider(ui);
        super::arrange_panel::node_buttons(ui, app, now_ms);
    }
}

/// The 16 px arrange glyphs.
#[derive(Debug, Clone, Copy)]
enum Glyph {
    AlignLeft,
    AlignCentreH,
    AlignRight,
    AlignTop,
    AlignCentreV,
    AlignBottom,
    DistributeH,
    DistributeV,
    FlipH,
    FlipV,
    RotateCw,
    RotateCcw,
    Grid,
    Circle,
}

fn glyph_button(
    ui: &mut egui::Ui,
    p: crate::theme::Palette,
    glyph: Glyph,
    enabled: bool,
    label: &str,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 24.0), sense);
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 3.0, p.hover);
    }
    let colour = if enabled { p.ink } else { p.outline };
    draw_glyph(ui.painter(), rect.center(), glyph, colour);
    let name = label.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name.clone())
    });
    response.on_hover_text(label)
}

#[expect(
    clippy::too_many_lines,
    reason = "straight-line drawing of one panel; splitting it would only scatter the layout"
)]
fn draw_glyph(painter: &egui::Painter, c: Pos2, glyph: Glyph, colour: Color32) {
    let s = Stroke::new(1.5_f32, colour);
    let at = |x: f32, y: f32| Pos2::new(c.x + x, c.y + y);
    let bar = |x: f32, y: f32, w: f32, h: f32| {
        painter.rect_stroke(
            Rect::from_min_size(at(x, y), vec2(w, h)),
            1.0,
            s,
            egui::StrokeKind::Middle,
        );
    };
    let seg = |a: Pos2, b: Pos2| {
        painter.line_segment([a, b], s);
    };
    match glyph {
        Glyph::AlignLeft => {
            seg(at(-6.0, -6.5), at(-6.0, 6.5));
            bar(-4.0, -4.5, 9.0, 3.5);
            bar(-4.0, 1.0, 5.0, 3.5);
        }
        Glyph::AlignCentreH => {
            seg(at(0.0, -6.5), at(0.0, 6.5));
            bar(-5.0, -4.5, 10.0, 3.5);
            bar(-3.0, 1.0, 6.0, 3.5);
        }
        Glyph::AlignRight => {
            seg(at(6.0, -6.5), at(6.0, 6.5));
            bar(-5.0, -4.5, 9.0, 3.5);
            bar(-1.0, 1.0, 5.0, 3.5);
        }
        Glyph::AlignTop => {
            seg(at(-6.5, -6.0), at(6.5, -6.0));
            bar(-4.5, -4.0, 3.5, 9.0);
            bar(1.0, -4.0, 3.5, 5.0);
        }
        Glyph::AlignCentreV => {
            seg(at(-6.5, 0.0), at(6.5, 0.0));
            bar(-4.5, -5.0, 3.5, 10.0);
            bar(1.0, -3.0, 3.5, 6.0);
        }
        Glyph::AlignBottom => {
            seg(at(-6.5, 6.0), at(6.5, 6.0));
            bar(-4.5, -3.0, 3.5, 9.0);
            bar(1.0, 1.0, 3.5, 5.0);
        }
        Glyph::DistributeH => {
            seg(at(-6.0, -6.5), at(-6.0, 6.5));
            seg(at(6.0, -6.5), at(6.0, 6.5));
            bar(-2.0, -4.0, 4.0, 8.0);
        }
        Glyph::DistributeV => {
            seg(at(-6.5, -6.0), at(6.5, -6.0));
            seg(at(-6.5, 6.0), at(6.5, 6.0));
            bar(-4.0, -2.0, 8.0, 4.0);
        }
        Glyph::FlipH => {
            painter.extend(egui::Shape::dashed_line(
                &[at(0.0, -7.5), at(0.0, 7.5)],
                s,
                2.0,
                1.5,
            ));
            painter.add(egui::Shape::closed_line(
                vec![at(-2.0, -5.0), at(-2.0, 6.0), at(-7.5, 6.0)],
                s,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![at(2.0, -5.0), at(2.0, 6.0), at(7.5, 6.0)],
                colour.gamma_multiply(0.35),
                s,
            ));
        }
        Glyph::FlipV => {
            painter.extend(egui::Shape::dashed_line(
                &[at(-7.5, 0.0), at(7.5, 0.0)],
                s,
                2.0,
                1.5,
            ));
            painter.add(egui::Shape::closed_line(
                vec![at(-5.0, -2.0), at(6.0, -2.0), at(6.0, -7.5)],
                s,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![at(-5.0, 2.0), at(6.0, 2.0), at(6.0, 7.5)],
                colour.gamma_multiply(0.35),
                s,
            ));
        }
        Glyph::RotateCw | Glyph::RotateCcw => {
            let dir = if matches!(glyph, Glyph::RotateCw) {
                1.0
            } else {
                -1.0
            };
            let pts: Vec<Pos2> = (0..=10_u8)
                .map(|i| {
                    let a = std::f32::consts::PI * (0.2 + 1.5 * f32::from(i) / 10.0);
                    at(dir * 6.0 * a.cos(), -6.0 * a.sin())
                })
                .collect();
            painter.add(egui::Shape::line(pts, s));
            seg(at(dir * 6.0, -3.0), at(dir * 6.0, -7.0));
            seg(at(dir * 6.0, -3.0), at(dir * 2.5, -3.0));
        }
        Glyph::Grid => {
            for (x, y) in [(-6.0, -6.0), (1.0, -6.0), (-6.0, 1.0), (1.0, 1.0)] {
                bar(x, y, 5.0, 5.0);
            }
        }
        Glyph::Circle => {
            painter.circle_stroke(c, 6.0, s);
            for (x, y) in [(0.0, -6.0), (5.2, 3.0), (-5.2, 3.0)] {
                painter.circle_filled(at(x, y), 1.8, colour);
            }
        }
    }
}

// ---- tool strip ----

/// The tool order of the design: select and node, shapes, then pen, text, knife and guides.
const STRIP: [ToolId; 13] = [
    ToolId::Select,
    ToolId::Node,
    ToolId::Rectangle,
    ToolId::Ellipse,
    ToolId::Polygon,
    ToolId::Star,
    ToolId::Line,
    ToolId::Arc,
    ToolId::ArcCenter,
    ToolId::Pen,
    ToolId::Knife,
    ToolId::Guide,
    ToolId::StartPoint,
];

/// The vertical strip of tools down the left edge: one 30 px icon button per tool.
pub(super) fn tool_strip(ctx: &egui::Context, app: &mut App, now_ms: u64) {
    let p = palette(ctx);
    egui::SidePanel::left("tools")
        .show_separator_line(false)
        .exact_width(40.0)
        .resizable(false)
        .frame(
            egui::Frame::NONE
                .fill(p.panel)
                .inner_margin(Margin::symmetric(5, 6)),
        )
        .show(ctx, |ui| {
            let full = ui.max_rect();
            hairline(
                ui.painter(),
                Pos2::new(full.right() + 5.5, full.top() - 6.0),
                Pos2::new(full.right() + 5.5, full.bottom() + 6.0),
                p.line,
            );
            ui.spacing_mut().item_spacing = vec2(0.0, 1.0);
            for tool in STRIP {
                if matches!(tool, ToolId::Rectangle | ToolId::Pen) {
                    ui.add_space(4.0);
                    let (rule, _) = ui.allocate_exact_size(vec2(30.0, 1.0), Sense::hover());
                    ui.painter().rect_filled(
                        Rect::from_center_size(rule.center(), vec2(18.0, 1.0)),
                        0.0,
                        p.line,
                    );
                    ui.add_space(4.0);
                }
                let label = app.loc.command_label(tool.command_id());
                let text = match app.state.registry().shortcut_of(tool.command_id()) {
                    Some(s) => format!(
                        "{label} ({})",
                        s.display(crate::commands::Platform::current())
                    ),
                    None => label,
                };
                let selected = app.state.tool_id() == tool;
                let (rect, response) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
                if selected {
                    ui.painter().rect_filled(rect, 3.0, p.tint);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 3.0, p.hover);
                }
                super::tool_glyph(
                    ui.painter(),
                    rect.center(),
                    tool,
                    if selected { p.accent } else { p.muted },
                );
                let name = text.clone();
                response.widget_info(move || {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        true,
                        selected,
                        name.clone(),
                    )
                });
                if response.on_hover_text(text).clicked() {
                    app.run(tool.command_id(), now_ms);
                }
            }
            ui.add_space(4.0);
            let (rule, _) = ui.allocate_exact_size(vec2(30.0, 1.0), Sense::hover());
            ui.painter().rect_filled(
                Rect::from_center_size(rule.center(), vec2(18.0, 1.0)),
                0.0,
                p.line,
            );
            ui.add_space(4.0);
            let label = app.loc.tr("text-button");
            let (rect, response) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, 3.0, p.hover);
            }
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "T",
                font(ui.ctx(), 15.0, Weight::Medium),
                p.muted,
            );
            let name = label.clone();
            response.widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone())
            });
            if response.on_hover_text(label).clicked() {
                app.run("text.open", now_ms);
            }
        });
}

// ---- status bar ----

/// The 24 px status bar: compile state, totals, pointer position, snap, grid and zoom.
pub(super) fn status_bar(ctx: &egui::Context, app: &mut App, now_ms: u64) {
    let p = palette(ctx);
    egui::TopBottomPanel::bottom("status")
        .show_separator_line(false)
        .exact_height(STATUS_H)
        .frame(
            egui::Frame::NONE
                .fill(p.panel)
                .inner_margin(Margin::symmetric(10, 0)),
        )
        .show(ctx, |ui| {
            let full = ui.max_rect();
            hairline(
                ui.painter(),
                Pos2::new(full.left() - 10.0, full.top() - 0.5),
                Pos2::new(full.right() + 10.0, full.top() - 0.5),
                p.line,
            );
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                status_left(ui, app, p);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    status_right(ui, app, p, now_ms);
                });
            });
        });
}

fn status_left(ui: &mut egui::Ui, app: &App, p: crate::theme::Palette) {
    super::extras::status_bar(ui, app);
    let warnings = app.state.warning_count();
    ui.label(text(
        ui.ctx(),
        app.loc.tr_args(
            "warnings-count",
            &[(
                "count",
                crate::l10n::Arg::Number(super::count_f64(warnings)),
            )],
        ),
        11.0,
        Weight::Regular,
        p.muted,
    ));
    let total_ms: u64 = app
        .state
        .operation_rows()
        .iter()
        .filter(|r| r.enabled)
        .filter_map(|r| r.time_ms)
        .sum();
    if total_ms > 0 {
        ui.label(text(
            ui.ctx(),
            crate::numfmt::format_duration_ms(total_ms),
            11.0,
            Weight::Regular,
            p.muted,
        ));
    }
}

fn status_right(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette, now_ms: u64) {
    ui.spacing_mut().item_spacing.x = 6.0;
    let dpi = 96.0 * f64::from(ui.ctx().pixels_per_point());
    let percent = (app.state.viewport().zoom() * 25.4 / dpi * 100.0).round();
    let zoom = ui.add(
        egui::Button::new(text(
            ui.ctx(),
            format!("{percent:.0}%"),
            11.0,
            Weight::Regular,
            p.ink,
        ))
        .frame(false),
    );
    zoom.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Zoom".to_owned())
    });
    if zoom.clicked() {
        app.run("view.true-scale", now_ms);
    }
    let snap = app.state.snapping();
    let snap_button = ui.add(
        egui::Button::new(text(
            ui.ctx(),
            app.loc.tr("status-snap-toggle"),
            11.0,
            Weight::Medium,
            if snap { p.accent } else { p.ink },
        ))
        .fill(if snap { p.tint } else { Color32::TRANSPARENT })
        .stroke(Stroke::NONE)
        .corner_radius(2.0),
    );
    if snap_button.clicked() {
        app.run("view.snap", now_ms);
    }
    if let Some(at) = app.pointer_mm {
        let fmt = app.format();
        ui.label(text(
            ui.ctx(),
            format!("{}, {}", fmt.length(at.x), fmt.length(at.y)),
            11.0,
            Weight::Regular,
            p.muted,
        ));
    }
}
