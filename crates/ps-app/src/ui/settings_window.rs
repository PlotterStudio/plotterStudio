//! The settings window: units, decimal separator, theme, language,
//! autosave, the two internet opt-ins (both off), preview quality, and shortcut rebinding with
//! conflicts shown. Changes take effect live and never touch project files.

use egui::Key;
use ps_library::{DecimalSeparator, DisplayUnits, GpuQuality, Theme};

use super::{App, key_name, selectable};
use crate::commands::{AssignError, Mods, Platform, Shortcut};
use crate::l10n::Arg;

/// Applies the theme setting to the egui context when it changes.
pub(super) fn apply_theme(ctx: &egui::Context, app: &mut App) {
    let theme = app.state.settings().theme;
    if app.applied_theme == Some(theme) {
        return;
    }
    if app.applied_theme.is_none() {
        crate::theme::install(ctx);
    }
    ctx.set_theme(match theme {
        Theme::System => egui::ThemePreference::System,
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
    });
    app.applied_theme = Some(theme);
}

/// The settings window, when it is open: a window of its own with the system's title bar, so it
/// behaves like any other window. Where the host has no windows of its own (the test harness) it
/// is drawn inside the main one instead.
pub(super) fn settings_window(ctx: &egui::Context, app: &mut App) {
    if !app.state.settings_open() {
        return;
    }
    let title = app.loc.tr("settings-title");
    let builder = egui::ViewportBuilder::default()
        .with_title(title.clone())
        .with_inner_size([560.0, 700.0])
        .with_min_inner_size([420.0, 360.0]);
    let mut close = false;
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("plotter-settings"),
        builder,
        |ctx, class| {
            capture_shortcut(ctx, app);
            if class == egui::ViewportClass::Embedded {
                let mut open = true;
                egui::Window::new(title.clone())
                    .open(&mut open)
                    .default_width(420.0)
                    .show(ctx, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(560.0)
                            .show(ui, |ui| contents(ui, app));
                    });
                close |= !open;
            } else {
                let p = super::zest::palette(ctx);
                let frame = egui::Frame::NONE
                    .fill(p.panel)
                    .inner_margin(egui::Margin::same(16));
                egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| contents(ui, app));
                });
                close |= ctx.input(|i| i.viewport().close_requested());
            }
        },
    );
    if close || !app.state.settings_open() {
        app.state.close_settings();
    }
}

fn contents(ui: &mut egui::Ui, app: &mut App) {
    appearance(ui, app);
    saving(ui, app);
    screen(ui, app);
    network(ui, app);
    automation(ui, app);

    ui.separator();
    ui.strong(app.loc.tr("settings-shortcuts"));
    shortcut_list(ui, app);

    ui.separator();
    if ui.button(app.loc.tr("settings-close")).clicked() {
        app.state.close_settings();
    }
}

fn appearance(ui: &mut egui::Ui, app: &mut App) {
    let settings = app.state.settings().clone();

    ui.strong(app.loc.tr("settings-units"));
    ui.horizontal(|ui| {
        for (units, key) in [
            (DisplayUnits::Millimetres, "settings-millimetres"),
            (DisplayUnits::Inches, "settings-inches"),
        ] {
            if selectable(ui, settings.units == units, app.loc.tr(key)).clicked() {
                app.state.update_settings(|s| s.units = units);
            }
        }
    });

    ui.strong(app.loc.tr("settings-separator"));
    ui.horizontal(|ui| {
        for (value, key) in [
            (DecimalSeparator::Auto, "settings-separator-auto"),
            (DecimalSeparator::Dot, "settings-separator-dot"),
            (DecimalSeparator::Comma, "settings-separator-comma"),
        ] {
            if selectable(ui, settings.decimal_separator == value, app.loc.tr(key)).clicked() {
                app.state.update_settings(|s| s.decimal_separator = value);
            }
        }
    });

    ui.strong(app.loc.tr("settings-theme"));
    ui.horizontal(|ui| {
        for (value, key) in [
            (Theme::System, "settings-theme-system"),
            (Theme::Light, "settings-theme-light"),
            (Theme::Dark, "settings-theme-dark"),
        ] {
            if selectable(ui, settings.theme == value, app.loc.tr(key)).clicked() {
                app.state.update_settings(|s| s.theme = value);
            }
        }
    });

    ui.strong(app.loc.tr("settings-language"));
    ui.horizontal(|ui| {
        let current = settings.locale.clone();
        if selectable(
            ui,
            current == "system",
            app.loc.tr("settings-language-system"),
        )
        .clicked()
        {
            app.state
                .update_settings(|s| "system".clone_into(&mut s.locale));
        }
        for language in app.loc.languages() {
            let name = app.loc.tr(&format!("language-{language}"));
            if selectable(ui, current == language, name).clicked() {
                app.state
                    .update_settings(|s| s.locale.clone_from(&language));
            }
        }
    });
}

fn saving(ui: &mut egui::Ui, app: &mut App) {
    let settings = app.state.settings().clone();
    ui.horizontal(|ui| {
        ui.label(app.loc.tr("settings-autosave"));
        let mut seconds = settings.autosave_interval_s;
        if ui
            .add(egui::DragValue::new(&mut seconds).range(5..=3600))
            .changed()
        {
            app.state
                .update_settings(|s| s.autosave_interval_s = seconds);
        }
    });
    let mut keep = settings.keep_backup;
    if ui
        .checkbox(&mut keep, app.loc.tr("settings-keep-backup"))
        .changed()
    {
        app.state.update_settings(|s| s.keep_backup = keep);
    }

    ui.strong(app.loc.tr("settings-gpu"));
    ui.horizontal(|ui| {
        for (value, key) in [
            (GpuQuality::Low, "settings-gpu-low"),
            (GpuQuality::Medium, "settings-gpu-medium"),
            (GpuQuality::High, "settings-gpu-high"),
        ] {
            if selectable(ui, settings.gpu_quality == value, app.loc.tr(key)).clicked() {
                app.state.update_settings(|s| s.gpu_quality = value);
            }
        }
    });
}

/// Calibrates 100% zoom: a bar drawn as 100 mm, and the length it measured on the screen.
fn screen(ui: &mut egui::Ui, app: &mut App) {
    ui.strong(app.loc.tr("settings-screen"));
    // Logical points are 1/96 inch, scaled by the calibration found so far.
    let width = 100.0 / 25.4 * 96.0 * app.state.settings().screen_calibration;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a bar a few hundred points wide fits an f32"
    )]
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width as f32, 8.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().strong_text_color());
    ui.horizontal(|ui| {
        let label = ui.label(app.loc.tr("settings-screen-measured"));
        ui.add(
            egui::DragValue::new(&mut app.screen_measured_mm)
                .range(1.0..=1000.0)
                .speed(0.1),
        )
        .labelled_by(label.id);
        if ui.button(app.loc.tr("settings-screen-apply")).clicked() {
            let measured = app.screen_measured_mm;
            if app.state.calibrate_screen(measured) {
                app.screen_measured_mm = 100.0;
            }
        }
    });
}

fn update_check_row(ui: &mut egui::Ui, app: &mut App) {
    let on = app.state.settings().network.update_check;
    ui.horizontal(|ui| {
        let button = egui::Button::new(app.loc.tr("settings-check-now"));
        if ui
            .add_enabled(on && !app.online.checking(), button)
            .clicked()
        {
            app.update_checked = true;
            let _ = app.online.check_for_update();
        }
        if !on || !app.update_checked {
            return;
        }
        let text = if app.online.checking() {
            app.loc.tr("update-checking")
        } else if let Some(error) = app.online.last_error() {
            app.loc
                .tr_args("update-failed", &[("reason", Arg::Text(error))])
        } else if let Some(release) = app.online.update() {
            let version = release.version.to_string();
            app.loc
                .tr_args("update-available", &[("version", Arg::Text(&version))])
        } else {
            app.loc.tr("update-up-to-date")
        };
        ui.label(text);
    });
}

fn network(ui: &mut egui::Ui, app: &mut App) {
    let settings = app.state.settings().clone();
    ui.strong(app.loc.tr("settings-network"));
    ui.label(app.loc.tr("settings-network-note"));
    let mut updates = settings.network.update_check;
    if ui
        .checkbox(&mut updates, app.loc.tr("settings-update-check"))
        .changed()
    {
        app.state
            .update_settings(|s| s.network.update_check = updates);
    }
    let mut index = settings.network.material_index;
    if ui
        .checkbox(&mut index, app.loc.tr("settings-material-index"))
        .changed()
    {
        app.state
            .update_settings(|s| s.network.material_index = index);
    }
    update_check_row(ui, app);
}

/// The automation server: whether it runs, where to connect, and the token clients need.
fn automation(ui: &mut egui::Ui, app: &mut App) {
    let automation = app.state.settings().automation.clone();
    ui.separator();
    ui.strong(app.loc.tr("settings-automation"));
    ui.label(app.loc.tr("settings-automation-note"));
    let mut enabled = automation.mcp_enabled;
    if ui
        .checkbox(&mut enabled, app.loc.tr("settings-mcp-enable"))
        .changed()
    {
        app.state.update_settings(|s| {
            s.automation.mcp_enabled = enabled;
            if enabled && s.automation.mcp_token.is_empty() {
                s.automation.mcp_token = crate::mcp::generate_token();
            }
        });
    }
    if !automation.mcp_enabled {
        return;
    }

    ui.horizontal(|ui| {
        let label = ui.label(app.loc.tr("settings-mcp-port"));
        let mut port = automation.mcp_port;
        if ui
            .add(egui::DragValue::new(&mut port).range(1024..=u16::MAX))
            .labelled_by(label.id)
            .changed()
        {
            app.state.update_settings(|s| s.automation.mcp_port = port);
        }
    });
    let mut network = automation.mcp_allow_network;
    if ui
        .checkbox(&mut network, app.loc.tr("settings-mcp-network"))
        .changed()
    {
        app.state
            .update_settings(|s| s.automation.mcp_allow_network = network);
    }
    if automation.mcp_allow_network {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            app.loc.tr("settings-mcp-network-note"),
        );
    }

    match app.mcp_status().clone() {
        crate::mcp::McpStatus::Off => {}
        crate::mcp::McpStatus::Listening { urls } => {
            ui.label(app.loc.tr("settings-mcp-listening"));
            for url in urls {
                ui.horizontal(|ui| {
                    ui.monospace(&url);
                    if ui.button(app.loc.tr("settings-mcp-copy")).clicked() {
                        ui.ctx().copy_text(url.clone());
                    }
                });
            }
        }
        crate::mcp::McpStatus::Failed(reason) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                app.loc
                    .tr_args("settings-mcp-failed", &[("reason", Arg::Text(&reason))]),
            );
        }
    }

    ui.horizontal(|ui| {
        ui.label(app.loc.tr("settings-mcp-token"));
        if app.mcp_token_shown {
            ui.monospace(&automation.mcp_token);
        } else {
            ui.monospace("\u{2022}".repeat(16));
        }
    });
    ui.horizontal(|ui| {
        let toggle = if app.mcp_token_shown {
            "settings-mcp-token-hide"
        } else {
            "settings-mcp-token-show"
        };
        if ui.button(app.loc.tr(toggle)).clicked() {
            app.mcp_token_shown = !app.mcp_token_shown;
        }
        if ui.button(app.loc.tr("settings-mcp-token-copy")).clicked() {
            ui.ctx().copy_text(automation.mcp_token.clone());
        }
        if ui.button(app.loc.tr("settings-mcp-token-new")).clicked() {
            app.state
                .update_settings(|s| s.automation.mcp_token = crate::mcp::generate_token());
        }
    });
    ui.label(app.loc.tr("settings-mcp-token-note"));

    // Whether the tools may move real hardware is the user's call, kept apart from the server and
    // forgotten when the app closes; no tool can change it.
    let mut armed = app.api_armed();
    if ui
        .checkbox(&mut armed, app.loc.tr("settings-mcp-arm"))
        .changed()
    {
        app.arm_api(armed);
    }
    ui.label(app.loc.tr("settings-mcp-arm-note"));
}

fn shortcut_list(ui: &mut egui::Ui, app: &mut App) {
    if let Some(message) = app.shortcut_error.clone() {
        ui.colored_label(ui.visuals().error_fg_color, message);
    }
    let platform = Platform::current();
    let commands: Vec<&'static str> = app.state.registry().all().iter().map(|c| c.id).collect();
    for id in commands {
        let label = app.loc.command_label(id);
        let current = app.state.registry().shortcut_of(id);
        let changed = app.state.settings().shortcuts.contains_key(id);
        ui.horizontal(|ui| {
            ui.label(label.clone());
            if app.capturing == Some(id) {
                ui.label(
                    app.loc
                        .tr_args("settings-shortcut-press", &[("command", Arg::Text(&label))]),
                );
                return;
            }
            let text = current.as_ref().map_or_else(
                || app.loc.tr("settings-shortcut-none"),
                |s| s.display(platform),
            );
            ui.weak(text);
            let change = ui.button(app.loc.tr("settings-shortcut-change"));
            let accessible = app.loc.tr_args(
                "settings-shortcut-change-for",
                &[("command", Arg::Text(&label))],
            );
            change.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, accessible.clone())
            });
            if change.clicked() {
                app.capturing = Some(id);
                app.shortcut_error = None;
            }
            if changed {
                let reset = ui.button(app.loc.tr("settings-shortcut-reset"));
                let accessible = app.loc.tr_args(
                    "settings-shortcut-reset-for",
                    &[("command", Arg::Text(&label))],
                );
                reset.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, accessible.clone())
                });
                if reset.clicked() {
                    app.state.reset_shortcut(id);
                    app.shortcut_error = None;
                }
            }
        });
    }
}

/// While a shortcut is being captured, the next key press becomes the binding (Escape cancels).
fn capture_shortcut(ctx: &egui::Context, app: &mut App) {
    let Some(id) = app.capturing else {
        return;
    };
    let events = ctx.input(|i| i.events.clone());
    for event in events {
        let egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } = event
        else {
            continue;
        };
        if key == Key::Escape {
            app.capturing = None;
            return;
        }
        let Some(name) = key_name(key) else { continue };
        let Some(mut shortcut) = Shortcut::parse(&name) else {
            continue;
        };
        let mods = Mods {
            command: modifiers.command,
            shift: modifiers.shift,
            alt: modifiers.alt,
        };
        shortcut.command = mods.command;
        shortcut.shift = mods.shift;
        shortcut.alt = mods.alt;
        match app.state.assign_shortcut(id, Some(shortcut.clone())) {
            Ok(()) => {
                app.shortcut_error = None;
                app.capturing = None;
            }
            Err(AssignError::Conflict { with }) => {
                let command = app.loc.command_label(with);
                app.shortcut_error = Some(app.loc.tr_args(
                    "settings-shortcut-conflict",
                    &[
                        (
                            "shortcut",
                            Arg::Text(&shortcut.display(Platform::current())),
                        ),
                        ("command", Arg::Text(&command)),
                    ],
                ));
                app.capturing = None;
            }
            Err(AssignError::UnknownCommand) => app.capturing = None,
        }
        return;
    }
}
