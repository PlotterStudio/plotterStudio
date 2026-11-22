//! The machine chip and panel: connection, state, progress, prompts, the job controls
//! and a jog pad, over the [`MachineController`](crate::machine::MachineController).

use ps_device::{DeviceEntry, DeviceId, JobOutcome, MachineState};

use super::{App, count_f64};
use crate::l10n::Arg;

/// The localisation key for a machine state.
fn state_key(state: &MachineState) -> &'static str {
    match state {
        MachineState::Idle => "state-idle",
        MachineState::Busy => "state-busy",
        MachineState::RunningJob => "state-running",
        MachineState::Paused => "state-paused",
        MachineState::WaitingForUser(_) => "state-waiting",
        MachineState::Fault(_) => "state-fault",
        MachineState::UpdatingFirmware => "state-updating",
        MachineState::Unknown => "state-unknown",
    }
}

/// The text of the chip in the top bar.
pub(super) fn chip_text(app: &App) -> String {
    match (app.machine.status(), app.machine.model()) {
        (Some(status), Some(model)) => {
            let state = app.loc.tr(state_key(&status.state));
            app.loc.tr_args(
                "machine-chip",
                &[("model", Arg::Text(&model)), ("state", Arg::Text(&state))],
            )
        }
        _ => app.loc.tr("machine-none"),
    }
}

/// The machine list as a dropdown from the machine chip, closing when you click elsewhere.
pub(super) fn machine_popup(chip: &egui::Response, app: &mut App, now_ms: u64) {
    let mut open = app.machine_open;
    egui::Popup::from_response(chip)
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::BOTTOM_END)
        .width(340.0)
        .show(|ui| {
            ui.set_width(320.0);
            let p = super::zest::palette(ui.ctx());
            ui.scope(|ui| {
                // The design's soft buttons and 8 px corners instead of egui's grey boxes.
                let v = ui.visuals_mut();
                for w in [
                    &mut v.widgets.inactive,
                    &mut v.widgets.hovered,
                    &mut v.widgets.active,
                ] {
                    w.corner_radius = egui::CornerRadius::same(8);
                    // An outline, so a checkbox's box can be seen before it is ticked.
                    w.bg_stroke = egui::Stroke::new(1.0_f32, p.outline);
                }
                v.widgets.inactive.weak_bg_fill = p.ground;
                v.widgets.hovered.weak_bg_fill = p.hover;
                v.widgets.active.weak_bg_fill = p.tint;
                ui.spacing_mut().button_padding = egui::vec2(12.0, 6.0);
                ui.spacing_mut().interact_size.y = 30.0;
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                contents(ui, app, now_ms);
            });
        });
    app.machine_open = open;
}

fn contents(ui: &mut egui::Ui, app: &mut App, now_ms: u64) {
    internet_section(ui, app);
    // The machine reports on its own thread; keep the panel fresh while it is open.
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(100));
    let connected = app.machine.status().is_some();
    if !connected {
        device_list(ui, app);
        return;
    }
    if let Some(status) = app.machine.status() {
        let state = app.loc.tr(state_key(&status.state));
        ui.label(
            app.loc
                .tr_args("machine-state", &[("state", Arg::Text(&state))]),
        );
        if let Some(p) = &status.progress {
            ui.label(app.loc.tr_args(
                "machine-progress",
                &[
                    (
                        "done",
                        Arg::Number(count_f64(usize::try_from(p.lines_done).unwrap_or(0))),
                    ),
                    (
                        "total",
                        Arg::Number(count_f64(usize::try_from(p.lines_total).unwrap_or(0))),
                    ),
                ],
            ));
        }
        if let Some(t) = status.temperature_c {
            ui.label(format!("{t:.0} °C"));
        }
        if let Some(b) = status.battery_percent {
            ui.label(format!("{b}%"));
        }
        for alert in &status.alerts {
            ui.label(&alert.message);
        }
    }
    if app.machine.is_lost() {
        ui.label(app.loc.tr("machine-lost"));
    }
    ui.separator();
    job_controls(ui, app, now_ms);
    if app.machine.can_jog() {
        ui.separator();
        jog_pad(ui, app);
    }
    if let Some(outcome) = app.machine.last_outcome() {
        let text = match outcome {
            JobOutcome::Completed { .. } => app.loc.tr("job-finished"),
            JobOutcome::Cancelled { .. } => app.loc.tr("job-cancelled"),
            JobOutcome::Faulted { error, .. } => app
                .loc
                .tr_args("job-faulted", &[("reason", Arg::Text(&error.message))]),
            JobOutcome::Lost { .. } => app.loc.tr("job-lost"),
        };
        ui.label(text);
    }
    ui.separator();
    if ui.button(app.loc.tr("machine-disconnect")).clicked() {
        app.machine.disconnect();
    }
}

/// Internet hosts plugins declare: each is blocked until the user allows it, and a host in use
/// is shown while it is used.
fn internet_section(ui: &mut egui::Ui, app: &mut App) {
    for (plugin, host) in app.machine.internet_in_use() {
        ui.label(app.loc.tr_args(
            "machine-host-in-use",
            &[("plugin", Arg::Text(&plugin)), ("host", Arg::Text(&host))],
        ));
    }
    for declared in app.machine.declared_hosts() {
        let key = if declared.allowed {
            "machine-host-block"
        } else {
            "machine-host-allow"
        };
        let label = app.loc.tr_args(key, &[("host", Arg::Text(&declared.host))]);
        if ui.button(label).clicked() {
            if declared.allowed {
                app.machine.revoke_host(&declared.plugin_id, &declared.host);
            } else {
                app.machine.allow_host(&declared.plugin_id, &declared.host);
            }
        }
    }
}

fn device_list(ui: &mut egui::Ui, app: &mut App) {
    let p = super::zest::palette(ui.ctx());
    let devices = app.machine.devices();
    ui.horizontal(|ui| {
        ui.label(super::zest::text(
            ui.ctx(),
            app.loc.tr("machine-title"),
            14.0,
            super::zest::Weight::Semibold,
            p.ink,
        ));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Looking is the user's act, and only the installed plugins know where to look.
            let can_look = app.machine.plugin_count() > 0;
            if ui
                .add_enabled(can_look, egui::Button::new(app.loc.tr("machine-refresh")))
                .clicked()
            {
                app.machine.refresh_in_background();
            }
            if app.machine.is_refreshing() {
                ui.spinner();
                // The scan finishes on its own thread; check back until it does.
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(150));
            }
        });
    });
    ui.add_space(6.0);
    if devices.is_empty() {
        let key = if app.machine.plugin_count() == 0 {
            "machine-no-plugins"
        } else {
            "machine-empty"
        };
        ui.label(super::zest::text(
            ui.ctx(),
            app.loc.tr(key),
            12.0,
            super::zest::Weight::Regular,
            p.muted,
        ));
    }
    for entry in devices {
        egui::Frame::NONE
            .fill(p.ground)
            .corner_radius(8.0)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(super::zest::text(
                            ui.ctx(),
                            entry.info.model.clone(),
                            13.0,
                            super::zest::Weight::Semibold,
                            p.ink,
                        ));
                        ui.label(super::zest::text(
                            ui.ctx(),
                            driver_line(&entry),
                            11.0,
                            super::zest::Weight::Regular,
                            p.muted,
                        ));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if entry.approved {
                            if accent_button(ui, p, &app.loc.tr("machine-connect")).clicked()
                                && let Err(e) = app.machine.connect(&entry.id)
                            {
                                app.notice = Some(e.to_string());
                            }
                        } else if ui.button(app.loc.tr("machine-allow")).clicked() {
                            app.machine.approve(&entry.id);
                            app.machine.refresh_in_background();
                        }
                    });
                });
                if !entry.approved {
                    ui.label(super::zest::text(
                        ui.ctx(),
                        entry.prompt.clone(),
                        11.0,
                        super::zest::Weight::Regular,
                        p.muted,
                    ));
                }
            });
        ui.add_space(4.0);
    }
    unclaimed_list(ui, app, p);
}

/// A button in the design's orange gradient, for the one thing to do next.
fn accent_button(ui: &mut egui::Ui, p: crate::theme::Palette, label: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        super::zest::font(ui.ctx(), 13.0, super::zest::Weight::Semibold),
        p.ink,
    );
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 28.0, 30.0),
        egui::Sense::click(),
    );
    super::zest::gradient_rect(ui.painter(), rect, 8.0);
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 8.0, egui::Color32::from_white_alpha(40));
    }
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        p.ink,
    );
    let said = label.to_owned();
    response.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, said.clone())
    });
    response
}

/// Devices a plugin found but could not confirm (a port that never answered, say), so a
/// plugged-in machine is never just missing.
fn unclaimed_list(ui: &mut egui::Ui, app: &App, p: crate::theme::Palette) {
    let found: Vec<String> = app
        .machine
        .unclaimed()
        .into_iter()
        .map(|d| d.name)
        .collect();
    if found.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.label(super::zest::text(
        ui.ctx(),
        app.loc.tr("machine-unclaimed"),
        11.0,
        super::zest::Weight::Semibold,
        p.muted,
    ));
    for name in found {
        ui.label(super::zest::text(
            ui.ctx(),
            name,
            11.0,
            super::zest::Weight::Regular,
            p.muted,
        ));
    }
}

fn job_controls(ui: &mut egui::Ui, app: &mut App, now_ms: u64) {
    let ready = app.machine.is_ready() && app.state.context().has_compile;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(ready, egui::Button::new(app.loc.tr("machine-send")))
            .clicked()
        {
            app.run("machine.send", now_ms);
        }
        if ui.button(app.loc.tr("machine-pause")).clicked() {
            app.machine.pause();
        }
        if ui.button(app.loc.tr("machine-resume")).clicked() {
            app.machine.resume();
        }
        if ui.button(app.loc.tr("machine-cancel")).clicked() {
            app.machine.cancel();
        }
    });
}

/// The distance one jog button moves, in millimetres.
const JOG_MM: f64 = 5.0;

fn jog_pad(ui: &mut egui::Ui, app: &mut App) {
    ui.horizontal(|ui| {
        for (key, dx, dy) in [
            ("machine-jog-left", -JOG_MM, 0.0),
            ("machine-jog-right", JOG_MM, 0.0),
            ("machine-jog-up", 0.0, -JOG_MM),
            ("machine-jog-down", 0.0, JOG_MM),
        ] {
            if ui.button(app.loc.tr(key)).clicked() {
                app.machine.jog(dx, dy);
            }
        }
    });
}

/// The port a machine was found on, when it was found on one (`serial:COM5` is `COM5`).
fn port_of(id: &DeviceId) -> Option<&str> {
    id.0.strip_prefix("serial:")
}

/// The line under a machine's name: its driver, and the port when there is one. The same machine
/// can be reached on two ports (USB and Bluetooth), and the port tells the two entries apart.
fn driver_line(entry: &DeviceEntry) -> String {
    match port_of(&entry.id) {
        Some(port) => format!("{} · {port}", entry.plugin_name),
        None => entry.plugin_name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_serial_machine_is_told_apart_by_its_port() {
        assert_eq!(port_of(&DeviceId("serial:COM5".into())), Some("COM5"));
        assert_eq!(port_of(&DeviceId("serial:COM6".into())), Some("COM6"));
        assert_eq!(port_of(&DeviceId("sim".into())), None);
        assert_eq!(port_of(&DeviceId("usb:20d3:0022".into())), None);
    }
}
