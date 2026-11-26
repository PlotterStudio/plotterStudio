//! The Prepare screen's side panels from the design: the 220 px layers panel on the left, and the
//! 288 px operations panel on the right with the selected row's settings below the list.

use std::collections::BTreeMap;

use egui::{Align, Color32, Layout, Margin, Pos2, Rect, Sense, Stroke, UiBuilder, vec2};
use ps_core::{ObjectId, OpRowId, PathKey};
use ps_model::{OpKind, ShapePath};

use super::App;
use super::zest::{self, Icon, Weight, font, hairline, palette, text};
use crate::l10n::Arg;
use crate::numfmt::format_duration_ms;
use crate::state::{OperationRow, ParamOrigin, ParamView};

const HEADER_H: f32 = 30.0;
/// The settings the design lists up front; the others fold under "More settings".
const COMMON: [&str; 6] = [
    "force",
    "speed",
    "passes",
    "tip_width",
    "blade_offset",
    "overcut",
];

/// The inks offered for a pen, as `#RRGGBB`.
const INKS: [&str; 12] = [
    "#231F20", "#6B6F80", "#D32F2F", "#F57C00", "#FBC02D", "#388E3C", "#00897B", "#1E88E5",
    "#6A1B9A", "#D81B60", "#6D4C41", "#FFFFFF",
];

/// The operation colour in the design's palette.
pub(super) fn op_colour(kind: OpKind) -> Color32 {
    let [r, g, b] = ps_render::colour_of(kind);
    Color32::from_rgb(r, g, b)
}

/// Draws an uppercase 11 px header label with the design's letter spacing.
fn tracked(ui: &mut egui::Ui, label: &str, colour: Color32) {
    let font = font(ui.ctx(), 11.0, Weight::Semibold);
    let upper = label.to_uppercase();
    let tracking = 0.9;
    let mut width = 0.0;
    let mut galleys = Vec::new();
    for ch in upper.chars() {
        let galley = ui
            .painter()
            .layout_no_wrap(ch.to_string(), font.clone(), colour);
        width += galley.size().x + tracking;
        galleys.push(galley);
    }
    let (rect, response) = ui.allocate_exact_size(vec2(width, 16.0), Sense::hover());
    let mut x = rect.left();
    for galley in galleys {
        let advance = galley.size().x + tracking;
        ui.painter().galley(
            Pos2::new(x, rect.center().y - galley.size().y / 2.0),
            galley,
            colour,
        );
        x += advance;
    }
    let name = label.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, name.clone())
    });
}

/// A 30 px panel header: a hairline below and a left-to-right row to fill.
fn header(ui: &mut egui::Ui, p: crate::theme::Palette, content: impl FnOnce(&mut egui::Ui)) {
    let rect = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), HEADER_H));
    ui.allocate_rect(rect, Sense::hover());
    hairline(
        ui.painter(),
        Pos2::new(rect.left(), rect.bottom() - 0.5),
        Pos2::new(rect.right(), rect.bottom() - 0.5),
        p.line,
    );
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(6.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            content(ui);
        },
    );
}

// ---- layers ----

/// The layers panel, or its 26 px collapsed strip.
pub(super) fn layers_panel(ctx: &egui::Context, app: &mut App, now_ms: u64) {
    let p = palette(ctx);
    if !app.layers_open {
        egui::SidePanel::left("layers_collapsed")
            .show_separator_line(false)
            .exact_width(26.0)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(p.panel)
                    .inner_margin(Margin::symmetric(2, 4)),
            )
            .show(ctx, |ui| {
                let full = ui.max_rect();
                hairline(
                    ui.painter(),
                    Pos2::new(full.right() + 1.5, full.top() - 4.0),
                    Pos2::new(full.right() + 1.5, full.bottom() + 4.0),
                    p.line,
                );
                let label = app.loc.tr("layers-show");
                let (rect, response) = ui.allocate_exact_size(vec2(22.0, 84.0), Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(rect, 3.0, p.hover);
                }
                zest::icon(
                    ui.painter(),
                    Pos2::new(rect.center().x, rect.top() + 12.0),
                    Icon::ChevronRight,
                    12.0,
                    p.muted,
                );
                let title = app.loc.tr("panel-layers").to_uppercase();
                let galley = ui.painter().layout_no_wrap(
                    title,
                    font(ui.ctx(), 11.0, Weight::Semibold),
                    p.muted,
                );
                let at = Pos2::new(rect.center().x + galley.size().y / 2.0, rect.top() + 24.0);
                ui.painter().add(
                    egui::epaint::TextShape::new(at, galley, p.muted)
                        .with_angle(std::f32::consts::FRAC_PI_2),
                );
                let name = label.clone();
                response.widget_info(move || {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone())
                });
                if response.clicked() {
                    app.layers_open = true;
                }
            });
        return;
    }
    egui::SidePanel::left("layers")
        .show_separator_line(false)
        .exact_width(220.0)
        .resizable(false)
        .frame(egui::Frame::NONE.fill(p.panel))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            hairline(
                ui.painter(),
                Pos2::new(full.right() - 0.5, full.top()),
                Pos2::new(full.right() - 0.5, full.bottom()),
                p.line,
            );
            let title = app.loc.tr("panel-layers");
            header(ui, p, |ui| {
                let hide = app.loc.tr("layers-hide");
                if zest::icon_button(
                    ui,
                    p,
                    Icon::ChevronDown,
                    vec2(22.0, 22.0),
                    12.0,
                    false,
                    &hide,
                )
                .clicked()
                {
                    app.layers_open = false;
                }
                tracked(ui, &title, p.muted);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = app.loc.command_label("file.import");
                    if zest::icon_button(ui, p, Icon::Plus, vec2(22.0, 22.0), 13.0, false, &label)
                        .clicked()
                    {
                        app.run("file.import", now_ms);
                    }
                });
            });
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(4.0);
                layer_rows(ui, app, p, now_ms);
            });
        });
}

#[expect(
    clippy::too_many_lines,
    reason = "straight-line drawing of one panel; splitting it would only scatter the layout"
)]
fn layer_rows(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette, now_ms: u64) {
    unfold_newly_selected(app);
    let selected: Vec<ObjectId> = app.state.selection().objects();
    // Looked up once: a large drawing lists many shapes.
    let inks: BTreeMap<OpRowId, String> = app
        .state
        .operation_rows()
        .into_iter()
        .filter_map(|row| Some((row.id, row.ink?)))
        .collect();
    let mat = app.state.current_mat();
    if let Some(used) = app.state.used_length_mm() {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(text(
                ui.ctx(),
                app.loc.tr_args(
                    "tree-roll-used",
                    &[
                        ("used", Arg::Number(used.round())),
                        ("total", Arg::Number(mat.size.height.get().round())),
                    ],
                ),
                11.0,
                Weight::Regular,
                p.muted,
            ));
        });
    }
    ui.spacing_mut().item_spacing.y = 0.0;
    for (index, (id, object)) in mat.objects.iter().enumerate() {
        let has_shapes = object.paths.len() > 1;
        let expanded = has_shapes && app.layers_expanded.contains(id);
        let touched = selected.contains(id);
        // Lit only when the whole object is selected, so that it is not mistaken for that when
        // only some of its shapes are.
        let on = touched
            && !app
                .state
                .selection()
                .shapes()
                .any(|(object, _)| object == *id);
        let label = app.loc.tr_args(
            "tree-object",
            &[("n", Arg::Number(super::count_f64(index + 1)))],
        );
        let width = ui.available_width() - 8.0;
        let (rect, response) = ui.allocate_exact_size(vec2(width, 24.0), Sense::click());
        let rect = rect.translate(vec2(4.0, 0.0));
        let hover = ui.rect_contains_pointer(rect);
        if hover {
            app.state.set_highlight_layer(Some((*id, None)));
        }
        if on {
            ui.painter().rect_filled(rect, 2.0, p.tint);
        } else if hover {
            ui.painter().rect_filled(rect, 2.0, p.ground);
        }
        ui.painter().text(
            Pos2::new(rect.left() + 22.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &label,
            font(
                ui.ctx(),
                12.0,
                if touched {
                    Weight::Semibold
                } else {
                    Weight::Regular
                },
            ),
            p.ink,
        );
        let mut kinds: Vec<OpKind> = Vec::new();
        for path in object.paths.iter() {
            for op in &path.ops {
                if !kinds.contains(&op.kind) {
                    kinds.push(op.kind);
                }
            }
        }
        let show_delete = hover || on;
        let right = rect.right() - 6.0 - if show_delete { 24.0 } else { 0.0 };
        kind_swatches(ui, &kinds, right, rect.center().y);
        let name = label.clone();
        response.widget_info(move || {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, name.clone())
        });
        let mut folded = false;
        if has_shapes {
            folded = expand_button(ui, app, p, &response, rect, &label, expanded);
            if folded && !app.layers_expanded.remove(id) {
                app.layers_expanded.insert(*id);
            }
        }
        if show_delete {
            let delete_rect = Rect::from_center_size(
                Pos2::new(rect.right() - 16.0, rect.center().y),
                vec2(22.0, 22.0),
            );
            let delete = ui.interact(delete_rect, response.id.with("delete"), Sense::click());
            if delete.hovered() {
                ui.painter().rect_filled(delete_rect, 3.0, p.hover);
            }
            zest::icon(
                ui.painter(),
                delete_rect.center(),
                Icon::Trash,
                14.0,
                if delete.hovered() { p.danger } else { p.muted },
            );
            let said = format!("{}: {label}", app.loc.command_label("edit.delete"));
            delete.widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, said.clone())
            });
            if delete.clicked() {
                app.state.delete_object(*id, now_ms);
                continue;
            }
        }
        if response.clicked() && !folded {
            app.state.select_object(*id);
        }
        if expanded {
            for (number, path) in object.paths.iter().enumerate() {
                shape_row(ui, app, p, (*id, &label), number + 1, path, &inks);
            }
        }
    }
}

/// Folds out the objects of a shape selection made since the last frame, wherever it was made.
fn unfold_newly_selected(app: &mut App) {
    let shapes: Vec<(ObjectId, PathKey)> = app
        .state
        .selection()
        .shapes()
        .map(|(object, path)| (object, path.clone()))
        .collect();
    if shapes != app.layers_seen_shapes {
        // Only for shapes not selected before, so that a list folded by hand stays folded.
        let new = shapes
            .iter()
            .filter(|s| !app.layers_seen_shapes.contains(s));
        app.layers_expanded.extend(new.map(|(object, _)| *object));
        app.layers_seen_shapes = shapes;
    }
}

/// The chevron at the start of an object's row; true when it was clicked.
fn expand_button(
    ui: &mut egui::Ui,
    app: &App,
    p: crate::theme::Palette,
    row: &egui::Response,
    rect: Rect,
    object: &str,
    expanded: bool,
) -> bool {
    let at = Rect::from_center_size(
        Pos2::new(rect.left() + 11.0, rect.center().y),
        vec2(18.0, 22.0),
    );
    let response = ui.interact(at, row.id.with("expand"), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(at, 3.0, p.hover);
    }
    zest::icon(
        ui.painter(),
        at.center(),
        if expanded {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        },
        11.0,
        p.muted,
    );
    let key = if expanded {
        "layers-hide-shapes"
    } else {
        "layers-show-shapes"
    };
    let said = app.loc.tr_args(key, &[("object", Arg::Text(object))]);
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, said.clone())
    });
    response.clicked()
}

/// A line swatch for each operation kind, ending at `right`; returns where they start.
fn kind_swatches(ui: &egui::Ui, kinds: &[OpKind], right: f32, y: f32) -> f32 {
    let mut x = right;
    for kind in kinds.iter().rev() {
        x -= 14.0;
        zest::line_swatch(
            ui.painter(),
            Pos2::new(x, y),
            14.0,
            op_colour(*kind),
            ps_render::style_of(*kind),
            2.0,
        );
        x -= 2.0;
    }
    x
}

/// One shape under its object: a click selects it alone, Shift- or Command-click adds it or
/// takes it out. Its swatches and ink dots show what runs on it.
fn shape_row(
    ui: &mut egui::Ui,
    app: &mut App,
    p: crate::theme::Palette,
    (object, object_label): (ObjectId, &str),
    number: usize,
    path: &ShapePath,
    inks: &BTreeMap<OpRowId, String>,
) {
    let on = app
        .state
        .selection()
        .shapes()
        .any(|(o, key)| o == object && *key == path.key);
    let width = ui.available_width() - 8.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 22.0), Sense::click());
    let rect = rect.translate(vec2(4.0, 0.0));
    let n = || Arg::Number(super::count_f64(number));

    let mut kinds: Vec<OpKind> = Vec::new();
    let mut colours: Vec<&str> = Vec::new();
    let mut runs: Vec<String> = Vec::new();
    for op in &path.ops {
        if !kinds.contains(&op.kind) {
            kinds.push(op.kind);
        }
        let ink = inks.get(&op.row).map(String::as_str);
        let kind = app.loc.tr(&format!("op-{}", op.kind.as_str()));
        runs.push(ink.map_or_else(|| kind.clone(), |ink| format!("{kind} {ink}")));
        if let Some(ink) = ink.filter(|ink| !colours.contains(ink)) {
            colours.push(ink);
        }
    }

    let hover = ui.rect_contains_pointer(rect);
    if hover {
        app.state
            .set_highlight_layer(Some((object, Some(path.key.clone()))));
    }
    if ui.is_rect_visible(rect) {
        if on {
            ui.painter().rect_filled(rect, 2.0, p.tint);
        } else if hover {
            ui.painter().rect_filled(rect, 2.0, p.ground);
        }
        ui.painter().text(
            Pos2::new(rect.left() + 38.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            app.loc.tr_args("tree-shape", &[("n", n())]),
            font(
                ui.ctx(),
                12.0,
                if on {
                    Weight::Semibold
                } else {
                    Weight::Regular
                },
            ),
            p.ink,
        );
        let mut x = kind_swatches(ui, &kinds, rect.right() - 6.0, rect.center().y);
        for ink in colours.iter().rev() {
            x -= 9.0;
            let centre = Pos2::new(x + 4.0, rect.center().y);
            ui.painter().circle_filled(centre, 4.0, parse_hex(ink));
            // An outline, so that white ink still shows.
            ui.painter()
                .circle_stroke(centre, 4.0, Stroke::new(1.0_f32, p.line));
            x -= 2.0;
        }
    }

    let mut said = app.loc.tr_args(
        "tree-shape-name",
        &[("object", Arg::Text(object_label)), ("n", n())],
    );
    if !runs.is_empty() {
        said = format!("{said}: {}", runs.join(", "));
    }
    response.widget_info(move || {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, said.clone())
    });
    if response.clicked() {
        let extend = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        if extend {
            app.state.toggle_shape(object, path.key.clone());
        } else {
            app.state.select_shapes(&[(object, path.key.clone())]);
        }
    }
}

// ---- operations and settings ----

/// The operations panel: the list, then the selected row's settings.
pub(super) fn operations_panel(ctx: &egui::Context, app: &mut App, now_ms: u64) {
    let p = palette(ctx);
    egui::SidePanel::right("operations")
        .show_separator_line(false)
        .exact_width(288.0)
        .resizable(false)
        .frame(egui::Frame::NONE.fill(p.panel))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            hairline(
                ui.painter(),
                Pos2::new(full.left() + 0.5, full.top()),
                Pos2::new(full.left() + 0.5, full.bottom()),
                p.line,
            );
            egui::ScrollArea::vertical().show(ui, |ui| {
                let rows = app.state.operation_rows();
                ops_header(ui, app, p, now_ms);
                quick_assign(ui, app, p, now_ms);
                if rows.is_empty() {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        ui.label(text(
                            ui.ctx(),
                            app.loc.tr("ops-empty"),
                            12.0,
                            Weight::Regular,
                            p.muted,
                        ));
                    });
                } else {
                    op_rows(ui, app, &rows, p, now_ms);
                    ops_footer(ui, app, &rows, p);
                    settings(ui, app, &rows, p, now_ms);
                }
                legend(ui, app, p);
            });
        });
}

fn ops_header(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette, now_ms: u64) {
    let mat_name = app.state.current_mat().name.clone();
    let title = format!("{} · {}", app.loc.tr("panel-operations"), mat_name);
    header(ui, p, |ui| {
        ui.add_space(4.0);
        tracked(ui, &title, p.muted);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = app.loc.tr("inspector-assign");
            let enabled = !app.state.selection().objects().is_empty();
            let response = zest::icon_button_enabled(
                ui,
                p,
                Icon::Plus,
                vec2(22.0, 22.0),
                13.0,
                enabled,
                &label,
            );
            if enabled {
                egui::Popup::menu(&response).show(|ui| assign_entries(ui, app, now_ms));
            }
        });
    });
}

/// One-click buttons that put an operation on the selection with the first suitable tool.
fn quick_assign(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette, now_ms: u64) {
    if app.state.selection().objects().is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.add_space(8.0);
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for kind in [
            OpKind::Cut,
            OpKind::Draw,
            OpKind::Fill,
            OpKind::Score,
            OpKind::Foil,
        ] {
            let Some(tool) = app.state.tools_for(kind).into_iter().next() else {
                continue;
            };
            let name = app.loc.tr(&format!("op-{}", kind.as_str()));
            let label = app
                .loc
                .tr_args("ops-quick-add", &[("name", Arg::Text(&name))]);
            let galley = ui.painter().layout_no_wrap(
                name.clone(),
                font(ui.ctx(), 11.0, Weight::Medium),
                p.ink,
            );
            let (rect, response) =
                ui.allocate_exact_size(vec2(galley.size().x + 26.0, 24.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, 12.0, p.hover);
            }
            ui.painter().rect_stroke(
                rect,
                12.0,
                Stroke::new(1.0_f32, p.outline),
                egui::StrokeKind::Inside,
            );
            zest::line_swatch(
                ui.painter(),
                Pos2::new(rect.left() + 7.0, rect.center().y),
                9.0,
                op_colour(kind),
                ps_render::style_of(kind),
                2.0,
            );
            ui.painter().galley(
                Pos2::new(rect.left() + 20.0, rect.center().y - galley.size().y / 2.0),
                galley,
                p.ink,
            );
            let said = label.clone();
            response.widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, said.clone())
            });
            if response.on_hover_text(&tool.name).clicked() {
                app.assign_operation(kind, &tool.reference, now_ms);
            }
        }
    });
}

fn assign_entries(ui: &mut egui::Ui, app: &mut App, now_ms: u64) {
    let mut chosen = None;
    for kind in OpKind::ALL {
        for tool in app.state.tools_for(kind) {
            let text = format!(
                "{} — {}",
                app.loc.tr(&format!("op-{}", kind.as_str())),
                tool.name
            );
            if ui.button(text).clicked() {
                chosen = Some((kind, tool.reference));
                ui.close();
            }
        }
    }
    if let Some((kind, tool)) = chosen {
        app.assign_operation(kind, &tool, now_ms);
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "straight-line drawing of one panel; splitting it would only scatter the layout"
)]
fn op_rows(
    ui: &mut egui::Ui,
    app: &mut App,
    rows: &[OperationRow],
    p: crate::theme::Palette,
    now_ms: u64,
) {
    ui.add_space(4.0);
    ui.spacing_mut().item_spacing.y = 1.0;
    let current = app.current_row(rows);
    for (index, row) in rows.iter().enumerate() {
        let on = Some(row.id) == current;
        let width = ui.available_width() - 8.0;
        let (rect, response) = ui.allocate_exact_size(vec2(width, 38.0), Sense::click());
        let rect = rect.translate(vec2(4.0, 0.0));
        let hover = ui.rect_contains_pointer(rect);
        if hover {
            // The canvas steps everything else back and lights this row's paths.
            app.state.set_highlight_row(Some(row.id));
        }
        if on {
            ui.painter().rect_filled(rect, 2.0, p.tint);
            ui.painter().rect_filled(
                Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
                0.0,
                p.orange,
            );
        } else if hover {
            ui.painter().rect_filled(rect, 2.0, p.ground);
        }
        let dim = if row.enabled { 1.0 } else { 0.35 };
        ui.painter().text(
            Pos2::new(rect.left() + 12.0, rect.center().y),
            egui::Align2::CENTER_CENTER,
            (index + 1).to_string(),
            font(ui.ctx(), 10.0, Weight::Regular),
            p.muted,
        );
        let colour = op_colour(row.kind).gamma_multiply(dim);
        zest::line_swatch(
            ui.painter(),
            Pos2::new(rect.left() + 24.0, rect.center().y),
            18.0,
            colour,
            ps_render::style_of(row.kind),
            2.5,
        );
        let ink = if row.enabled { p.ink } else { p.muted };
        // Repeated operations are numbered, so each row has a name of its own.
        let mut name = app.loc.tr(&format!("op-{}", row.kind.as_str()));
        let nth = rows[..=index].iter().filter(|r| r.kind == row.kind).count();
        if nth > 1 {
            name = app.loc.tr_args(
                "ops-nth",
                &[
                    ("name", Arg::Text(&name)),
                    ("n", Arg::Number(super::count_f64(nth))),
                ],
            );
        }
        ui.painter().text(
            Pos2::new(rect.left() + 50.0, rect.center().y - 8.0),
            egui::Align2::LEFT_CENTER,
            &name,
            font(ui.ctx(), 12.0, Weight::Semibold),
            ink,
        );
        let sub = if row.holder.is_empty() {
            row.tool_name.clone()
        } else {
            format!("{} · {}", row.tool_name, row.holder)
        };
        let sub_rect = Rect::from_min_max(
            Pos2::new(rect.left() + 50.0, rect.center().y + 1.0),
            Pos2::new(rect.right() - 82.0, rect.bottom()),
        );
        ui.painter().with_clip_rect(sub_rect).text(
            Pos2::new(sub_rect.left(), rect.center().y + 8.0),
            egui::Align2::LEFT_CENTER,
            &sub,
            font(ui.ctx(), 11.0, Weight::Regular),
            p.muted,
        );
        let time = row.time_ms.map_or_else(String::new, format_duration_ms);
        ui.painter().text(
            Pos2::new(rect.right() - 60.0, rect.center().y - 8.0),
            egui::Align2::RIGHT_CENTER,
            time,
            font(ui.ctx(), 11.0, Weight::Regular),
            p.muted,
        );
        // A pen row shows its ink, which is what tells two rows of one kind apart.
        if let Some(ink) = &row.ink {
            let at = Pos2::new(rect.right() - 74.0, rect.center().y + 8.0);
            ui.painter()
                .circle_filled(at, 4.0, parse_hex(ink).gamma_multiply(dim));
            ui.painter()
                .circle_stroke(at, 4.0, Stroke::new(1.0_f32, p.outline));
        }
        let label = app.loc.tr("ops-select");
        let row_name = format!("{label}: {name}");
        response.widget_info(move || {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, row_name.clone())
        });
        if response.clicked() {
            app.selected_row = Some(row.id);
            // Choosing an operation also selects the objects that carry it.
            app.state.select_row_objects(row.id);
        }
        // Take the operation off its paths (shown on the row you point at or have chosen).
        if hover || on {
            let trash_rect = Rect::from_center_size(
                Pos2::new(rect.right() - 42.0, rect.center().y),
                vec2(24.0, 24.0),
            );
            let trash = ui.interact(trash_rect, response.id.with("trash"), Sense::click());
            if trash.hovered() {
                ui.painter().rect_filled(trash_rect, 3.0, p.hover);
            }
            zest::icon(
                ui.painter(),
                trash_rect.center(),
                Icon::Trash,
                14.0,
                if trash.hovered() { p.danger } else { p.muted },
            );
            let said = format!("{}: {name}", app.loc.tr("ops-remove"));
            trash.widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, said.clone())
            });
            if trash.clicked() {
                app.state.remove_row(row.id, now_ms);
            }
        }
        // The eye: turns the whole row on or off.
        let eye_rect = Rect::from_center_size(
            Pos2::new(rect.right() - 16.0, rect.center().y),
            vec2(24.0, 24.0),
        );
        let eye = ui.interact(eye_rect, response.id.with("eye"), Sense::click());
        if eye.hovered() {
            ui.painter().rect_filled(eye_rect, 3.0, p.hover);
        }
        let eye_label = app.loc.tr("ops-enable");
        let enabled = row.enabled;
        eye.widget_info(move || {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, enabled, eye_label.clone())
        });
        zest::icon(
            ui.painter(),
            eye_rect.center(),
            if row.enabled { Icon::Eye } else { Icon::EyeOff },
            14.0,
            if row.enabled { p.ink } else { p.muted },
        );
        if !row.enabled {
            // Struck through, so an operation that is off cannot be mistaken for one that is on.
            let y = rect.top() + 14.0;
            let chars = f32::from(u16::try_from(name.chars().count()).unwrap_or(0));
            ui.painter().line_segment(
                [
                    Pos2::new(rect.left() + 50.0, y),
                    Pos2::new(rect.left() + 50.0 + 6.5 * chars, y),
                ],
                Stroke::new(1.0_f32, p.muted),
            );
        }
        if eye.clicked() {
            app.state
                .set_operation_row_enabled(row.id, !row.enabled, now_ms);
        }
    }
}

fn ops_footer(ui: &mut egui::Ui, app: &App, rows: &[OperationRow], p: crate::theme::Palette) {
    let total: u64 = rows
        .iter()
        .filter(|r| r.enabled)
        .filter_map(|r| r.time_ms)
        .sum();
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(text(
            ui.ctx(),
            app.loc.tr("ops-runs-top-to-bottom"),
            11.0,
            Weight::Regular,
            p.muted,
        ));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(10.0);
            if total > 0 {
                ui.label(text(
                    ui.ctx(),
                    format_duration_ms(total),
                    11.0,
                    Weight::Regular,
                    p.muted,
                ));
            }
        });
    });
    ui.add_space(6.0);
    let line = ui.cursor();
    hairline(
        ui.painter(),
        line.min,
        Pos2::new(line.min.x + ui.available_width(), line.min.y),
        p.line,
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "straight-line drawing of one panel; splitting it would only scatter the layout"
)]
fn settings(
    ui: &mut egui::Ui,
    app: &mut App,
    rows: &[OperationRow],
    p: crate::theme::Palette,
    now_ms: u64,
) {
    let Some(id) = app.current_row(rows) else {
        return;
    };
    let Some(row) = rows.iter().find(|r| r.id == id) else {
        return;
    };
    let index = rows.iter().position(|r| r.id == id).unwrap_or(0);
    let name = app.loc.tr(&format!("op-{}", row.kind.as_str()));
    egui::Frame::NONE
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(18.0, 10.0), Sense::hover());
                zest::line_swatch(
                    ui.painter(),
                    Pos2::new(rect.left(), rect.center().y),
                    18.0,
                    op_colour(row.kind),
                    ps_render::style_of(row.kind),
                    2.5,
                );
                let title = app
                    .loc
                    .tr_args("ops-settings-title", &[("name", Arg::Text(&name))]);
                ui.label(text(ui.ctx(), title, 13.0, Weight::Semibold, p.ink));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(text(
                        ui.ctx(),
                        app.loc.tr_args(
                            "control-paths",
                            &[("count", Arg::Number(super::count_f64(row.paths)))],
                        ),
                        11.0,
                        Weight::Regular,
                        p.muted,
                    ));
                });
            });
            tool_tile(ui, app, row, p);
            let can_add = app.state.can_add_selection_to_row(id);
            let add = ui.add_enabled(
                can_add,
                egui::Button::new(text(
                    ui.ctx(),
                    app.loc.tr("ops-add-selection"),
                    12.0,
                    Weight::Medium,
                    p.accent,
                ))
                .frame(false),
            );
            if add.clicked() {
                app.state.add_selection_to_row(id, now_ms);
            }
            let can_remove = app.state.can_remove_selection_from_row(id);
            let remove = ui.add_enabled(
                can_remove,
                egui::Button::new(text(
                    ui.ctx(),
                    app.loc.tr("ops-remove-selection"),
                    12.0,
                    Weight::Medium,
                    p.accent,
                ))
                .frame(false),
            );
            if remove.clicked() {
                app.state.remove_selection_from_row(id, now_ms);
            }
            ink_colour(ui, app, id, p, now_ms);
            fill_settings(ui, app, id, p, now_ms);
            order_controls(ui, app, rows, index, p, now_ms);
            let (common, more): (Vec<ParamView>, Vec<ParamView>) = app
                .state
                .row_params(id)
                .into_iter()
                .partition(|v| COMMON.contains(&v.name.as_str()));
            for view in &common {
                param(ui, app, id, view, p, now_ms);
            }
            if !more.is_empty() {
                ui.collapsing(
                    text(
                        ui.ctx(),
                        app.loc.tr("ops-more-settings"),
                        12.0,
                        Weight::Medium,
                        p.muted,
                    ),
                    |ui| {
                        for view in &more {
                            param(ui, app, id, view, p, now_ms);
                        }
                    },
                );
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let width = (ui.available_width() - 6.0) / 2.0;
                let reset = ui.add_sized(
                    [width, 26.0],
                    egui::Button::new(text(
                        ui.ctx(),
                        app.loc.tr("ops-reset-all"),
                        12.0,
                        Weight::Medium,
                        p.ink,
                    ))
                    .fill(p.panel)
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0x84, 0x8a, 0xa3)))
                    .corner_radius(3.0),
                );
                if reset.clicked() {
                    app.state.reset_row_params(id, now_ms);
                }
                ui.add_enabled(
                    false,
                    egui::Button::new(text(
                        ui.ctx(),
                        app.loc.tr("ops-save-preset"),
                        12.0,
                        Weight::Semibold,
                        p.accent,
                    ))
                    .min_size(vec2(width, 26.0))
                    .fill(p.tint)
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0xff, 0xd2, 0xa8)))
                    .corner_radius(3.0),
                );
            });
            let remove = ui.add(
                egui::Button::new(text(
                    ui.ctx(),
                    format!("{}: {name}", app.loc.tr("ops-remove")),
                    12.0,
                    Weight::Medium,
                    p.danger,
                ))
                .frame(false),
            );
            if remove.clicked() {
                app.state.remove_row(id, now_ms);
            }
        });
}

fn parse_hex(hex: &str) -> Color32 {
    let digits = hex.trim_start_matches('#');
    let channel = |i: usize| {
        digits
            .get(i..i + 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
            .unwrap_or(0)
    };
    Color32::from_rgb(channel(0), channel(2), channel(4))
}

/// The ink of a pen row: a swatch per common colour and a picker for any other.
fn ink_colour(
    ui: &mut egui::Ui,
    app: &mut App,
    row: OpRowId,
    p: crate::theme::Palette,
    now_ms: u64,
) {
    let Some(current) = app.state.row_pen_colour(row) else {
        return;
    };
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(text(
            ui.ctx(),
            app.loc.tr("ops-ink"),
            12.0,
            Weight::Regular,
            p.ink,
        ));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for hex in INKS {
                let (rect, response) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
                let chosen = current.eq_ignore_ascii_case(hex);
                ui.painter()
                    .circle_filled(rect.center(), 8.0, parse_hex(hex));
                ui.painter().circle_stroke(
                    rect.center(),
                    8.0,
                    Stroke::new(1.0_f32, Color32::from_black_alpha(40)),
                );
                if chosen {
                    ui.painter()
                        .circle_stroke(rect.center(), 10.0, Stroke::new(2.0_f32, p.accent));
                }
                let name = format!("{}: {hex}", app.loc.tr("ops-ink"));
                response.widget_info(move || {
                    egui::WidgetInfo::selected(egui::WidgetType::Button, true, chosen, name.clone())
                });
                if response.on_hover_text(hex).clicked() {
                    app.state.set_row_pen_colour(row, Some(hex), now_ms);
                }
            }
            // Any other colour.
            let c = parse_hex(&current);
            let mut rgb = [c.r(), c.g(), c.b()];
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                let hex = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
                app.state.set_row_pen_colour(row, Some(&hex), now_ms);
            }
        });
    });
}

/// The pattern, angle, overlap and outline of a fill.
fn fill_settings(
    ui: &mut egui::Ui,
    app: &mut App,
    row: OpRowId,
    p: crate::theme::Palette,
    now_ms: u64,
) {
    let Some(mut fill) = app.state.row_fill(row) else {
        return;
    };
    let before = fill;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal(|ui| {
            ui.label(text(
                ui.ctx(),
                app.loc.tr("fill-pattern"),
                12.0,
                Weight::Regular,
                p.ink,
            ));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let names = [
                    (ps_geometry::Pattern::Parallel, "fill-parallel"),
                    (ps_geometry::Pattern::Cross, "fill-cross"),
                    (ps_geometry::Pattern::ZigZag, "fill-zigzag"),
                    (ps_geometry::Pattern::Concentric, "fill-concentric"),
                ];
                let shown = names
                    .iter()
                    .find(|(pattern, _)| *pattern == fill.pattern)
                    .map_or_else(String::new, |(_, key)| app.loc.tr(key));
                let label = app.loc.tr("fill-pattern");
                let combo = egui::ComboBox::from_id_salt("fill_pattern")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for (pattern, key) in names {
                            ui.selectable_value(&mut fill.pattern, pattern, app.loc.tr(key));
                        }
                    });
                combo.response.widget_info(move || {
                    egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, label.clone())
                });
            });
        });
        let mut angle = ParamView::plain("fill-angle", fill.angle_deg, 0.0, 180.0, 1.0, "°");
        plain_slider(ui, app, p, &mut angle, "fill-angle");
        fill.angle_deg = angle.value;
        // The overlap is shown as a percentage of the pen's width.
        let mut overlap =
            ParamView::plain("fill-overlap", fill.overlap * 100.0, 0.0, 60.0, 1.0, "%");
        plain_slider(ui, app, p, &mut overlap, "fill-overlap");
        fill.overlap = overlap.value / 100.0;
        let label = app.loc.tr("fill-outline");
        ui.checkbox(&mut fill.outline_pass, label);
    });
    if fill != before {
        app.state.set_row_fill(row, fill, now_ms);
    }
}

/// A labelled slider with a value box, for settings that are not tool parameters.
fn plain_slider(
    ui: &mut egui::Ui,
    app: &App,
    p: crate::theme::Palette,
    view: &mut ParamView,
    key: &str,
) {
    let label = app.loc.tr(key);
    ui.horizontal(|ui| {
        let name = ui.label(text(ui.ctx(), &label, 12.0, Weight::Regular, p.ink));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let mut value = view.value;
            value_box(ui, p, &mut value, view, name.id);
            view.value = value;
        });
    });
    let mut value = view.value;
    if slider(ui, p, &mut value, view, &label) {
        view.value = value;
    }
}

fn tool_tile(ui: &mut egui::Ui, app: &App, row: &OperationRow, p: crate::theme::Palette) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    ui.painter().rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0_f32, p.outline),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        app.loc.tr("ops-tool"),
        font(ui.ctx(), 11.0, Weight::Regular),
        p.muted,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 40.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &row.tool_name,
        font(ui.ctx(), 12.0, Weight::Medium),
        p.ink,
    );
    let label = format!("{}: {}", app.loc.tr("ops-tool"), row.tool_name);
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.clone())
    });
}

/// Move up, move down and "run after the row above" for the selected row.
fn order_controls(
    ui: &mut egui::Ui,
    app: &mut App,
    rows: &[OperationRow],
    index: usize,
    p: crate::theme::Palette,
    now_ms: u64,
) {
    let row = &rows[index];
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (key, enabled, delta, glyph) in [
            ("ops-move-up", index > 0, -1, Icon::ChevronLeft),
            (
                "ops-move-down",
                index + 1 < rows.len(),
                1,
                Icon::ChevronRight,
            ),
        ] {
            let label = app.loc.tr(key);
            let sense = if enabled {
                Sense::click()
            } else {
                Sense::hover()
            };
            let (rect, response) = ui.allocate_exact_size(vec2(24.0, 22.0), sense);
            if enabled && response.hovered() {
                ui.painter().rect_filled(rect, 3.0, p.hover);
            }
            zest::icon(
                ui.painter(),
                rect.center(),
                glyph,
                12.0,
                if enabled { p.muted } else { p.outline },
            );
            let name = label.clone();
            response.widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name.clone())
            });
            if response.on_hover_text(label).clicked() {
                app.state.move_operation_row(row.id, delta, now_ms);
            }
        }
        let mut locked = row.locked_after_previous;
        let lock = ui.add_enabled(
            index > 0,
            egui::Checkbox::new(
                &mut locked,
                text(
                    ui.ctx(),
                    app.loc.tr("ops-lock"),
                    11.0,
                    Weight::Regular,
                    p.muted,
                ),
            ),
        );
        if lock.changed() {
            app.state
                .set_row_locked_after_previous(row.id, locked, now_ms);
        }
    });
}

fn param(
    ui: &mut egui::Ui,
    app: &mut App,
    row: OpRowId,
    view: &ParamView,
    p: crate::theme::Palette,
    now_ms: u64,
) {
    let label = {
        let key = format!("param-{}", view.name);
        let t = app.loc.tr(&key);
        if t == key { humanise(&view.name) } else { t }
    };
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let name = ui.label(text(ui.ctx(), &label, 12.0, Weight::Regular, p.ink));
            let (badge, fill, ink) = match (view.modified, view.origin) {
                (true, _) | (_, ParamOrigin::Changed) => {
                    (app.loc.tr("param-changed"), p.tint, p.accent)
                }
                (_, ParamOrigin::Preset) => (
                    app.loc.tr("param-preset"),
                    Color32::from_rgb(0xe3, 0xed, 0xff),
                    Color32::from_rgb(0x1d, 0x5a, 0xd0),
                ),
                (_, ParamOrigin::Default) => (app.loc.tr("param-default"), p.ground, p.muted),
            };
            zest::badge(ui, &badge, fill, ink);
            if view.modified {
                let reset = ui.add(
                    egui::Button::new(text(
                        ui.ctx(),
                        app.loc.tr("param-reset"),
                        10.0,
                        Weight::Semibold,
                        p.accent,
                    ))
                    .frame(false),
                );
                reset.widget_info({
                    let label = format!("{} {label}", app.loc.tr("param-reset"));
                    move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone())
                });
                if reset.clicked() {
                    app.state.reset_row_param(row, &view.name, now_ms);
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut value = view.value;
                value_box(ui, p, &mut value, view, name.id);
                if (value - view.value).abs() > f64::EPSILON {
                    app.state.set_row_param(row, &view.name, value, now_ms);
                }
            });
        });
        let mut value = view.value;
        if slider(ui, p, &mut value, view, &label) {
            app.state.set_row_param(row, &view.name, value, now_ms);
        }
    });
}

fn humanise(name: &str) -> String {
    let mut out = name.replace(['_', '-'], " ");
    if let Some(first) = out.get(0..1) {
        out = first.to_uppercase() + &out[1..];
    }
    out
}

fn value_box(
    ui: &mut egui::Ui,
    p: crate::theme::Palette,
    value: &mut f64,
    view: &ParamView,
    labelled_by: egui::Id,
) {
    egui::Frame::NONE
        .fill(p.panel)
        .stroke(Stroke::new(1.0_f32, p.outline))
        .corner_radius(3.0)
        .inner_margin(Margin::symmetric(5, 0))
        .show(ui, |ui| {
            ui.set_height(18.0);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                if !view.unit.is_empty() {
                    ui.label(text(ui.ctx(), &view.unit, 10.0, Weight::Regular, p.muted));
                }
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
                    let decimals = if view.step >= 1.0 { 0 } else { 2 };
                    let drag = ui.add(
                        egui::DragValue::new(value)
                            .range(view.min..=view.max)
                            .speed(view.step)
                            .fixed_decimals(decimals),
                    );
                    drag.labelled_by(labelled_by);
                });
            });
        });
}

/// The design's slider: a 2 px track, orange up to the value, with a ringed thumb.
fn slider(
    ui: &mut egui::Ui,
    p: crate::theme::Palette,
    value: &mut f64,
    view: &ParamView,
    label: &str,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::click_and_drag());
    let span = (view.max - view.min).max(f64::EPSILON);
    let fraction = ((*value - view.min) / span).clamp(0.0, 1.0);
    let (left, right) = (rect.left() + 6.0, rect.right() - 6.0);
    let to_x = |f: f64| left + super::f32_of(f) * (right - left);
    let thumb = to_x(fraction);
    let y = rect.center().y;
    ui.painter().rect_filled(
        Rect::from_min_max(Pos2::new(left, y - 1.0), Pos2::new(right, y + 1.0)),
        1.0,
        p.outline,
    );
    ui.painter().rect_filled(
        Rect::from_min_max(Pos2::new(left, y - 1.0), Pos2::new(thumb, y + 1.0)),
        1.0,
        p.orange,
    );
    ui.painter()
        .circle_filled(Pos2::new(thumb, y), 5.5, p.panel);
    ui.painter()
        .circle_stroke(Pos2::new(thumb, y), 5.0, Stroke::new(1.5_f32, p.accent));
    let name = label.to_owned();
    let current = *value;
    response.widget_info(move || {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, name.clone());
        info.value = Some(current);
        info
    });
    let mut changed = false;
    if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let f = f64::from(((pos.x - left) / (right - left)).clamp(0.0, 1.0));
        let raw = view.min + f * span;
        let stepped = (raw / view.step).round() * view.step;
        let next = stepped.clamp(view.min, view.max);
        if (next - *value).abs() > f64::EPSILON {
            *value = next;
            changed = true;
        }
    }
    if response.has_focus() {
        let nudge = ui.input(|i| {
            if i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowUp) {
                1.0
            } else if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowDown) {
                -1.0
            } else {
                0.0
            }
        });
        if nudge != 0.0 {
            *value = (*value + nudge * view.step).clamp(view.min, view.max);
            changed = true;
        }
    }
    changed
}

/// The colour legend, folded away at the foot of the panel.
fn legend(ui: &mut egui::Ui, app: &App, p: crate::theme::Palette) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.collapsing(
            text(
                ui.ctx(),
                app.loc.tr("ops-legend"),
                11.0,
                Weight::Regular,
                p.muted,
            ),
            |ui| {
                for entry in super::operation_legend() {
                    let [r, g, b] = entry.colour;
                    ui.horizontal(|ui| {
                        ui.colored_label(Color32::from_rgb(r, g, b), "■");
                        ui.label(format!(
                            "{} — {:?}, {}",
                            entry.kind.as_str(),
                            entry.style,
                            entry.icon
                        ));
                    });
                }
            },
        );
    });
}
