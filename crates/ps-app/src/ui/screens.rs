//! The roomy screens of the design: Preview with its simulation bar, Export with its single
//! card, and the Running overlay (a prompt and a progress pill) that sits over either.

use egui::{Align, Color32, Layout, Margin, Pos2, Rect, Sense, Stroke, UiBuilder, vec2};
use ps_render::{MarkerKind, PLAYBACK_SPEEDS, STEPS_PER_SECOND, markers, step_at_fraction};

use super::zest::{self, Icon, Weight, font, palette, text};
use super::{App, chrome, count_f64, extras, machine_panel};
use crate::l10n::Arg;
use crate::numfmt::format_duration_ms;
use crate::state::{JobStatus, Mode};

// ---- shared pieces ----

/// The header of the roomy screens: project name, mode switch card, library and machine pill.
pub(super) fn roomy_header(ctx: &egui::Context, app: &mut App, now_ms: u64, library: bool) {
    let p = palette(ctx);
    chrome::menu_strip(ctx, app, now_ms);
    egui::TopBottomPanel::top("roomy_header")
        .show_separator_line(false)
        .exact_height(72.0)
        .frame(egui::Frame::NONE.fill(p.ground))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            let centre = Rect::from_center_size(full.center(), vec2(302.0, 56.0));
            let left = Rect::from_min_max(
                Pos2::new(full.left() + 24.0, full.top()),
                Pos2::new(centre.left() - 16.0, full.bottom()),
            );
            let right = Rect::from_min_max(
                Pos2::new(centre.right() + 16.0, full.top()),
                Pos2::new(full.right() - 24.0, full.bottom()),
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(left)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    zest::logo(ui, 32.0, p.ink);
                    let name = app.state.window_title();
                    let name = name
                        .trim_end_matches(" — Plotter Studio")
                        .trim_end_matches('*');
                    ui.label(text(ui.ctx(), name, 15.0, Weight::Semibold, p.ink));
                },
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(centre)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| mode_card(ui, app, p, now_ms, library),
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(right)
                    .layout(Layout::right_to_left(Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    machine_pill(ui, app, p);
                    let label = app.loc.tr("library-button");
                    let on = app.library_open;
                    if zest::icon_button(ui, p, Icon::Book, vec2(40.0, 40.0), 18.0, on, &label)
                        .clicked()
                    {
                        app.library_open = !app.library_open;
                    }
                },
            );
        });
}

fn mode_card(
    ui: &mut egui::Ui,
    app: &mut App,
    p: crate::theme::Palette,
    now_ms: u64,
    library: bool,
) {
    let warnings = app.state.warning_count();
    egui::Frame::NONE
        .fill(p.panel)
        .corner_radius(10.0)
        .shadow(egui::epaint::Shadow {
            offset: [0, 6],
            blur: 20,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(0x17, 0x1a, 0x3a, 20),
        })
        .inner_margin(Margin::same(4))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
            for (mode, key, id) in [
                (Mode::Prepare, "mode-prepare", "mode.prepare"),
                (Mode::Preview, "mode-preview", "mode.preview"),
                (Mode::Export, "mode-export", "mode.export"),
            ] {
                let selected = !library && app.state.mode() == mode;
                let label = app.loc.tr(key);
                let badge = (mode == Mode::Export && warnings > 0).then_some(warnings);
                let response = chrome::segment(ui, p, &label, selected, badge, 40.0, 18.0);
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

/// The machine chip as a pill with a ringed status dot.
fn machine_pill(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette) {
    let label = machine_panel::chip_text(app);
    let ready = app.machine.is_ready();
    let galley =
        ui.painter()
            .layout_no_wrap(label.clone(), font(ui.ctx(), 13.0, Weight::Medium), p.ink);
    let size = vec2(galley.size().x + 14.0 + 8.0 + 10.0 + 16.0, 40.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    zest::shadow(ui.painter(), rect, 20, false);
    ui.painter().rect_filled(rect, 20.0, p.panel);
    let (dot, ring) = if ready {
        (p.ok, Color32::from_rgb(0xdc, 0xf7, 0xeb))
    } else {
        (p.orange, p.tint)
    };
    let c = Pos2::new(rect.left() + 18.0, rect.center().y);
    ui.painter().circle_filled(c, 8.0, ring);
    ui.painter().circle_filled(c, 4.0, dot);
    ui.painter().galley(
        Pos2::new(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0),
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

/// A large gradient button with a dark label; returns its response.
fn primary_button(
    ui: &mut egui::Ui,
    p: crate::theme::Palette,
    label: &str,
    size: egui::Vec2,
    glyph: Option<Icon>,
    enabled: bool,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    if enabled {
        zest::shadow(ui.painter(), rect, 10, false);
        zest::gradient_rect(ui.painter(), rect, 10.0);
    } else {
        ui.painter().rect_filled(rect, 10.0, p.ground);
    }
    let colour = if enabled { p.ink } else { p.muted };
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        font(ui.ctx(), 16.0, Weight::Semibold),
        colour,
    );
    let icon_w = if glyph.is_some() { 26.0 } else { 0.0 };
    let total = galley.size().x + icon_w;
    let left = rect.center().x - total / 2.0;
    if let Some(glyph) = glyph {
        zest::icon(
            ui.painter(),
            Pos2::new(left + 9.0, rect.center().y),
            glyph,
            18.0,
            colour,
        );
    }
    ui.painter().galley(
        Pos2::new(left + icon_w, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );
    let name = label.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name.clone())
    });
    response
}

fn clock(ms: u64) -> String {
    let s = ms / 1_000;
    format!("{}:{:02}", s / 60, s % 60)
}

// ---- preview ----

/// The Preview screen: the mat in a white card with the view switch and the simulation bar.
pub(super) fn preview_screen(ctx: &egui::Context, app: &mut App, now_ms: u64) {
    let p = palette(ctx);
    roomy_header(ctx, app, now_ms, false);
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(p.ground).inner_margin(Margin {
            left: 24,
            right: 24,
            top: 0,
            bottom: 24,
        }))
        .show(ctx, |ui| {
            let card = ui.available_rect_before_wrap();
            zest::shadow(ui.painter(), card, 20, false);
            ui.painter().rect_filled(card, 20.0, p.panel);
            let canvas_area = Rect::from_min_max(
                Pos2::new(card.left() + 24.0, card.top() + 24.0),
                Pos2::new(card.right() - 24.0, card.bottom() - 112.0),
            );
            ui.scope_builder(UiBuilder::new().max_rect(canvas_area), |ui| {
                super::canvas(ui, app, now_ms);
            });
            let toggle = Rect::from_min_size(
                Pos2::new(card.left() + 16.0, card.top() + 16.0),
                vec2(card.width() - 32.0, 40.0),
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(toggle)
                    .layout(Layout::left_to_right(Align::Min)),
                |ui| view_toggle(ui, app, p),
            );
            let bar = Rect::from_min_max(
                Pos2::new(card.left() + 24.0, card.bottom() - 24.0 - 68.0),
                Pos2::new(card.right() - 24.0, card.bottom() - 24.0),
            );
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(bar)
                    .layout(Layout::top_down(Align::Center)),
                |ui| sim_bar(ui, app, p),
            );
        });
}

fn view_toggle(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette) {
    ui.spacing_mut().item_spacing.x = 8.0;
    egui::Frame::NONE
        .fill(p.ground)
        .corner_radius(10.0)
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (material, key) in [(true, "preview-realistic"), (false, "preview-toolpath")] {
                let selected = app.preview_material == material;
                let label = app.loc.tr(key);
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
                    if selected { p.ink } else { p.muted },
                );
                let (rect, response) =
                    ui.allocate_exact_size(vec2(galley.size().x + 28.0, 34.0), Sense::click());
                if selected {
                    zest::shadow(ui.painter(), rect, 6, false);
                    ui.painter().rect_filled(rect, 6.0, p.panel);
                }
                let colour = if selected { p.ink } else { p.muted };
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
                    app.preview_material = material;
                }
            }
        });
    if app.preview_material {
        egui::Frame::NONE
            .fill(p.ground)
            .corner_radius(10.0)
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| extras::view_controls(ui, app));
    }
}

/// The floating bar under the preview: play, time, timeline with tool segments, speed.
#[expect(
    clippy::too_many_lines,
    reason = "straight-line drawing of one panel; splitting it would only scatter the layout"
)]
fn sim_bar(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette) {
    let total = app
        .state
        .compiled()
        .and_then(|c| c.job.mats.first())
        .map_or(0, |m| m.steps.len());
    let width = ui.available_width().min(760.0);
    let (bar, _) = ui.allocate_exact_size(vec2(width, 68.0), Sense::hover());
    zest::shadow(ui.painter(), bar, 34, true);
    ui.painter().rect_filled(bar, 34.0, p.panel);
    if total == 0 {
        let message = match app.state.job_status() {
            JobStatus::Failed(why) => app
                .loc
                .tr_args("preview-failed", &[("why", Arg::Text(&why))]),
            JobStatus::Ready | JobStatus::OutOfDate => app.loc.tr("preview-nothing"),
            _ => extras::job_status_text(app),
        };
        let response = ui.interact(bar, ui.id().with("nothing"), Sense::hover());
        ui.painter().text(
            bar.center(),
            egui::Align2::CENTER_CENTER,
            &message,
            font(ui.ctx(), 13.0, Weight::Regular),
            p.muted,
        );
        response.widget_info(move || {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, message.clone())
        });
        return;
    }
    // The first time the job is shown it is shown finished; playing starts it over.
    if app.preview_unseen {
        app.preview_step = total;
        app.preview_unseen = false;
    }
    advance(ui, app, total);
    let total_ms = app
        .state
        .compiled()
        .and_then(|c| c.job.mats.first().map(|m| m.estimate.machine_time_ms))
        .unwrap_or(0);
    let elapsed = u64::try_from(
        u128::from(total_ms) * u128::try_from(app.preview_step).unwrap_or(0)
            / u128::try_from(total).unwrap_or(1),
    )
    .unwrap_or(total_ms);

    // Play / pause.
    let play_rect = Rect::from_center_size(
        Pos2::new(bar.left() + 10.0 + 24.0, bar.center().y),
        vec2(48.0, 48.0),
    );
    let play = ui.interact(play_rect, ui.id().with("play"), Sense::click());
    zest::shadow(ui.painter(), play_rect, 24, false);
    zest::gradient_rect(ui.painter(), play_rect, 24.0);
    zest::icon(
        ui.painter(),
        play_rect.center(),
        if app.preview_playing {
            Icon::Pause
        } else {
            Icon::Play
        },
        18.0,
        p.ink,
    );
    let label = if app.preview_playing {
        app.loc.tr("preview-pause")
    } else {
        app.loc.tr("preview-play")
    };
    let name = label.clone();
    play.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone())
    });
    if play.clicked() {
        app.preview_playing = !app.preview_playing;
        if app.preview_playing && app.preview_step >= total {
            app.preview_step = 0;
        }
        app.preview_pos = count_f64(app.preview_step);
    }

    // Times.
    ui.painter().text(
        Pos2::new(play_rect.right() + 16.0, bar.center().y),
        egui::Align2::LEFT_CENTER,
        clock(elapsed),
        font(ui.ctx(), 14.0, Weight::Semibold),
        p.ink,
    );
    let speed_text = format!("{}×", app.preview_speed);
    let speed_rect = Rect::from_center_size(
        Pos2::new(bar.right() - 20.0 - 25.0, bar.center().y),
        vec2(50.0, 36.0),
    );
    let speed = ui.interact(speed_rect, ui.id().with("speed"), Sense::click());
    ui.painter().rect_filled(speed_rect, 18.0, p.ground);
    ui.painter().text(
        speed_rect.center(),
        egui::Align2::CENTER_CENTER,
        &speed_text,
        font(ui.ctx(), 13.0, Weight::Semibold),
        p.ink,
    );
    let speed_label = format!("{}: {speed_text}", app.loc.tr("preview-speed"));
    speed.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, speed_label.clone())
    });
    if speed.clicked() {
        let at = PLAYBACK_SPEEDS
            .iter()
            .position(|s| (*s - app.preview_speed).abs() < f64::EPSILON)
            .unwrap_or(0);
        app.preview_speed = PLAYBACK_SPEEDS[(at + 1) % PLAYBACK_SPEEDS.len()];
    }
    ui.painter().text(
        Pos2::new(speed_rect.left() - 16.0, bar.center().y),
        egui::Align2::RIGHT_CENTER,
        clock(total_ms),
        font(ui.ctx(), 14.0, Weight::Regular),
        p.muted,
    );

    // Timeline between the two.
    let track = Rect::from_min_max(
        Pos2::new(play_rect.right() + 16.0 + 54.0, bar.center().y - 12.0),
        Pos2::new(speed_rect.left() - 16.0 - 44.0, bar.center().y + 12.0),
    );
    timeline(ui, app, p, track, total);
    app.preview_step = app.preview_step.min(total);
}

/// Moves the playhead while playing.
fn advance(ui: &egui::Ui, app: &mut App, total: usize) {
    app.preview_step = app.preview_step.min(total);
    if !app.preview_playing {
        return;
    }
    let dt = f64::from(ui.input(|i| i.stable_dt));
    app.preview_pos += dt * app.preview_speed * STEPS_PER_SECOND;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a non-negative step position below the program length"
    )]
    let step = (app.preview_pos.max(0.0) as usize).min(total);
    app.preview_step = step;
    if step >= total {
        app.preview_playing = false;
    }
    ui.ctx().request_repaint();
}

fn timeline(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette, track: Rect, total: usize) {
    let response = ui.interact(track, ui.id().with("timeline"), Sense::click_and_drag());
    let bar_y = track.center().y;
    let steps = app
        .state
        .compiled()
        .and_then(|c| c.job.mats.first())
        .map(|m| markers(&m.steps))
        .unwrap_or_default();
    let tools = app
        .state
        .compiled()
        .and_then(|c| c.job.mats.first())
        .map(|m| m.tool_table.clone())
        .unwrap_or_default();
    let frac_x = |step: usize| {
        track.left() + track.width() * super::f32_of(count_f64(step) / count_f64(total.max(1)))
    };
    // Segments: from each tool change to the next, in that tool's colour.
    let mut starts: Vec<(usize, u8)> = steps
        .iter()
        .filter_map(|m| match m.kind {
            MarkerKind::ToolChange { slot } => Some((m.step, slot)),
            MarkerKind::Pause { .. } => None,
        })
        .collect();
    if starts.first().is_none_or(|(s, _)| *s > 0) {
        starts.insert(0, (0, 0));
    }
    let played = frac_x(app.preview_step);
    for (i, (start, slot)) in starts.iter().enumerate() {
        let end = starts.get(i + 1).map_or(total, |(s, _)| *s);
        let (x0, x1) = (frac_x(*start) + 1.0, frac_x(end) - 1.0);
        if x1 <= x0 {
            continue;
        }
        let colour = extras::slot_colour(&tools, Some(*slot));
        let pale = lerp_white(colour, 0.7);
        let seg = |a: f32, b: f32, c: Color32| {
            if b > a {
                ui.painter().rect_filled(
                    Rect::from_min_max(Pos2::new(a, bar_y - 3.0), Pos2::new(b, bar_y + 3.0)),
                    2.0,
                    c,
                );
            }
        };
        seg(x0, x1.min(played).max(x0), colour);
        seg(x0.max(played).min(x1), x1, pale);
    }
    for marker in &steps {
        let x = frac_x(marker.step);
        let colour = match marker.kind {
            MarkerKind::ToolChange { .. } => Color32::from_rgb(0x84, 0x8a, 0xa3),
            MarkerKind::Pause { .. } => p.danger,
        };
        ui.painter().rect_filled(
            Rect::from_center_size(Pos2::new(x, bar_y), vec2(2.0, 16.0)),
            1.0,
            colour,
        );
    }
    ui.painter()
        .circle_filled(Pos2::new(played, bar_y), 10.0, p.panel);
    ui.painter()
        .circle_stroke(Pos2::new(played, bar_y), 8.5, Stroke::new(3.0_f32, p.ink));
    let label = app.loc.tr("preview-timeline");
    let value = count_f64(app.preview_step) / count_f64(total.max(1));
    response.widget_info(move || {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, label.clone());
        info.value = Some(value);
        info
    });
    if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let fraction = f64::from(((pos.x - track.left()) / track.width()).clamp(0.0, 1.0));
        app.preview_step = step_at_fraction(total, fraction);
        app.preview_pos = count_f64(app.preview_step);
        app.preview_playing = false;
    }
}

fn lerp_white(c: Color32, t: f32) -> Color32 {
    let mix = |v: u8| {
        let out = f32::from(v) + (255.0 - f32::from(v)) * t;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "an interpolation towards white stays in 0..=255"
        )]
        let byte = out.round().clamp(0.0, 255.0) as u8;
        byte
    };
    Color32::from_rgb(mix(c.r()), mix(c.g()), mix(c.b()))
}

// ---- export ----

/// The Export screen: one card with the job summary, problems and the send button.
pub(super) fn export_screen(ctx: &egui::Context, app: &mut App, now_ms: u64) {
    let p = palette(ctx);
    roomy_header(ctx, app, now_ms, false);
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(p.ground))
        .show(ctx, |ui| {
            // Fill the window, so the scroll bar sits at its edge and not beside the card.
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(72.0);
                    let width = ui.available_width().min(520.0);
                    ui.vertical_centered(|ui| {
                        ui.set_width(width);
                        egui::Frame::NONE
                            .fill(p.panel)
                            .corner_radius(20.0)
                            .inner_margin(Margin::same(32))
                            .show(ui, |ui| {
                                ui.set_width(width - 64.0);
                                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                    export_card(ui, app, p, now_ms);
                                });
                            });
                    });
                    if app.gcode_open {
                        ui.add_space(16.0);
                        ui.vertical_centered(|ui| {
                            ui.set_width(width);
                            egui::Frame::NONE
                                .fill(p.panel)
                                .corner_radius(20.0)
                                .inner_margin(Margin::same(24))
                                .show(ui, |ui| {
                                    ui.set_width(width - 48.0);
                                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                        extras::export_gcode(ui, app);
                                    });
                                });
                        });
                        ui.add_space(32.0);
                    }
                });
        });
}

#[expect(
    clippy::too_many_lines,
    reason = "straight-line drawing of one panel; splitting it would only scatter the layout"
)]
fn export_card(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette, now_ms: u64) {
    ui.spacing_mut().item_spacing.y = 8.0;
    let ready = app.state.context().has_compile;
    let summary = app.state.compile_summary();
    let has_job = summary.as_ref().is_some_and(|s| s.mats > 0);
    let title = if has_job {
        app.loc.tr("export-ready-title")
    } else {
        app.loc.tr("export-nothing-title")
    };
    ui.label(text(ui.ctx(), title, 28.0, Weight::Semibold, p.ink));
    if let Some(s) = summary.filter(|s| s.mats > 0) {
        let time = format_duration_ms(s.machine_time_ms);
        ui.label(text(
            ui.ctx(),
            app.loc.tr_args(
                "export-summary",
                &[
                    ("mats", Arg::Number(count_f64(s.mats))),
                    ("time", Arg::Text(&time)),
                    ("changes", Arg::Number(f64::from(s.tool_changes))),
                ],
            ),
            14.0,
            Weight::Regular,
            p.muted,
        ));
    } else {
        let words = match app.state.job_status() {
            JobStatus::NoMachine | JobStatus::Blocked { .. } | JobStatus::Failed(_) => {
                extras::job_status_text(app)
            }
            _ => app.loc.tr("export-nothing"),
        };
        ui.label(text(ui.ctx(), words, 14.0, Weight::Regular, p.muted));
    }
    ui.add_space(12.0);
    mat_rows(ui, app, p);
    problems(ui, app, p);
    ui.add_space(8.0);
    let machine = app.machine.model().unwrap_or_default();
    let send_label = if machine.is_empty() {
        app.loc.tr("machine-send")
    } else {
        app.loc
            .tr_args("export-send-to", &[("machine", Arg::Text(&machine))])
    };
    let can_send = ready && app.machine.is_ready();
    let width = ui.available_width();
    if primary_button(
        ui,
        p,
        &send_label,
        vec2(width, 52.0),
        Some(Icon::Send),
        can_send,
    )
    .clicked()
    {
        app.run("machine.send", now_ms);
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 36.0;
        let galley_w = 200.0;
        ui.add_space((ui.available_width() - galley_w) / 2.0);
        let save = ui.add_enabled(
            ready,
            egui::Button::new(text(
                ui.ctx(),
                app.loc.tr("export-button"),
                14.0,
                Weight::Medium,
                p.ink,
            ))
            .frame(false),
        );
        if save.clicked() {
            app.run("file.export", now_ms);
        }
        let view = ui.add(
            egui::Button::new(text(
                ui.ctx(),
                app.loc.tr("export-gcode"),
                14.0,
                Weight::Medium,
                p.ink,
            ))
            .frame(false),
        );
        if view.clicked() {
            app.gcode_open = !app.gcode_open;
        }
    });
    if let Some(status) = &app.export_status {
        ui.label(text(ui.ctx(), status, 13.0, Weight::Regular, p.muted));
    }
    ui.collapsing(
        text(
            ui.ctx(),
            app.loc.tr("export-options"),
            13.0,
            Weight::Medium,
            p.muted,
        ),
        |ui| extras::job_options(ui, app, now_ms),
    );
}

fn mat_rows(ui: &mut egui::Ui, app: &App, p: crate::theme::Palette) {
    let names: Vec<String> = app
        .state
        .mat_summaries()
        .into_iter()
        .map(|m| m.name)
        .collect();
    let times: Vec<u64> = app
        .state
        .compiled()
        .map(|c| {
            c.job
                .mats
                .iter()
                .map(|m| m.estimate.machine_time_ms)
                .collect()
        })
        .unwrap_or_default();
    for (i, ms) in times.iter().enumerate() {
        let name = names.get(i).cloned().unwrap_or_default();
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
        ui.painter().rect_filled(rect, 14.0, p.ground);
        let thumb = Rect::from_min_size(rect.min + vec2(10.0, 10.0), vec2(52.0, 52.0));
        ui.painter().rect_filled(thumb, 10.0, p.panel);
        ui.painter().rect_filled(
            Rect::from_center_size(thumb.center(), vec2(30.0, 34.0)),
            2.0,
            Color32::from_rgb(0xea, 0xd6, 0xb2),
        );
        ui.painter().text(
            Pos2::new(rect.left() + 76.0, rect.center().y - 9.0),
            egui::Align2::LEFT_CENTER,
            &name,
            font(ui.ctx(), 14.0, Weight::Semibold),
            p.ink,
        );
        ui.painter().text(
            Pos2::new(rect.left() + 76.0, rect.center().y + 11.0),
            egui::Align2::LEFT_CENTER,
            format_duration_ms(*ms),
            font(ui.ctx(), 13.0, Weight::Regular),
            p.muted,
        );
        let label = format!("{name}: {}", format_duration_ms(*ms));
        response.widget_info(move || {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.clone())
        });
        ui.add_space(0.0);
    }
}

/// One problem the job reports, in words, with where it is.
struct Problem {
    message: String,
    /// "Mat 1 · Object 3", when the problem is about a particular object.
    place: Option<String>,
    /// The mat and object to show when the user asks.
    target: Option<(ps_core::MatId, Option<ps_core::ObjectId>)>,
    error: bool,
    count: usize,
}

fn problem_list(app: &App) -> Vec<Problem> {
    let doc = app.state.document();
    let mut out: Vec<Problem> = Vec::new();
    for d in &app.state.problems() {
        if d.severity == ps_core::Severity::Info {
            continue;
        }
        let args: Vec<(&str, Arg<'_>)> = d
            .args
            .iter()
            .map(|(k, v)| (k.as_str(), Arg::Text(v)))
            .collect();
        let key = d.key.as_str();
        let found = app.loc.tr_args(key, &args);
        // A key with no message yet reads as its own words rather than as a code.
        let message = if found == key {
            let words = key.trim_start_matches("diag-").replace('-', " ");
            let mut chars = words.chars();
            chars.next().map_or_else(String::new, |c| {
                c.to_uppercase().collect::<String>() + chars.as_str()
            })
        } else {
            found
        };
        let (place, target) = match &d.location {
            Some(loc) => {
                let mat = loc
                    .mat
                    .and_then(|id| doc.mat(id).map(|m| (id, m.name.clone())));
                let object = loc.mat.zip(loc.object).and_then(|(m, o)| {
                    let mat = doc.mat(m)?;
                    let n = mat.objects.keys().position(|id| *id == o)? + 1;
                    Some((o, n))
                });
                let place = match (&mat, object) {
                    (Some((_, name)), Some((_, n))) => Some(format!(
                        "{name} · {}",
                        app.loc
                            .tr_args("tree-object", &[("n", Arg::Number(count_f64(n)))])
                    )),
                    (Some((_, name)), None) => Some(name.clone()),
                    _ => None,
                };
                (place, mat.map(|(id, _)| (id, object.map(|(o, _)| o))))
            }
            None => (None, None),
        };
        let error = d.severity == ps_core::Severity::Error;
        if let Some(same) = out
            .iter_mut()
            .find(|p| p.message == message && p.place == place)
        {
            same.count += 1;
        } else {
            out.push(Problem {
                message,
                place,
                target,
                error,
                count: 1,
            });
        }
    }
    out
}

fn problems(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette) {
    let list = problem_list(app);
    if list.is_empty() {
        ui.label(text(
            ui.ctx(),
            app.loc.tr("export-no-problems"),
            13.0,
            Weight::Regular,
            p.muted,
        ));
        return;
    }
    let mut show: Option<(ps_core::MatId, Option<ps_core::ObjectId>)> = None;
    egui::Frame::NONE
        .fill(Color32::from_rgb(0xff, 0xe4, 0xe4))
        .corner_radius(14.0)
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                zest::icon(ui.painter(), rect.center(), Icon::Warning, 18.0, p.danger);
                ui.label(text(
                    ui.ctx(),
                    app.loc.tr_args(
                        "export-problem-count",
                        &[("count", Arg::Number(count_f64(list.len())))],
                    ),
                    13.0,
                    Weight::Semibold,
                    p.danger,
                ));
            });
            for problem in &list {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let mut message = problem.message.clone();
                    if problem.count > 1 {
                        message = format!("{message} (×{})", problem.count);
                    }
                    ui.add(egui::Label::new(text(
                        ui.ctx(),
                        message,
                        13.0,
                        Weight::Regular,
                        if problem.error { p.danger } else { p.ink },
                    )));
                    ui.horizontal(|ui| {
                        if let Some(place) = &problem.place {
                            ui.label(text(ui.ctx(), place, 12.0, Weight::Regular, p.muted));
                        }
                        if problem.target.is_some() {
                            let link = ui.add(
                                egui::Button::new(text(
                                    ui.ctx(),
                                    app.loc.tr("export-show-problem"),
                                    12.0,
                                    Weight::Semibold,
                                    p.accent,
                                ))
                                .frame(false),
                            );
                            if link.clicked() {
                                show = problem.target;
                            }
                        }
                    });
                });
            }
        });
    if let Some((mat, object)) = show {
        app.state.set_current_mat(mat);
        if let Some(object) = object {
            app.state.select_object(object);
        }
        app.state.run_command("mode.prepare", 0);
        app.canvas_fitted = false;
    }
}

// ---- running ----

/// The overlay while a job runs: a modal prompt when the machine asks for something, and a
/// progress pill at the foot of the screen.
pub(super) fn running_overlay(ctx: &egui::Context, app: &mut App) {
    let p = palette(ctx);
    let prompt = app.machine.prompt().cloned();
    let progress = app.machine.status().and_then(|s| {
        s.progress
            .as_ref()
            .map(|pr| (pr.lines_done, pr.lines_total))
    });
    if let Some(prompt) = prompt {
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::new("running_scrim"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.painter().rect_filled(
                    screen,
                    0.0,
                    Color32::from_rgba_unmultiplied(0xf4, 0xf5, 0xfa, 0xb3),
                );
                ui.allocate_rect(screen, Sense::click_and_drag());
            });
        egui::Area::new(egui::Id::new("running_prompt"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -40.0])
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(p.panel)
                    .corner_radius(20.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 30,
                        spread: 0,
                        color: Color32::from_rgba_unmultiplied(0x17, 0x1a, 0x3a, 50),
                    })
                    .inner_margin(Margin::same(28))
                    .show(ui, |ui| {
                        ui.set_width(364.0);
                        ui.spacing_mut().item_spacing.y = 14.0;
                        ui.vertical_centered(|ui| {
                            ui.label(text(
                                ui.ctx(),
                                &prompt.message,
                                22.0,
                                Weight::Semibold,
                                p.ink,
                            ));
                            let width = ui.available_width();
                            let done = primary_button(
                                ui,
                                p,
                                &app.loc.tr("machine-prompt-done"),
                                vec2(width, 52.0),
                                None,
                                true,
                            );
                            if done.clicked() {
                                app.machine.answer_prompt();
                            }
                            let cancel = ui.add(
                                egui::Button::new(text(
                                    ui.ctx(),
                                    app.loc.tr("machine-cancel"),
                                    14.0,
                                    Weight::Medium,
                                    p.danger,
                                ))
                                .frame(false),
                            );
                            if cancel.clicked() {
                                app.machine.cancel();
                            }
                        });
                    });
            });
    }
    if let Some((done, total)) = progress.filter(|(_, total)| *total > 0) {
        let paused = app
            .machine
            .status()
            .is_some_and(|s| s.state == ps_device::MachineState::Paused);
        progress_pill(ctx, app, p, (done, total), paused);
    }
}

/// How far above the screen's foot the progress pill floats. In the preview it clears the
/// playback bar, which takes the same place there.
fn pill_lift(mode: Mode) -> f32 {
    if mode == Mode::Preview { 140.0 } else { 48.0 }
}

fn progress_pill(
    ctx: &egui::Context,
    app: &mut App,
    p: crate::theme::Palette,
    (done, total): (u64, u64),
    paused: bool,
) {
    let fraction = if total == 0 {
        0.0
    } else {
        #[expect(clippy::cast_precision_loss, reason = "line counts are far below 2^52")]
        let f = done as f64 / total as f64;
        f
    };
    let lift = pill_lift(app.state.mode());
    egui::Area::new(egui::Id::new("progress_pill"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -lift])
        .show(ctx, |ui| {
            let (rect, body) = ui.allocate_exact_size(vec2(560.0, 64.0), Sense::hover());
            let said = app.loc.tr("job-progress");
            body.widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, said.clone())
            });
            zest::shadow(ui.painter(), rect, 32, true);
            ui.painter().rect_filled(rect, 32.0, p.panel);
            ui.painter().text(
                Pos2::new(
                    rect.left() + 22.0,
                    rect.center().y - if paused { 8.0 } else { 0.0 },
                ),
                egui::Align2::LEFT_CENTER,
                app.state.current_mat().name.clone(),
                font(ui.ctx(), 14.0, Weight::Semibold),
                p.ink,
            );
            let track = Rect::from_min_max(
                Pos2::new(
                    rect.left() + 110.0,
                    rect.center().y - 4.0 - if paused { 8.0 } else { 0.0 },
                ),
                Pos2::new(
                    rect.right() - 160.0,
                    rect.center().y + 4.0 - if paused { 8.0 } else { 0.0 },
                ),
            );
            ui.painter().rect_filled(track, 4.0, p.ground);
            ui.painter().rect_filled(
                Rect::from_min_size(
                    track.min,
                    vec2(track.width() * super::f32_of(fraction), track.height()),
                ),
                4.0,
                p.orange,
            );
            pill_status(ui, app, p, (rect, track), (fraction, paused));
            let button = Rect::from_center_size(
                Pos2::new(rect.right() - 10.0 - 22.0, rect.center().y),
                vec2(44.0, 44.0),
            );
            let response = ui.interact(button, ui.id().with("pause"), Sense::click());
            ui.painter().circle_filled(button.center(), 22.0, p.ground);
            let (icon, key) = if paused {
                (Icon::Play, "machine-resume")
            } else {
                (Icon::Pause, "machine-pause")
            };
            zest::icon(ui.painter(), button.center(), icon, 16.0, p.ink);
            let name = app.loc.tr(key);
            response.widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone())
            });
            if response.clicked() {
                if paused {
                    app.machine.resume();
                } else {
                    app.machine.pause();
                }
            }
        });
}

/// The percentage (or "Paused" with a note on what that means) beside the progress track.
fn pill_status(
    ui: &mut egui::Ui,
    app: &App,
    p: crate::theme::Palette,
    (rect, track): (Rect, Rect),
    (fraction, paused): (f64, bool),
) {
    let status_text = if paused {
        app.loc.tr("state-paused")
    } else {
        format!("{:.0}%", fraction * 100.0)
    };
    let status_at = Pos2::new(track.right() + 16.0, track.center().y);
    let status_galley = ui.painter().layout_no_wrap(
        status_text.clone(),
        font(ui.ctx(), 14.0, Weight::Semibold),
        if paused { p.orange } else { p.ink },
    );
    let status_rect = Rect::from_min_size(
        status_at - vec2(0.0, status_galley.size().y / 2.0),
        status_galley.size(),
    );
    ui.painter().galley(status_rect.min, status_galley, p.ink);
    if paused {
        // Pausing only stops what is sent next: the machine finishes the moves it holds.
        let note = app.loc.tr("job-pausing-note");
        ui.painter().text(
            Pos2::new(rect.left() + 110.0, rect.center().y + 14.0),
            egui::Align2::LEFT_CENTER,
            &note,
            font(ui.ctx(), 11.0, Weight::Regular),
            p.muted,
        );
        ui.interact(status_rect, ui.id().with("paused-text"), Sense::hover())
            .widget_info(move || {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, status_text.clone())
            });
    }
}
