//! Ground truth for the window layer. The UI is a thin layer over `AppState`,
//! so these tests drive it through the AccessKit tree, the way a screen reader sees it: every
//! control must have a label, and the panels must show what the state holds. An unlabelled control
//! fails here.

use std::time::Duration;

use egui::Key;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use ps_device::DeviceId;
use ps_geometry::Point;
use ps_library::{DisplayUnits, Settings};
use ps_model::OpKind;
use ps_render::{colour_of, icon_name, style_of};

use super::*;
use crate::commands::Mods;
use crate::machine::MachineController;
use crate::online::OnlineController;
use crate::state::Effect;
use crate::state::Mode;
use crate::tools::{CanvasEvent, ToolId};

const NONE: Mods = Mods {
    command: false,
    shift: false,
    alt: false,
};

fn app() -> App {
    let mut a = App::new(Settings::default(), "en");
    // Pictures are built on the spot, so they are there when a test looks.
    a.preview_blocking = true;
    a
}

/// The number in a labelled numeric field, as assistive technology reads it.
fn field_value(h: &Harness<'static, App>, label: &str) -> f64 {
    h.get_by_label(label)
        .accesskit_node()
        .numeric_value()
        .unwrap_or_else(|| panic!("{label} has no number"))
}

fn harness(app: App) -> Harness<'static, App> {
    Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_state(|ctx, app: &mut App| show(ctx, app, 0), app)
}

struct NoDialogs;

impl crate::shell::Dialogs for NoDialogs {
    fn open_project(&mut self) -> Option<std::path::PathBuf> {
        None
    }
    fn import_drawing(&mut self) -> Option<std::path::PathBuf> {
        None
    }
    fn save_project(&mut self, _suggested: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn pick_folder(&mut self) -> Option<std::path::PathBuf> {
        None
    }
    fn pick_package(&mut self) -> Option<std::path::PathBuf> {
        None
    }
}

/// A harness that also carries out the effects commands leave for the shell.
fn shell_harness(app: App) -> Harness<'static, App> {
    Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_state(
            |ctx, app: &mut App| {
                show(ctx, app, 0);
                let effect = app.take_effect();
                crate::shell::handle_effect(app, effect, &mut NoDialogs, 0);
            },
            app,
        )
}

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn draw_rect(app: &mut App, from: Point, to: Point) {
    app.state.run_command("tool.rectangle", 0);
    app.state.canvas_event(CanvasEvent::Press(from), NONE, 0);
    app.state.canvas_event(CanvasEvent::Drag(to), NONE, 0);
    app.state.canvas_event(CanvasEvent::Release(to), NONE, 0);
}

// ---- top bar ----

#[test]
fn the_top_bar_has_the_three_modes_and_clicking_one_switches() {
    let mut h = harness(app());
    for label in ["Prepare", "Preview", "Export"] {
        h.get_by_label(label);
    }
    h.get_by_label("Preview").click();
    h.run();
    assert_eq!(h.state().state.mode(), Mode::Preview);
    h.get_by_label("Export").click();
    h.run();
    assert_eq!(h.state().state.mode(), Mode::Export);
}

#[test]
fn the_warnings_counter_says_how_many_problems_there_are() {
    let mut h = harness(app());
    h.run();
    h.get_by_label("0 warnings");
    let mut a = app();
    draw_rect(&mut a, p(400.0, 400.0), p(450.0, 450.0));
    let n = a.state.warning_count();
    assert!(n >= 1, "an object off the mat is a problem");
    let expected = if n == 1 {
        "1 warning".to_owned()
    } else {
        format!("{n} warnings")
    };
    let mut h = harness(a);
    h.run();
    h.get_by_label(&expected);
}

#[test]
fn the_machine_chip_says_when_nothing_is_connected() {
    let mut h = harness(app());
    h.run();
    h.get_by_label("No machine connected");
}

// ---- tools ----

#[test]
fn the_tool_buttons_are_labelled_and_switch_the_tool() {
    let mut h = harness(app());
    h.run();
    h.get_by_label("Rectangle (R)").click();
    h.run();
    assert_eq!(h.state().state.tool_id(), ToolId::Rectangle);
    h.get_by_label("Ellipse (E)").click();
    h.run();
    assert_eq!(h.state().state.tool_id(), ToolId::Ellipse);
}

// ---- project tree ----

#[test]
fn the_project_tree_lists_the_mat_and_its_objects() {
    let mut a = app();
    draw_rect(&mut a, p(10.0, 10.0), p(40.0, 30.0));
    draw_rect(&mut a, p(60.0, 10.0), p(90.0, 30.0));
    let mut h = harness(a);
    h.run();
    h.get_by_label("Mat 1");
    h.get_by_label("Object 1");
    h.get_by_label("Object 2");
}

#[test]
fn clicking_an_object_in_the_tree_selects_it() {
    let mut a = app();
    draw_rect(&mut a, p(10.0, 10.0), p(40.0, 30.0));
    draw_rect(&mut a, p(60.0, 10.0), p(90.0, 30.0));
    a.state.run_command("edit.undo", 0);
    a.state.run_command("edit.redo", 0);
    let first = a
        .state
        .current_mat()
        .objects
        .keys()
        .next()
        .copied()
        .unwrap();
    let mut h = harness(a);
    h.run();
    h.get_by_label("Object 1").click();
    h.run();
    assert_eq!(h.state().state.selection().objects(), vec![first]);
}

#[test]
fn a_selected_object_is_marked_selected_for_screen_readers() {
    let mut a = app();
    draw_rect(&mut a, p(10.0, 10.0), p(40.0, 30.0));
    let mut h = harness(a);
    h.run();
    let node = h.get_by_label("Object 1");
    assert_eq!(
        node.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True)
    );
}

// ---- inspector ----

#[test]
fn the_control_bar_shows_nothing_selected_or_the_selected_objects_place_and_size() {
    let mut h = harness(app());
    h.run();
    h.get_by_label("Selection: Nothing selected");

    let mut a = app();
    draw_rect(&mut a, p(10.0, 20.0), p(40.0, 50.0));
    let mut h = harness(a);
    h.run();
    assert!((field_value(&h, "X position") - 10.0).abs() < 1e-9);
    assert!((field_value(&h, "Y position") - 20.0).abs() < 1e-9);
    assert!((field_value(&h, "Width") - 30.0).abs() < 1e-9);
    assert!((field_value(&h, "Height") - 30.0).abs() < 1e-9);
}

#[test]
fn the_control_bar_follows_the_display_units() {
    let settings = Settings {
        units: DisplayUnits::Inches,
        ..Settings::default()
    };
    let mut a = App::new(settings, "en");
    draw_rect(&mut a, p(25.4, 50.8), p(76.2, 101.6));
    let mut h = harness(a);
    h.run();
    assert!((field_value(&h, "X position") - 1.0).abs() < 1e-9);
    assert!((field_value(&h, "Y position") - 2.0).abs() < 1e-9);
}

// ---- operations panel ----

#[test]
fn the_operations_panel_explains_when_there_are_none() {
    let mut a = app();
    draw_rect(&mut a, p(10.0, 10.0), p(40.0, 30.0));
    let mut h = harness(a);
    h.run();
    h.get_by_label("No operations yet");
}

// ---- palette ----

#[test]
fn the_palette_opens_filters_by_label_and_runs_a_command() {
    let mut a = app();
    a.state.run_command("palette.open", 0);
    let mut h = harness(a);
    h.run();
    h.get_by_role_and_label(Role::Window, "Command palette");
    h.get_by_label("Type a command…");
    h.get_by_label("Zoom to fit");
    h.state_mut().palette_query = "ellip".to_owned();
    h.run();
    h.get_by_label("Ellipse");
    assert!(h.query_by_label("Zoom to fit").is_none(), "filtered out");
    // Let the window settle into its new size before aiming at a button in it.
    h.step();
    h.step();
    h.get_by_label("Ellipse").click();
    h.run();
    assert_eq!(h.state().state.tool_id(), ToolId::Ellipse);
    assert!(!h.state().state.palette_open(), "it closes after running");
    assert_eq!(h.state().palette_query, "", "and forgets the query");
}

#[test]
fn the_palette_says_when_nothing_matches() {
    let mut a = app();
    a.state.run_command("palette.open", 0);
    a.palette_query = "zzzzqqq".to_owned();
    let mut h = harness(a);
    h.run();
    h.get_by_label("No command matches");
}

#[test]
fn the_palette_is_not_there_when_closed() {
    let mut h = harness(app());
    h.run();
    assert!(
        h.query_by_role_and_label(Role::Window, "Command palette")
            .is_none()
    );
}

// ---- accessibility ----

#[test]
fn every_button_in_every_mode_has_a_label() {
    for mode in ["mode.prepare", "mode.preview", "mode.export"] {
        let mut a = app();
        draw_rect(&mut a, p(10.0, 10.0), p(40.0, 30.0));
        a.state.run_command(mode, 0);
        let mut h = harness(a);
        h.run();
        let unlabelled: Vec<_> = h
            .query_all_by_role(Role::Button)
            .filter(|n| {
                n.accesskit_node()
                    .label()
                    .is_none_or(|l| l.trim().is_empty())
            })
            .map(|n| format!("{:?}", n.accesskit_node().id()))
            .collect();
        assert!(
            unlabelled.is_empty(),
            "{mode}: unlabelled buttons {unlabelled:?}"
        );
    }
}

#[test]
fn operations_are_not_shown_by_colour_alone() {
    // Each kind has a line style and an icon name; the operations legend names them.
    let mut h = harness(app());
    h.run();
    for kind in OpKind::ALL {
        let _ = (style_of(kind), icon_name(kind));
    }
    assert_eq!(operation_legend().len(), OpKind::ALL.len());
    assert!(operation_legend().iter().all(|e| !e.icon.is_empty()));
}

// ---- keyboard ----

#[test]
fn key_names_follow_the_shortcut_notation() {
    assert_eq!(key_name(Key::ArrowLeft), Some("Left".to_owned()));
    assert_eq!(key_name(Key::Delete), Some("Delete".to_owned()));
    assert_eq!(key_name(Key::V), Some("V".to_owned()));
    assert_eq!(key_name(Key::Num0), Some("0".to_owned()));
    assert_eq!(key_name(Key::Equals), Some("Equals".to_owned()));
    assert_eq!(key_name(Key::Minus), Some("Minus".to_owned()));
    assert_eq!(key_name(Key::F6), Some("F6".to_owned()));
}

#[test]
fn pressing_a_bound_key_runs_its_command() {
    let mut h = harness(app());
    h.run();
    h.key_press(Key::R);
    h.run();
    assert_eq!(h.state().state.tool_id(), ToolId::Rectangle);
}

// ---- operations, export and preview ----

fn compiled_app() -> App {
    let mut a = app();
    draw_rect(&mut a, p(20.0, 20.0), p(80.0, 60.0));
    assert!(
        a.state
            .assign_operation(OpKind::Cut, "generic.drag-blade@1.0.0", 0)
    );
    a.state.tick(100);
    assert!(a.state.wait_for_compile(Duration::from_secs(60)));
    a
}

#[test]
fn the_tool_choices_for_a_kind_come_from_the_library() {
    let a = app();
    let cut = a.state.tools_for(OpKind::Cut);
    assert!(
        cut.iter()
            .any(|t| t.reference == "generic.drag-blade@1.0.0"),
        "{cut:?}"
    );
    assert!(cut.iter().all(|t| !t.name.is_empty()));
    let draw = a.state.tools_for(OpKind::Draw);
    assert!(!draw.is_empty(), "there are pens");
    assert!(
        draw.iter().all(|t| !cut.contains(t)),
        "a blade does not draw"
    );
}

#[test]
fn the_inspector_offers_to_assign_an_operation_to_the_selection() {
    let mut a = app();
    draw_rect(&mut a, p(10.0, 10.0), p(40.0, 30.0));
    let mut h = harness(a);
    h.run();
    h.get_by_label("Assign operation").click();
    h.run();
    h.get_by_label_contains("Cut — Fine-point").click();
    h.run();
    let ops = h
        .state()
        .state
        .current_mat()
        .objects
        .values()
        .next()
        .unwrap()
        .paths[0]
        .ops
        .len();
    assert_eq!(ops, 1);
}

#[test]
fn nothing_selected_means_nothing_to_assign() {
    let mut h = harness(app());
    h.run();
    assert!(
        h.get_by_label("Assign operation")
            .accesskit_node()
            .is_disabled(),
        "the add button waits for a selection"
    );
}

#[test]
fn the_operations_panel_lists_assigned_operations() {
    let mut h = harness(compiled_app());
    h.run();
    h.get_by_label_contains("Select operation: Cut");
    h.get_by_label("Cut settings");
    h.get_by_label("Tool: Fine-point blade (45°)");
    assert!(h.query_by_label("No operations yet").is_none());
}

#[test]
fn export_mode_says_what_there_is_to_export() {
    let mut a = compiled_app();
    a.state.run_command("mode.export", 0);
    let mut h = harness(a);
    h.run();
    h.get_by_label_contains("1 mat");
    h.get_by_label("Save as file");
    h.get_by_label("No problems");
    h.get_by_label("G-code");
}

#[test]
fn export_mode_without_a_job_says_how_to_get_one() {
    let mut a = app();
    a.state.run_command("mode.export", 0);
    let mut h = harness(a);
    h.run();
    h.get_by_label("Nothing to export yet: assign an operation to a path");
}

#[test]
fn the_export_button_asks_the_shell_to_export() {
    let mut a = compiled_app();
    // The welcome window sits where the button is, so it has been seen already.
    a.state.update_settings(|s| s.first_run_seen = true);
    a.state.run_command("mode.export", 0);
    let mut h = harness(a);
    h.get_by_label("Save as file").click();
    h.get_by_label("Save as file").click();
    h.run();
    assert_eq!(h.state_mut().take_effect(), Effect::Export);
}

#[test]
fn preview_mode_has_a_timeline_with_play_and_speed() {
    let mut a = compiled_app();
    a.state.run_command("mode.preview", 0);
    let mut h = harness(a);
    h.run();
    h.get_by_label("Timeline");
    h.get_by_label("Play");
    h.get_by_label_contains("Speed");
}

#[test]
fn play_toggles_to_pause_and_back() {
    let mut a = compiled_app();
    a.state.run_command("mode.preview", 0);
    let mut h = harness(a);
    h.run();
    // Playing keeps repainting, so step a fixed small number of frames instead of running to rest.
    h.get_by_label("Play").click_accesskit();
    h.run_steps(2);
    assert!(h.state().preview_playing);
    h.get_by_label("Pause").click_accesskit();
    h.run_steps(2);
    assert!(!h.state().preview_playing);
}

#[test]
fn preview_without_a_job_says_so() {
    let mut a = app();
    a.state.run_command("mode.preview", 0);
    let mut h = harness(a);
    h.run();
    h.get_by_label("Nothing to preview yet");
}

#[test]
fn the_status_bar_reports_the_compile_state() {
    let mut h = harness(compiled_app());
    h.run();
    h.get_by_label("Job ready");
    let mut a = compiled_app();
    a.state.run_command("nudge.right", 500);
    let mut h = harness(a);
    h.run();
    h.get_by_label("Job out of date");
}

// ---- machine panel ----

fn sim_app() -> App {
    let mut a = App::with_simulator(Settings::default(), "en");
    a.machine.refresh();
    a
}

/// Runs frames until a label shows up (the machine works on its own thread).
fn wait_for_label(h: &mut Harness<'static, App>, label: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        h.run_steps(2);
        if h.query_by_label(label).is_some() {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "never saw {label:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_machine_chip_opens_the_machine_panel() {
    let mut h = harness(sim_app());
    h.run_steps(3);
    assert!(
        h.query_by_label("Look again").is_none(),
        "closed to begin with"
    );
    h.get_by_label("No machine connected").click();
    h.run_steps(3);
    h.get_by_label("Look again");
}

#[test]
fn an_unapproved_machine_asks_the_user_before_connecting() {
    let mut a = sim_app();
    a.machine_open = true;
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label_contains("Allow Built-in simulator to use Simulated machine");
    h.get_by_label("Allow").click();
    h.run_steps(3);
    h.get_by_label("Connect");
}

#[test]
fn connecting_shows_the_machines_state_in_the_chip() {
    let mut a = sim_app();
    a.machine_open = true;
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Allow").click_accesskit();
    h.run_steps(2);
    h.get_by_label("Connect").click_accesskit();
    wait_for_label(&mut h, "Simulated plotter — Idle");
    h.get_by_label("Disconnect");
    h.get_by_label("Disconnect").click_accesskit();
    h.run_steps(2);
    h.get_by_label("No machine connected");
}

#[test]
fn the_panel_offers_jog_and_job_controls_for_a_connected_machine() {
    let mut a = sim_app();
    a.machine_open = true;
    let id = DeviceId("sim".to_owned());
    a.machine.approve(&id);
    a.machine.connect(&id).unwrap();
    let mut h = harness(a);
    wait_for_label(&mut h, "Simulated plotter — Idle");
    for label in [
        "Jog left",
        "Jog right",
        "Jog up",
        "Jog down",
        "Pause",
        "Resume",
        "Cancel",
        "Send to machine",
    ] {
        h.get_by_label(label);
    }
}

#[test]
fn sending_a_compiled_job_shows_the_prompt_progress_and_the_result() {
    let mut a = compiled_app();
    a.machine = MachineController::new(true);
    a.machine.refresh();
    a.machine_open = true;
    let id = DeviceId("sim".to_owned());
    a.machine.approve(&id);
    a.machine.connect(&id).unwrap();
    let mut h = shell_harness(a);
    wait_for_label(&mut h, "Simulated plotter — Idle");
    h.get_by_label("Send to machine").click_accesskit();
    // Answer whatever the job asks for until it finishes.
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        h.run_steps(2);
        if h.query_by_label("Job finished").is_some() {
            break;
        }
        if h.query_by_label("Done").is_some() {
            h.get_by_label("Done").click_accesskit();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the job never finished"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A compiled job running on the simulator, up to the point where the progress pill shows.
fn running_job_harness(mode: &str) -> Harness<'static, App> {
    let mut a = compiled_app();
    a.state.run_command(mode, 0);
    a.machine = MachineController::new(true);
    a.machine.refresh();
    let id = DeviceId("sim".to_owned());
    a.machine.approve(&id);
    a.machine.connect(&id).unwrap();
    let mut h = shell_harness(a);
    h.run_steps(3);
    h.state_mut().run("machine.send", 0);
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    // The pill's pause button is the only "Pause" while the machine panel is closed.
    while h.query_by_label("Pause").is_none() {
        h.run_steps(2);
        if h.query_by_label("Done").is_some() {
            h.get_by_label("Done").click_accesskit();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the progress pill never showed"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    h
}

#[test]
fn pausing_a_running_job_is_visible_on_the_progress_pill_and_can_be_undone_there() {
    let mut h = running_job_harness("mode.export");
    h.get_by_label("Pause").click_accesskit();
    // The machine goes on with what it already has, so the pill must say the press was heard.
    wait_for_label(&mut h, "Resume");
    h.get_by_label("Paused");
    h.get_by_label("Resume").click_accesskit();
    wait_for_label(&mut h, "Pause");
    assert!(h.query_by_label("Paused").is_none());
}

#[test]
fn the_progress_pill_stays_clear_of_the_preview_bar() {
    let h = running_job_harness("mode.preview");
    let pill = h.get_by_label("Job progress").rect();
    let play = h.get_by_label("Play").rect();
    let timeline = h.get_by_label("Timeline").rect();
    assert!(
        !pill.intersects(play) && !pill.intersects(timeline),
        "pill {pill:?} overlaps the preview bar ({play:?}, {timeline:?})"
    );
    assert!(
        pill.bottom() <= play.top(),
        "the pill sits above the bar: {pill:?} vs {play:?}"
    );
}

#[test]
fn without_a_compiled_job_send_is_disabled() {
    let mut a = sim_app();
    a.machine_open = true;
    let id = DeviceId("sim".to_owned());
    a.machine.approve(&id);
    a.machine.connect(&id).unwrap();
    let mut h = harness(a);
    wait_for_label(&mut h, "Simulated plotter — Idle");
    assert!(
        h.get_by_label("Send to machine")
            .accesskit_node()
            .is_disabled()
    );
}

// ---- crash recovery ----

fn autosave_with_one_object() -> (tempfile::TempDir, ps_model::AutosaveEntry) {
    let mut a = app();
    draw_rect(&mut a, p(10.0, 10.0), p(40.0, 30.0));
    let dir = tempfile::tempdir().unwrap();
    let bytes = ps_model::Project::new(a.state.document().as_ref().clone())
        .to_bytes()
        .unwrap();
    let path = ps_model::Autosaver::new(dir.path(), 5)
        .write("untitled", 1_700_000_000_000, &bytes)
        .unwrap();
    let entry = ps_model::AutosaveEntry {
        path,
        time_ms: 1_700_000_000_000,
    };
    (dir, entry)
}

#[test]
fn after_a_crash_the_app_offers_to_restore_the_newest_autosave_and_shows_when_it_was_made() {
    let (_dir, entry) = autosave_with_one_object();
    let mut a = app();
    a.offer_recovery(entry);
    let mut h = harness(a);
    h.run();
    h.get_by_role_and_label(Role::Window, "Recover unsaved work?");
    h.get_by_label_contains("2023-11-14T22:13:20Z");
    h.get_by_label("Restore");
    h.get_by_label("Discard");
}

#[test]
fn restoring_loads_the_autosave_and_keeps_it_marked_as_unsaved() {
    let (_dir, entry) = autosave_with_one_object();
    let mut a = app();
    a.offer_recovery(entry);
    let mut h = harness(a);
    h.run();
    h.get_by_label("Restore").click();
    h.run();
    assert_eq!(h.state().state.current_mat().objects.len(), 1);
    assert!(
        h.state().state.is_dirty(),
        "the recovered work still needs saving"
    );
    assert!(
        h.query_by_role_and_label(Role::Window, "Recover unsaved work?")
            .is_none()
    );
}

#[test]
fn discarding_leaves_the_project_alone() {
    let (_dir, entry) = autosave_with_one_object();
    let mut a = app();
    a.offer_recovery(entry);
    let mut h = harness(a);
    h.run();
    h.get_by_label("Discard").click();
    h.run();
    assert_eq!(h.state().state.current_mat().objects.len(), 0);
    assert!(
        h.query_by_role_and_label(Role::Window, "Recover unsaved work?")
            .is_none()
    );
}

#[test]
fn an_unreadable_autosave_is_reported_and_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("0000000000000001.pstudio");
    std::fs::write(&bad, b"junk").unwrap();
    let mut a = app();
    a.offer_recovery(ps_model::AutosaveEntry {
        path: bad,
        time_ms: 1,
    });
    let mut h = harness(a);
    h.run();
    h.get_by_label("Restore").click();
    h.run();
    assert!(
        h.state()
            .notice
            .as_deref()
            .unwrap()
            .contains("Could not restore")
    );
    assert!(
        h.query_by_role_and_label(Role::Window, "Recover unsaved work?")
            .is_none()
    );
}

// ---- settings window ----

fn settings_app() -> App {
    let mut a = app();
    a.state.run_command("settings.open", 0);
    a
}

#[test]
fn the_settings_window_lists_the_choices_with_labels() {
    let mut h = harness(settings_app());
    h.run_steps(3);
    h.get_by_role_and_label(Role::Window, "Settings");
    for label in [
        "Millimetres",
        "Inches",
        "Light",
        "Dark",
        "Autosave every (seconds)",
        "Keep the previous version when saving",
        "Check for updates",
        "Download the material index",
    ] {
        h.get_by_label(label);
    }
}

#[test]
fn the_network_features_are_off_by_default_and_the_window_says_nothing_is_sent() {
    let mut h = harness(settings_app());
    h.run_steps(3);
    assert_eq!(
        h.get_by_label("Check for updates")
            .accesskit_node()
            .toggled(),
        Some(egui::accesskit::Toggled::False)
    );
    assert_eq!(
        h.get_by_label("Download the material index")
            .accesskit_node()
            .toggled(),
        Some(egui::accesskit::Toggled::False)
    );
    h.get_by_label("Nothing is sent over the internet unless you turn one of these on");
}

#[test]
fn turning_on_a_network_feature_is_remembered() {
    let mut h = harness(settings_app());
    h.run_steps(3);
    h.get_by_label("Check for updates").click_accesskit();
    h.run_steps(3);
    assert!(h.state().state.settings().network.update_check);
    assert!(!h.state().state.settings().network.material_index);
}

#[test]
fn choosing_inches_changes_the_units_everywhere_at_once() {
    let mut a = settings_app();
    draw_rect(&mut a, p(25.4, 50.8), p(76.2, 101.6));
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Inches").click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().state.settings().units, DisplayUnits::Inches);
    assert!((field_value(&h, "X position") - 1.0).abs() < 1e-9);
    assert!((field_value(&h, "Y position") - 2.0).abs() < 1e-9);
}

#[test]
fn choosing_a_theme_applies_it() {
    let mut h = harness(settings_app());
    h.run_steps(3);
    h.get_by_label("Dark").click_accesskit();
    h.run_steps(3);
    assert!(h.ctx.style().visuals.dark_mode);
    h.get_by_label("Light").click_accesskit();
    h.run_steps(3);
    assert!(!h.ctx.style().visuals.dark_mode);
}

#[test]
fn a_shortcut_can_be_rebound_from_the_settings_window() {
    let mut h = harness(settings_app());
    h.run_steps(3);
    h.get_by_label("Change shortcut for Rectangle")
        .click_accesskit();
    h.run_steps(3);
    h.get_by_label("Press the new shortcut for Rectangle");
    h.key_press(Key::Q);
    h.run_steps(3);
    assert_eq!(h.state().state.settings().shortcuts["tool.rectangle"], "Q");
    h.get_by_label("Change shortcut for Rectangle");
}

#[test]
fn a_taken_shortcut_is_refused_with_the_name_of_the_command_that_has_it() {
    let mut h = harness(settings_app());
    h.run_steps(3);
    h.get_by_label("Change shortcut for Rectangle")
        .click_accesskit();
    h.run_steps(3);
    h.key_press(Key::E);
    h.run_steps(3);
    h.get_by_label("E is already used by Ellipse");
    assert!(h.state().state.settings().shortcuts.is_empty());
}

#[test]
fn the_language_list_names_the_available_languages() {
    let mut h = harness(settings_app());
    h.run_steps(3);
    h.get_by_label("Language");
    assert_eq!(h.state().loc.languages(), vec!["en"]);
}

#[test]
fn the_settings_window_can_be_closed() {
    let mut h = harness(settings_app());
    h.run_steps(3);
    h.get_by_label("Close settings").click_accesskit();
    h.run_steps(3);
    assert!(!h.state().state.settings_open());
}

// ---- arrange panel ----

#[test]
fn the_control_bar_offers_the_nine_anchors_and_the_transform_buttons() {
    let mut a = app();
    draw_rect(&mut a, p(10.0, 20.0), p(40.0, 50.0));
    let mut h = harness(a);
    h.run_steps(3);
    for label in [
        "Rotate 90° clockwise",
        "Mirror horizontally",
        "X position",
        "Y position",
        "Width",
        "Height",
    ] {
        h.get_by_label(label);
    }
    h.get_by_label("Anchor").click_accesskit();
    h.run_steps(3);
    for label in [
        "Anchor top left",
        "Anchor top",
        "Anchor top right",
        "Anchor left",
        "Anchor centre",
        "Anchor right",
        "Anchor bottom left",
        "Anchor bottom",
        "Anchor bottom right",
    ] {
        h.get_by_label(label);
    }
}

#[test]
fn choosing_an_anchor_changes_which_point_the_position_describes() {
    let mut a = app();
    draw_rect(&mut a, p(10.0, 20.0), p(40.0, 50.0));
    let mut h = harness(a);
    h.run_steps(3);
    assert!((field_value(&h, "X position") - 10.0).abs() < 1e-9);
    h.get_by_label("Anchor").click_accesskit();
    h.run_steps(3);
    h.get_by_label("Anchor bottom right").click_accesskit();
    h.run_steps(3);
    assert!((field_value(&h, "X position") - 40.0).abs() < 1e-9);
    assert!((field_value(&h, "Y position") - 50.0).abs() < 1e-9);
}

#[test]
fn the_transform_buttons_change_the_selection() {
    let mut a = app();
    draw_rect(&mut a, p(10.0, 20.0), p(40.0, 70.0));
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Rotate 90° clockwise").click_accesskit();
    h.run_steps(3);
    let b = h.state().state.selection_bounds().unwrap();
    assert!(
        (b.width() - 50.0).abs() < 1e-6 && (b.height() - 30.0).abs() < 1e-6,
        "{b:?}"
    );
}

#[test]
fn aligning_and_distributing_are_offered_only_for_several_objects() {
    let mut a = app();
    draw_rect(&mut a, p(10.0, 20.0), p(40.0, 50.0));
    let mut h = harness(a);
    h.run_steps(3);
    assert!(
        h.get_by_label("Align left edges")
            .accesskit_node()
            .is_disabled()
    );

    let mut a = app();
    draw_rect(&mut a, p(10.0, 20.0), p(40.0, 50.0));
    draw_rect(&mut a, p(60.0, 20.0), p(90.0, 50.0));
    a.state.run_command("edit.select-all", 0);
    let mut h = harness(a);
    h.run_steps(3);
    assert!(
        !h.get_by_label("Align left edges")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        !h.get_by_label("Distribute horizontally")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn a_grid_array_can_be_made_from_the_inspector() {
    let mut a = app();
    draw_rect(&mut a, p(10.0, 20.0), p(40.0, 50.0));
    a.array_columns = 3;
    a.array_rows = 2;
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Grid array").click();
    h.run_steps(3);
    h.get_by_label("Make grid").click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().state.current_mat().objects.len(), 6);
}

#[test]
fn a_circular_array_can_be_made_from_the_inspector() {
    let mut a = app();
    draw_rect(&mut a, p(100.0, 100.0), p(110.0, 110.0));
    a.circle_count = 5;
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Circular array").click();
    h.run_steps(3);
    h.get_by_label("Make circle").click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().state.current_mat().objects.len(), 5);
}

// ---- mats, material and machine pickers ----

#[test]
fn mats_are_added_from_the_tab_strip_and_duplicated_or_removed_from_the_mat_menu() {
    let mut h = harness(app());
    h.run_steps(3);
    h.get_by_label("New mat").click_accesskit();
    h.run_steps(3);
    h.get_by_label("Add mat").click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().state.document().mats.len(), 2);
    h.get_by_label("Mat 1");
    h.get_by_label("Mat 2");
    h.get_by_label("New mat").click_accesskit();
    h.run_steps(3);
    h.get_by_label("Duplicate mat").click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().state.document().mats.len(), 3);
    // Each tab has its own close button once there is more than one mat.
    h.get_by_label_contains("Remove mat:").click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().state.document().mats.len(), 2);
}

#[test]
fn clicking_a_mat_in_the_tree_switches_to_it() {
    let mut a = app();
    draw_rect(&mut a, p(10.0, 10.0), p(30.0, 30.0));
    a.state.add_mat(1);
    let first = a.state.mat_summaries()[0].id;
    let mut h = harness(a);
    h.run_steps(3);
    assert_ne!(h.state().state.current_mat().id, first);
    h.get_by_label("Mat 1").click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().state.current_mat().id, first);
    h.get_by_label("Object 1");
}

#[test]
fn the_current_mat_is_marked_selected_for_screen_readers() {
    let mut a = app();
    a.state.add_mat(1);
    let mut h = harness(a);
    h.run_steps(3);
    assert_eq!(
        h.get_by_label("Mat 2").accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    assert_eq!(
        h.get_by_label("Mat 1").accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::False)
    );
}

#[test]
fn the_top_bar_has_a_machine_profile_picker_and_a_material_picker() {
    let mut h = harness(app());
    h.run_steps(3);
    h.get_by_label("Machine profile");
    h.get_by_label("Material");
}

#[test]
fn choosing_a_material_sets_it_on_the_current_mat() {
    let a = app();
    let first = a.state.material_choices().remove(0);
    let mut h = harness(a);
    h.run_steps(3);
    let combos: Vec<_> = h.query_all_by_role(Role::ComboBox).collect();
    combos[1].click();
    h.run_steps(3);
    h.get_by_label(&first.name).click();
    h.run_steps(3);
    assert_eq!(
        h.state().state.current_material_reference().as_deref(),
        Some(first.reference.as_str())
    );
}

// ---- tool options ----

#[test]
fn the_polygon_tool_shows_its_side_count_and_the_star_tool_its_inner_radius() {
    let mut a = app();
    a.state.run_command("tool.polygon", 0);
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Sides");
    assert!(h.query_by_label("Inner radius").is_none());

    let mut a = app();
    a.state.run_command("tool.star", 0);
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Points");
    h.get_by_label("Inner radius");

    let mut h = harness(app());
    h.run_steps(3);
    assert!(h.query_by_label("Sides").is_none());
}

#[test]
fn enter_finishes_a_pen_path_from_the_keyboard() {
    let mut a = app();
    a.state.run_command("tool.pen", 0);
    for at in [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)] {
        a.state.canvas_event(CanvasEvent::Press(at), NONE, 0);
        a.state.canvas_event(CanvasEvent::Release(at), NONE, 0);
    }
    let mut h = harness(a);
    h.run_steps(3);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().state.current_mat().objects.len(), 1);
}

// ---- node editing in the inspector ----

fn app_with_selected_node() -> App {
    let mut a = app();
    a.state.run_command("tool.pen", 0);
    for at in [p(0.0, 0.0), p(20.0, 0.0), p(20.0, 20.0)] {
        a.state.canvas_event(CanvasEvent::Press(at), NONE, 0);
        a.state.canvas_event(CanvasEvent::Release(at), NONE, 0);
    }
    a.state.canvas_event(CanvasEvent::Enter, NONE, 0);
    a.state.run_command("tool.node", 0);
    a.state
        .canvas_event(CanvasEvent::Press(p(20.0, 0.0)), NONE, 0);
    a.state
        .canvas_event(CanvasEvent::Release(p(20.0, 0.0)), NONE, 0);
    a
}

#[test]
fn a_selected_node_offers_the_node_commands() {
    let mut h = harness(app_with_selected_node());
    h.run_steps(3);
    // The commands are in a menu, so the control bar stays on one row.
    h.get_by_label("Node").click_accesskit();
    h.run_steps(3);
    for label in [
        "Delete node",
        "Make segment a curve",
        "Make segment a line",
        "Smooth node",
        "Corner node",
    ] {
        h.get_by_label(label);
    }
}

#[test]
fn the_node_commands_are_not_offered_without_a_selected_node() {
    let mut a = app();
    draw_rect(&mut a, p(10.0, 10.0), p(40.0, 30.0));
    let mut h = harness(a);
    h.run_steps(3);
    assert!(h.query_by_label("Node").is_none());
}

#[test]
fn clicking_smooth_node_changes_the_path() {
    let mut h = harness(app_with_selected_node());
    h.run_steps(3);
    h.get_by_label("Node").click_accesskit();
    h.run_steps(3);
    h.get_by_label("Smooth node").click_accesskit();
    h.run_steps(3);
    let mat = h.state().state.current_mat();
    let geom = &mat.objects.values().next().unwrap().paths[0].geom;
    assert!(matches!(geom.segs[0], ps_geometry::Seg::Cubic { .. }));
}

// ---- update check ----

struct FeedFake(Option<&'static str>);

impl ps_net::Http for FeedFake {
    fn get(&self, _url: &str, _max: usize) -> Result<Vec<u8>, ps_net::HttpError> {
        self.0
            .map(|text| text.as_bytes().to_vec())
            .ok_or(ps_net::HttpError::Status(503))
    }
}

fn app_with_feed(feed: Option<&'static str>, update_check: bool) -> App {
    let settings = Settings {
        network: ps_library::Network {
            update_check,
            material_index: false,
        },
        ..Settings::default()
    };
    let mut a = App::new(settings.clone(), "en");
    a.online = OnlineController::new(
        std::sync::Arc::new(FeedFake(feed)),
        semver::Version::new(1, 0, 0),
        "https://example.org/feed.json".to_owned(),
        "https://example.org/index.json".to_owned(),
        Vec::new(),
        settings.network,
    );
    a.state.run_command("settings.open", 0);
    a
}

const NEWER: &str = r#"{"schema":1,"releases":[{"version":"9.0.0","date":"d","notes":"n","url":"https://example.org/9.0.0"}]}"#;

fn wait_for_text(h: &mut Harness<'static, App>, text: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        h.run_steps(2);
        if h.query_all_by_label_contains(text).next().is_some() {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "never saw {text:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_update_button_only_works_when_update_checks_are_on() {
    let mut h = harness(app_with_feed(Some(NEWER), false));
    h.run_steps(3);
    assert!(
        h.get_by_label("Check for updates now")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn checking_for_updates_shows_the_new_version_in_the_status_bar() {
    let mut h = harness(app_with_feed(Some(NEWER), true));
    h.run_steps(3);
    h.get_by_label("Check for updates now").click_accesskit();
    wait_for_text(&mut h, "Update available: 9.0.0");
}

#[test]
fn a_failed_check_says_why_in_the_settings_window() {
    let mut h = harness(app_with_feed(None, true));
    h.run_steps(3);
    h.get_by_label("Check for updates now").click_accesskit();
    wait_for_text(&mut h, "503");
}

#[test]
fn an_up_to_date_app_says_so() {
    let mut h = harness(app_with_feed(Some(r#"{"schema":1,"releases":[]}"#), true));
    h.run_steps(3);
    h.get_by_label("Check for updates now").click_accesskit();
    wait_for_text(&mut h, "up to date");
}

// ---- the first-run notice ----

#[test]
fn a_new_installation_says_what_is_and_is_not_sent_over_the_internet() {
    let mut h = harness(app());
    h.run_steps(3);
    h.get_by_role_and_label(Role::Window, "Welcome to Plotter Studio");
    h.get_by_label_contains("Plotter Studio never sends anything over the internet by itself");
    h.get_by_label_contains("no accounts, no usage counters");
    h.get_by_label("Get started");
}

#[test]
fn getting_started_dismisses_it_and_it_is_remembered() {
    let mut h = harness(app());
    h.run_steps(3);
    h.get_by_label("Get started").click_accesskit();
    h.run_steps(3);
    assert!(
        h.query_by_role_and_label(Role::Window, "Welcome to Plotter Studio")
            .is_none()
    );
    assert!(h.state().state.settings().first_run_seen);
    assert!(
        h.state_mut().state.take_settings_changed(),
        "so it gets saved"
    );
}

#[test]
fn it_is_not_shown_again_once_seen() {
    let settings = Settings {
        first_run_seen: true,
        ..Settings::default()
    };
    let mut h = harness(App::new(settings, "en"));
    h.run_steps(3);
    assert!(
        h.query_by_role_and_label(Role::Window, "Welcome to Plotter Studio")
            .is_none()
    );
}

#[test]
fn the_notice_is_not_shown_over_a_crash_recovery_prompt() {
    // The prompt about unsaved work comes first; the welcome waits.
    let (_dir, entry) = autosave_with_one_object();
    let mut a = app();
    a.offer_recovery(entry);
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_role_and_label(Role::Window, "Recover unsaved work?");
    assert!(
        h.query_by_role_and_label(Role::Window, "Welcome to Plotter Studio")
            .is_none()
    );
}

// ---- keyboard focus between panels ----

#[test]
fn f6_moves_the_keyboard_focus_from_panel_to_panel() {
    let mut a = app();
    a.state.update_settings(|s| s.first_run_seen = true);
    let mut h = harness(a);
    h.run_steps(3);
    h.key_press(Key::F6);
    h.run_steps(3);
    assert!(
        h.get_by_label("Prepare").accesskit_node().is_focused(),
        "the top bar"
    );
    h.key_press(Key::F6);
    h.run_steps(3);
    assert!(
        h.get_by_label("New mat").accesskit_node().is_focused(),
        "the project tree"
    );
}

#[test]
fn shortcuts_keep_working_after_a_panel_took_the_focus() {
    let mut a = app();
    a.state.update_settings(|s| s.first_run_seen = true);
    let mut h = harness(a);
    h.run_steps(3);
    h.key_press(Key::F6);
    h.run_steps(3);
    h.key_press(Key::R);
    h.run_steps(3);
    assert_eq!(h.state().state.tool_id(), ToolId::Rectangle);
}

#[test]
fn shortcuts_are_quiet_while_typing_in_the_palette() {
    let mut a = app();
    a.state.run_command("palette.open", 0);
    let mut h = harness(a);
    h.run_steps(3);
    h.key_press(Key::R);
    h.run_steps(3);
    assert_eq!(h.state().state.tool_id(), ToolId::Select);
}

// ---- the fixed screens of the design ----

fn first_run_done() -> App {
    let mut a = app();
    a.state.update_settings(|s| s.first_run_seen = true);
    a
}

#[test]
fn the_prepare_screen_has_layers_a_canvas_and_operations_each_named() {
    let mut h = harness(first_run_done());
    h.run_steps(3);
    for title in ["Layers", "Canvas", "Operations · Mat 1"] {
        h.get_by_label(title);
    }
}

#[test]
fn the_export_screen_says_when_there_is_nothing_to_send() {
    let mut a = first_run_done();
    a.state.run_command("mode.export", 0);
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Nothing to send yet");
}

#[test]
fn the_preview_screen_says_when_there_is_nothing_to_preview() {
    let mut a = first_run_done();
    a.state.run_command("mode.preview", 0);
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Realistic");
    h.get_by_label("Toolpath");
}

#[test]
fn with_no_device_plugin_the_machine_panel_says_to_install_one() {
    let mut a = app();
    a.machine_open = true;
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label_contains("No device plugins are installed");
    assert_eq!(h.query_all_by_label("No machines found").count(), 0);
}

#[test]
fn the_machine_panel_says_when_a_plugin_found_no_machines() {
    let dir = tempfile::tempdir().unwrap();
    {
        use ps_device::{DeviceFilter, PluginManifest, TransportKind, pack};
        let manifest = PluginManifest {
            id: "acme.quiet".to_owned(),
            name: "Acme Quiet".to_owned(),
            filters: vec![DeviceFilter {
                transport: TransportKind::Serial,
                usb: None,
            }],
            internet_hosts: Vec::new(),
        };
        let bytes = pack(&manifest, b"\0asm\x0d\0\x01\0", "1.0.0").unwrap();
        std::fs::write(dir.path().join("quiet.psdevice"), bytes).unwrap();
    }
    let mut a = app();
    a.machine = MachineController::new(false);
    a.machine.install_plugins(dir.path());
    a.machine_open = true;
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("No machines found");
    assert_eq!(
        h.query_all_by_label_contains("No device plugins").count(),
        0
    );
}

#[test]
fn nothing_is_looked_for_until_the_user_asks() {
    // `with_simulator` looks once on purpose (the built-in machine needs no hardware), so this
    // builds the controller by hand.
    let mut a = app();
    a.machine = MachineController::new(true);
    a.machine_open = true;
    let mut h = harness(a);
    h.run_steps(5);
    assert_eq!(h.state().machine.devices().len(), 0);
    assert!(!h.state().machine.is_refreshing());
    h.get_by_label("Look again").click_accesskit();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while h.state().machine.devices().is_empty() {
        assert!(std::time::Instant::now() < deadline, "the look never ended");
        h.run_steps(2);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn look_again_refreshes_the_machine_list() {
    let mut a = App::with_simulator(Settings::default(), "en");
    a.machine_open = true;
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Look again").click_accesskit();
    h.run_steps(3);
    h.get_by_label_contains("Allow Built-in simulator to use Simulated machine");
}

#[test]
fn installing_plugins_reports_files_that_cannot_be_used() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bad.psdevice"), b"not a package").unwrap();
    let mut a = app();
    a.install_plugins(dir.path());
    let notice = a.notice.clone().expect("a notice");
    assert!(notice.contains("bad.psdevice"), "{notice}");
}

#[test]
fn installing_plugins_from_a_clean_folder_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = app();
    a.install_plugins(dir.path());
    assert!(a.notice.is_none());
}

// ---- the library window ----

fn library_app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut a = app();
    a.open_library(dir.path()).unwrap();
    (dir, a)
}

fn material_package(id: &str, name: &str) -> Vec<u8> {
    use ps_library::{Entry, LibraryInfo, builtin_entries, export_package};
    let mut m = builtin_entries()
        .unwrap()
        .into_iter()
        .find_map(|e| match e {
            Entry::Material(m) => Some(m),
            _ => None,
        })
        .unwrap();
    m.id = id.to_owned();
    m.name = name.to_owned();
    let info = LibraryInfo {
        schema_version: 1,
        id: "acme.pack".into(),
        name: "Acme".into(),
        version: "1.0.0".into(),
        license: "CC0-1.0".into(),
    };
    export_package(&info, &[(m, std::collections::BTreeMap::new())]).unwrap()
}

#[test]
fn the_library_screen_opens_from_the_header_and_lists_built_in_tools() {
    let (_dir, a) = library_app();
    let mut h = harness(a);
    h.run_steps(3);
    assert!(
        h.query_by_label("Tools").is_none(),
        "the tabs are not shown yet"
    );
    h.get_by_label("Library").click_accesskit();
    h.run_steps(3);
    h.get_by_label("Tools").click_accesskit();
    h.run_steps(3);
    h.get_by_label_contains("Fine-point blade");
    assert!(
        h.query_all_by_label("Built-in").count() >= 4,
        "every shipped tool is marked"
    );
}

#[test]
fn the_library_window_has_a_tab_for_each_kind() {
    let (_dir, mut a) = library_app();
    a.library_open = true;
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Materials").click_accesskit();
    h.run_steps(3);
    // The card and the details of the first card both name it.
    assert!(h.query_all_by_label_contains("Cardstock").count() >= 1);
    h.get_by_label("Machines").click_accesskit();
    h.run_steps(3);
    assert!(h.query_all_by_label_contains("Cutter S").count() >= 1);
}

#[test]
fn an_installed_package_shows_up_and_reaches_the_materials_picker() {
    let (_dir, mut a) = library_app();
    a.library_open = true;
    a.install_package(&material_package("acme.glitter", "Glitter card"), None);
    assert!(
        a.state()
            .material_choices()
            .iter()
            .any(|m| m.name == "Glitter card")
    );
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Materials").click_accesskit();
    h.run_steps(3);
    h.get_by_label_contains("Glitter card");
    h.get_by_label("Remove");
    h.get_by_label_contains("Installed 1 material");
}

#[test]
fn a_user_entry_can_be_removed_and_a_built_in_one_has_no_remove_button() {
    let (_dir, mut a) = library_app();
    a.library_open = true;
    a.install_package(&material_package("acme.glitter", "Glitter card"), None);
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Tools").click_accesskit();
    h.run_steps(3);
    assert!(
        h.query_by_label("Remove").is_none(),
        "tools are all built in"
    );
    h.get_by_label("Materials").click_accesskit();
    h.run_steps(3);
    h.get_by_label("Remove").click_accesskit();
    h.run_steps(3);
    assert!(h.query_by_label_contains("Glitter card").is_none());
    assert!(
        h.state()
            .state()
            .material_choices()
            .iter()
            .all(|m| m.name != "Glitter card")
    );
}

#[test]
fn installing_the_same_package_again_asks_what_to_do() {
    let (_dir, mut a) = library_app();
    a.library_open = true;
    let bytes = material_package("acme.glitter", "Glitter card");
    a.install_package(&bytes, None);
    a.install_package(&bytes, None);
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label_contains("acme.glitter is already installed");
    h.get_by_label("Keep both").click_accesskit();
    h.run_steps(3);
    assert!(h.query_by_label_contains("is already installed").is_none());
    h.get_by_label("Materials").click_accesskit();
    h.run_steps(3);
    assert_eq!(
        h.query_all_by_label("Remove").count(),
        2,
        "the original and the copy"
    );
}

#[test]
fn a_file_that_is_not_a_package_says_why() {
    let (_dir, mut a) = library_app();
    a.library_open = true;
    a.install_package(b"not a zip", None);
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label_contains("Could not install");
}

// ---- the online tab of the library window ----

struct Pages(std::collections::BTreeMap<String, Vec<u8>>);

impl ps_net::Http for Pages {
    fn get(&self, url: &str, _max: usize) -> Result<Vec<u8>, ps_net::HttpError> {
        self.0
            .get(url)
            .cloned()
            .ok_or(ps_net::HttpError::Status(404))
    }
}

fn online_app(material_index: bool, package: &[u8]) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = Settings::default();
    settings.network.material_index = material_index;
    let network = settings.network;
    let mut a = App::new(settings, "en");
    a.open_library(dir.path()).unwrap();
    let mut pages = std::collections::BTreeMap::new();
    pages.insert(
        "https://example.org/glitter.zip".to_owned(),
        package.to_vec(),
    );
    a.online = crate::online::OnlineController::new(
        std::sync::Arc::new(Pages(pages)),
        semver::Version::new(1, 0, 0),
        "https://example.org/feed.json".into(),
        "https://example.org/index.json".into(),
        Vec::new(),
        network,
    );
    a.library_open = true;
    (dir, a)
}

fn listed_index(package: &[u8]) -> ps_net::MaterialIndex {
    ps_net::MaterialIndex {
        generated: "2026-01-01".into(),
        packages: vec![ps_net::PackageEntry {
            id: "acme.glitter".into(),
            name: "Glitter card".into(),
            version: semver::Version::new(1, 0, 0),
            kind: "material".into(),
            url: "https://example.org/glitter.zip".into(),
            sha256: ps_library::sha256_hex(package),
            size: u64::try_from(package.len()).unwrap(),
            license: "CC0-1.0".into(),
            category: "paper".into(),
            brand: "Acme".into(),
            machines: Vec::new(),
            tools: Vec::new(),
        }],
    }
}

#[test]
fn the_online_tab_says_when_online_materials_are_off() {
    let (_dir, a) = online_app(false, b"x");
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Online").click_accesskit();
    h.run_steps(3);
    h.get_by_label_contains("Online materials are off");
    assert!(h.query_by_label("Refresh list").is_none());
}

#[test]
fn the_online_tab_can_ask_for_the_list_when_it_is_on() {
    let (_dir, a) = online_app(true, b"x");
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Online").click_accesskit();
    h.run_steps(3);
    h.get_by_label("Refresh list").click_accesskit();
    h.run_steps(2);
    assert!(h.state().online.refreshing_index() || h.state().online.last_error().is_some());
}

#[test]
fn a_listed_package_can_be_installed_from_the_online_tab() {
    let package = material_package("acme.glitter", "Glitter card");
    let (_dir, mut a) = online_app(true, &package);
    a.online.use_index_for_tests(listed_index(&package));
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Online").click_accesskit();
    h.run_steps(3);
    h.get_by_label_contains("Glitter card");
    h.get_by_label("Install").click_accesskit();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !h
        .state()
        .state()
        .material_choices()
        .iter()
        .any(|m| m.name == "Glitter card")
    {
        h.run_steps(2);
        assert!(std::time::Instant::now() < deadline, "never installed");
        std::thread::sleep(Duration::from_millis(5));
    }
}

// ---- the text window ----

fn text_app() -> App {
    let mut a = app();
    a.fonts = Some(crate::fonts::FontLibrary::bundled_only());
    a
}

#[test]
fn the_text_window_opens_from_the_top_bar() {
    let mut h = harness(text_app());
    h.run_steps(3);
    assert!(h.query_by_label("Text content").is_none());
    h.get_by_label("Add text").click_accesskit();
    h.run_steps(3);
    // The text is typed in a card on the canvas, not in a window of its own.
    assert!(h.query_by_role_and_label(Role::Window, "Text").is_none());
    h.get_by_label("Text content");
    h.get_by_label("Add to mat");
}

#[test]
fn the_text_command_is_in_the_palette_registry() {
    let mut a = text_app();
    a.state_mut().run_command("text.open", 0);
    assert!(a.state().text_open());
}

#[test]
fn adding_text_from_the_window_places_an_object_on_the_mat() {
    let mut a = text_app();
    a.state_mut().run_command("text.open", 0);
    let mut h = harness(a);
    h.run_steps(3);
    assert_eq!(h.state().state().current_mat().objects.len(), 0);
    h.get_by_label("Add to mat").click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().state().current_mat().objects.len(), 1);
}

#[test]
fn the_text_window_edits_the_text_and_options() {
    let mut a = text_app();
    a.state_mut().run_command("text.open", 0);
    let mut h = harness(a);
    h.run_steps(3);
    for label in [
        "Text content",
        "Font",
        "Size (mm)",
        "Letter spacing (mm)",
        "Line height",
        "Left",
        "Centre",
        "Right",
        "Single-stroke font for pens",
    ] {
        h.get_by_label(label);
    }
    h.get_by_label("Single-stroke font for pens")
        .click_accesskit();
    h.run_steps(3);
    h.get_by_label("Roman simplex");
    assert!(h.state().text_params.stroke.is_some());
    h.get_by_label("Centre").click_accesskit();
    h.run_steps(2);
    assert_eq!(h.state().text_params.align, ps_import::Align::Centre);
}

#[test]
fn blank_text_shows_why_nothing_was_added() {
    let mut a = text_app();
    a.text_params.text = String::new();
    a.state_mut().run_command("text.open", 0);
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Add to mat").click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().state().current_mat().objects.len(), 0);
    h.get_by_label_contains("no text");
}

// ---- the operations table ----

fn two_row_app() -> App {
    let mut a = compiled_app();
    draw_rect(&mut a, p(150.0, 150.0), p(200.0, 190.0));
    a.state
        .assign_operation(OpKind::Draw, "pen.fineliner-black-0.4@1.0.0", 0);
    a
}

#[test]
fn the_settings_show_how_many_paths_the_row_has() {
    let mut h = harness(compiled_app());
    h.run();
    assert!(h.query_all_by_label("1 path").count() >= 1);
}

#[test]
fn a_row_can_be_switched_off_and_back_on() {
    let mut h = harness(compiled_app());
    h.run();
    h.get_by_label("Enable operation").click_accesskit();
    h.run_steps(3);
    assert!(!h.state().state().operation_rows()[0].enabled);
    h.get_by_label("Enable operation").click_accesskit();
    h.run_steps(3);
    assert!(h.state().state().operation_rows()[0].enabled);
}

#[test]
fn the_selected_row_has_move_buttons_that_change_the_run_order() {
    let mut h = harness(two_row_app());
    h.run();
    let before = h.state().state().operation_rows();
    assert_eq!(before.len(), 2);
    h.get_by_label("Move down").click_accesskit();
    h.run_steps(3);
    let after = h.state().state().operation_rows();
    assert_eq!(after[0].id, before[1].id);
    assert_eq!(after[1].id, before[0].id);
}

#[test]
fn the_selected_row_can_be_locked_to_run_after_the_one_above() {
    let mut h = harness(two_row_app());
    h.run();
    h.get_by_label_contains("Select operation: Draw")
        .click_accesskit();
    h.run_steps(3);
    h.get_by_label("Run after previous row").click_accesskit();
    h.run_steps(3);
    assert!(h.state().state().operation_rows()[1].locked_after_previous);
}

#[test]
fn the_status_bar_names_what_the_cursor_snapped_to() {
    let mut a = app();
    a.state.run_command("tool.rectangle", 0);
    a.state.resize_canvas(1200.0, 900.0);
    a.state.run_command("view.zoom-fit", 0);
    a.state
        .canvas_pointer_event(CanvasEvent::Hover(p(10.2, 19.9)), NONE, 0);
    let mut h = harness(a);
    h.run_steps(2);
    h.get_by_label("Snap: grid");
}

#[test]
fn the_status_bar_says_nothing_about_snapping_when_nothing_snapped() {
    let mut a = app();
    a.state.run_command("tool.rectangle", 0);
    let mut h = harness(a);
    h.run_steps(2);
    assert!(h.query_by_label_contains("Snap:").is_none());
}

#[test]
fn a_row_with_a_material_preset_says_so() {
    let mut a = compiled_app();
    let cardstock = a
        .state
        .material_choices()
        .into_iter()
        .find(|m| m.name.contains("Cardstock"))
        .unwrap();
    a.state.set_mat_material(Some(&cardstock.reference), 1);
    a.state.tick(200);
    assert!(a.state.wait_for_compile(Duration::from_secs(60)));
    let mut h = harness(a);
    h.run();
    assert!(h.query_all_by_label("Preset").count() >= 1);
    h.get_by_label_contains("Cardstock 80 lb");
}

#[test]
fn the_machine_panel_has_no_switches_for_kinds_of_scan() {
    // Which buses to look at is each plugin's business; the app offers one way to look.
    let mut a = App::with_simulator(Settings::default(), "en");
    a.machine_open = true;
    let mut h = harness(a);
    h.run_steps(3);
    for label in [
        "Scan serial ports",
        "Scan USB devices",
        "Scan network",
        "Scan Bluetooth",
        "Also look for",
    ] {
        assert_eq!(h.query_all_by_label(label).count(), 0, "{label}");
    }
    h.get_by_label("Look again");
}

#[test]
fn the_export_panel_offers_the_ordering_mode_and_the_pen_check() {
    let mut a = compiled_app();
    a.state.run_command("mode.export", 0);
    let mut h = harness(a);
    h.run();
    h.get_by_label("Job options").click_accesskit();
    h.run_steps(3);
    h.get_by_label("Minimise tool changes");
    h.get_by_label("Follow strict order").click_accesskit();
    h.run_steps(3);
    assert_eq!(
        h.state().state().ordering(),
        ps_model::OrderingMode::StrictOrder
    );
    h.get_by_label("Draw a check mark before each pen")
        .click_accesskit();
    h.run_steps(3);
    assert!(h.state().state().pen_check());
}

#[test]
fn preview_mode_can_switch_between_the_material_view_and_the_toolpath() {
    let mut a = compiled_app();
    a.state.run_command("mode.preview", 0);
    let mut h = harness(a);
    h.run_steps(3);
    assert!(h.state().preview_material, "the realistic view comes first");
    h.get_by_label("Toolpath").click_accesskit();
    h.run_steps(2);
    assert!(!h.state().preview_material);
    h.get_by_label("Realistic").click_accesskit();
    h.run_steps(4);
    assert!(h.state().preview_material);
    assert!(h.state().preview_texture.is_some(), "the picture was built");
}

#[test]
fn the_material_view_picture_follows_the_timeline() {
    let mut a = compiled_app();
    a.state.run_command("mode.preview", 0);
    a.preview_material = true;
    a.preview_step = 0;
    let mut h = harness(a);
    h.run_steps(3);
    let first = h.state().preview_texture.as_ref().map(|t| t.key);
    h.state_mut().preview_step = 5;
    h.run_steps(3);
    let later = h.state().preview_texture.as_ref().map(|t| t.key);
    assert_ne!(first, later, "a new step rebuilds the picture");
}

#[test]
fn loaded_svg_stroke_fonts_are_offered_next_to_the_built_in_ones() {
    let mut a = text_app();
    let mut fonts = crate::fonts::FontLibrary::bundled_only();
    fonts
        .add_stroke_font(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><font horiz-adv-x="600"><font-face font-family="Mini Single" cap-height="700"/><glyph unicode="H" horiz-adv-x="800" d="M 100 0 L 100 700"/></font></svg>"#,
        )
        .unwrap();
    a.fonts = Some(fonts);
    a.text_params.stroke = Some(ps_import::StrokeFont::RomanSimplex);
    a.state_mut().run_command("text.open", 0);
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Mini Single").click_accesskit();
    h.run_steps(2);
    assert_eq!(
        h.state().text_params.svg_font.as_deref(),
        Some("Mini Single")
    );
    h.get_by_label("Roman simplex").click_accesskit();
    h.run_steps(2);
    assert_eq!(h.state().text_params.svg_font, None);
}

#[test]
fn a_declared_internet_host_is_offered_to_allow_and_then_to_block() {
    let dir = tempfile::tempdir().unwrap();
    {
        use ps_device::{DeviceFilter, PluginManifest, TransportKind, pack};
        let manifest = PluginManifest {
            id: "acme.cloud".to_owned(),
            name: "Acme Cloud".to_owned(),
            filters: vec![DeviceFilter {
                transport: TransportKind::Serial,
                usb: None,
            }],
            internet_hosts: vec!["api.acme.example".to_owned()],
        };
        let bytes = pack(&manifest, b"\0asm\x0d\0\x01\0", "1.0.0").unwrap();
        std::fs::write(dir.path().join("cloud.psdevice"), bytes).unwrap();
    }
    let mut a = compiled_app();
    a.machine = MachineController::new(false);
    a.machine.install_plugins(dir.path());
    a.machine_open = true;
    let mut h = shell_harness(a);
    h.run_steps(3);
    h.get_by_label("Allow api.acme.example").click_accesskit();
    h.run_steps(3);
    assert!(h.state().machine.declared_hosts()[0].allowed);
    h.get_by_label("Block api.acme.example").click_accesskit();
    h.run_steps(3);
    assert!(!h.state().machine.declared_hosts()[0].allowed);
}

#[test]
fn the_settings_window_calibrates_the_screen_from_a_measured_ruler() {
    let mut a = app();
    a.state_mut().run_command("settings.open", 0);
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Screen calibration");
    h.get_by_label("Measured length of the bar (mm)");
    h.get_by_label("Apply measurement").click_accesskit();
    h.run_steps(2);
    // The default measurement is the nominal 100 mm, so nothing changes.
    assert!((h.state().state().settings().screen_calibration - 1.0).abs() < 1e-12);
}

#[test]
fn applying_a_different_measurement_changes_the_calibration() {
    let mut a = app();
    a.screen_measured_mm = 95.0;
    a.state_mut().run_command("settings.open", 0);
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("Apply measurement").click_accesskit();
    h.run_steps(2);
    let c = h.state().state().settings().screen_calibration;
    assert!((c - 100.0 / 95.0).abs() < 1e-12, "{c}");
}

#[test]
fn the_material_view_offers_flat_tilted_and_weeded_pictures() {
    let mut a = compiled_app();
    a.state.run_command("mode.preview", 0);
    a.preview_material = true;
    let mut h = harness(a);
    h.run_steps(3);
    let flat = h.state().preview_texture.as_ref().map(|t| t.key);
    h.get_by_label("Tilt and orbit").click_accesskit();
    h.run_steps(4);
    assert!(matches!(
        h.state().preview_view,
        crate::PreviewView::Tilted { .. }
    ));
    let tilted = h.state().preview_texture.as_ref().map(|t| t.key);
    assert_ne!(flat, tilted, "a new view rebuilds the picture");
    // There are no number fields to type angles into: the picture itself is dragged.
    h.get_by_label("Drag the picture to turn it");
    assert!(h.query_by_label("Tilt (degrees)").is_none());
    h.get_by_label("Weeded").click_accesskit();
    h.run_steps(4);
    assert!(matches!(
        h.state().preview_view,
        crate::PreviewView::Weeded { .. }
    ));
    h.get_by_label("Lift (mm)");
    h.get_by_label("Flat").click_accesskit();
    h.run_steps(3);
    assert_eq!(h.state().preview_view, crate::PreviewView::Flat);
}

#[test]
fn changing_the_tilt_rebuilds_the_picture() {
    let mut a = compiled_app();
    a.state.run_command("mode.preview", 0);
    a.preview_material = true;
    a.preview_view = crate::PreviewView::Tilted {
        tilt_deg: 30.0,
        azimuth_deg: 0.0,
    };
    let mut h = harness(a);
    h.run_steps(3);
    let before = h.state().preview_texture.as_ref().map(|t| t.key);
    h.state_mut().preview_view = crate::PreviewView::Tilted {
        tilt_deg: 55.0,
        azimuth_deg: 20.0,
    };
    h.run_steps(3);
    let after = h.state().preview_texture.as_ref().map(|t| t.key);
    assert_ne!(before, after);
}

#[test]
fn the_tree_offers_a_roll_mat_only_for_machines_with_a_roll() {
    let mut a = app();
    a.state_mut().set_machine("generic.cutter-small");
    let mut h = harness(a);
    h.run_steps(3);
    h.get_by_label("New mat").click_accesskit();
    h.run_steps(3);
    assert!(h.query_by_label("Add roll mat").is_none());

    let mut b = app();
    b.state_mut().set_machine("generic.cutter-medium");
    let mut h = harness(b);
    h.run_steps(3);
    h.get_by_label("New mat").click_accesskit();
    h.run_steps(3);
    h.get_by_label("Add roll mat").click_accesskit();
    h.run_steps(3);
    assert_eq!(
        h.state().state().current_mat().mat_type,
        ps_model::MatType::Matless
    );
}

#[test]
fn a_roll_shows_how_much_of_it_the_content_uses() {
    let mut b = app();
    b.state_mut().set_machine("generic.cutter-medium");
    b.state_mut().add_matless_mat(0);
    draw_rect(&mut b, p(20.0, 400.0), p(80.0, 500.0));
    let mut h = harness(b);
    h.run_steps(3);
    h.get_by_label_contains("Used length: 500 mm of 3658 mm");
}

fn tool_entry(slot: u8, tool: &str, colour: Option<&str>) -> ps_job::ToolEntry {
    ps_job::ToolEntry {
        slot,
        tool: tool.parse().unwrap(),
        holder: ps_job::HolderId::new("clamp-b"),
        label: tool.to_owned(),
        pen: colour.map(|c| ps_job::PenVariant {
            colour: Some(c.to_owned()),
            tip_width_um: None,
        }),
    }
}

#[test]
fn the_toolpath_is_drawn_in_the_ink_the_pen_was_given_and_the_work_colour_otherwise() {
    use egui::Color32;
    let tools = [
        tool_entry(1, "pen.fineliner-black-0.4@1.0.0", Some("#D32F2F")),
        tool_entry(2, "pen.fineliner-black-0.4@1.0.0", None),
        tool_entry(3, "generic.scoring-stylus@1.0.0", None),
        tool_entry(4, "generic.drag-blade@1.0.0", None),
    ];
    let colour = |slot| extras::slot_colour(&tools, Some(slot));
    assert_eq!(
        colour(1),
        Color32::from_rgb(0xD3, 0x2F, 0x2F),
        "the chosen ink"
    );
    let [r, g, b] = colour_of(OpKind::Score);
    assert_eq!(
        colour(3),
        Color32::from_rgb(r, g, b),
        "scoring has its own colour"
    );
    let [r, g, b] = colour_of(OpKind::Cut);
    assert_eq!(
        colour(4),
        Color32::from_rgb(r, g, b),
        "cutting has its own colour"
    );
    assert_ne!(
        colour(2),
        colour(4),
        "a plain pen is not drawn in the cut colour"
    );
}

// ---- why there is no job ----

fn app_without_a_machine() -> App {
    let mut a = App::new(
        Settings {
            default_machine: Some("acme.mill".to_owned()),
            ..Settings::default()
        },
        "en",
    );
    a.preview_blocking = true;
    a
}

/// A project the chosen machine cannot run: a foil tool on a pen plotter.
fn blocked_app() -> App {
    let mut a = app();
    assert!(a.state.set_machine("generic.grbl-pen-plotter"));
    draw_rect(&mut a, p(20.0, 20.0), p(80.0, 60.0));
    assert!(
        a.state
            .assign_operation(OpKind::Foil, "generic.foil-transfer@1.0.0", 0)
    );
    a.state.tick(100);
    assert!(a.state.wait_for_compile(Duration::from_secs(60)));
    a
}

#[test]
fn without_a_machine_the_status_bar_and_the_preview_say_to_choose_one() {
    let mut a = app_without_a_machine();
    a.state.run_command("mode.preview", 0);
    let mut h = harness(a);
    h.run();
    assert!(
        h.query_all_by_label("Choose a machine (top bar) to prepare the job")
            .count()
            >= 1
    );
    assert!(
        h.query_by_label("Nothing to preview yet").is_none(),
        "the vague message is gone"
    );
}

#[test]
fn a_blocked_job_says_how_many_errors_and_never_shows_rendering() {
    let mut a = blocked_app();
    a.preview_material = true;
    a.state.run_command("mode.preview", 0);
    let mut h = harness(a);
    h.run();
    assert!(
        h.query_all_by_label_contains("blocks the job. See the Export tab")
            .count()
            >= 1,
        "the single error is named: {:?}",
        h.state().state.job_status()
    );
    assert!(
        h.query_by_label("Job out of date").is_none(),
        "a finished compile with errors is not out of date"
    );
    assert!(
        h.query_by_label("Rendering…").is_none(),
        "there is nothing to render, and the screen must not claim otherwise"
    );
}

// ---- start points ----

#[test]
fn a_shape_without_an_operation_shows_no_start_point_and_one_with_an_operation_does() {
    let mut a = app();
    draw_rect(&mut a, p(20.0, 20.0), p(80.0, 60.0));
    let first_path = |a: &App| {
        a.state
            .current_mat()
            .objects
            .values()
            .next()
            .expect("the rectangle")
            .paths[0]
            .clone()
    };
    assert!(
        !start_is_meaningful(&first_path(&a)),
        "nothing is done along an undecorated shape"
    );
    assert!(
        a.state
            .assign_operation(OpKind::Cut, "generic.drag-blade@1.0.0", 0)
    );
    assert!(start_is_meaningful(&first_path(&a)));
    let row = a.state.operation_rows()[0].id;
    assert!(a.state.remove_row(row, 10));
    assert!(
        !start_is_meaningful(&first_path(&a)),
        "deleting the operation takes the start point away"
    );
}

// ---- the automation (MCP) server in the settings ----

/// Clicks a widget in the settings window, scrolling it into view first: the window is a short
/// scrolling area and the automation section lies below its fold.
fn click_in_settings(h: &mut Harness<'static, App>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run_steps(3);
    h.get_by_label(label).click_accesskit();
    h.run_steps(3);
}

fn automation_app(status: crate::mcp::McpStatus) -> App {
    let mut a = settings_app();
    a.state.update_settings(|s| {
        s.automation.mcp_enabled = true;
        s.automation.mcp_token = "0123456789abcdef".to_owned();
    });
    a.set_mcp_status(status);
    a
}

#[test]
fn the_server_is_off_by_default_and_the_window_shows_nothing_else_about_it() {
    let mut h = harness(settings_app());
    h.run_steps(3);
    assert_eq!(
        h.get_by_label("Enable the MCP server")
            .accesskit_node()
            .toggled(),
        Some(egui::accesskit::Toggled::False)
    );
    assert!(h.query_by_label("Port").is_none());
    assert!(h.query_by_label("Access token:").is_none());
}

#[test]
fn turning_the_server_on_makes_a_token_and_shows_the_options() {
    let mut h = harness(settings_app());
    h.run_steps(3);
    click_in_settings(&mut h, "Enable the MCP server");
    let settings = h.state().state.settings().automation.clone();
    assert!(settings.mcp_enabled);
    assert_eq!(settings.mcp_token.len(), 64, "a token was made");
    h.get_by_label("Port");
    h.get_by_label("Access token:");
    h.get_by_label("Allow other computers on the network to connect");
}

#[test]
fn the_addresses_to_connect_to_are_listed_once_the_server_listens() {
    let status = crate::mcp::McpStatus::Listening {
        urls: vec![
            "http://192.168.1.20:8765/mcp".to_owned(),
            "http://127.0.0.1:8765/mcp".to_owned(),
        ],
    };
    let mut h = harness(automation_app(status));
    h.run_steps(3);
    h.get_by_label("http://192.168.1.20:8765/mcp");
    h.get_by_label("http://127.0.0.1:8765/mcp");
}

#[test]
fn a_server_that_could_not_start_says_why() {
    let status = crate::mcp::McpStatus::Failed("cannot listen on port 8765: in use".to_owned());
    let mut h = harness(automation_app(status));
    h.run_steps(3);
    h.get_by_label_contains("The server could not start: cannot listen on port 8765");
}

#[test]
fn the_token_is_hidden_until_the_user_asks_to_see_it() {
    let mut h = harness(automation_app(crate::mcp::McpStatus::Off));
    h.run_steps(3);
    assert!(
        h.query_by_label("0123456789abcdef").is_none(),
        "dots, not the token"
    );
    click_in_settings(&mut h, "Show token");
    h.get_by_label("0123456789abcdef");
    click_in_settings(&mut h, "Hide token");
    assert!(h.query_by_label("0123456789abcdef").is_none());
}

#[test]
fn a_new_token_replaces_the_old_one() {
    let mut h = harness(automation_app(crate::mcp::McpStatus::Off));
    h.run_steps(3);
    click_in_settings(&mut h, "Make a new token");
    let token = h.state().state.settings().automation.mcp_token.clone();
    assert_eq!(token.len(), 64);
    assert_ne!(token, "0123456789abcdef");
}

#[test]
fn allowing_the_network_warns_that_the_token_is_all_that_protects_the_app() {
    let mut h = harness(automation_app(crate::mcp::McpStatus::Off));
    h.run_steps(3);
    assert!(
        h.query_by_label_contains("Anyone on your network")
            .is_none(),
        "no warning while it is local only"
    );
    click_in_settings(&mut h, "Allow other computers on the network to connect");
    assert!(h.state().state.settings().automation.mcp_allow_network);
    h.get_by_label_contains("Anyone on your network");
}

#[test]
fn moving_the_machine_is_a_separate_choice_that_is_off_until_the_user_makes_it() {
    let mut h = harness(automation_app(crate::mcp::McpStatus::Off));
    h.run_steps(3);
    assert!(!h.state().api_armed());
    click_in_settings(
        &mut h,
        "Let the API move the machine until the app is closed",
    );
    assert!(h.state().api_armed());
}
