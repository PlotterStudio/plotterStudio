//! The plugins part of the settings window: what is installed, and two ways to install more, from
//! a file on the disk or from a link. Both end the same way: the package is checked, copied into
//! the plugins folder and made available, so nobody has to find that folder.

use egui::Key;

use super::App;
use crate::l10n::Arg;
use crate::state::Effect;

/// What the last install attempt came to, for the line under the buttons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PluginStatus {
    /// The message, already in the user's language.
    pub(crate) text: String,
    /// True when it is an error and should be drawn as one.
    pub(crate) failed: bool,
}

impl App {
    /// Installs a plugin package from `bytes`: checks it, copies it into the plugins folder and
    /// makes it available, then says how it went in the settings window.
    pub fn install_plugin_bytes(&mut self, bytes: &[u8]) {
        let Some(dir) = self.plugins_dir.clone() else {
            self.fail_plugin_install("settings-plugin-no-folder");
            return;
        };
        self.plugin_status = Some(match self.machine.install_plugin(&dir, bytes) {
            Ok(done) => {
                tracing::info!(plugin = %done.id, restart = done.restart_needed, "plugin installed");
                let key = if done.restart_needed {
                    "settings-plugin-restart"
                } else {
                    "settings-plugin-installed"
                };
                PluginStatus {
                    text: self.loc.tr_args(key, &[("name", Arg::Text(&done.name))]),
                    failed: false,
                }
            }
            Err(reason) => self.plugin_failure(&reason),
        });
    }

    /// Starts downloading the plugin at the link in the settings window.
    pub(crate) fn start_plugin_download(&mut self) {
        if self.plugins_dir.is_none() {
            self.fail_plugin_install("settings-plugin-no-folder");
            return;
        }
        let url = self.plugin_url.trim().to_owned();
        self.plugin_status = None;
        if self.online.download_plugin(&url).is_err() {
            // Only one runs at a time; the status line already says it is downloading.
            tracing::debug!("a plugin download is already running");
        }
    }

    /// Installs a plugin a download finished, or reports why it failed.
    pub(super) fn finish_plugin_download(&mut self) {
        match self.online.take_plugin() {
            Some(Ok(bytes)) => self.install_plugin_bytes(&bytes),
            Some(Err(reason)) => self.plugin_status = Some(self.plugin_failure(&reason)),
            None => {}
        }
    }

    /// Shows in the settings window why a plugin could not be installed.
    pub fn report_plugin_failure(&mut self, reason: &str) {
        self.plugin_status = Some(self.plugin_failure(reason));
    }

    fn plugin_failure(&self, reason: &str) -> PluginStatus {
        PluginStatus {
            text: self
                .loc
                .tr_args("settings-plugin-failed", &[("reason", Arg::Text(reason))]),
            failed: true,
        }
    }

    fn fail_plugin_install(&mut self, key: &str) {
        self.plugin_status = Some(PluginStatus {
            text: self.loc.tr(key),
            failed: true,
        });
    }
}

/// The plugins section of the settings window.
pub(super) fn plugins(ui: &mut egui::Ui, app: &mut App) {
    ui.separator();
    ui.strong(app.loc.tr("settings-plugins"));
    ui.label(app.loc.tr("settings-plugins-note"));

    let installed = app.machine.installed_plugins();
    if installed.is_empty() {
        ui.weak(app.loc.tr("settings-plugins-none"));
    }
    for plugin in installed {
        ui.horizontal(|ui| {
            ui.label(&plugin.name);
            ui.weak(&plugin.id);
        });
    }

    if ui
        .button(app.loc.tr("settings-plugin-install-file"))
        .clicked()
    {
        app.effect = Effect::InstallPlugin;
    }

    let busy = app.online.plugin_downloading();
    ui.horizontal(|ui| {
        let label = ui.label(app.loc.tr("settings-plugin-url"));
        let edit = ui
            .add(
                egui::TextEdit::singleline(&mut app.plugin_url)
                    .hint_text("https://")
                    .desired_width(260.0),
            )
            .labelled_by(label.id);
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let ready = !busy && !app.plugin_url.trim().is_empty();
        let button = egui::Button::new(app.loc.tr("settings-plugin-install-url"));
        let clicked = ui.add_enabled(ready, button).clicked();
        if ready && (clicked || enter) {
            app.start_plugin_download();
        }
    });

    if busy {
        ui.label(app.loc.tr("settings-plugin-downloading"));
    } else if let Some(status) = &app.plugin_status {
        if status.failed {
            ui.colored_label(ui.visuals().error_fg_color, &status.text);
        } else {
            ui.label(&status.text);
        }
    }
}
