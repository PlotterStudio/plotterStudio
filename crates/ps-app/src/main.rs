//! The Plotter Studio window: an eframe shell around [`ps_app::App`].
//!
//! All behaviour lives in the library; this file only opens the window, answers dialogs with the
//! native file choosers, runs autosave and crash recovery, and hands the library the frame time
//! and the display density.

// A release build on Windows is a window program: without this it is a console program, and
// double-clicking it (or its shortcut) opens a command-line window that closes the app with it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use ps_app::{
    App, Dialogs, Mode, SessionGuard, Snapshot, Startup, handle_effect, mcp, show,
    write_panic_files,
};
use ps_library::{Settings, default_settings_path, default_state_dir};
use ps_model::ProcessProbe;

/// The native file and folder choosers.
struct NativeDialogs;

impl Dialogs for NativeDialogs {
    fn open_project(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("Plotter Studio project", &["pstudio"])
            .pick_file()
    }

    fn import_drawing(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("Drawings", &["svg", "svgz", "dxf", "pdf", "ai"])
            .pick_file()
    }

    fn save_project(&mut self, suggested: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("Plotter Studio project", &["pstudio"])
            .set_file_name(suggested)
            .save_file()
    }

    fn pick_folder(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new().pick_folder()
    }

    fn pick_package(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("Material package", &["psmaterials", "zip"])
            .pick_file()
    }
}

/// Asks the operating system whether a process is running.
struct SystemProbe;

impl ProcessProbe for SystemProbe {
    fn is_alive(&self, pid: u32) -> bool {
        let pid = sysinfo::Pid::from_u32(pid);
        let mut system = sysinfo::System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        system.process(pid).is_some()
    }
}

struct Shell {
    app: App,
    /// Requests from an MCP client on stdin (`--mcp`), served between frames.
    mcp: Option<Receiver<mcp::Request>>,
    started: Instant,
    settings_path: Option<PathBuf>,
    last_title: String,
    guard: Option<SessionGuard>,
    snapshot: Snapshot,
    last_mode: Mode,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

impl eframe::App for Shell {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now_ms = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.app
            .state_mut()
            .set_display_dpi(96.0 * f64::from(ctx.pixels_per_point()));
        if let Some(requests) = &self.mcp {
            while let Ok(request) = requests.try_recv() {
                request.serve(&mut self.app, unix_now_ms());
            }
        }
        show(ctx, &mut self.app, now_ms);
        let effect = self.app.take_effect();
        handle_effect(&mut self.app, effect, &mut NativeDialogs, unix_now());
        self.app.state_mut().tick(now_ms);

        // Autosave: every interval when changed, and at once on a mode switch.
        let mode = self.app.state().mode();
        let switched = mode != self.last_mode;
        self.last_mode = mode;
        let interval = self.app.state().settings().autosave_interval_s;
        if let Some(guard) = &mut self.guard
            && let Err(e) = guard.tick(self.app.state(), interval, unix_now_ms(), switched)
        {
            self.app.set_notice(e.to_string());
        }
        if self.app.state_mut().take_settings_changed()
            && let Some(path) = &self.settings_path
        {
            let _ = self.app.state().settings().save(path);
        }
        self.snapshot.update(self.app.state());

        let title = self.app.state().window_title();
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
        // Wake up in time for the debounced compile and for results arriving from the pool.
        // Quickly while a job is being built, so the result shows the moment it is ready.
        let wait = if self.app.state().compile_running() {
            30
        } else {
            100
        };
        ctx.request_repaint_after(std::time::Duration::from_millis(wait));
    }

    fn on_exit(&mut self) {
        if let Some(path) = &self.settings_path {
            // Recent files are the only setting the app changes by itself; a failed write is
            // not worth interrupting the exit for.
            let _ = self.app.state().settings().save(path);
        }
        if let Some(guard) = &self.guard {
            guard.finish();
        }
    }
}

/// Saves the unsaved work and writes a local crash report when the app panics.
fn install_panic_hook(state_dir: PathBuf, snapshot: Snapshot) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        tracing::error!(panic = %info, "the application panicked");
        let _ = write_panic_files(
            &state_dir,
            &snapshot,
            &info.to_string(),
            &backtrace,
            &[],
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            unix_now_ms(),
        );
        previous(info);
    }));
}

/// The icon in the window's title bar and the taskbar; without one eframe shows its own.
fn window_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(include_bytes!("../icons/icon-256.png")).ok()
}

fn main() -> eframe::Result {
    // Kept until the program ends, so the last lines reach the file.
    let _log = ps_library::default_log_dir().and_then(|dir| ps_app::logging::init(&dir));
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        "Plotter Studio starting"
    );
    let settings_path = default_settings_path();
    let settings = settings_path
        .as_deref()
        .map_or_else(Settings::default, |p| Settings::load_or_default(p).0);
    let locale = if settings.locale == "system" {
        sys_locale::get_locale().unwrap_or_else(|| "en".to_owned())
    } else {
        settings.locale.clone()
    };
    // `--simulate` makes the built-in simulated machine available to connect to.
    let simulate = std::env::args().any(|a| a == "--simulate");
    let mut app = if simulate {
        App::with_simulator(settings, &locale)
    } else {
        App::new(settings, &locale)
    };
    if let Some(root) = ps_library::default_root()
        && let Err(e) = app.open_library(&root)
    {
        tracing::error!(error = %e, "the library could not be opened");
        app.set_notice(format!("The library could not be opened: {e}"));
    }
    if let Some(root) = ps_library::default_root() {
        app.keep_device_approvals(&root.join("permissions.json"));
    }
    if let Some(dir) = ps_library::default_plugins_dir() {
        app.install_plugins(&dir);
    }

    let snapshot = Snapshot::default();
    let mut guard = None;
    if let Some(state_dir) = default_state_dir() {
        let (g, startup) =
            SessionGuard::start(&state_dir, &SystemProbe, std::process::id(), unix_now_ms());
        match startup {
            Startup::Crashed {
                autosave: Some(entry),
            } => app.offer_recovery(entry),
            Startup::AnotherInstance => app.set_notice(
                "Another copy of Plotter Studio is already using this data folder".to_owned(),
            ),
            Startup::Fresh | Startup::Crashed { autosave: None } => {}
        }
        install_panic_hook(state_dir, snapshot.clone());
        guard = Some(g);
    }

    // `--mcp` serves the automation API to an MCP client over stdin/stdout while the window is
    // open (stdout then carries protocol messages only). Moving real hardware additionally needs
    // `--arm-api`, which only a person can pass; no tool can set it.
    let mcp = std::env::args()
        .any(|a| a == "--mcp")
        .then(mcp::spawn_stdio);
    if std::env::args().any(|a| a == "--arm-api") {
        app.arm_api(true);
    }

    // `--demo` starts with a small sample project.
    if std::env::args().any(|a| a == "--demo") {
        app.load_demo();
        if std::env::args().any(|a| a == "--library") {
            app.show_library();
        }
        // `--mode=preview` or `--mode=export` opens that screen, for looking at it.
        for mode in ["preview", "export"] {
            if std::env::args().any(|a| a == format!("--mode={mode}")) {
                app.state_mut().run_command(&format!("mode.{mode}"), 0);
            }
        }
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 800.0])
        .with_title("Plotter Studio");
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "Plotter Studio",
        options,
        Box::new(move |cc| {
            let mut app = app;
            if let Some(state) = &cc.wgpu_render_state {
                app.use_gpu_canvas(state.target_format);
            }
            Ok(Box::new(Shell {
                app,
                mcp,
                started: Instant::now(),
                settings_path,
                last_title: String::new(),
                guard,
                snapshot,
                last_mode: Mode::Prepare,
            }))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::window_icon;

    #[test]
    fn the_window_has_its_own_icon_not_the_toolkit_default() {
        let icon = window_icon().expect("the embedded icon decodes");
        assert_eq!((icon.width, icon.height), (256, 256));
        assert!(
            icon.rgba.as_chunks::<4>().0.iter().any(|px| px[3] > 0),
            "the picture is not empty"
        );
    }
}
