//! The window layer: panels over [`AppState`].
//!
//! Panels read the state each frame and send commands back; nothing here decides behaviour. Every
//! control has a text label so the AccessKit tree is complete, and operations are shown
//! with a line style and an icon name as well as a colour.

use egui::{Align, Color32, CornerRadius, Key, Layout, Sense, Stroke};
use ps_compiler::effective_start_geom;
use ps_geometry::Point;
use ps_library::Settings;
use ps_model::{LineStyle, OpKind};
use ps_render::{colour_of, dash_pattern_px, fill_mesh, flatten_tolerance_mm, icon_name, style_of};

use crate::commands::Mods;
use crate::l10n::Localizer;
use crate::machine::MachineController;
use crate::numfmt::NumberFormat;
use crate::online::OnlineController;
use crate::state::{AppState, Effect, FocusPanel, Mode};
use crate::tools::{CanvasEvent, Overlay, ToolId};

const GRID_MM: f64 = 10.0;
const MIN_GRID_PX: f64 = 8.0;

/// The whole application: state plus the UI-only bits.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent open/closed and on/off flags of separate panels"
)]
pub struct App {
    pub(crate) state: AppState,
    pub(crate) loc: Localizer,
    pub(crate) palette_query: String,
    pub(crate) effect: Effect,
    pub(crate) palette_focused: bool,
    pub(crate) preview_step: usize,
    pub(crate) preview_playing: bool,
    /// Whether the finished job has not been shown yet since the app started.
    pub(crate) preview_unseen: bool,
    pub(crate) preview_material: bool,
    /// How the material preview is seen.
    pub(crate) preview_view: crate::PreviewView,
    pub(crate) preview_texture: Option<extras::PreviewTexture>,
    /// The preview picture being built on another thread, if one is.
    pub(crate) preview_worker: Option<extras::PreviewWorker>,
    /// The last preview built, kept while only the view changes.
    pub(crate) preview_cache: Option<extras::PreviewCache>,
    /// Whether to build the preview picture on the spot instead of on a thread.
    pub(crate) preview_blocking: bool,
    /// The picture key a build already failed for, so it is not tried again every frame.
    pub(crate) preview_failed: Option<(u64, usize, u32, u64)>,
    pub(crate) preview_speed: f64,
    pub(crate) preview_pos: f64,
    pub(crate) export_status: Option<String>,
    pub(crate) notice: Option<String>,
    pub(crate) machine: MachineController,
    /// Whether tool calls may move real hardware; only a person sets it.
    pub(crate) api_armed: bool,
    /// How the automation server is doing, for the settings window.
    pub(crate) mcp_status: crate::mcp::McpStatus,
    /// Whether the access token is shown in the settings window instead of dots.
    pub(crate) mcp_token_shown: bool,
    pub(crate) machine_open: bool,
    pub(crate) library: Option<crate::library_manager::LibraryManager>,
    pub(crate) library_open: bool,
    pub(crate) fonts: Option<crate::fonts::FontLibrary>,
    pub(crate) text_params: crate::text_object::TextParams,
    /// Where the text being typed will land on the mat, set when the card opens.
    pub(crate) text_anchor: Option<Point>,
    /// The outlines of the text being typed, for the preview, and the settings they are for.
    pub(crate) text_preview: Option<(crate::text_object::TextParams, TextOutlines)>,
    /// Whether the card's text field has been given the keyboard since it opened.
    pub(crate) text_focused: bool,
    /// The length the user measured of the 100 mm calibration bar, in millimetres.
    pub(crate) screen_measured_mm: f64,
    pub(crate) library_tab: library_window::LibraryTab,
    pub(crate) library_note: Option<library_window::LibraryNote>,
    /// The library entry whose details show.
    pub(crate) library_selected: Option<String>,
    /// What the library search box holds.
    pub(crate) library_query: String,
    pub(crate) focus_target: Option<FocusPanel>,
    pub(crate) gpu_format: Option<wgpu::TextureFormat>,
    pub(crate) online: OnlineController,
    pub(crate) update_checked: bool,
    /// The folder plugins are installed into, once the app knows it.
    pub(crate) plugins_dir: Option<std::path::PathBuf>,
    /// What the link box in the plugins settings holds.
    pub(crate) plugin_url: String,
    /// How the last plugin install went.
    pub(crate) plugin_status: Option<plugin_section::PluginStatus>,
    pub(crate) recovery: Option<ps_model::AutosaveEntry>,
    pub(crate) applied_theme: Option<ps_library::Theme>,
    /// Whether the mat has been fitted to the canvas once, now that the canvas has a size.
    pub(crate) canvas_fitted: bool,
    /// Whether the layers panel is open (it folds to a thin strip).
    pub(crate) layers_open: bool,
    /// Whether the Export screen shows the G-code listing.
    pub(crate) gcode_open: bool,
    /// The operations row whose settings are shown.
    pub(crate) selected_row: Option<ps_core::OpRowId>,
    /// Where the pointer is over the canvas, in millimetres.
    pub(crate) pointer_mm: Option<Point>,
    system_locale: String,
    loc_for: String,
    pub(crate) capturing: Option<&'static str>,
    pub(crate) shortcut_error: Option<String>,
    pub(crate) anchor: crate::arrange::Anchor,
    pub(crate) keep_proportions: bool,
    pub(crate) array_columns: u32,
    pub(crate) array_rows: u32,
    pub(crate) array_dx: f64,
    pub(crate) array_dy: f64,
    pub(crate) circle_count: u32,
    pub(crate) circle_degrees: f64,
    pub(crate) gcode_cache: Option<(u64, Vec<String>)>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl App {
    /// An application with these settings for a locale tag such as `en-US`.
    #[must_use]
    pub fn new(settings: Settings, locale: &str) -> Self {
        let network = settings.network;
        Self {
            state: AppState::new(settings),
            loc: Localizer::new(locale),
            palette_query: String::new(),
            effect: Effect::None,
            palette_focused: false,
            preview_step: 0,
            preview_playing: false,
            preview_unseen: true,
            preview_material: true,
            preview_view: crate::PreviewView::Flat,
            preview_texture: None,
            preview_worker: None,
            preview_cache: None,
            preview_blocking: false,
            preview_failed: None,
            preview_speed: 1.0,
            preview_pos: 0.0,
            export_status: None,
            notice: None,
            machine: MachineController::new(false),
            api_armed: false,
            mcp_status: crate::mcp::McpStatus::Off,
            mcp_token_shown: false,
            online: OnlineController::live(network),
            update_checked: false,
            plugins_dir: None,
            plugin_url: String::new(),
            plugin_status: None,
            machine_open: false,
            library: None,
            library_open: false,
            fonts: None,
            text_params: crate::text_object::TextParams::default(),
            text_anchor: None,
            text_preview: None,
            text_focused: false,
            screen_measured_mm: 100.0,
            library_tab: library_window::LibraryTab::Kind(ps_library::LibraryKind::Material),
            library_note: None,
            library_selected: None,
            library_query: String::new(),
            focus_target: None,
            gpu_format: None,
            recovery: None,
            applied_theme: None,
            canvas_fitted: false,
            layers_open: true,
            gcode_open: false,
            selected_row: None,
            pointer_mm: None,
            system_locale: locale.to_owned(),
            loc_for: String::new(),
            capturing: None,
            shortcut_error: None,
            anchor: crate::arrange::Anchor::TopLeft,
            keep_proportions: false,
            array_columns: 2,
            array_rows: 2,
            array_dx: 20.0,
            array_dy: 20.0,
            circle_count: 6,
            circle_degrees: 360.0,
            gcode_cache: None,
        }
    }

    /// An application whose machine list includes the built-in simulator.
    #[must_use]
    pub fn with_simulator(settings: Settings, locale: &str) -> Self {
        let mut app = Self::new(settings, locale);
        app.machine = MachineController::new(true);
        app.machine.refresh();
        app
    }

    /// Rebuilds the localiser when the language setting changed.
    fn refresh_language(&mut self) {
        let chosen = self.state.settings().locale.clone();
        if chosen == self.loc_for {
            return;
        }
        let tag = if chosen == "system" {
            self.system_locale.clone()
        } else {
            chosen.clone()
        };
        self.loc = Localizer::new(&tag);
        self.loc_for = chosen;
    }

    /// Shows a message in the status bar.
    pub fn set_notice(&mut self, text: String) {
        self.notice = Some(text);
    }

    /// Keeps device approvals, allowed internet hosts and certificate pins in `file`, so the
    /// user is asked once rather than every session.
    pub fn keep_device_approvals(&mut self, file: &std::path::Path) {
        if let Err(e) = self.machine.persist_permissions(file) {
            self.notice = Some(format!("Saved device approvals could not be read: {e}"));
        }
    }

    /// Installs the device plugins in `dir`, and tells the user about any that cannot be used.
    /// Plugins the user installs later from the settings are copied into this folder.
    pub fn install_plugins(&mut self, dir: &std::path::Path) {
        self.plugins_dir = Some(dir.to_path_buf());
        let load = self.machine.install_plugins(dir);
        if !load.problems.is_empty() {
            self.notice = Some(format!("Plugins not loaded: {}", load.problems.join("; ")));
        }
    }

    /// Draws the canvas on the GPU into targets of this format instead of with egui's painter.
    pub fn use_gpu_canvas(&mut self, format: wgpu::TextureFormat) {
        self.gpu_format = Some(format);
    }

    /// Offers the user an autosave left by a session that did not close properly.
    pub fn offer_recovery(&mut self, entry: ps_model::AutosaveEntry) {
        self.recovery = Some(entry);
    }

    /// The state, for the window shell.
    #[must_use]
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// The state, for the window shell.
    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    /// The effect left by the last frame (a dialog or file to handle), cleared on read.
    pub fn take_effect(&mut self) -> Effect {
        std::mem::replace(&mut self.effect, Effect::None)
    }

    fn format(&self) -> NumberFormat {
        let s = self.state.settings();
        NumberFormat::new(s.units, s.decimal_separator, &s.locale)
    }

    fn run(&mut self, id: &str, now_ms: u64) {
        let effect = self.state.run_command(id, now_ms);
        if effect != Effect::None {
            self.effect = effect;
        }
    }
}

/// The outlines of some text, flattened: each with its points and whether it is closed.
pub(crate) type TextOutlines = Vec<(Vec<Point>, bool)>;

/// One line of the operations legend: a kind with the cues that identify it besides colour.
#[derive(Debug, Clone, PartialEq)]
pub struct LegendEntry {
    /// The kind of operation.
    pub kind: OpKind,
    /// The line style it is drawn with.
    pub style: LineStyle,
    /// The icon beside it.
    pub icon: &'static str,
    /// Its colour.
    pub colour: [u8; 3],
}

/// The legend for every kind of operation.
#[must_use]
pub fn operation_legend() -> Vec<LegendEntry> {
    OpKind::ALL
        .iter()
        .map(|&kind| LegendEntry {
            kind,
            style: style_of(kind),
            icon: icon_name(kind),
            colour: colour_of(kind),
        })
        .collect()
}

/// The shortcut notation for an egui key, if it can be bound.
#[must_use]
pub fn key_name(key: Key) -> Option<String> {
    let name = match key {
        Key::ArrowLeft => "Left",
        Key::ArrowRight => "Right",
        Key::ArrowUp => "Up",
        Key::ArrowDown => "Down",
        Key::Delete => "Delete",
        Key::Backspace => "Backspace",
        Key::Escape => "Escape",
        Key::Enter => "Enter",
        Key::Tab => "Tab",
        Key::Space => "Space",
        Key::Equals => "Equals",
        Key::Minus => "Minus",
        Key::Home => "Home",
        Key::End => "End",
        Key::Comma => "Comma",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        other => {
            let n = other.name();
            let single_letter = n.len() == 1 && n.chars().all(|c| c.is_ascii_alphabetic());
            let function_key = n.starts_with('F') && n[1..].chars().all(|c| c.is_ascii_digit());
            return (single_letter || function_key).then(|| n.to_owned());
        }
    };
    Some(name.to_owned())
}

/// Draws one frame. `now_ms` is the frame time, for debouncing and undo merging.
pub fn show(ctx: &egui::Context, app: &mut App, now_ms: u64) {
    settings_window::apply_theme(ctx, app);
    app.refresh_language();
    app.machine.poll();
    let network = app.state.settings().network;
    app.online.set_network(network);
    app.online.poll();
    app.finish_download();
    app.finish_plugin_download();
    let ready = app.machine.is_ready();
    app.state.set_device_ready(ready);
    handle_keys(ctx, app, now_ms);
    app.focus_target = app.state.take_focus_request();
    if app.library_open {
        library_window::library_screen(ctx, app, now_ms);
    } else {
        match app.state.mode() {
            Mode::Prepare => prepare_screen(ctx, app, now_ms),
            Mode::Preview => screens::preview_screen(ctx, app, now_ms),
            Mode::Export => screens::export_screen(ctx, app, now_ms),
        }
    }
    screens::running_overlay(ctx, app);
    settings_window::settings_window(ctx, app);
    extras::recovery_window(ctx, app);
    extras::welcome_window(ctx, app);
    if app.state.palette_open() {
        palette(ctx, app, now_ms);
    } else {
        app.palette_focused = false;
    }
}

impl App {
    /// Opens the library screen.
    pub fn show_library(&mut self) {
        self.library_open = true;
    }

    /// Fills the project with a small sample card so every panel has something to show; used by
    /// `--demo` for trying the app out.
    pub fn load_demo(&mut self) {
        use crate::tools::CanvasEvent as E;
        let none = Mods::default();
        let draw = |state: &mut AppState, tool: &str, from: Point, to: Point| {
            state.run_command(tool, 0);
            state.canvas_event(E::Press(from), none, 0);
            state.canvas_event(E::Drag(to), none, 0);
            state.canvas_event(E::Release(to), none, 0);
        };
        let state = &mut self.state;
        state.set_machine("generic.cutter-medium");
        state.set_mat_material(Some("community.cardstock-80lb@1.0.0"), 0);
        draw(
            state,
            "tool.rectangle",
            Point::new(25.0, 25.0),
            Point::new(175.0, 125.0),
        );
        state.assign_operation(OpKind::Cut, "generic.drag-blade@1.0.0", 0);
        draw(
            state,
            "tool.line",
            Point::new(100.0, 25.0),
            Point::new(100.0, 125.0),
        );
        state.assign_operation(OpKind::Score, "generic.scoring-stylus@1.0.0", 0);
        draw(
            state,
            "tool.star",
            Point::new(60.0, 150.0),
            Point::new(90.0, 180.0),
        );
        state.assign_operation(OpKind::Draw, "pen.fineliner-black-0.4@1.0.0", 0);
        draw(
            state,
            "tool.ellipse",
            Point::new(120.0, 150.0),
            Point::new(160.0, 190.0),
        );
        state.assign_operation(OpKind::Cut, "generic.drag-blade@1.0.0", 0);
        state.run_command("tool.select", 0);
        state.update_settings(|s| s.first_run_seen = true);
        self.recovery = None;
    }

    /// The operations row whose settings show: the chosen one when it still exists, else the
    /// first.
    pub(crate) fn current_row(
        &self,
        rows: &[crate::state::OperationRow],
    ) -> Option<ps_core::OpRowId> {
        self.selected_row
            .filter(|id| rows.iter().any(|r| r.id == *id))
            .or_else(|| rows.first().map(|r| r.id))
    }
}

/// The Prepare screen of the design: header, control bar, tool strip, layers, canvas with its mat
/// tabs, operations and settings, status bar.
fn prepare_screen(ctx: &egui::Context, app: &mut App, now_ms: u64) {
    // A row sets this while the pointer is on it, so it must be cleared before the panels draw.
    app.state.set_highlight_row(None);
    chrome::prepare_header(ctx, app, now_ms);
    chrome::control_bar(ctx, app, now_ms);
    chrome::status_bar(ctx, app, now_ms);
    chrome::tool_strip(ctx, app, now_ms);
    side::layers_panel(ctx, app, now_ms);
    side::operations_panel(ctx, app, now_ms);
    let p = zest::palette(ctx);
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(p.canvas))
        .show(ctx, |ui| {
            mat_tabs(ui, app, p, now_ms);
            canvas(ui, app, now_ms);
        });
}

/// The strip of mat tabs above the canvas, with the machine and material pickers at its end.
#[expect(
    clippy::too_many_lines,
    reason = "straight-line drawing of one panel; splitting it would only scatter the layout"
)]
fn mat_tabs(ui: &mut egui::Ui, app: &mut App, p: crate::theme::Palette, now_ms: u64) {
    let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), 30.0));
    ui.allocate_rect(rect, Sense::hover());
    ui.painter().rect_filled(rect, 0.0, p.ground);
    zest::hairline(
        ui.painter(),
        egui::pos2(rect.left(), rect.bottom() - 0.5),
        egui::pos2(rect.right(), rect.bottom() - 0.5),
        p.line,
    );
    let current_material = app.state.current_material_reference();
    let material_name = app
        .state
        .material_choices()
        .into_iter()
        .find(|m| Some(&m.reference) == current_material.as_ref())
        .map(|m| m.name);
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(6.0, 0.0)))
            .layout(Layout::left_to_right(Align::Max)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            let summaries = app.state.mat_summaries();
            let removable = summaries.len() > 1;
            for summary in summaries {
                let material = material_name.clone().filter(|_| summary.current);
                let label = match &material {
                    Some(m) => format!("{} · {m}", summary.name),
                    None => summary.name.clone(),
                };
                let label_text = label.clone();
                let weight = if summary.current {
                    zest::Weight::Semibold
                } else {
                    zest::Weight::Medium
                };
                let colour = if summary.current { p.ink } else { p.muted };
                let galley =
                    ui.painter()
                        .layout_no_wrap(label, zest::font(ui.ctx(), 12.0, weight), colour);
                let width = galley.size().x + 20.0 + 16.0 + if removable { 20.0 } else { 0.0 };
                let (tab, response) =
                    ui.allocate_exact_size(egui::vec2(width, 26.0), Sense::click());
                if summary.current {
                    ui.painter().rect(
                        tab,
                        CornerRadius {
                            nw: 3,
                            ne: 3,
                            sw: 0,
                            se: 0,
                        },
                        p.canvas,
                        Stroke::new(1.0_f32, p.outline),
                        egui::StrokeKind::Inside,
                    );
                } else if response.hovered() {
                    ui.painter().rect_filled(tab, 3.0, p.hover);
                }
                let swatch = if material.is_some() {
                    Color32::from_rgb(0xd9, 0xb9, 0x89)
                } else {
                    p.outline
                };
                ui.painter().rect_filled(
                    egui::Rect::from_center_size(
                        egui::pos2(tab.left() + 15.0, tab.center().y),
                        egui::vec2(10.0, 10.0),
                    ),
                    2.0,
                    swatch,
                );
                ui.painter().galley(
                    egui::pos2(tab.left() + 26.0, tab.center().y - galley.size().y / 2.0),
                    galley,
                    colour,
                );
                let name = label_text.clone();
                let current = summary.current;
                response.widget_info(move || {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        true,
                        current,
                        name.clone(),
                    )
                });
                let mut closed = false;
                if removable && (summary.current || ui.rect_contains_pointer(tab)) {
                    let close = egui::Rect::from_center_size(
                        egui::pos2(tab.right() - 14.0, tab.center().y),
                        egui::vec2(18.0, 18.0),
                    );
                    let button = ui.interact(close, response.id.with("close"), Sense::click());
                    if button.hovered() {
                        ui.painter().rect_filled(close, 3.0, p.hover);
                    }
                    zest::icon(
                        ui.painter(),
                        close.center(),
                        zest::Icon::Close,
                        10.0,
                        if button.hovered() { p.danger } else { p.muted },
                    );
                    let said = format!("{}: {label_text}", app.loc.command_label("mat.remove"));
                    button.widget_info(move || {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, said.clone())
                    });
                    closed = button.clicked();
                }
                if closed {
                    app.state.set_current_mat(summary.id);
                    app.run("mat.remove", now_ms);
                } else if response.clicked() {
                    app.state.set_current_mat(summary.id);
                }
            }
            ui.add_space(4.0);
            let add = zest::icon_button(
                ui,
                p,
                zest::Icon::Plus,
                egui::vec2(24.0, 24.0),
                12.0,
                false,
                &app.loc.tr("mat-new"),
            );
            if app.focus_target == Some(FocusPanel::Tree) {
                add.request_focus();
            }
            egui::Popup::menu(&add).show(|ui| {
                ui.set_min_width(150.0);
                let roll = app.state.roll().is_some();
                for id in ["mat.add", "mat.add-roll", "mat.duplicate"] {
                    if id == "mat.add-roll" && !roll {
                        continue;
                    }
                    if ui.button(app.loc.command_label(id)).clicked() {
                        app.run(id, now_ms);
                        ui.close();
                    }
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.add_space(4.0);
                pickers(ui, app, now_ms);
            });
        },
    );
}

/// Whether the keyboard is typing into a text field. egui's own `wants_keyboard_input` is true
/// whenever any widget has focus, which would silence every shortcut after a Tab or F6.
fn typing_in_a_field(ctx: &egui::Context) -> bool {
    ctx.memory(egui::Memory::focused)
        .is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some())
}

fn handle_keys(ctx: &egui::Context, app: &mut App, now_ms: u64) {
    if typing_in_a_field(ctx) || app.capturing.is_some() {
        return;
    }
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
        let Some(name) = key_name(key) else { continue };
        let mods = Mods {
            command: modifiers.command,
            shift: modifiers.shift,
            alt: modifiers.alt,
        };
        if app.state.handle_key(mods, &name, now_ms) {
            let effect = app.state.take_effect();
            if effect != Effect::None {
                app.effect = effect;
            }
        } else if key == Key::Escape {
            app.state.canvas_event(CanvasEvent::Escape, mods, now_ms);
        } else if key == Key::Enter {
            app.state.canvas_event(CanvasEvent::Enter, mods, now_ms);
        }
    }
}

/// A 16 px line drawing of a tool, centred on `c`.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per tool, each a few line segments"
)]
pub(super) fn tool_glyph(painter: &egui::Painter, c: egui::Pos2, tool: ToolId, colour: Color32) {
    use egui::{Shape, pos2, vec2};
    let s = Stroke::new(1.5_f32, colour);
    let at = |x: f32, y: f32| pos2(c.x + x, c.y + y);
    let line = |points: Vec<egui::Pos2>| {
        painter.add(Shape::line(points, s));
    };
    let closed = |points: Vec<egui::Pos2>| {
        painter.add(Shape::closed_line(points, s));
    };
    let ring = |n: u8, outer: f32, inner: f32, turn: f32| -> Vec<egui::Pos2> {
        (0..n)
            .map(|i| {
                let a = std::f32::consts::TAU * f32::from(i) / f32::from(n) + turn;
                let r = if inner > 0.0 && i % 2 == 1 {
                    inner
                } else {
                    outer
                };
                at(r * a.cos(), r * a.sin())
            })
            .collect()
    };
    match tool {
        ToolId::Select => closed(vec![
            at(-4.0, -6.0),
            at(-4.0, 6.0),
            at(-1.0, 3.0),
            at(2.0, 7.0),
            at(4.0, 5.5),
            at(1.0, 1.5),
            at(5.0, 1.0),
        ]),
        ToolId::Node => {
            closed(vec![
                at(-6.0, -6.0),
                at(-6.0, 3.0),
                at(-3.5, 0.5),
                at(-0.5, 5.0),
                at(1.0, 4.0),
                at(-1.5, -0.5),
                at(2.0, -1.0),
            ]);
            painter.rect_stroke(
                egui::Rect::from_center_size(at(5.0, 5.0), vec2(5.0, 5.0)),
                1.0,
                s,
                egui::StrokeKind::Middle,
            );
        }
        ToolId::Rectangle => {
            painter.rect_stroke(
                egui::Rect::from_center_size(c, vec2(12.0, 9.0)),
                1.5,
                s,
                egui::StrokeKind::Middle,
            );
        }
        ToolId::Ellipse => {
            painter.add(Shape::ellipse_stroke(c, vec2(6.5, 4.5), s));
        }
        ToolId::Line => line(vec![at(-6.0, 6.0), at(6.0, -6.0)]),
        ToolId::Polygon => closed(ring(6, 7.0, 0.0, 0.0)),
        ToolId::Star => closed(ring(10, 7.5, 3.2, -std::f32::consts::FRAC_PI_2)),
        ToolId::Arc | ToolId::ArcCenter => {
            line(
                (0_u8..=12_u8)
                    .map(|i| {
                        let a = std::f32::consts::PI * (1.0 + f32::from(i) / 12.0);
                        at(7.0 * a.cos(), 3.0 + 7.0 * a.sin())
                    })
                    .collect(),
            );
            if tool == ToolId::ArcCenter {
                painter.circle_filled(at(0.0, 3.0), 1.5, colour);
            }
        }
        ToolId::Pen => {
            closed(vec![
                at(-6.0, 6.0),
                at(-5.0, 2.0),
                at(3.0, -6.0),
                at(6.0, -3.0),
                at(-2.0, 5.0),
            ]);
            line(vec![at(1.0, -4.0), at(4.0, -1.0)]);
        }
        ToolId::Knife => {
            closed(vec![
                at(-6.0, 6.0),
                at(-3.0, -5.0),
                at(6.0, -6.0),
                at(3.0, 5.0),
            ]);
            line(vec![at(-3.0, 1.0), at(4.0, 0.0)]);
        }
        ToolId::Guide => {
            line(vec![at(-7.0, 0.0), at(-3.0, 0.0)]);
            line(vec![at(-1.0, 0.0), at(3.0, 0.0)]);
            line(vec![at(5.0, 0.0), at(7.0, 0.0)]);
        }
        ToolId::StartPoint => {
            painter.circle_stroke(c, 6.0, s);
            painter.circle_filled(at(6.0, 0.0), 2.2, colour);
        }
    }
}

/// A toggle button that tells screen readers whether it is selected.
pub(super) fn selectable(
    ui: &mut egui::Ui,
    selected: bool,
    label: impl Into<String>,
) -> egui::Response {
    let label = label.into();
    let response = ui.selectable_label(selected, label.clone());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label.clone())
    });
    response
}

/// The options of the active tool: sides of a polygon, points and inner radius of a star.
pub(super) fn tool_options(ui: &mut egui::Ui, app: &mut App) {
    let tool = app.state.tool_id();
    if !matches!(tool, ToolId::Polygon | ToolId::Star) {
        return;
    }
    ui.separator();
    let label = app.loc.tr(if tool == ToolId::Polygon {
        "tool-sides"
    } else {
        "tool-points"
    });
    let name = ui.label(label);
    let mut sides = app.state.tool_sides();
    let drag = ui.add(egui::DragValue::new(&mut sides).range(3..=32));
    if drag.changed() {
        app.state.set_tool_sides(sides);
    }
    drag.labelled_by(name.id);
    if tool == ToolId::Star {
        let name = ui.label(app.loc.tr("tool-inner-radius"));
        let mut ratio = app.state.star_ratio();
        let drag = ui.add(
            egui::DragValue::new(&mut ratio)
                .range(0.05..=0.95)
                .speed(0.01),
        );
        if drag.changed() {
            app.state.set_star_ratio(ratio);
        }
        drag.labelled_by(name.id);
    }
}

/// The machine profile and material pickers in the top bar.
fn pickers(ui: &mut egui::Ui, app: &mut App, now_ms: u64) {
    let machines = app.state.machine_choices();
    let current_machine = app.state.current_machine_id().unwrap_or_default();
    let shown = machines
        .iter()
        .find(|m| m.id == current_machine)
        .map_or_else(String::new, |m| m.name.clone());
    let mut chosen_machine: Option<String> = None;
    let machine_label = app.loc.tr("machine-picker-label");
    let machine_combo = egui::ComboBox::from_id_salt("machine_profile")
        .selected_text(shown)
        .show_ui(ui, |ui| {
            for m in &machines {
                if ui
                    .selectable_label(m.id == current_machine, m.name.clone())
                    .clicked()
                {
                    chosen_machine = Some(m.id.clone());
                }
            }
        });
    machine_combo.response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, machine_label.clone())
    });
    if let Some(id) = chosen_machine {
        app.state.set_machine(&id);
    }

    let materials = app.state.material_choices();
    let current_material = app.state.current_material_reference();
    let none_label = app.loc.tr("mat-picker-none");
    let shown = materials
        .iter()
        .find(|m| Some(&m.reference) == current_material.as_ref())
        .map_or_else(|| none_label.clone(), |m| m.name.clone());
    let mut chosen: Option<Option<String>> = None;
    let material_label = app.loc.tr("mat-picker-material");
    let material_combo = egui::ComboBox::from_id_salt("mat_material")
        .selected_text(shown)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(current_material.is_none(), none_label)
                .clicked()
            {
                chosen = Some(None);
            }
            for m in &materials {
                if ui
                    .selectable_label(
                        Some(&m.reference) == current_material.as_ref(),
                        m.name.clone(),
                    )
                    .clicked()
                {
                    chosen = Some(Some(m.reference.clone()));
                }
            }
        });
    material_combo.response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, material_label.clone())
    });
    if let Some(reference) = chosen {
        app.state.set_mat_material(reference.as_deref(), now_ms);
    }
}

fn count_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

fn palette(ctx: &egui::Context, app: &mut App, now_ms: u64) {
    let title = app.loc.tr("palette-title");
    let mut chosen: Option<&'static str> = None;
    let mut close = false;
    egui::Window::new(title)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            let hint = app.loc.tr("palette-placeholder");
            let edit =
                ui.add(egui::TextEdit::singleline(&mut app.palette_query).hint_text(hint.clone()));
            edit.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, hint.clone())
            });
            if !app.palette_focused {
                edit.request_focus();
                app.palette_focused = true;
            }
            let loc = &app.loc;
            let hits = app.state.registry().search(
                &app.palette_query,
                &|id| loc.command_label(id),
                &app.state.context(),
            );
            if hits.is_empty() {
                ui.label(app.loc.tr("palette-empty"));
            }
            for hit in &hits {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(hit.enabled, egui::Button::new(hit.label.clone()))
                        .clicked()
                    {
                        chosen = Some(hit.id);
                    }
                    if let Some(s) = &hit.shortcut {
                        ui.weak(s.display(crate::commands::Platform::current()));
                    }
                });
            }
            if ui.input(|i| i.key_pressed(Key::Enter)) {
                chosen = hits.iter().find(|h| h.enabled).map(|h| h.id);
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                close = true;
            }
        });
    if let Some(id) = chosen {
        app.state.close_palette();
        app.palette_query.clear();
        app.run(id, now_ms);
    } else if close {
        app.state.close_palette();
        app.palette_query.clear();
    }
}

// ---- the canvas ----

#[expect(
    clippy::cast_possible_truncation,
    reason = "screen coordinates are f32 in egui; millimetre positions fit with room to spare"
)]
pub(super) fn f32_of(v: f64) -> f32 {
    v as f32
}

fn to_screen(state: &AppState, origin: egui::Pos2, p: Point) -> egui::Pos2 {
    let s = state.viewport().world_to_screen(p);
    egui::pos2(origin.x + f32_of(s.x), origin.y + f32_of(s.y))
}

fn world_of(state: &AppState, origin: egui::Pos2, pos: egui::Pos2) -> Point {
    state.screen_to_world(Point::new(
        f64::from(pos.x - origin.x),
        f64::from(pos.y - origin.y),
    ))
}

fn canvas(ui: &mut egui::Ui, app: &mut App, now_ms: u64) {
    let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let rect = response.rect;
    if app.focus_target == Some(FocusPanel::Canvas) {
        response.request_focus();
    }
    let canvas_name = app.loc.tr("panel-canvas");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, canvas_name.clone())
    });
    app.state
        .resize_canvas(f64::from(rect.width()), f64::from(rect.height()));
    if !app.canvas_fitted && rect.width() > 50.0 && rect.height() > 50.0 {
        app.canvas_fitted = true;
        app.run("view.zoom-fit", now_ms);
    }
    let pal = crate::theme::Palette::of(ui.ctx().theme());
    painter.rect_filled(
        rect,
        0.0,
        if app.state.mode() == Mode::Prepare {
            pal.canvas
        } else {
            pal.panel
        },
    );

    app.pointer_mm = response
        .hover_pos()
        .map(|pos| world_of(&app.state, rect.min, pos));
    canvas_input(ui, app, &response, now_ms);
    if app.state.mode() == Mode::Preview && app.preview_material {
        orbit(ui, app, &response);
    }
    // The system has no rotate cursor, so over the rotate zone it is hidden and drawn here.
    let mut rotate_at = None;
    if response.hovered() && app.state.mode() == Mode::Prepare {
        let cursor = app.state.tool_pointer();
        if cursor == crate::tools::Cursor::Rotate {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            rotate_at = response.hover_pos();
        } else {
            ui.ctx().set_cursor_icon(cursor_icon(cursor));
        }
    }
    if app.state.mode() == Mode::Preview {
        if app.preview_material && extras::draw_material(&painter, app, rect.min) {
            // The composited picture replaces the flat mat and the toolpath.
        } else {
            draw_mat(&painter, app, rect.min);
            extras::draw_toolpath(&painter, app, rect.min);
        }
    } else if let Some(format) = app.gpu_format {
        let dark = ui.visuals().dark_mode;
        let scene = crate::canvas_scene::build_scene_at(
            &app.state,
            dark,
            f64::from(ui.ctx().pixels_per_point()),
        );
        let scene = if format.is_srgb() {
            scene.for_srgb_target()
        } else {
            scene
        };
        painter.add(gpu_callback::scene_shape(rect, scene, format));
    } else {
        draw_mat(&painter, app, rect.min);
        draw_objects(&painter, app, rect.min);
    }
    // Handles and guides belong to editing; the preview is only for looking.
    if app.state.mode() == Mode::Prepare {
        draw_overlay(&painter, app, rect.min);
    }
    if let Some(at) = rotate_at {
        draw_rotate_cursor(&painter, at);
    }
    if app.state.mode() == Mode::Prepare {
        text_window::inline(ui.ctx(), app, rect, now_ms);
    }
    let gpu = app.gpu_format.is_some() && app.state.mode() != Mode::Preview;
    if app.state.mode() == Mode::Prepare {
        rulers::draw(&painter, app, rect, !gpu);
    } else if !gpu {
        rulers::draw_guides(&painter, app, rect);
    }
}

/// A circular arrow with a white edge, standing in for the pointer over the rotate zone.
fn draw_rotate_cursor(painter: &egui::Painter, at: egui::Pos2) {
    let radius = 8.0_f32;
    let arc: Vec<egui::Pos2> = (0..=18_u8)
        .map(|i| {
            let a = std::f32::consts::PI * (0.15 + 1.5 * f32::from(i) / 18.0);
            at + egui::vec2(radius * a.cos(), -radius * a.sin())
        })
        .collect();
    let end = arc[arc.len() - 1];
    let head = [
        end + egui::vec2(-1.0, -6.5),
        end + egui::vec2(-1.0, 4.5),
        end + egui::vec2(6.5, -1.0),
    ];
    for (width, colour) in [
        (4.5_f32, Color32::WHITE),
        (1.6_f32, Color32::from_rgb(0x1b, 0x1f, 0x3a)),
    ] {
        painter.add(egui::Shape::line(arc.clone(), Stroke::new(width, colour)));
    }
    painter.add(egui::Shape::convex_polygon(
        head.to_vec(),
        Color32::from_rgb(0x1b, 0x1f, 0x3a),
        Stroke::new(1.5_f32, Color32::WHITE),
    ));
}

/// Turns the tilted material view by dragging the picture: sideways spins it, up and down tips it.
fn orbit(ui: &egui::Ui, app: &mut App, response: &egui::Response) {
    let crate::PreviewView::Tilted {
        tilt_deg,
        azimuth_deg,
    } = &mut app.preview_view
    else {
        return;
    };
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(if response.dragged() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Grab
        });
    }
    if response.dragged_by(egui::PointerButton::Primary) {
        let d = response.drag_delta();
        *azimuth_deg = (*azimuth_deg - f64::from(d.x) * 0.5).rem_euclid(360.0);
        *tilt_deg = (*tilt_deg + f64::from(d.y) * 0.3).clamp(0.0, ps_render::MAX_TILT_DEG);
    }
}

/// The egui pointer for a tool's cursor.
fn cursor_icon(cursor: crate::tools::Cursor) -> egui::CursorIcon {
    use crate::tools::{Cursor, Handle};
    match cursor {
        Cursor::Arrow => egui::CursorIcon::Default,
        Cursor::Crosshair => egui::CursorIcon::Crosshair,
        Cursor::Move => egui::CursorIcon::Move,
        Cursor::Rotate => egui::CursorIcon::Alias,
        Cursor::Resize(Handle::North | Handle::South) => egui::CursorIcon::ResizeVertical,
        Cursor::Resize(Handle::East | Handle::West) => egui::CursorIcon::ResizeHorizontal,
        Cursor::Resize(Handle::NorthWest | Handle::SouthEast) => egui::CursorIcon::ResizeNwSe,
        Cursor::Resize(Handle::NorthEast | Handle::SouthWest) => egui::CursorIcon::ResizeNeSw,
    }
}

fn canvas_input(ui: &egui::Ui, app: &mut App, response: &egui::Response, now_ms: u64) {
    let origin = response.rect.min;
    let mods = ui.input(|i| Mods {
        command: i.modifiers.command,
        shift: i.modifiers.shift,
        alt: i.modifiers.alt,
    });
    if response.hovered() {
        let (scroll, hover) = ui.input(|i| (i.smooth_scroll_delta.y, i.pointer.hover_pos()));
        if let (true, Some(pos)) = (scroll.abs() > f32::EPSILON, hover) {
            let factor = (f64::from(scroll) * 0.002).exp();
            app.state.zoom_at(
                Point::new(f64::from(pos.x - origin.x), f64::from(pos.y - origin.y)),
                factor,
            );
        }
    }
    // While text is being typed, a click on the canvas says where it goes.
    if app.state.text_open()
        && response.clicked_by(egui::PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        app.text_anchor = Some(world_of(&app.state, origin, pos));
        return;
    }
    let panning = response.dragged_by(egui::PointerButton::Middle)
        || (response.dragged_by(egui::PointerButton::Primary)
            && ui.input(|i| i.key_down(Key::Space)));
    if panning {
        let d = response.drag_delta();
        app.state.pan_by_screen(f64::from(d.x), f64::from(d.y));
        return;
    }
    // The preview is for looking: tools only work while preparing.
    if app.state.mode() != Mode::Prepare {
        return;
    }
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(pos) = ui.input(|i| i.pointer.press_origin())
    {
        let w = world_of(&app.state, origin, pos);
        app.state
            .canvas_pointer_event(CanvasEvent::Press(w), mods, now_ms);
    } else if response.dragged_by(egui::PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        let w = world_of(&app.state, origin, pos);
        app.state
            .canvas_pointer_event(CanvasEvent::Drag(w), mods, now_ms);
    }
    if response.double_clicked_by(egui::PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        let w = world_of(&app.state, origin, pos);
        app.state
            .canvas_pointer_event(CanvasEvent::DoubleClick(w), mods, now_ms);
    }
    if !response.dragged()
        && let Some(pos) = response.hover_pos()
    {
        let w = world_of(&app.state, origin, pos);
        app.state
            .canvas_pointer_event(CanvasEvent::Hover(w), mods, now_ms);
    }
    if response.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        let w = world_of(&app.state, origin, pos);
        app.state
            .canvas_pointer_event(CanvasEvent::Release(w), mods, now_ms);
    } else if response.clicked_by(egui::PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        let w = world_of(&app.state, origin, pos);
        app.state
            .canvas_pointer_event(CanvasEvent::Press(w), mods, now_ms);
        app.state
            .canvas_pointer_event(CanvasEvent::Release(w), mods, now_ms);
    }
}

fn draw_mat(painter: &egui::Painter, app: &App, origin: egui::Pos2) {
    let state = &app.state;
    let mat = state.current_mat();
    let (w, h) = (mat.size.width.get(), mat.size.height.get());
    let mat_rect = egui::Rect::from_two_pos(
        to_screen(state, origin, Point::new(0.0, 0.0)),
        to_screen(state, origin, Point::new(w, h)),
    );
    painter.rect_filled(
        mat_rect.expand(12.0),
        6.0,
        Color32::from_rgb(0xDF, 0xE3, 0xEC),
    );
    painter.rect_filled(mat_rect, 0.0, Color32::WHITE);
    if let Some(used) = state.used_length_mm() {
        // Where the content on a roll ends: the material below it is still unused.
        let y = to_screen(state, origin, Point::new(0.0, used)).y;
        painter.line_segment(
            [
                egui::pos2(mat_rect.left(), y),
                egui::pos2(mat_rect.right(), y),
            ],
            Stroke::new(1.0_f32, Color32::from_rgb(0xC0, 0x50, 0x20)),
        );
    }
    let zoom = state.viewport().zoom();
    if GRID_MM * zoom >= MIN_GRID_PX {
        let grid = Stroke::new(1.0_f32, Color32::from_rgb(0xE8, 0xEA, 0xF1));
        let mut x = 0.0;
        while x <= w {
            painter.line_segment(
                [
                    to_screen(state, origin, Point::new(x, 0.0)),
                    to_screen(state, origin, Point::new(x, h)),
                ],
                grid,
            );
            x += GRID_MM;
        }
        let mut y = 0.0;
        while y <= h {
            painter.line_segment(
                [
                    to_screen(state, origin, Point::new(0.0, y)),
                    to_screen(state, origin, Point::new(w, y)),
                ],
                grid,
            );
            y += GRID_MM;
        }
    }
}

fn draw_objects(painter: &egui::Painter, app: &App, origin: egui::Pos2) {
    let state = &app.state;
    let zoom = state.viewport().zoom();
    let tol = flatten_tolerance_mm(zoom);
    let selected = state.selection().objects();
    let mat = state.current_mat();
    for object in mat.objects.values().filter(|o| !o.hidden) {
        let is_selected = selected.contains(&object.id);
        for path in object.paths.iter() {
            let kind = path.ops.first().map(|op| op.kind);
            let [r, g, b] = kind.map_or([60, 60, 60], colour_of);
            let colour = if is_selected {
                Color32::from_rgb(0x15, 0x65, 0xC0)
            } else {
                Color32::from_rgb(r, g, b)
            };
            let world = path.geom.transformed(object.transform);
            if path.geom.closed {
                let mesh = fill_mesh(std::slice::from_ref(&world), path.fill_rule, tol);
                let mut m = egui::Mesh::default();
                for v in &mesh.vertices {
                    let pos =
                        to_screen(state, origin, Point::new(f64::from(v[0]), f64::from(v[1])));
                    m.colored_vertex(pos, colour.gamma_multiply(0.15));
                }
                m.indices.clone_from(&mesh.indices);
                painter.add(egui::Shape::mesh(m));
            }
            let points: Vec<egui::Pos2> = ps_geometry::flatten(&world, tol)
                .into_iter()
                .map(|p| to_screen(state, origin, p))
                .collect();
            let stroke = Stroke::new(if is_selected { 2.0_f32 } else { 1.25_f32 }, colour);
            let style = kind.map_or(LineStyle::Solid, style_of);
            let dashes = dash_pattern_px(style);
            if let [on, off, ..] = dashes.as_slice() {
                painter.extend(egui::Shape::dashed_line(
                    &close_if(&points, path.geom.closed),
                    stroke,
                    f32_of(*on),
                    f32_of(*off),
                ));
            } else if path.geom.closed {
                painter.add(egui::Shape::closed_line(points, stroke));
            } else {
                painter.add(egui::Shape::line(points, stroke));
            }
        }
    }
}

fn close_if(points: &[egui::Pos2], closed: bool) -> Vec<egui::Pos2> {
    let mut v = points.to_vec();
    if closed && let Some(first) = points.first() {
        v.push(*first);
    }
    v
}

/// The dashed box around the selection with a square on each corner and side: drag inside to
/// move, a square to scale, just outside a corner to rotate. While a gesture runs the box follows it.
fn draw_selection_box(painter: &egui::Painter, app: &App, origin: egui::Pos2, accent: Color32) {
    let state = &app.state;
    if state.mode() != Mode::Prepare || state.tool_id() != ToolId::Select {
        return;
    }
    let Some(bounds) = state.live_bounds().or_else(|| state.selection_bounds()) else {
        return;
    };
    let zoom = state.viewport().zoom();
    let delta = state.live_transform().map(|(_, t)| t);
    let stroke = Stroke::new(1.2_f32, accent);
    if let Some(t) = delta
        && t.as_coeffs()[1].abs() + t.as_coeffs()[2].abs() > 1e-9
    {
        // Rotating: the outline of the box turns with the objects.
        let corners = [
            Point::new(bounds.x0, bounds.y0),
            Point::new(bounds.x1, bounds.y0),
            Point::new(bounds.x1, bounds.y1),
            Point::new(bounds.x0, bounds.y1),
        ]
        .map(|c| to_screen(state, origin, t * c));
        painter.add(egui::Shape::closed_line(corners.to_vec(), stroke));
        return;
    }
    let moved = delta.map_or(bounds, |t| t.transform_rect_bbox(bounds));
    let outer = crate::tools::selection_box(moved, zoom);
    let rect = egui::Rect::from_two_pos(
        to_screen(state, origin, Point::new(outer.x0, outer.y0)),
        to_screen(state, origin, Point::new(outer.x1, outer.y1)),
    );
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    for pair in corners.windows(2) {
        painter.extend(egui::Shape::dashed_line(
            &[pair[0], pair[1]],
            stroke,
            4.0,
            3.0,
        ));
    }
    for handle in crate::tools::visible_handles(moved, zoom) {
        let at = handle.point(outer);
        let centre = to_screen(state, origin, at);
        let square = egui::Rect::from_center_size(centre, egui::vec2(8.0, 8.0));
        painter.rect_filled(square, 1.5, Color32::WHITE);
        painter.rect_stroke(
            square,
            1.5,
            Stroke::new(1.5_f32, accent),
            egui::StrokeKind::Middle,
        );
    }
}

/// Whether a path's start is worth showing: something will be cut or drawn along it, or the user
/// fixed its start by hand. A shape with no operation has no start to speak of.
fn start_is_meaningful(path: &ps_model::ShapePath) -> bool {
    path.start_locked || path.ops.iter().any(|op| op.enabled)
}

/// Where each selected path starts and which way it runs: a green dot with an arrow along the
/// first stretch. A start the user fixed is ringed in orange.
fn draw_start_points(painter: &egui::Painter, app: &App, origin: egui::Pos2) {
    let state = &app.state;
    if state.mode() != Mode::Prepare {
        return;
    }
    let selected = state.selection().objects();
    if selected.is_empty() {
        return;
    }
    let live = state.live_transform();
    let mat = state.current_mat();
    for object in mat.objects.values().filter(|o| selected.contains(&o.id)) {
        let transform = match &live {
            Some((ids, delta)) if ids.contains(&object.id) => *delta * object.transform,
            _ => object.transform,
        };
        for path in object
            .paths
            .iter()
            .filter(|p| !p.geom.is_empty() && start_is_meaningful(p))
        {
            // Where the cut really begins: a path the user has not given a start follows the
            // compiler's own rule for the tool that draws it, so the marker never lies.
            let drive = path
                .ops
                .iter()
                .find(|op| op.enabled)
                .and_then(|op| state.libraries().resolve_tool(&op.tool.tool).ok())
                .map(|found| found.item.drive);
            let geom = match drive {
                Some(drive) => effective_start_geom(path, drive),
                None => path.geom.clone(),
            };
            let world = geom.transformed(transform);
            let start = to_screen(state, origin, world.start);
            let green = Color32::from_rgb(0x2B, 0xC4, 0x8A);
            if path.start_locked {
                painter.circle_stroke(
                    start,
                    7.0,
                    Stroke::new(2.0_f32, Color32::from_rgb(0xFF, 0x8A, 0x1F)),
                );
            }
            painter.circle_filled(start, 4.5, Color32::WHITE);
            painter.circle_filled(start, 3.5, green);
            if let Some((_, direction)) = world.sample_at(2.0 / state.viewport().zoom().max(1e-6)) {
                let d = egui::vec2(f32_of(direction.x), f32_of(direction.y));
                if d.length() > 1e-6 {
                    let d = d.normalized();
                    let tip = start + d * 14.0;
                    let side = egui::vec2(-d.y, d.x) * 3.5;
                    painter.add(egui::Shape::convex_polygon(
                        vec![tip, start + d * 8.0 + side, start + d * 8.0 - side],
                        green,
                        Stroke::NONE,
                    ));
                }
            }
        }
    }
}

fn draw_overlay(painter: &egui::Painter, app: &App, origin: egui::Pos2) {
    let state = &app.state;
    let accent = Color32::from_rgb(0xB2, 0x47, 0x00);
    draw_selection_box(painter, app, origin, accent);
    draw_start_points(painter, app, origin);
    for shape in state.overlay() {
        match shape {
            Overlay::Polyline { points, closed } => {
                let pts: Vec<egui::Pos2> = points
                    .iter()
                    .map(|p| to_screen(state, origin, *p))
                    .collect();
                if closed {
                    painter.add(egui::Shape::closed_line(pts, Stroke::new(1.5_f32, accent)));
                } else {
                    painter.add(egui::Shape::line(pts, Stroke::new(1.5_f32, accent)));
                }
            }
            Overlay::Marker { at } => {
                let centre = to_screen(state, origin, at);
                painter.circle_filled(centre, 6.0, accent.gamma_multiply(0.18));
                painter.circle_stroke(centre, 6.0, Stroke::new(2.0_f32, accent));
            }
            Overlay::Arms { arms } => {
                for (node, handle) in arms {
                    let (a, b) = (
                        to_screen(state, origin, node),
                        to_screen(state, origin, handle),
                    );
                    painter.line_segment([a, b], Stroke::new(1.0_f32, accent));
                    painter.circle_filled(b, 3.5, Color32::WHITE);
                    painter.circle_stroke(b, 3.5, Stroke::new(1.5_f32, accent));
                }
            }
            Overlay::Handles { points, selected } => {
                for (index, q) in points.iter().enumerate() {
                    let centre = to_screen(state, origin, *q);
                    let chosen = selected == Some(index);
                    let half = if chosen { 5.0_f32 } else { 4.0_f32 };
                    let square =
                        egui::Rect::from_center_size(centre, egui::vec2(half * 2.0, half * 2.0));
                    painter.rect_filled(square, 0.0, if chosen { accent } else { Color32::WHITE });
                    painter.rect_stroke(
                        square,
                        0.0,
                        Stroke::new(1.5_f32, accent),
                        egui::StrokeKind::Middle,
                    );
                }
            }
            Overlay::Marquee { rect, enclosed } => {
                let r = egui::Rect::from_two_pos(
                    to_screen(state, origin, Point::new(rect.x0, rect.y0)),
                    to_screen(state, origin, Point::new(rect.x1, rect.y1)),
                );
                painter.rect_filled(r, 0.0, accent.gamma_multiply(0.1));
                let stroke = Stroke::new(1.0_f32, accent);
                if enclosed {
                    painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
                } else {
                    painter.extend(egui::Shape::dashed_line(
                        &[
                            r.left_top(),
                            r.right_top(),
                            r.right_bottom(),
                            r.left_bottom(),
                            r.left_top(),
                        ],
                        stroke,
                        4.0,
                        3.0,
                    ));
                }
            }
        }
    }
}

mod arrange_panel;
mod chrome;
mod extras;
mod gpu_callback;
mod library_window;
mod machine_panel;
mod plugin_section;
mod rulers;
mod screens;
mod settings_window;
mod side;
mod text_window;
mod zest;

#[cfg(test)]
mod tests;
