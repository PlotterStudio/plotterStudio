//! The mode-specific panels: the status bar, the assign-operation menu, the export
//! panel with diagnostics and the read-only G-code view, and the preview timeline.

use egui::{Color32, RichText, Stroke};
use ps_geometry::Point;
use ps_model::OpKind;
use ps_render::{SegmentKind, segments, tool_position};

use super::{App, count_f64, selectable, to_screen};
use crate::export::{LineClass, classify_gcode_line, gcode_for_mat};
use crate::l10n::Arg;
use crate::state::JobStatus;

/// The colour a tool slot is drawn in on the toolpath view: the ink of a pen that has one chosen,
/// otherwise the colour of the kind of work the tool does.
pub(super) fn slot_colour(tools: &[ps_job::ToolEntry], slot: Option<u8>) -> Color32 {
    let Some(entry) = slot.and_then(|s| tools.iter().find(|t| t.slot == s)) else {
        return Color32::from_gray(60);
    };
    if let Some(hex) = entry.pen.as_ref().and_then(|p| p.colour.as_deref())
        && let Some(c) = parse_hex_colour(hex)
    {
        return c;
    }
    let id = entry.tool.id();
    let kind = if id.starts_with("pen.") {
        // The pen's own ink is black unless the user chose another.
        return Color32::from_rgb(0x22, 0x22, 0x28);
    } else if id.contains("scor") {
        OpKind::Score
    } else if id.contains("foil") {
        OpKind::Foil
    } else {
        OpKind::Cut
    };
    let [r, g, b] = ps_render::colour_of(kind);
    Color32::from_rgb(r, g, b)
}

fn parse_hex_colour(hex: &str) -> Option<Color32> {
    let digits = hex.strip_prefix('#')?;
    let value = u32::from_str_radix(digits, 16)
        .ok()
        .filter(|_| digits.len() == 6)?;
    let [_, red, green, blue] = value.to_be_bytes();
    Some(Color32::from_rgb(red, green, blue))
}

/// The first-run notice: what is and is not sent over the internet.
pub(super) fn welcome_window(ctx: &egui::Context, app: &mut App) {
    if app.state.settings().first_run_seen || app.recovery.is_some() {
        return;
    }
    let mut dismissed = false;
    egui::Window::new(app.loc.tr("welcome-title"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.label(app.loc.tr("welcome-privacy"));
            ui.label(app.loc.tr("welcome-optional"));
            if ui.button(app.loc.tr("welcome-start")).clicked() {
                dismissed = true;
            }
        });
    if dismissed {
        app.state.update_settings(|s| s.first_run_seen = true);
    }
}

/// The prompt after a crash: restore the newest autosave or discard it.
pub(super) fn recovery_window(ctx: &egui::Context, app: &mut App) {
    let Some(entry) = app.recovery.clone() else {
        return;
    };
    let time = crate::clock::rfc3339_utc(entry.time_ms / 1_000);
    let mut choice: Option<bool> = None;
    egui::Window::new(app.loc.tr("recover-title"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(
                app.loc
                    .tr_args("recover-body", &[("time", Arg::Text(&time))]),
            );
            ui.horizontal(|ui| {
                if ui.button(app.loc.tr("recover-restore")).clicked() {
                    choice = Some(true);
                }
                if ui.button(app.loc.tr("recover-discard")).clicked() {
                    choice = Some(false);
                }
            });
        });
    match choice {
        Some(true) => {
            match crate::project_io::open_project(&entry.path) {
                Ok(opened) => {
                    app.state.adopt_recovered(opened);
                    app.notice = None;
                }
                Err(e) => {
                    app.notice = Some(app.loc.tr_args(
                        "notice-restore-failed",
                        &[("reason", Arg::Text(&e.to_string()))],
                    ));
                }
            }
            app.recovery = None;
        }
        Some(false) => app.recovery = None,
        None => {}
    }
}

/// The words for where the job stands. The status bar, the preview and the Export tab all use
/// them, so they never disagree about why there is no job.
pub(super) fn job_status_text(app: &App) -> String {
    match app.state.job_status() {
        JobStatus::NoMachine => app.loc.tr("status-no-machine"),
        JobStatus::Compiling => app.loc.tr("status-compiling"),
        JobStatus::Failed(why) => app
            .loc
            .tr_args("status-failed", &[("why", Arg::Text(&why))]),
        JobStatus::Blocked { errors } => app.loc.tr_args(
            "status-blocked",
            &[("count", Arg::Number(count_f64(errors)))],
        ),
        JobStatus::NothingToDo => app.loc.tr("status-nothing-to-do"),
        JobStatus::OutOfDate => app.loc.tr("status-stale"),
        JobStatus::Ready => app.loc.tr("status-ready"),
    }
}

/// The compile state, bottom of the window.
pub(super) fn status_bar(ui: &mut egui::Ui, app: &App) {
    let ready = app.state.job_status() == JobStatus::Ready;
    let palette = crate::theme::Palette::of(ui.ctx().theme());
    ui.horizontal(|ui| {
        let dot = if ready { palette.ok } else { palette.orange };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 3.5, dot);
        ui.label(job_status_text(app));
        if let Some((_, kind)) = app.state.last_snap() {
            ui.separator();
            let what = app.loc.tr(&format!("snap-{}", snap_word(kind)));
            ui.label(
                app.loc
                    .tr_args("status-snap", &[("what", Arg::Text(&what))]),
            );
        }
        if let Some(notice) = &app.notice {
            ui.separator();
            ui.label(notice);
        }
        if let Some(release) = app.online.update() {
            ui.separator();
            let version = release.version.to_string();
            ui.hyperlink_to(
                app.loc
                    .tr_args("update-available", &[("version", Arg::Text(&version))]),
                &release.url,
            );
        }
    });
}

/// The "Assign operation" menu for the selection.
pub(super) fn assign_menu(ui: &mut egui::Ui, app: &mut App, now_ms: u64) {
    let label = app.loc.tr("inspector-assign");
    let mut chosen: Option<(OpKind, String)> = None;
    ui.menu_button(label, |ui| {
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
    });
    if let Some((kind, tool)) = chosen {
        app.assign_operation(kind, &tool, now_ms);
    }
}

/// The G-code heading and the read-only listing.
pub(super) fn export_gcode(ui: &mut egui::Ui, app: &mut App) {
    let p = super::zest::palette(ui.ctx());
    ui.label(super::zest::text(
        ui.ctx(),
        app.loc.tr("export-gcode"),
        14.0,
        super::zest::Weight::Semibold,
        p.ink,
    ));
    ui.add_space(6.0);
    gcode_view(ui, app);
}

fn gcode_view(ui: &mut egui::Ui, app: &mut App) {
    let generation = app.state.compiled().map(|c| c.doc_generation);
    if app.gcode_cache.as_ref().map(|(g, _)| *g) != generation {
        app.gcode_cache = generation
            .zip(gcode_for_mat(&app.state, 0))
            .map(|(g, text)| (g, text.lines().map(str::to_owned).collect()));
    }
    let Some((_, lines)) = &app.gcode_cache else {
        let p = super::zest::palette(ui.ctx());
        let words = match app.state.job_status() {
            JobStatus::NothingToDo | JobStatus::Ready => app.loc.tr("export-gcode-none"),
            _ => job_status_text(app),
        };
        ui.label(super::zest::text(
            ui.ctx(),
            words,
            13.0,
            super::zest::Weight::Regular,
            p.muted,
        ));
        return;
    };
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::vertical()
        .id_salt("gcode_listing")
        .max_height(360.0)
        .auto_shrink([false, false])
        .show_rows(ui, row_height, lines.len(), |ui, range| {
            for line in &lines[range] {
                let colour = match classify_gcode_line(line) {
                    LineClass::Comment => Color32::GRAY,
                    LineClass::Travel => Color32::from_rgb(0x90, 0x90, 0x90),
                    LineClass::Draw => Color32::from_rgb(0x15, 0x65, 0xC0),
                    LineClass::Tool => Color32::from_rgb(0x2E, 0x7D, 0x32),
                    LineClass::Pause => Color32::from_rgb(0xC6, 0x28, 0x28),
                    LineClass::Other => ui.visuals().text_color(),
                };
                ui.label(RichText::new(line).monospace().color(colour));
            }
        });
}

/// Draws the toolpath reached by the timeline in place of the design.
pub(super) fn draw_toolpath(painter: &egui::Painter, app: &App, origin: egui::Pos2) {
    let Some(mat_job) = app.state.compiled().and_then(|c| c.job.mats.first()) else {
        return;
    };
    for segment in segments(&mat_job.steps, app.preview_step) {
        let a = to_screen(&app.state, origin, segment.from);
        let b = to_screen(&app.state, origin, segment.to);
        match segment.kind {
            SegmentKind::Travel => {
                painter.extend(egui::Shape::dashed_line(
                    &[a, b],
                    Stroke::new(1.0_f32, Color32::from_gray(160)),
                    4.0,
                    3.0,
                ));
            }
            SegmentKind::Draw { slot } => {
                painter.line_segment(
                    [a, b],
                    Stroke::new(1.75_f32, slot_colour(&mat_job.tool_table, slot)),
                );
            }
        }
    }
    if let Some(at) = tool_position(&mat_job.steps, app.preview_step) {
        painter.circle_filled(
            to_screen(&app.state, origin, at),
            4.0,
            Color32::from_rgb(0xC6, 0x28, 0x28),
        );
    }
}

/// The word used in a snap message key for a kind of snap.
fn snap_word(kind: ps_render::SnapKind) -> &'static str {
    use ps_render::SnapKind;
    match kind {
        SnapKind::Node => "node",
        SnapKind::Intersection => "intersection",
        SnapKind::Anchor => "anchor",
        SnapKind::Midpoint => "midpoint",
        SnapKind::Guide => "guide",
        SnapKind::Edge => "edge",
        SnapKind::Grid => "grid",
    }
}

/// The ordering mode and pen check for the whole job.
pub(super) fn job_options(ui: &mut egui::Ui, app: &mut App, now_ms: u64) {
    use ps_model::OrderingMode;
    ui.horizontal(|ui| {
        for (mode, key) in [
            (OrderingMode::MinimiseToolChanges, "export-order-minimise"),
            (OrderingMode::StrictOrder, "export-order-strict"),
        ] {
            let label = app.loc.tr(key);
            if selectable(ui, app.state.ordering() == mode, label).clicked() {
                app.state.set_ordering(mode, now_ms);
            }
        }
    });
    let mut check = app.state.pen_check();
    let response = ui.checkbox(&mut check, app.loc.tr("export-pen-check"));
    if response.changed() {
        app.state.set_pen_check(check, now_ms);
    }
    ui.separator();
}

/// The material view's choices: flat, tilted and orbiting, or weeded.
pub(super) fn view_controls(ui: &mut egui::Ui, app: &mut App) {
    use crate::PreviewView;
    let current = app.preview_view;
    let (is_flat, is_tilt, is_weeded) = (
        current == PreviewView::Flat,
        matches!(current, PreviewView::Tilted { .. }),
        matches!(current, PreviewView::Weeded { .. }),
    );
    if selectable(ui, is_flat, app.loc.tr("preview-view-flat")).clicked() {
        app.preview_view = PreviewView::Flat;
    }
    if selectable(ui, is_tilt, app.loc.tr("preview-view-tilt")).clicked() && !is_tilt {
        app.preview_view = PreviewView::Tilted {
            tilt_deg: 55.0,
            azimuth_deg: 0.0,
        };
    }
    if selectable(ui, is_weeded, app.loc.tr("preview-view-weeded")).clicked() && !is_weeded {
        app.preview_view = PreviewView::Weeded { lift_mm: 6.0 };
    }
    match &mut app.preview_view {
        PreviewView::Flat => {}
        PreviewView::Tilted { .. } => {
            ui.label(app.loc.tr("preview-orbit-hint"));
        }
        PreviewView::Weeded { lift_mm } => {
            let label = ui.label(app.loc.tr("preview-lift"));
            ui.add(egui::DragValue::new(lift_mm).range(0.0..=40.0).speed(0.2))
                .labelled_by(label.id);
        }
    }
}

/// The composited material picture and what it was built from.
pub(crate) struct PreviewTexture {
    /// What the picture shows: job generation, step and scale.
    pub(crate) key: (u64, usize, u32, u64),
    texture: egui::TextureHandle,
    /// The picture's size in pixels, and its pixels per millimetre.
    size_px: [usize; 2],
    scale: f64,
}

impl std::fmt::Debug for PreviewTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewTexture")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

/// The largest side of the preview picture, in pixels.
const MAX_PREVIEW_SIDE: f64 = 4096.0;

/// A picture being built on another thread, and what it is for.
pub(crate) struct PreviewWorker {
    key: (u64, usize, u32, u64),
    scale: f64,
    done: std::sync::mpsc::Receiver<Option<Built>>,
}

/// What a worker hands back: the preview it built (kept, so turning the view only re-renders) and
/// the picture of it.
type Built = (std::sync::Arc<crate::state::Preview>, ps_render::Image);

/// The expensive part of a picture, kept while only the view changes.
pub(crate) struct PreviewCache {
    /// The job generation, step and scale it was built for.
    key: (u64, usize, u32),
    preview: std::sync::Arc<crate::state::Preview>,
}

impl std::fmt::Debug for PreviewCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewCache")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for PreviewWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewWorker")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

/// Draws the material view over the mat; false when there is no job to show.
///
/// The picture is built on a worker thread, so the window stays responsive while it renders: the
/// last finished picture stays on screen until the next one is ready, and only one is built at a
/// time (the newest request wins when it finishes).
pub(super) fn draw_material(painter: &egui::Painter, app: &mut App, origin: egui::Pos2) -> bool {
    let Some(generation) = app.state.compiled().map(|c| c.doc_generation) else {
        return false;
    };
    // A job blocked by errors has nothing to draw; say nothing here and let the preview bar and
    // status bar give the reason, rather than "Rendering…" for ever.
    if !app.state.can_preview() {
        return false;
    }
    let mat = app.state.current_mat();
    let (w, h) = (mat.size.width.get(), mat.size.height.get());
    let wanted = ps_render::scale_for_zoom(app.state.viewport().zoom());
    // The same quality all the time, so playing does not trade the picture for speed and then
    // snap back when it stops.
    let scale = ps_render::adaptive_scale(wanted, ps_render::Quality::High)
        .min(MAX_PREVIEW_SIDE / w.max(h).max(1.0));
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a small positive scale, rounded to hundredths"
    )]
    let key = (
        generation,
        app.preview_step,
        (scale * 100.0).round() as u32,
        view_hash(&app.preview_view),
    );
    take_finished(painter.ctx(), app);
    if app.preview_failed == Some(key) {
        return false;
    }
    let out_of_date = app.preview_texture.as_ref().is_none_or(|t| t.key != key);
    if out_of_date && app.preview_worker.is_none() {
        start_picture(painter.ctx(), app, key, scale);
    }
    if app.preview_worker.is_some() {
        painter
            .ctx()
            .request_repaint_after(std::time::Duration::from_millis(30));
    }
    let Some(t) = &app.preview_texture else {
        // Nothing to show yet: say so rather than leave the mat blank.
        let at = to_screen(&app.state, origin, Point::new(w / 2.0, h / 2.0));
        let pal = super::zest::palette(painter.ctx());
        painter.text(
            at,
            egui::Align2::CENTER_CENTER,
            app.loc.tr("preview-rendering"),
            super::zest::font(painter.ctx(), 13.0, super::zest::Weight::Medium),
            pal.muted,
        );
        return true;
    };
    // The picture is centred on the mat; a tilted or turned view has its own proportions, drawn
    // at the same scale as the flat one.
    let centre = to_screen(&app.state, origin, Point::new(w / 2.0, h / 2.0));
    let zoom = app.state.viewport().zoom();
    #[expect(clippy::cast_precision_loss, reason = "picture sizes are small")]
    let (half_w, half_h) = (
        t.size_px[0] as f64 / t.scale * zoom / 2.0,
        t.size_px[1] as f64 / t.scale * zoom / 2.0,
    );
    #[expect(
        clippy::cast_possible_truncation,
        reason = "screen coordinates are f32"
    )]
    let rect = egui::Rect::from_center_size(
        centre,
        egui::vec2((half_w * 2.0) as f32, (half_h * 2.0) as f32),
    );
    painter.image(
        t.texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    true
}

/// Starts building the picture for `key`: on a thread, or at once when the app asks for that (the
/// tests do, so a picture is there when they look).
fn start_picture(ctx: &egui::Context, app: &mut App, key: (u64, usize, u32, u64), scale: f64) {
    let Some(inputs) = app.state.preview_inputs() else {
        return;
    };
    let (step, view) = (app.preview_step, app.preview_view);
    let base = (key.0, key.1, key.2);
    let cached = app
        .preview_cache
        .as_ref()
        .filter(|c| c.key == base)
        .map(|c| std::sync::Arc::clone(&c.preview));
    let build = move || -> Option<Built> {
        let preview = match cached {
            Some(preview) => preview,
            None => std::sync::Arc::new(inputs.build(scale, step)?),
        };
        let picture = preview.render(&view);
        Some((preview, picture))
    };
    if app.preview_blocking {
        if let Some((preview, picture)) = build() {
            app.preview_cache = Some(PreviewCache { key: base, preview });
            install_picture(ctx, app, key, scale, &picture);
        } else {
            app.preview_failed = Some(key);
        }
        return;
    }
    let (send, done) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // The window may have closed or moved on; nobody is listening then, which is fine.
        let _ = send.send(build());
    });
    app.preview_worker = Some(PreviewWorker { key, scale, done });
}

/// Takes the picture a worker has finished, if one has.
fn take_finished(ctx: &egui::Context, app: &mut App) {
    let Some(worker) = &app.preview_worker else {
        return;
    };
    match worker.done.try_recv() {
        Ok(result) => {
            let (key, scale) = (worker.key, worker.scale);
            app.preview_worker = None;
            if let Some((preview, picture)) = result {
                app.preview_cache = Some(PreviewCache {
                    key: (key.0, key.1, key.2),
                    preview,
                });
                install_picture(ctx, app, key, scale, &picture);
            } else {
                app.preview_failed = Some(key);
            }
            ctx.request_repaint();
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
        Err(std::sync::mpsc::TryRecvError::Disconnected) => app.preview_worker = None,
    }
}

fn install_picture(
    ctx: &egui::Context,
    app: &mut App,
    key: (u64, usize, u32, u64),
    scale: f64,
    picture: &ps_render::Image,
) {
    let size = [picture.width as usize, picture.height as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, &picture.rgba);
    let texture = ctx.load_texture("material_preview", image, egui::TextureOptions::LINEAR);
    app.preview_texture = Some(PreviewTexture {
        key,
        texture,
        size_px: size,
        scale,
    });
}

/// A number that changes whenever the view's settings do.
fn view_hash(view: &crate::PreviewView) -> u64 {
    use crate::PreviewView;
    let bits = |v: f64| v.to_bits();
    match *view {
        PreviewView::Flat => 0,
        PreviewView::Tilted {
            tilt_deg,
            azimuth_deg,
        } => 1 ^ bits(tilt_deg).rotate_left(7) ^ bits(azimuth_deg).rotate_left(23),
        PreviewView::Weeded { lift_mm } => 2 ^ bits(lift_mm).rotate_left(11),
    }
}
