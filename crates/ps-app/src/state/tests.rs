//! Ground truth for the app state: panels send commands, the state applies them
//! to the document store, and a changed document schedules a debounced compile. The UI is a thin
//! layer over this, so behaviour is tested here without a window.

use ps_core::MatId;
use ps_geometry::{Point, Rect};
use ps_library::Settings;
use ps_model::{Document, GuideAxis, OpKind, ProjectMeta};
use ps_render::SnapKind;
use std::time::Duration;

use super::*;
use crate::arrange::{AlignKind, Anchor, Axis};
use crate::commands::{AssignError, Mods, Shortcut};
use crate::tools::{CanvasEvent, ToolId};

const NONE: Mods = Mods {
    command: false,
    shift: false,
    alt: false,
};

fn state() -> AppState {
    AppState::new(Settings::default())
}

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn objects(s: &AppState) -> usize {
    s.current_mat().objects.len()
}

/// Draws a rectangle with the rectangle tool.
fn draw_rect(s: &mut AppState, from: Point, to: Point, now: u64) {
    s.run_command("tool.rectangle", now);
    s.canvas_event(CanvasEvent::Press(from), NONE, now);
    s.canvas_event(CanvasEvent::Drag(to), NONE, now);
    s.canvas_event(CanvasEvent::Release(to), NONE, now);
}

#[test]
fn a_new_state_has_one_empty_mat_and_the_select_tool() {
    let s = state();
    assert_eq!(s.document().mats.len(), 1);
    assert_eq!(objects(&s), 0);
    assert_eq!(s.tool_id(), ToolId::Select);
    assert_eq!(s.mode(), Mode::Prepare);
    assert!(s.selection().is_empty());
    assert!(!s.can_undo());
    assert!(!s.is_dirty());
}

#[test]
fn the_mat_is_twelve_inches_square_by_default() {
    let m = state().current_mat();
    assert!((m.size.width.get() - 304.8).abs() < 1e-9);
    assert!((m.size.height.get() - 304.8).abs() < 1e-9);
}

#[test]
fn tool_commands_switch_the_active_tool() {
    let mut s = state();
    s.run_command("tool.rectangle", 0);
    assert_eq!(s.tool_id(), ToolId::Rectangle);
    s.run_command("tool.ellipse", 0);
    assert_eq!(s.tool_id(), ToolId::Ellipse);
    s.run_command("tool.select", 0);
    assert_eq!(s.tool_id(), ToolId::Select);
}

#[test]
fn drawing_adds_an_object_selects_it_and_marks_the_project_changed() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(40.0, 30.0), 100);
    assert_eq!(objects(&s), 1);
    assert_eq!(s.selection().len(), 1);
    assert!(s.can_undo());
    assert!(s.is_dirty());
    let id = s.selection().objects()[0];
    assert!(s.current_mat().objects.contains_key(&id));
}

#[test]
fn undo_removes_the_object_and_the_selection_forgets_it() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(40.0, 30.0), 100);
    s.run_command("edit.undo", 200);
    assert_eq!(objects(&s), 0);
    assert!(s.selection().is_empty());
    assert!(s.can_redo());
    s.run_command("edit.redo", 300);
    assert_eq!(objects(&s), 1);
}

#[test]
fn delete_removes_the_selection_in_one_undo_step() {
    let mut s = state();
    draw_rect(&mut s, p(0.0, 0.0), p(10.0, 10.0), 0);
    draw_rect(&mut s, p(50.0, 50.0), p(60.0, 60.0), 0);
    s.run_command("edit.select-all", 0);
    assert_eq!(s.selection().objects().len(), 2);
    s.run_command("edit.delete", 0);
    assert_eq!(objects(&s), 0);
    assert!(s.selection().is_empty());
    s.run_command("edit.undo", 0);
    assert_eq!(objects(&s), 2, "one undo brings both back");
}

#[test]
fn duplicate_copies_the_selection_offset_by_five_millimetres_and_selects_the_copy() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(30.0, 30.0), 0);
    let original = s.selection().objects()[0];
    s.run_command("edit.duplicate", 0);
    assert_eq!(objects(&s), 2);
    let copy = s.selection().objects()[0];
    assert_ne!(copy, original);
    let mat = s.current_mat();
    let (a, b) = (
        mat.objects[&original].transform.as_coeffs(),
        mat.objects[&copy].transform.as_coeffs(),
    );
    assert!((b[4] - a[4] - 5.0).abs() < 1e-9 && (b[5] - a[5] - 5.0).abs() < 1e-9);
}

#[test]
fn duplicate_is_one_undo_step() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(30.0, 30.0), 0);
    s.run_command("edit.duplicate", 0);
    s.run_command("edit.undo", 0);
    assert_eq!(objects(&s), 1);
}

#[test]
fn select_all_selects_every_object_on_the_mat() {
    let mut s = state();
    draw_rect(&mut s, p(0.0, 0.0), p(10.0, 10.0), 0);
    draw_rect(&mut s, p(50.0, 50.0), p(60.0, 60.0), 0);
    s.run_command("edit.select-all", 0);
    assert_eq!(s.selection().objects().len(), 2);
}

#[test]
fn nudging_moves_the_selection_and_shift_moves_it_ten_times_as_far() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(30.0, 30.0), 0);
    let id = s.selection().objects()[0];
    let x = |s: &AppState| s.current_mat().objects[&id].transform.as_coeffs()[4];
    let before = x(&s);
    s.run_command("nudge.right", 0);
    assert!((x(&s) - before - 1.0).abs() < 1e-9);
    s.run_command("nudge.right-big", 0);
    assert!((x(&s) - before - 11.0).abs() < 1e-9);
    s.run_command("nudge.left", 0);
    assert!((x(&s) - before - 10.0).abs() < 1e-9);
}

#[test]
fn nudging_with_nothing_selected_does_nothing() {
    let mut s = state();
    let effect = s.run_command("nudge.right", 0);
    assert_eq!(effect, Effect::None);
    assert!(!s.can_undo());
}

#[test]
fn commands_that_cannot_run_are_ignored() {
    let mut s = state();
    assert_eq!(s.run_command("edit.delete", 0), Effect::None);
    assert_eq!(s.run_command("edit.undo", 0), Effect::None);
    assert!(!s.is_dirty());
}

#[test]
fn an_unknown_command_is_ignored() {
    let mut s = state();
    assert_eq!(s.run_command("no.such.command", 0), Effect::None);
}

#[test]
fn mode_commands_switch_the_mode_and_keep_the_selection() {
    let mut s = state();
    draw_rect(&mut s, p(0.0, 0.0), p(10.0, 10.0), 0);
    s.run_command("mode.preview", 0);
    assert_eq!(s.mode(), Mode::Preview);
    s.run_command("mode.export", 0);
    assert_eq!(s.mode(), Mode::Export);
    s.run_command("mode.prepare", 0);
    assert_eq!(s.mode(), Mode::Prepare);
    assert_eq!(
        s.selection().len(),
        1,
        "the three modes share one selection"
    );
}

#[test]
fn zoom_commands_change_the_viewport() {
    let mut s = state();
    let z = s.viewport().zoom();
    s.run_command("view.zoom-in", 0);
    assert!(s.viewport().zoom() > z);
    s.run_command("view.zoom-out", 0);
    s.run_command("view.zoom-out", 0);
    assert!(s.viewport().zoom() < z);
}

#[test]
fn zoom_to_fit_shows_the_whole_mat() {
    let mut s = state();
    s.resize_canvas(800.0, 600.0);
    s.run_command("view.zoom-fit", 0);
    let v = s.visible_rect();
    assert!(
        v.x0 <= 0.0 && v.y0 <= 0.0 && v.x1 >= 304.8 && v.y1 >= 304.8,
        "{v:?}"
    );
}

#[test]
fn true_scale_uses_the_displays_dpi() {
    let mut s = state();
    s.set_display_dpi(96.0);
    s.run_command("view.true-scale", 0);
    assert!((s.viewport().zoom() - 96.0 / 25.4).abs() < 1e-9);
}

#[test]
fn the_palette_command_toggles_the_palette() {
    let mut s = state();
    assert!(!s.palette_open());
    s.run_command("palette.open", 0);
    assert!(s.palette_open());
    s.close_palette();
    assert!(!s.palette_open());
}

#[test]
fn file_commands_ask_the_ui_to_do_the_work() {
    let mut s = state();
    assert_eq!(s.run_command("file.new", 0), Effect::NewProject);
    assert_eq!(s.run_command("file.open", 0), Effect::OpenProject);
    draw_rect(&mut s, p(0.0, 0.0), p(10.0, 10.0), 0);
    assert_eq!(s.run_command("file.save", 0), Effect::Save);
    assert_eq!(s.run_command("file.save-as", 0), Effect::SaveAs);
}

#[test]
fn export_and_send_need_a_compiled_job() {
    let mut s = state();
    assert_eq!(s.run_command("file.export", 0), Effect::None);
    s.set_compile_ready(true);
    assert_eq!(s.run_command("file.export", 0), Effect::Export);
    assert_eq!(
        s.run_command("machine.send", 0),
        Effect::None,
        "no machine ready"
    );
    s.set_device_ready(true);
    assert_eq!(s.run_command("machine.send", 0), Effect::SendToMachine);
}

#[test]
fn marking_the_project_saved_clears_the_changed_flag() {
    let mut s = state();
    draw_rect(&mut s, p(0.0, 0.0), p(10.0, 10.0), 0);
    assert!(s.is_dirty());
    s.mark_saved();
    assert!(!s.is_dirty());
    s.run_command("edit.undo", 0);
    assert!(s.is_dirty(), "undoing past the save point changes it again");
}

#[test]
fn an_edit_schedules_a_compile_sixty_milliseconds_later() {
    let mut s = state();
    draw_rect(&mut s, p(0.0, 0.0), p(10.0, 10.0), 1_000);
    assert!(!s.take_compile_due(1_059));
    assert!(s.take_compile_due(1_060));
    assert!(!s.take_compile_due(2_000), "it fires once");
}

#[test]
fn selecting_does_not_schedule_a_compile_or_change_the_document() {
    let mut s = state();
    draw_rect(&mut s, p(0.0, 0.0), p(20.0, 20.0), 0);
    s.take_compile_due(1_000);
    s.mark_saved();
    s.run_command("edit.select-all", 2_000);
    assert!(!s.take_compile_due(3_000));
    assert!(!s.is_dirty());
}

#[test]
fn clicking_an_object_with_the_select_tool_selects_it_and_dragging_moves_it() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(40.0, 40.0), 0);
    let id = s.selection().objects()[0];
    s.run_command("tool.select", 0);
    s.canvas_event(CanvasEvent::Press(p(200.0, 200.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(p(200.0, 200.0)), NONE, 0);
    assert!(s.selection().is_empty(), "clicking empty space clears");
    s.canvas_event(CanvasEvent::Press(p(25.0, 25.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(p(25.0, 25.0)), NONE, 0);
    assert_eq!(s.selection().objects(), vec![id]);
    let before = s.current_mat().objects[&id].transform.as_coeffs()[4];
    s.canvas_event(CanvasEvent::Press(p(25.0, 25.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Drag(p(45.0, 25.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(p(45.0, 25.0)), NONE, 0);
    let after = s.current_mat().objects[&id].transform.as_coeffs()[4];
    assert!((after - before - 20.0).abs() < 1e-9);
}

#[test]
fn a_drag_in_progress_shows_the_tools_overlay() {
    let mut s = state();
    s.run_command("tool.rectangle", 0);
    assert_eq!(s.overlay().len(), 0);
    s.canvas_event(CanvasEvent::Press(p(0.0, 0.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Drag(p(10.0, 10.0)), NONE, 0);
    assert_eq!(s.overlay().len(), 1);
    s.canvas_event(CanvasEvent::Escape, NONE, 0);
    assert_eq!(s.overlay().len(), 0);
    assert_eq!(objects(&s), 0);
}

#[test]
fn a_key_press_runs_the_command_bound_to_it() {
    let mut s = state();
    assert!(s.handle_key(NONE, "R", 0));
    assert_eq!(s.tool_id(), ToolId::Rectangle);
    assert!(s.handle_key(NONE, "E", 0));
    assert_eq!(s.tool_id(), ToolId::Ellipse);
    assert!(!s.handle_key(NONE, "Q", 0), "nothing is bound to Q");
}

#[test]
fn a_rebound_shortcut_is_used() {
    let mut settings = Settings::default();
    settings
        .shortcuts
        .insert("tool.rectangle".to_owned(), "Q".to_owned());
    let mut s = AppState::new(settings);
    assert!(s.handle_key(NONE, "Q", 0));
    assert_eq!(s.tool_id(), ToolId::Rectangle);
    assert!(!s.handle_key(NONE, "R", 0), "R is free now");
}

#[test]
fn shortcuts_are_ignored_while_the_palette_is_open() {
    let mut s = state();
    s.run_command("palette.open", 0);
    assert!(!s.handle_key(NONE, "R", 0), "typing goes to the search box");
    assert_eq!(s.tool_id(), ToolId::Select);
}

#[test]
fn the_context_reflects_the_state() {
    let mut s = state();
    let c = s.context();
    assert!(!c.has_selection && !c.can_undo && !c.can_redo);
    draw_rect(&mut s, p(0.0, 0.0), p(10.0, 10.0), 0);
    let c = s.context();
    assert!(c.has_selection && c.can_undo && c.has_project);
}

#[test]
fn the_warning_count_comes_from_the_validation_pass() {
    let mut s = state();
    // An object wholly outside the mat is a layout problem.
    draw_rect(&mut s, p(400.0, 400.0), p(450.0, 450.0), 0);
    assert!(s.warning_count() >= 1);
}

#[test]
fn replacing_the_document_resets_history_and_selection() {
    let mut s = state();
    draw_rect(&mut s, p(0.0, 0.0), p(10.0, 10.0), 0);
    s.load_document(Document::new(ProjectMeta::new(
        "Other",
        "2026-01-01T00:00:00Z",
        "0.1.0",
    )));
    assert!(!s.can_undo());
    assert!(s.selection().is_empty());
    assert!(!s.is_dirty());
    assert_eq!(
        s.document().mats.len(),
        1,
        "a document with no mat gets a default one, so there is always something to draw on"
    );
    assert!(!s.can_undo(), "adding it is not an undo step");
}

// ---- operations and the background compile ----

const BLADE: &str = "generic.drag-blade@1.0.0";

#[test]
fn assigning_an_operation_puts_it_on_every_path_of_the_selection_in_one_undo_step() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(40.0, 40.0), 0);
    assert!(s.assign_operation(OpKind::Cut, BLADE, 0));
    let mat = s.current_mat();
    let object = mat.objects.values().next().unwrap();
    assert_eq!(object.paths[0].ops.len(), 1);
    assert_eq!(object.paths[0].ops[0].kind, OpKind::Cut);
    assert!(object.paths[0].ops[0].enabled);
    s.run_command("edit.undo", 0);
    let mat = s.current_mat();
    assert_eq!(mat.objects.values().next().unwrap().paths[0].ops.len(), 0);
}

#[test]
fn assigning_needs_a_selection_and_a_valid_tool_reference() {
    let mut s = state();
    assert!(
        !s.assign_operation(OpKind::Cut, BLADE, 0),
        "nothing selected"
    );
    draw_rect(&mut s, p(10.0, 10.0), p(40.0, 40.0), 0);
    assert!(!s.assign_operation(OpKind::Cut, "not a reference", 0));
    assert_eq!(
        s.current_mat().objects.values().next().unwrap().paths[0]
            .ops
            .len(),
        0
    );
}

#[test]
fn an_edit_compiles_in_the_background_and_the_result_arrives() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(60.0, 50.0), 0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    assert!(s.compiled().is_none(), "nothing compiled yet");
    s.tick(10);
    assert!(s.compiled().is_none(), "not due before the debounce passes");
    s.tick(100);
    assert!(
        s.wait_for_compile(Duration::from_secs(60)),
        "a result arrived"
    );
    let c = s.compiled().expect("a compiled job");
    assert_eq!(c.job.mats.len(), 1);
    assert!(!c.job.is_blocked());
    assert!(s.context().has_compile, "export is now available");
    let summary = s.compile_summary().unwrap();
    assert_eq!(summary.mats, 1);
    assert!(summary.machine_time_ms > 0);
}

#[test]
fn a_document_with_no_operations_has_nothing_to_export() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(60.0, 50.0), 0);
    s.tick(100);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    assert_eq!(s.compiled().unwrap().job.mats.len(), 0);
    assert!(!s.context().has_compile, "no operations, no export");
}

#[test]
fn an_edit_after_a_compile_makes_the_old_result_unavailable_until_the_next_one() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(60.0, 50.0), 0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    s.tick(100);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    assert!(s.context().has_compile);
    s.run_command("nudge.right", 200);
    assert!(
        !s.context().has_compile,
        "the job no longer matches the document"
    );
    s.tick(300);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    assert!(s.context().has_compile);
}

#[test]
fn compiling_does_not_block_the_caller() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(60.0, 50.0), 0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    // tick only starts the work and collects what is already finished.
    s.tick(100);
    s.tick(101);
    assert!(s.compile_running() || s.compiled().is_some());
    s.wait_for_compile(Duration::from_secs(60));
    assert!(!s.compile_running());
}

// ---- settings ----

#[test]
fn changing_a_setting_marks_the_settings_changed_once() {
    let mut s = state();
    assert!(!s.take_settings_changed());
    s.update_settings(|settings| settings.keep_backup = false);
    assert!(!s.settings().keep_backup);
    assert!(s.take_settings_changed());
    assert!(!s.take_settings_changed(), "reading clears it");
}

#[test]
fn an_unchanged_setting_is_not_a_change() {
    let mut s = state();
    s.update_settings(|settings| settings.keep_backup = true);
    assert!(!s.take_settings_changed());
}

#[test]
fn rebinding_a_shortcut_updates_the_registry_and_the_saved_overrides() {
    let mut s = state();
    let q = Shortcut::parse("Q").unwrap();
    assert_eq!(s.assign_shortcut("tool.rectangle", Some(q.clone())), Ok(()));
    assert!(s.handle_key(NONE, "Q", 0));
    assert_eq!(s.tool_id(), ToolId::Rectangle);
    assert_eq!(s.settings().shortcuts["tool.rectangle"], "Q");
    assert!(s.take_settings_changed());
}

#[test]
fn a_conflicting_shortcut_is_refused_and_nothing_changes() {
    let mut s = state();
    let e = Shortcut::parse("E").unwrap();
    assert_eq!(
        s.assign_shortcut("tool.rectangle", Some(e)),
        Err(AssignError::Conflict {
            with: "tool.ellipse"
        })
    );
    assert!(s.settings().shortcuts.is_empty());
    assert!(!s.take_settings_changed());
}

#[test]
fn resetting_a_shortcut_removes_its_override() {
    let mut s = state();
    s.assign_shortcut("tool.rectangle", Shortcut::parse("Q"))
        .unwrap();
    s.reset_shortcut("tool.rectangle");
    assert!(s.settings().shortcuts.is_empty());
    assert!(s.handle_key(NONE, "R", 0));
}

#[test]
fn the_settings_command_opens_the_settings_window_state() {
    let mut s = state();
    assert!(!s.settings_open());
    s.run_command("settings.open", 0);
    assert!(s.settings_open());
    s.run_command("settings.open", 0);
    assert!(!s.settings_open(), "it toggles");
}

// ---- arranging the selection ----

fn bounds_of_selection(s: &AppState) -> Rect {
    s.selection_bounds().expect("something is selected")
}

fn two_squares() -> AppState {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(30.0, 30.0), 0);
    draw_rect(&mut s, p(60.0, 40.0), p(90.0, 60.0), 0);
    s.run_command("edit.select-all", 0);
    s
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn the_selection_bounds_cover_every_selected_object() {
    let s = two_squares();
    let b = bounds_of_selection(&s);
    assert!(
        near(b.x0, 10.0) && near(b.y0, 10.0) && near(b.x1, 90.0) && near(b.y1, 60.0),
        "{b:?}"
    );
    assert!(state().selection_bounds().is_none());
}

#[test]
fn numeric_entry_moves_the_chosen_anchor_to_the_typed_position() {
    let mut s = two_squares();
    assert!(s.move_selection_to(Anchor::TopLeft, 100.0, 50.0, 1));
    let b = bounds_of_selection(&s);
    assert!(near(b.x0, 100.0) && near(b.y0, 50.0), "{b:?}");
    assert!(
        near(b.width(), 80.0) && near(b.height(), 50.0),
        "the arrangement is kept"
    );
    s.run_command("edit.undo", 2);
    assert!(near(bounds_of_selection(&s).x0, 10.0), "one undo step");
}

#[test]
fn numeric_entry_resizes_about_the_chosen_anchor() {
    let mut s = two_squares();
    assert!(s.resize_selection(Anchor::BottomRight, 40.0, 25.0, false, 1));
    let b = bounds_of_selection(&s);
    assert!(near(b.width(), 40.0) && near(b.height(), 25.0), "{b:?}");
    assert!(
        near(b.x1, 90.0) && near(b.y1, 60.0),
        "the anchor stayed put: {b:?}"
    );
}

#[test]
fn a_nonsense_size_changes_nothing() {
    let mut s = two_squares();
    let before = s.generation();
    assert!(!s.resize_selection(Anchor::Centre, 0.0, 10.0, false, 1));
    assert_eq!(s.generation(), before);
}

#[test]
fn rotating_a_quarter_turn_swaps_width_and_height() {
    let mut s = two_squares();
    assert!(s.rotate_selection(90.0, 1));
    let b = bounds_of_selection(&s);
    assert!(near(b.width(), 50.0) && near(b.height(), 80.0), "{b:?}");
}

#[test]
fn mirroring_keeps_the_bounds() {
    let mut s = two_squares();
    let before = bounds_of_selection(&s);
    assert!(s.mirror_selection(Axis::Horizontal, 1));
    let after = bounds_of_selection(&s);
    assert!(near(before.x0, after.x0) && near(before.x1, after.x1));
}

#[test]
fn aligning_lines_objects_up_with_the_selection_edge() {
    let mut s = two_squares();
    assert!(s.align_selection(AlignKind::Left, 1));
    let mat = s.current_mat();
    for object in mat.objects.values() {
        let b = object.paths[0].geom.transformed(object.transform).bounds();
        assert!(near(b.x0, 10.0), "{b:?}");
    }
}

#[test]
fn distributing_needs_three_objects() {
    let mut s = two_squares();
    assert!(!s.distribute_selection(Axis::Horizontal, 1));
    draw_rect(&mut s, p(200.0, 10.0), p(210.0, 20.0), 0);
    s.run_command("edit.select-all", 0);
    assert!(s.distribute_selection(Axis::Horizontal, 2));
}

#[test]
fn a_grid_array_adds_the_copies_in_one_undo_step_and_selects_everything() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(20.0, 20.0), 0);
    assert!(s.grid_array_selection(3, 2, 30.0, 25.0, 1));
    assert_eq!(objects(&s), 6);
    assert_eq!(s.selection().objects().len(), 6);
    s.run_command("edit.undo", 2);
    assert_eq!(objects(&s), 1);
}

#[test]
fn a_circular_array_adds_evenly_spaced_copies_about_the_selection_centre() {
    let mut s = state();
    draw_rect(&mut s, p(100.0, 100.0), p(110.0, 110.0), 0);
    assert!(s.circular_array_selection(4, 360.0, 1));
    assert_eq!(objects(&s), 4);
}

#[test]
fn arrange_commands_are_available_from_the_keyboard_and_the_palette() {
    let mut s = two_squares();
    for id in [
        "arrange.align-left",
        "arrange.align-right",
        "arrange.align-top",
        "arrange.align-bottom",
        "arrange.align-centre-h",
        "arrange.align-centre-v",
        "arrange.distribute-h",
        "arrange.distribute-v",
        "arrange.mirror-h",
        "arrange.mirror-v",
        "arrange.rotate-cw",
        "arrange.rotate-ccw",
    ] {
        assert!(s.registry().get(id).is_some(), "{id}");
        s.run_command(id, 1);
    }
    assert!(s.can_undo());
}

#[test]
fn rotating_by_command_turns_a_quarter() {
    let mut s = two_squares();
    s.run_command("arrange.rotate-cw", 1);
    let b = bounds_of_selection(&s);
    assert!(near(b.width(), 50.0) && near(b.height(), 80.0), "{b:?}");
}

// ---- mats, materials and the machine profile ----

#[test]
fn adding_a_mat_creates_a_numbered_twelve_inch_mat_and_switches_to_it() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(30.0, 30.0), 0);
    assert!(s.add_mat(1));
    assert_eq!(s.document().mats.len(), 2);
    assert_eq!(s.current_mat().name, "Mat 2");
    assert_eq!(objects(&s), 0, "the new mat is empty");
    assert!(
        s.selection().is_empty(),
        "selection belongs to the mat that was left"
    );
    assert!((s.current_mat().size.width.get() - 304.8).abs() < 1e-9);
}

#[test]
fn adding_a_mat_is_one_undo_step_and_undo_returns_to_a_mat_that_exists() {
    let mut s = state();
    s.add_mat(1);
    s.run_command("edit.undo", 2);
    assert_eq!(s.document().mats.len(), 1);
    assert_eq!(s.current_mat().name, "Mat 1");
}

#[test]
fn switching_mats_changes_what_the_canvas_edits() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(30.0, 30.0), 0);
    let first = s.current_mat().id;
    s.add_mat(1);
    let second = s.current_mat().id;
    draw_rect(&mut s, p(10.0, 10.0), p(30.0, 30.0), 2);
    assert_eq!(objects(&s), 1);
    s.set_current_mat(first);
    assert_eq!(s.current_mat().id, first);
    assert_eq!(objects(&s), 1);
    s.set_current_mat(second);
    assert_eq!(s.current_mat().id, second);
    // An unknown mat is ignored.
    s.set_current_mat(MatId::new());
    assert_eq!(s.current_mat().id, second);
}

#[test]
fn duplicating_a_mat_copies_its_objects_under_new_ids() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(30.0, 30.0), 0);
    s.assign_operation(OpKind::Cut, "generic.drag-blade@1.0.0", 0);
    let original = s.current_mat();
    assert!(s.duplicate_mat(1));
    let copy = s.current_mat();
    assert_ne!(copy.id, original.id);
    assert_eq!(copy.name, "Mat 1 (copy)");
    assert_eq!(copy.objects.len(), 1);
    let (a, b) = (
        original.objects.values().next().unwrap(),
        copy.objects.values().next().unwrap(),
    );
    assert_ne!(a.id, b.id);
    assert_eq!(b.paths[0].ops.len(), 1, "operations come along");
    assert_eq!(s.document().mats.len(), 2);
    s.run_command("edit.undo", 2);
    assert_eq!(s.document().mats.len(), 1);
}

#[test]
fn the_last_mat_cannot_be_removed() {
    let mut s = state();
    assert!(!s.remove_current_mat(1));
    assert_eq!(s.document().mats.len(), 1);
}

#[test]
fn removing_a_mat_switches_to_a_remaining_one() {
    let mut s = state();
    let first = s.current_mat().id;
    s.add_mat(1);
    assert!(s.remove_current_mat(2));
    assert_eq!(s.document().mats.len(), 1);
    assert_eq!(s.current_mat().id, first);
}

#[test]
fn the_mat_list_names_every_mat_with_its_object_count() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(30.0, 30.0), 0);
    s.add_mat(1);
    let list = s.mat_summaries();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].name, "Mat 1");
    assert_eq!(list[0].objects, 1);
    assert_eq!(list[1].name, "Mat 2");
    assert!(list[1].current && !list[0].current);
}

#[test]
fn mat_commands_are_available_by_id() {
    let mut s = state();
    s.run_command("mat.add", 1);
    assert_eq!(s.document().mats.len(), 2);
    s.run_command("mat.duplicate", 2);
    assert_eq!(s.document().mats.len(), 3);
    s.run_command("mat.remove", 3);
    assert_eq!(s.document().mats.len(), 2);
}

#[test]
fn the_library_offers_materials_and_a_material_can_be_set_on_the_mat() {
    let mut s = state();
    let choices = s.material_choices();
    assert!(!choices.is_empty(), "the built-in library has materials");
    assert!(
        choices
            .iter()
            .all(|m| !m.name.is_empty() && m.reference.contains('@'))
    );
    assert!(s.set_mat_material(Some(&choices[0].reference), 1));
    assert_eq!(
        s.current_material_reference().as_deref(),
        Some(choices[0].reference.as_str())
    );
    assert!(s.set_mat_material(None, 2));
    assert!(s.current_material_reference().is_none());
    assert!(!s.set_mat_material(Some("junk"), 3), "not a reference");
}

#[test]
fn the_machine_profile_can_be_chosen_and_is_remembered_in_the_settings() {
    let mut s = state();
    let machines = s.machine_choices();
    assert_ne!(machines.len(), 0);
    assert_eq!(
        s.current_machine_id().as_deref(),
        Some("generic.cutter-small")
    );
    let other = machines.iter().find(|m| m.id != "generic.cutter-small");
    if let Some(other) = other {
        assert!(s.set_machine(&other.id));
        assert_eq!(s.current_machine_id().as_deref(), Some(other.id.as_str()));
        assert_eq!(
            s.settings().default_machine.as_deref(),
            Some(other.id.as_str())
        );
        assert!(s.take_settings_changed());
    }
    assert!(!s.set_machine("no.such.machine"));
}

#[test]
fn changing_the_machine_recompiles() {
    let mut s = state();
    let machines = s.machine_choices();
    if let Some(other) = machines.iter().find(|m| m.id != "generic.cutter-small") {
        draw_rect(&mut s, p(10.0, 10.0), p(30.0, 30.0), 0);
        s.take_compile_due(1_000);
        s.set_machine(&other.id);
        assert!(s.take_compile_due(1_000), "a compile is scheduled");
    }
}

// ---- tool settings ----

#[test]
fn the_polygon_tool_uses_the_chosen_number_of_sides() {
    let mut s = state();
    s.set_tool_sides(8);
    assert_eq!(s.tool_sides(), 8);
    s.run_command("tool.polygon", 0);
    s.canvas_event(CanvasEvent::Press(p(50.0, 50.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(p(60.0, 50.0)), NONE, 0);
    let mat = s.current_mat();
    let object = mat.objects.values().next().unwrap();
    assert_eq!(ps_geometry::node_count(&object.paths[0].geom), 8);
}

#[test]
fn the_side_count_and_star_ratio_are_kept_within_sensible_limits() {
    let mut s = state();
    s.set_tool_sides(1);
    assert_eq!(s.tool_sides(), 3);
    s.set_tool_sides(500);
    assert_eq!(s.tool_sides(), 32);
    s.set_star_ratio(0.0);
    assert!((s.star_ratio() - 0.05).abs() < 1e-12);
    s.set_star_ratio(5.0);
    assert!((s.star_ratio() - 0.95).abs() < 1e-12);
}

#[test]
fn the_pen_commits_on_enter_through_the_key_handler() {
    let mut s = state();
    s.run_command("tool.pen", 0);
    for at in [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)] {
        s.canvas_event(CanvasEvent::Press(at), NONE, 0);
        s.canvas_event(CanvasEvent::Release(at), NONE, 0);
    }
    s.canvas_event(CanvasEvent::Enter, NONE, 0);
    assert_eq!(objects(&s), 1);
    assert_eq!(s.selection().len(), 1);
}

// ---- node commands ----

/// A three-node open path drawn with the pen, then the node tool with node 1 selected.
fn with_selected_node() -> AppState {
    let mut s = state();
    s.run_command("tool.pen", 0);
    for at in [p(0.0, 0.0), p(20.0, 0.0), p(20.0, 20.0)] {
        s.canvas_event(CanvasEvent::Press(at), NONE, 0);
        s.canvas_event(CanvasEvent::Release(at), NONE, 0);
    }
    s.canvas_event(CanvasEvent::Enter, NONE, 0);
    s.run_command("tool.node", 0);
    s.canvas_event(CanvasEvent::Press(p(20.0, 0.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(p(20.0, 0.0)), NONE, 0);
    s
}

fn the_path(s: &AppState) -> ps_geometry::Path {
    s.current_mat().objects.values().next().unwrap().paths[0]
        .geom
        .clone()
}

#[test]
fn clicking_a_node_with_the_node_tool_selects_that_node() {
    let s = with_selected_node();
    let item = s.selection().items().next().unwrap();
    assert_eq!(item.node, Some(1));
    assert!(item.path.is_some());
}

#[test]
fn dragging_a_node_moves_it_as_one_undo_step() {
    let mut s = with_selected_node();
    s.canvas_event(CanvasEvent::Press(p(20.0, 0.0)), NONE, 5);
    s.canvas_event(CanvasEvent::Drag(p(30.0, 8.0)), NONE, 5);
    s.canvas_event(CanvasEvent::Release(p(30.0, 8.0)), NONE, 5);
    let moved = ps_geometry::node_position(&the_path(&s), 1).unwrap();
    assert!(
        (moved.x - 30.0).abs() < 1e-6 && (moved.y - 8.0).abs() < 1e-6,
        "{moved:?}"
    );
    s.run_command("edit.undo", 6);
    let back = ps_geometry::node_position(&the_path(&s), 1).unwrap();
    assert!((back.x - 20.0).abs() < 1e-6 && back.y.abs() < 1e-6);
}

#[test]
fn deleting_the_selected_node_joins_its_neighbours() {
    let mut s = with_selected_node();
    s.run_command("node.delete", 1);
    assert_eq!(ps_geometry::node_count(&the_path(&s)), 2);
}

#[test]
fn the_selected_nodes_following_segment_can_become_a_curve_and_a_line_again() {
    let mut s = with_selected_node();
    s.run_command("node.to-curve", 1);
    assert!(matches!(
        the_path(&s).segs[1],
        ps_geometry::Seg::Cubic { .. }
    ));
    s.run_command("node.to-line", 2);
    assert!(matches!(
        the_path(&s).segs[1],
        ps_geometry::Seg::Line { .. }
    ));
}

#[test]
fn smoothing_and_cornering_the_selected_node_change_its_handles() {
    let mut s = with_selected_node();
    s.run_command("node.smooth", 1);
    assert!(matches!(
        the_path(&s).segs[0],
        ps_geometry::Seg::Cubic { .. }
    ));
    s.run_command("node.corner", 2);
    let ps_geometry::Seg::Cubic { c2, to, .. } = the_path(&s).segs[0] else {
        panic!();
    };
    assert!((c2.x - to.x).abs() < 1e-9 && (c2.y - to.y).abs() < 1e-9);
}

#[test]
fn node_commands_do_nothing_without_a_selected_node() {
    let mut s = state();
    draw_rect(&mut s, p(0.0, 0.0), p(10.0, 10.0), 0);
    let before = s.generation();
    s.run_command("node.delete", 1);
    s.run_command("node.smooth", 1);
    assert_eq!(s.generation(), before);
}

// ---- panel focus ----

#[test]
fn the_focus_command_walks_the_panels_in_a_predictable_order_and_wraps() {
    use crate::state::FocusPanel::{Bottom, Canvas, Inspector, TopBar, Tree};
    let mut s = state();
    assert_eq!(s.take_focus_request(), None);
    let mut seen = Vec::new();
    for _ in 0..6 {
        s.run_command("focus.next-panel", 0);
        seen.push(s.take_focus_request().expect("a panel to focus"));
    }
    assert_eq!(seen, vec![TopBar, Tree, Canvas, Inspector, Bottom, TopBar]);
}

#[test]
fn a_focus_request_is_delivered_once() {
    let mut s = state();
    s.run_command("focus.next-panel", 0);
    assert!(s.take_focus_request().is_some());
    assert_eq!(s.take_focus_request(), None);
}

#[test]
fn shift_f6_goes_back() {
    use crate::state::FocusPanel::{Bottom, TopBar};
    let mut s = state();
    s.run_command("focus.next-panel", 0);
    s.take_focus_request();
    s.run_command("focus.previous-panel", 0);
    assert_eq!(
        s.take_focus_request(),
        Some(Bottom),
        "from the first panel back to the last"
    );
    s.run_command("focus.next-panel", 0);
    assert_eq!(s.take_focus_request(), Some(TopBar));
}

// ---- text ----

fn text(s: &str) -> crate::text_object::TextParams {
    crate::text_object::TextParams {
        text: s.to_owned(),
        ..Default::default()
    }
}

#[test]
fn adding_text_places_one_selected_object_with_a_path_per_contour() {
    let mut s = state();
    let fonts = crate::fonts::FontLibrary::bundled_only();
    s.add_text(&text("o"), &fonts, p(30.0, 40.0), 0).unwrap();
    assert_eq!(objects(&s), 1);
    let mat = s.current_mat();
    let object = mat.objects.values().next().unwrap();
    assert_eq!(object.paths.len(), 2, "outer and counter");
    assert_eq!(s.selection().objects(), vec![object.id]);
    let b = object.bounds().unwrap();
    assert!(
        (b.x0 - 30.0).abs() < 1e-6 && (b.y0 - 40.0).abs() < 1e-6,
        "{b:?}"
    );
}

#[test]
fn adding_text_is_one_undo_step() {
    let mut s = state();
    let fonts = crate::fonts::FontLibrary::bundled_only();
    s.add_text(&text("Hello"), &fonts, p(10.0, 10.0), 0)
        .unwrap();
    assert!(s.can_undo());
    s.run_command("edit.undo", 1);
    assert_eq!(objects(&s), 0);
}

#[test]
fn blank_text_changes_nothing_and_says_why() {
    let mut s = state();
    let fonts = crate::fonts::FontLibrary::bundled_only();
    let error = s.add_text(&text("  "), &fonts, p(0.0, 0.0), 0).unwrap_err();
    assert!(error.contains("no text"), "{error}");
    assert_eq!(objects(&s), 0);
    assert!(!s.can_undo());
}

#[test]
fn single_stroke_text_makes_open_paths_a_pen_can_draw() {
    let mut s = state();
    let fonts = crate::fonts::FontLibrary::bundled_only();
    let params = crate::text_object::TextParams {
        stroke: Some(ps_import::StrokeFont::RomanSimplex),
        ..text("H")
    };
    s.add_text(&params, &fonts, p(5.0, 5.0), 0).unwrap();
    let mat = s.current_mat();
    let object = mat.objects.values().next().unwrap();
    assert_eq!(object.paths.len(), 3);
    assert!(object.paths.iter().all(|p| !p.geom.closed));
}

#[test]
fn text_paths_have_distinct_keys() {
    let mut s = state();
    let fonts = crate::fonts::FontLibrary::bundled_only();
    s.add_text(&text("88"), &fonts, p(0.0, 0.0), 0).unwrap();
    let mat = s.current_mat();
    let object = mat.objects.values().next().unwrap();
    let keys: std::collections::BTreeSet<_> = object.paths.iter().map(|p| p.key.clone()).collect();
    assert_eq!(keys.len(), object.paths.len());
}

// ---- the operations table rows ----

const PEN: &str = "pen.fineliner-black-0.4@1.0.0";
const SCORE: &str = "generic.scoring-stylus@1.0.0";

/// A 100 x 50 mm rectangle at `x` on the mat, left selected.
fn rect_at(s: &mut AppState, x: f64) {
    draw_rect(s, p(x, 10.0), p(x + 100.0, 60.0), 0);
}

#[test]
fn a_mat_with_no_operations_has_no_rows() {
    assert_eq!(state().operation_rows(), Vec::<OperationRow>::new());
}

#[test]
fn assigning_the_same_tool_and_kind_twice_makes_two_rows() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    let first = s.assign_operation_row(OpKind::Cut, BLADE, 0).unwrap();
    rect_at(&mut s, 150.0);
    let second = s.assign_operation_row(OpKind::Cut, BLADE, 0).unwrap();
    assert_ne!(first, second);
    let ids: Vec<OpRowId> = s.operation_rows().iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![first, second], "in the order they were added");
}

#[test]
fn one_object_can_be_drawn_twice_in_two_inks() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    let blue = s.assign_operation_row(OpKind::Draw, PEN, 0).unwrap();
    let red = s.assign_operation_row(OpKind::Draw, PEN, 1).unwrap();
    assert!(s.set_row_pen_colour(blue, Some("#1E88E5"), 2));
    assert!(s.set_row_pen_colour(red, Some("#D32F2F"), 3));
    assert_eq!(s.row_pen_colour(blue).as_deref(), Some("#1E88E5"));
    assert_eq!(s.row_pen_colour(red).as_deref(), Some("#D32F2F"));
    let mat = s.current_mat();
    assert_eq!(mat.objects.values().next().unwrap().paths[0].ops.len(), 2);
}

#[test]
fn a_new_operation_runs_last_even_on_the_first_object() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    rect_at(&mut s, 150.0);
    s.assign_operation(OpKind::Draw, PEN, 0);
    let first_object = *s.current_mat().objects.keys().next().unwrap();
    s.select_object(first_object);
    let added = s.assign_operation_row(OpKind::Score, SCORE, 1).unwrap();
    assert_eq!(s.operation_rows()[2].id, added);
    s.run_command("edit.undo", 2);
    assert_eq!(
        s.operation_rows().len(),
        2,
        "one undo removes the operation"
    );
}

#[test]
fn the_selection_can_join_an_existing_row_with_its_settings() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    let row = s.assign_operation_row(OpKind::Draw, PEN, 0).unwrap();
    assert!(s.set_row_pen_colour(row, Some("#D32F2F"), 1));
    assert!(!s.can_add_selection_to_row(row), "already on it");
    rect_at(&mut s, 150.0);
    assert!(s.can_add_selection_to_row(row));
    assert!(s.add_selection_to_row(row, 2));
    let rows = s.operation_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].paths, 2);
    assert!(
        (rows[0].length_mm - 600.0).abs() < 1e-6,
        "two 100 x 50 rectangles"
    );
    let inks: Vec<Option<String>> = s
        .current_mat()
        .objects
        .values()
        .flat_map(|o| o.paths.iter())
        .flat_map(|p| p.ops.iter())
        .map(|op| op.tool.pen_colour.clone())
        .collect();
    assert_eq!(inks, vec![Some("#D32F2F".to_owned()); 2]);
    assert!(!s.add_selection_to_row(row, 3), "nothing left to add");
    s.run_command("edit.undo", 4);
    assert_eq!(s.operation_rows()[0].paths, 1);
}

#[test]
fn a_path_joining_a_row_that_is_off_stays_off() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    let row = s.assign_operation_row(OpKind::Cut, BLADE, 0).unwrap();
    assert!(s.set_operation_row_enabled(row, false, 1));
    rect_at(&mut s, 150.0);
    assert!(s.add_selection_to_row(row, 2));
    let got = &s.operation_rows()[0];
    assert!(!got.enabled && !got.partly);
}

#[test]
fn rows_of_one_kind_and_tool_count_their_own_paths() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    rect_at(&mut s, 150.0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    let rows = s.operation_rows();
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].paths, rows[1].paths), (1, 1));
    assert!(
        (rows[1].length_mm - 300.0).abs() < 1e-6,
        "one 100 x 50 rectangle"
    );
}

#[test]
fn a_different_kind_or_tool_is_a_different_row() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    rect_at(&mut s, 150.0);
    s.assign_operation(OpKind::Draw, PEN, 0);
    let rows = s.operation_rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].kind, OpKind::Cut);
    assert_eq!(rows[1].kind, OpKind::Draw);
    assert_eq!(rows[0].paths, 1);
    assert!((rows[1].length_mm - 300.0).abs() < 1e-6);
}

#[test]
fn rows_can_be_moved_and_the_order_sticks() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    rect_at(&mut s, 150.0);
    s.assign_operation(OpKind::Draw, PEN, 0);
    let first = s.operation_rows()[0].id;
    assert!(s.move_operation_row(first, 1, 1));
    let rows = s.operation_rows();
    assert_eq!(rows[0].kind, OpKind::Draw);
    assert_eq!(rows[1].kind, OpKind::Cut);
    assert!(!s.move_operation_row(first, 1, 2), "already last");
    assert!(s.move_operation_row(first, -1, 3));
    assert_eq!(s.operation_rows()[0].kind, OpKind::Cut);
}

#[test]
fn moving_a_row_is_one_undo_step() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    rect_at(&mut s, 150.0);
    s.assign_operation(OpKind::Draw, PEN, 0);
    let first = s.operation_rows()[0].id;
    s.move_operation_row(first, 1, 1);
    s.run_command("edit.undo", 2);
    assert_eq!(s.operation_rows()[0].kind, OpKind::Cut);
}

#[test]
fn disabling_a_row_turns_off_all_its_operations_in_one_undo_step() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    let row = s.assign_operation_row(OpKind::Cut, BLADE, 0).unwrap();
    rect_at(&mut s, 150.0);
    assert!(s.add_selection_to_row(row, 0));
    assert!(s.set_operation_row_enabled(row, false, 1));
    assert!(!s.operation_rows()[0].enabled);
    assert!(
        s.current_mat()
            .objects
            .values()
            .flat_map(|o| o.paths.iter())
            .flat_map(|p| p.ops.iter())
            .all(|op| !op.enabled)
    );
    s.run_command("edit.undo", 2);
    assert!(s.operation_rows()[0].enabled);
}

#[test]
fn a_row_with_some_operations_off_is_partly_enabled() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    let cut = s.assign_operation_row(OpKind::Cut, BLADE, 0).unwrap();
    rect_at(&mut s, 150.0);
    assert!(s.add_selection_to_row(cut, 0));
    let mat = s.current_mat();
    let object = mat.objects.values().next().unwrap();
    let (key, op) = (object.paths[0].key.clone(), object.paths[0].ops[0].clone());
    // Turn one path's operation off directly, leaving the other running.
    let off: Box<dyn ModelCommand> = Box::new(SetOpEnabled {
        mat: s.mat_id(),
        object: object.id,
        path: key,
        op: op.id,
        enabled: false,
    });
    s.commit(&[off], 0);
    let row = &s.operation_rows()[0];
    assert!(row.partly);
}

// ---- drag ordering and locks ----

fn three_rows() -> AppState {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    rect_at(&mut s, 150.0);
    s.assign_operation(OpKind::Draw, PEN, 0);
    rect_at(&mut s, 300.0);
    s.assign_operation(OpKind::Score, "generic.scoring-stylus@1.0.0", 0);
    s
}

fn kinds(s: &AppState) -> Vec<OpKind> {
    s.operation_rows().iter().map(|r| r.kind).collect()
}

#[test]
fn a_row_can_be_dropped_at_any_position() {
    let mut s = three_rows();
    assert_eq!(kinds(&s), vec![OpKind::Cut, OpKind::Draw, OpKind::Score]);
    let cut = s.operation_rows()[0].id;
    assert!(s.move_operation_row_to(cut, 2, 1));
    assert_eq!(kinds(&s), vec![OpKind::Draw, OpKind::Score, OpKind::Cut]);
    assert!(!s.move_operation_row_to(cut, 2, 2), "already there");
    assert!(!s.move_operation_row_to(cut, 9, 3), "past the end");
    let score = s.operation_rows()[1].id;
    assert!(s.move_operation_row_to(score, 0, 4));
    assert_eq!(kinds(&s), vec![OpKind::Score, OpKind::Draw, OpKind::Cut]);
}

#[test]
fn a_row_starts_unlocked_and_can_be_locked_after_the_one_above() {
    let mut s = three_rows();
    assert!(s.operation_rows().iter().all(|r| !r.locked_after_previous));
    let second = s.operation_rows()[1].id;
    assert!(s.set_row_locked_after_previous(second, true, 1));
    assert!(s.operation_rows()[1].locked_after_previous);
    assert!(!s.operation_rows()[2].locked_after_previous);
    assert!(s.set_row_locked_after_previous(second, false, 2));
    assert!(!s.operation_rows()[1].locked_after_previous);
}

#[test]
fn the_first_row_has_nothing_to_run_after() {
    let mut s = three_rows();
    let first = s.operation_rows()[0].id;
    assert!(!s.set_row_locked_after_previous(first, true, 1));
}

#[test]
fn a_lock_follows_the_row_when_the_order_changes() {
    let mut s = three_rows();
    let rows = s.operation_rows();
    let (cut, draw) = (rows[0].id, rows[1].id);
    s.set_row_locked_after_previous(draw, true, 1);
    s.move_operation_row_to(cut, 2, 2);
    // The draw row is now first, so its lock (to run after the cut) no longer reads as "after
    // the row above", but the lock itself is still there for the compiler.
    assert!(
        s.current_mat()
            .run_after
            .iter()
            .any(|(a, b)| *a == cut && *b == draw)
    );
}

// ---- snapping and guides ----

fn sized() -> AppState {
    let mut s = state();
    s.resize_canvas(1200.0, 900.0);
    s.run_command("view.zoom-fit", 0);
    s
}

#[test]
fn snapping_is_on_by_default_and_can_be_toggled() {
    let mut s = sized();
    assert!(s.snapping());
    s.run_command("view.snap", 0);
    assert!(!s.snapping());
    s.run_command("view.snap", 0);
    assert!(s.snapping());
}

#[test]
fn the_cursor_snaps_to_a_grid_crossing_and_says_what_it_hit() {
    let s = sized();
    let (at, kind) = s.snap_point(p(10.3, 19.8));
    assert!(
        (at.x - 10.0).abs() < 1e-9 && (at.y - 20.0).abs() < 1e-9,
        "{at:?}"
    );
    assert_eq!(kind, Some(SnapKind::Grid));
}

#[test]
fn far_from_everything_the_cursor_stays_where_it_is() {
    let s = sized();
    // Midway between grid lines, far from the mat edges: nothing within the radius at this zoom
    // when the radius is under 5 mm.
    let mut s = s;
    s.run_command("view.zoom-in", 0);
    s.run_command("view.zoom-in", 0);
    s.run_command("view.zoom-in", 0);
    let c = s.viewport().center();
    let probe = p(
        (c.x / 10.0).floor() * 10.0 + 5.0,
        (c.y / 10.0).floor() * 10.0 + 5.0,
    );
    let (at, kind) = s.snap_point(probe);
    assert_eq!(at, probe);
    assert_eq!(kind, None);
}

#[test]
fn nodes_beat_the_grid_when_both_are_close() {
    let mut s = sized();
    draw_rect(&mut s, p(50.0, 50.0), p(155.0, 97.0), 0);
    s.run_command("tool.select", 0);
    let (at, kind) = s.snap_point(p(155.2, 97.1));
    assert!(
        (at.x - 155.0).abs() < 1e-6 && (at.y - 97.0).abs() < 1e-6,
        "{at:?}"
    );
    assert_eq!(kind, Some(SnapKind::Node));
}

#[test]
fn the_mat_edge_is_a_snap_target() {
    let s = sized();
    let (at, kind) = s.snap_point(p(0.3, 123.4));
    assert!(at.x.abs() < 1e-9, "{at:?}");
    assert_eq!(kind, Some(SnapKind::Edge));
}

/// Draws a rectangle the way the canvas widget does, so the corners snap.
fn pointer_rect(s: &mut AppState, from: Point, to: Point, mods: Mods, now: u64) {
    s.run_command("tool.rectangle", now);
    s.canvas_pointer_event(CanvasEvent::Press(from), mods, now);
    s.canvas_pointer_event(CanvasEvent::Drag(to), mods, now);
    s.canvas_pointer_event(CanvasEvent::Release(to), mods, now);
}

#[test]
fn drawing_snaps_both_corners_to_the_grid() {
    let mut s = sized();
    pointer_rect(&mut s, p(10.2, 19.9), p(59.8, 40.1), NONE, 0);
    let mat = s.current_mat();
    let b = mat.objects.values().next().unwrap().bounds().unwrap();
    assert!(
        (b.x0 - 10.0).abs() < 1e-6 && (b.y0 - 20.0).abs() < 1e-6,
        "{b:?}"
    );
    assert!(
        (b.x1 - 60.0).abs() < 1e-6 && (b.y1 - 40.0).abs() < 1e-6,
        "{b:?}"
    );
}

#[test]
fn the_command_key_bypasses_snapping_and_so_does_turning_it_off() {
    let mut s = sized();
    let command = Mods {
        command: true,
        ..NONE
    };
    pointer_rect(&mut s, p(10.2, 19.9), p(59.8, 40.1), command, 0);
    let mat = s.current_mat();
    let b = mat.objects.values().next().unwrap().bounds().unwrap();
    assert!((b.x0 - 10.2).abs() < 1e-6, "{b:?}");
    s.run_command("view.snap", 1);
    pointer_rect(&mut s, p(100.2, 19.9), p(140.0, 40.1), NONE, 2);
    let last = s
        .current_mat()
        .objects
        .values()
        .last()
        .unwrap()
        .bounds()
        .unwrap();
    assert!((last.x0 - 100.2).abs() < 1e-6, "{last:?}");
}

#[test]
fn guides_attract_the_cursor_more_than_the_grid() {
    let mut s = sized();
    s.add_guide(GuideAxis::Vertical, 33.3, 0);
    // A grid line crossing at (30, 40) is 3.3 mm away horizontally; the guide at x = 33.3 is
    // 0.1 mm away, so the cursor lands on the guide and keeps its own y.
    let (at, kind) = s.snap_point(p(33.4, 41.7));
    assert!((at.x - 33.3).abs() < 1e-9, "{at:?}");
    assert_eq!(kind, Some(SnapKind::Guide));
}

#[test]
fn guides_can_be_added_removed_and_undone() {
    let mut s = sized();
    assert!(s.add_guide(GuideAxis::Horizontal, 25.0, 0));
    assert!(s.add_guide(GuideAxis::Vertical, 40.0, 1));
    assert_eq!(s.current_mat().guides.len(), 2);
    assert!(s.remove_guide_near(GuideAxis::Horizontal, 25.1, 1.0, 2));
    assert_eq!(s.current_mat().guides.len(), 1);
    assert!(
        !s.remove_guide_near(GuideAxis::Horizontal, 90.0, 1.0, 3),
        "nothing there"
    );
    s.run_command("edit.undo", 4);
    assert_eq!(s.current_mat().guides.len(), 2);
}

#[test]
fn the_same_guide_is_not_added_twice() {
    let mut s = sized();
    assert!(s.add_guide(GuideAxis::Vertical, 40.0, 0));
    assert!(!s.add_guide(GuideAxis::Vertical, 40.0, 1));
}

#[test]
fn where_two_paths_cross_is_a_snap_target() {
    let mut s = sized();
    s.run_command("tool.line", 0);
    s.canvas_event(CanvasEvent::Press(p(20.0, 20.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(p(83.0, 71.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Press(p(83.0, 23.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(p(23.0, 71.0)), NONE, 0);
    s.run_command("tool.select", 0);
    // The lines cross at their common midpoint-ish point; find it from the geometry itself.
    let mat = s.current_mat();
    let lines: Vec<_> = mat
        .objects
        .values()
        .map(|o| o.paths[0].geom.transformed(o.transform))
        .collect();
    let hit = ps_render::segment_intersection(
        lines[0].start,
        lines[0].end(),
        lines[1].start,
        lines[1].end(),
    )
    .expect("they cross");
    let (at, kind) = s.snap_point(p(hit.x + 0.2, hit.y - 0.2));
    assert_eq!(kind, Some(SnapKind::Intersection));
    assert!((at.x - hit.x).abs() < 1e-6 && (at.y - hit.y).abs() < 1e-6);
}

// ---- preset and time columns ----

#[test]
fn a_row_names_the_material_preset_it_uses_when_there_is_one() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    assert_eq!(s.operation_rows()[0].preset, None, "no material yet");
    let cardstock = s
        .material_choices()
        .into_iter()
        .find(|m| m.name.contains("Cardstock"))
        .unwrap();
    s.set_mat_material(Some(&cardstock.reference), 1);
    assert_eq!(
        s.operation_rows()[0].preset.as_deref(),
        Some("Cardstock 80 lb")
    );
}

#[test]
fn a_row_with_no_preset_for_its_tool_has_none() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.assign_operation(OpKind::Perforate, BLADE, 0);
    let vinyl = s
        .material_choices()
        .into_iter()
        .find(|m| m.name.to_lowercase().contains("vinyl"))
        .unwrap();
    s.set_mat_material(Some(&vinyl.reference), 1);
    assert_eq!(s.operation_rows()[0].preset, None);
}

#[test]
fn rows_have_no_time_until_a_job_is_compiled() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    assert_eq!(s.operation_rows()[0].time_ms, None);
}

#[test]
fn a_compiled_jobs_time_is_shared_between_rows_by_length() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    draw_rect(&mut s, p(150.0, 10.0), p(200.0, 35.0), 0); // 150 mm outline
    s.assign_operation(OpKind::Draw, PEN, 0);
    s.tick(100);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    let rows = s.operation_rows();
    let total = s.compiled().unwrap().job.mats[0].estimate.machine_time_ms;
    let (a, b) = (rows[0].time_ms.unwrap(), rows[1].time_ms.unwrap());
    assert!(
        a.abs_diff(total * 2 / 3) <= 1 && b.abs_diff(total / 3) <= 1,
        "{a} {b} of {total}"
    );
    assert!(a + b <= total);
}

// ---- job settings ----

#[test]
fn the_ordering_mode_and_pen_check_are_job_settings_with_defaults() {
    let s = state();
    assert_eq!(s.ordering(), ps_model::OrderingMode::MinimiseToolChanges);
    assert!(!s.pen_check());
}

#[test]
fn changing_them_is_an_undoable_edit_that_recompiles() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    assert!(s.set_ordering(ps_model::OrderingMode::StrictOrder, 1));
    assert!(s.set_pen_check(true, 2));
    assert_eq!(s.ordering(), ps_model::OrderingMode::StrictOrder);
    assert!(s.pen_check());
    assert!(!s.set_pen_check(true, 3), "no change, no edit");
    s.run_command("edit.undo", 5000);
    assert!(!s.pen_check() || s.ordering() == ps_model::OrderingMode::MinimiseToolChanges);
    s.tick(10_000);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
}

// ---- the material preview ----

fn compiled_state() -> AppState {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    draw_rect(&mut s, p(150.0, 10.0), p(200.0, 35.0), 0);
    s.assign_operation(OpKind::Draw, PEN, 0);
    s.tick(100);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    s
}

#[test]
fn there_is_no_preview_before_a_job_is_compiled() {
    assert!(state().preview(1.0, usize::MAX).is_none());
}

#[test]
fn the_preview_has_one_look_per_tool_with_its_effect_colour_and_width() {
    let s = compiled_state();
    let p = s.preview(2.0, usize::MAX).unwrap();
    let pen = p
        .looks
        .iter()
        .find(|l| l.effect == ps_render::Effect::Ink)
        .expect("the pen");
    assert_eq!(pen.colour, [0x23, 0x1F, 0x20], "the pen's own black");
    assert!((pen.width_mm - 0.4).abs() < 1e-9);
    let blade = p
        .looks
        .iter()
        .find(|l| l.effect == ps_render::Effect::Cut)
        .expect("the blade");
    assert!(blade.width_mm > 0.0);
}

#[test]
fn the_preview_image_shows_the_pen_where_the_job_draws() {
    let s = compiled_state();
    let p = s.preview(10.0, usize::MAX).unwrap();
    assert_eq!(
        (p.image.width, p.image.height),
        (3048, 3048),
        "304.8 mm at 10 px/mm"
    );
    let at = |x: usize, y: usize| p.image.rgba[(y * p.image.width as usize + x) * 4];
    // The pen square's top edge: y = 10 mm, x from 150 to 200 mm.
    assert!(
        at(1750, 100) < 80,
        "dark ink on the card: {}",
        at(1750, 100)
    );
    // Far from any mark the card is light.
    assert!(at(1500, 1500) > 200, "{}", at(1500, 1500));
}

#[test]
fn fewer_steps_show_less_of_the_job() {
    let s = compiled_state();
    let all = s.preview(2.0, usize::MAX).unwrap().image;
    let none = s.preview(2.0, 0).unwrap().image;
    assert_ne!(all, none);
}

#[test]
fn the_base_colour_follows_the_material() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    let vinyl = s
        .material_choices()
        .into_iter()
        .find(|m| m.name.to_lowercase().contains("vinyl"))
        .unwrap();
    let card = s
        .material_choices()
        .into_iter()
        .find(|m| m.name.contains("Cardstock"))
        .unwrap();
    s.tick(100);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    s.set_mat_material(Some(&card.reference), 1);
    s.tick(200);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    let a = s.preview(1.0, usize::MAX).unwrap().base;
    s.set_mat_material(Some(&vinyl.reference), 2);
    s.tick(300);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    let b = s.preview(1.0, usize::MAX).unwrap().base;
    assert_ne!(a, b);
}

#[test]
fn true_scale_follows_the_users_screen_calibration() {
    let mut s = state();
    s.set_display_dpi(96.0);
    s.update_settings(|settings| settings.screen_calibration = 1.25);
    s.run_command("view.true-scale", 0);
    assert!((s.viewport().zoom() - 96.0 / 25.4 * 1.25).abs() < 1e-9);
}

#[test]
fn a_measured_ruler_recalibrates_the_screen_and_refits_true_scale() {
    let mut s = state();
    s.set_display_dpi(96.0);
    assert!(s.calibrate_screen(95.0));
    assert!((s.settings().screen_calibration - 100.0 / 95.0).abs() < 1e-12);
    assert!(s.take_settings_changed());
    s.run_command("view.true-scale", 0);
    assert!((s.viewport().zoom() - 96.0 / 25.4 * 100.0 / 95.0).abs() < 1e-9);
}

#[test]
fn a_bad_ruler_measurement_changes_nothing() {
    let mut s = state();
    assert!(!s.calibrate_screen(0.0));
    assert!((s.settings().screen_calibration - 1.0).abs() < f64::EPSILON);
    assert!(!s.take_settings_changed());
}

// ---- tilted and weeded views ----

#[test]
fn a_tilted_preview_is_foreshortened_and_a_flat_one_is_the_composite() {
    let s = compiled_state();
    let p = s.preview(2.0, usize::MAX).unwrap();
    let flat = p.render(&PreviewView::Flat);
    assert_eq!(flat, p.image);
    let tilted = p.render(&PreviewView::Tilted {
        tilt_deg: 60.0,
        azimuth_deg: 0.0,
    });
    assert_eq!(tilted.width, p.image.width);
    assert!(tilted.height < p.image.height, "foreshortened");
}

#[test]
fn a_weeded_preview_lifts_the_pieces_the_blade_cut_free() {
    let s = compiled_state();
    let p = s.preview(2.0, usize::MAX).unwrap();
    assert!(p.pieces.count() >= 1, "the blade cut a closed square");
    let weeded = p.render(&PreviewView::Weeded { lift_mm: 8.0 });
    assert_ne!(weeded, p.image);
    assert_eq!(
        (weeded.width, weeded.height),
        (p.image.width, p.image.height)
    );
}

#[test]
fn only_vinyl_has_its_waste_removed_when_weeded() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    let vinyl = s
        .material_choices()
        .into_iter()
        .find(|m| m.name.to_lowercase().contains("vinyl"))
        .unwrap();
    let card = s
        .material_choices()
        .into_iter()
        .find(|m| m.name.contains("Cardstock"))
        .unwrap();
    s.set_mat_material(Some(&card.reference), 1);
    s.tick(100);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    assert!(!s.preview(1.0, usize::MAX).unwrap().vinyl);
    s.set_mat_material(Some(&vinyl.reference), 2);
    s.tick(200);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    assert!(s.preview(1.0, usize::MAX).unwrap().vinyl);
}

// ---- matless rolls ----

const EXPLORE_3: &str = "generic.cutter-medium";

#[test]
fn a_roll_mat_needs_a_machine_that_declares_a_roll() {
    let mut s = state();
    assert!(s.set_machine("generic.cutter-small"));
    assert!(s.roll().is_none());
    assert!(!s.add_matless_mat(0), "the Air 2 has no roll");
}

#[test]
fn a_roll_mat_is_the_rolls_width_and_longest_cut_and_is_matless() {
    let mut s = state();
    assert!(s.set_machine(EXPLORE_3));
    let before = s.mat_summaries().len();
    assert!(s.add_matless_mat(0));
    assert_eq!(s.mat_summaries().len(), before + 1);
    let mat = s.current_mat();
    assert_eq!(mat.mat_type, ps_model::MatType::Matless);
    assert!((mat.size.width.get() - 330.2).abs() < 1e-9);
    assert!((mat.size.height.get() - 3657.6).abs() < 1e-9);
    assert!(mat.name.starts_with("Roll"), "{}", mat.name);
}

#[test]
fn fitting_a_long_mat_fits_its_width_and_shows_the_start_of_the_roll() {
    let mut s = state();
    s.set_machine(EXPLORE_3);
    s.add_matless_mat(0);
    s.run_command("view.zoom-fit", 0);
    let v = s.visible_rect();
    assert!(
        v.x0 <= 0.0 && v.x1 >= 330.2,
        "the whole width is shown: {v:?}"
    );
    assert!(v.y0 <= 0.0, "from the start of the roll: {v:?}");
    assert!(v.y1 < 3657.6 / 2.0, "not the whole 12 ft: {v:?}");
}

#[test]
fn the_used_length_of_a_roll_is_how_far_the_content_reaches() {
    let mut s = state();
    s.set_machine(EXPLORE_3);
    s.add_matless_mat(0);
    assert_eq!(s.used_length_mm(), None, "nothing is placed yet");
    draw_rect(&mut s, p(20.0, 400.0), p(80.0, 500.0), 0);
    let used = s.used_length_mm().unwrap();
    assert!((used - 500.0).abs() < 1.0, "{used}");
}

#[test]
fn an_ordinary_mat_has_no_used_length() {
    let mut s = state();
    draw_rect(&mut s, p(20.0, 40.0), p(80.0, 100.0), 0);
    assert_eq!(s.used_length_mm(), None);
}

#[test]
fn the_add_roll_command_is_registered() {
    let registry = Registry::new(&std::collections::BTreeMap::new());
    assert!(registry.get("mat.add-roll").is_some());
}

// ---- the settings of an operation row (design: the operations panel) ----

fn cut_row(s: &mut AppState) -> OpRowId {
    draw_rect(s, p(10.0, 10.0), p(40.0, 40.0), 0);
    assert!(s.assign_operation(OpKind::Cut, BLADE, 0));
    s.operation_rows()[0].id
}

#[test]
fn a_row_lists_its_tools_parameters_with_their_ranges_and_defaults() {
    let mut s = state();
    let row = cut_row(&mut s);
    let params = s.row_params(row);
    let force = params.iter().find(|q| q.name == "force").expect("force");
    assert_eq!(force.unit, "gf");
    // The blade allows 50 to 4000 gf; the machine only 0 to 400, and the range is what the
    // machine can do.
    assert!((force.min - 50.0).abs() < 1e-9 && (force.max - 400.0).abs() < 1e-9);
    let speed = params.iter().find(|q| q.name == "speed").expect("speed");
    assert!(
        (speed.max - 101.6).abs() < 1e-9,
        "the machine's top speed, not the blade's 400: {}",
        speed.max
    );
    assert!(
        (force.value - 350.0).abs() < 1e-9,
        "the tool default: {}",
        force.value
    );
    assert!(!force.modified);
    let passes = params.iter().find(|q| q.name == "passes").expect("passes");
    assert!(
        (passes.step - 1.0).abs() < 1e-9,
        "a whole-number parameter steps by one"
    );
}

#[test]
fn setting_a_parameter_overrides_it_for_every_operation_of_the_row_and_undoes() {
    let mut s = state();
    let row = cut_row(&mut s);
    assert!(s.set_row_param(row, "force", 300.0, 1));
    let force = s
        .row_params(row)
        .into_iter()
        .find(|q| q.name == "force")
        .unwrap();
    assert!((force.value - 300.0).abs() < 1e-9);
    assert!(force.modified);
    s.run_command("edit.undo", 2);
    let force = s
        .row_params(row)
        .into_iter()
        .find(|q| q.name == "force")
        .unwrap();
    assert!((force.value - 350.0).abs() < 1e-9);
}

#[test]
fn a_parameter_is_reset_to_where_the_chain_gets_it_without_the_override() {
    let mut s = state();
    let row = cut_row(&mut s);
    s.set_row_param(row, "speed", 40.0, 1);
    assert!(s.reset_row_param(row, "speed", 2));
    let speed = s
        .row_params(row)
        .into_iter()
        .find(|q| q.name == "speed")
        .unwrap();
    assert!(!speed.modified);
    assert!((speed.value - 100.0).abs() < 1e-9);
}

#[test]
fn a_value_outside_the_tools_range_is_refused() {
    let mut s = state();
    let row = cut_row(&mut s);
    assert!(!s.set_row_param(row, "force", 99_999.0, 1));
    assert!(!s.set_row_param(row, "no-such-parameter", 1.0, 1));
}

#[test]
fn a_row_has_a_tool_name_and_a_holder_for_display() {
    let mut s = state();
    let row = cut_row(&mut s);
    let rows = s.operation_rows();
    let r = rows.iter().find(|r| r.id == row).unwrap();
    assert_eq!(r.tool_name, "Fine-point blade (45°)");
    assert_eq!(r.holder, "Holder B");
}

// ---- removing operations and objects, pen colour, and finding a row's objects ----

const PEN_TOOL: &str = "pen.fineliner-black-0.4@1.0.0";

fn draw_row(s: &mut AppState) -> OpRowId {
    draw_rect(s, p(10.0, 10.0), p(40.0, 40.0), 0);
    assert!(s.assign_operation(OpKind::Draw, PEN_TOOL, 0));
    s.operation_rows()[0].id
}

#[test]
fn removing_a_row_takes_its_operation_off_every_path_and_is_one_undo_step() {
    let mut s = state();
    let row = draw_row(&mut s);
    draw_rect(&mut s, p(60.0, 10.0), p(90.0, 40.0), 1);
    assert!(s.add_selection_to_row(row, 1));
    assert_eq!(s.operation_rows().len(), 1, "both rectangles share the row");
    assert!(s.remove_row(row, 2));
    assert_eq!(s.operation_rows().len(), 0);
    s.run_command("edit.undo", 3);
    assert_eq!(s.operation_rows()[0].paths, 2, "one undo brings both back");
}

#[test]
fn a_row_knows_which_objects_carry_it_and_can_select_them() {
    let mut s = state();
    let row = draw_row(&mut s);
    draw_rect(&mut s, p(60.0, 10.0), p(90.0, 40.0), 1);
    assert!(s.select_row_objects(row));
    assert_eq!(
        s.selection().objects().len(),
        1,
        "only the first has the row"
    );
    s.run_command("edit.select-all", 2);
    assert_eq!(s.selection().objects().len(), 2);
}

#[test]
fn an_object_can_be_deleted_by_id_without_selecting_it() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(40.0, 40.0), 0);
    draw_rect(&mut s, p(60.0, 10.0), p(90.0, 40.0), 1);
    let first = *s.current_mat().objects.keys().next().unwrap();
    assert!(s.delete_object(first, 2));
    assert_eq!(s.current_mat().objects.len(), 1);
    assert!(!s.current_mat().objects.contains_key(&first));
    s.run_command("edit.undo", 3);
    assert_eq!(s.current_mat().objects.len(), 2);
}

#[test]
fn a_rows_pen_colour_is_the_pens_own_until_changed() {
    let mut s = state();
    let row = draw_row(&mut s);
    assert_eq!(s.row_pen_colour(row).as_deref(), Some("#231F20"));
    assert!(s.set_row_pen_colour(row, Some("#C42B2B"), 1));
    assert_eq!(s.row_pen_colour(row).as_deref(), Some("#C42B2B"));
    assert!(s.set_row_pen_colour(row, None, 2));
    assert_eq!(s.row_pen_colour(row).as_deref(), Some("#231F20"));
}

#[test]
fn only_pens_have_a_colour() {
    let mut s = state();
    let row = cut_row(&mut s);
    assert_eq!(s.row_pen_colour(row), None);
    assert!(!s.set_row_pen_colour(row, Some("#C42B2B"), 1));
}

#[test]
fn the_pens_size_is_a_setting_of_its_row() {
    let mut s = state();
    let row = draw_row(&mut s);
    let size = s
        .row_params(row)
        .into_iter()
        .find(|q| q.name == "tip_width")
        .expect("a pen has a size");
    assert!((size.value - 0.4).abs() < 1e-9);
    assert!(s.set_row_param(row, "tip_width", 1.0, 1));
}

#[test]
fn removing_a_mat_leaves_another_and_the_last_mat_stays() {
    let mut s = state();
    s.run_command("mat.add", 0);
    assert_eq!(s.document().mats.len(), 2);
    s.run_command("mat.remove", 1);
    assert_eq!(s.document().mats.len(), 1);
    s.run_command("mat.remove", 2);
    assert_eq!(s.document().mats.len(), 1, "a project always has a mat");
}

#[test]
fn a_fill_row_has_default_fill_settings_that_can_be_changed_for_every_path() {
    use ps_geometry::Pattern;
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(40.0, 40.0), 0);
    assert!(s.assign_operation(OpKind::Fill, PEN_TOOL, 0));
    let row = s.operation_rows()[0].id;
    let default = s.row_fill(row).expect("a fill row has settings");
    assert_eq!(default.pattern, Pattern::Parallel);
    assert!((default.overlap - 0.15).abs() < 1e-9);
    let mut changed = default;
    changed.overlap = 0.5;
    changed.pattern = Pattern::Cross;
    assert!(s.set_row_fill(row, changed, 1));
    assert_eq!(s.row_fill(row), Some(changed));
    s.run_command("edit.undo", 2);
    assert_eq!(s.row_fill(row), Some(default));
}

#[test]
fn only_fill_rows_have_fill_settings() {
    let mut s = state();
    let row = cut_row(&mut s);
    assert!(s.row_fill(row).is_none());
}

// ---- the machine, and what the job status says ----

/// The built-in library plus one more machine, as an installed plugin's library would add.
fn library_with_machine(id: &str, bump_patch: bool) -> LibrarySnapshot {
    let mut entries = builtin_entries().expect("the built-in library loads");
    let mut profile = entries
        .iter()
        .find_map(|e| match e {
            ps_library::Entry::Profile(p) if p.id == "generic.cutter-small" => Some(p.clone()),
            _ => None,
        })
        .expect("a generic cutter exists");
    profile.id = id.to_owned();
    if bump_patch {
        profile.version.patch += 1;
    }
    entries.push(ps_library::Entry::Profile(profile));
    LibrarySnapshot::from_entries(entries)
}

fn state_preferring(machine: &str) -> AppState {
    AppState::new(Settings {
        default_machine: Some(machine.to_owned()),
        ..Settings::default()
    })
}

#[test]
fn a_saved_machine_from_an_installed_library_is_chosen_once_that_library_loads() {
    let mut s = state_preferring("acme.mill");
    assert_eq!(
        s.current_machine_id(),
        None,
        "the built-in library does not have it"
    );
    s.replace_libraries(library_with_machine("acme.mill", false));
    assert_eq!(s.current_machine_id().as_deref(), Some("acme.mill"));
}

#[test]
fn loading_a_newer_version_of_the_chosen_machine_switches_to_it() {
    let mut s = state_preferring("acme.mill");
    s.replace_libraries(library_with_machine("acme.mill", false));
    let before = s.profile().unwrap().version.clone();
    s.replace_libraries(library_with_machine("acme.mill", true));
    assert!(s.profile().unwrap().version > before);
}

#[test]
fn a_machine_that_leaves_the_library_falls_back_to_the_default() {
    let mut s = state_preferring("acme.mill");
    s.replace_libraries(library_with_machine("acme.mill", false));
    s.replace_libraries(library_with_machine("acme.other", false));
    assert_eq!(
        s.current_machine_id().as_deref(),
        Some("generic.cutter-small")
    );
}

#[test]
fn without_a_machine_nothing_compiles_and_the_status_says_to_choose_one() {
    let mut s = state_preferring("acme.mill");
    draw_rect(&mut s, p(10.0, 10.0), p(60.0, 50.0), 0);
    s.assign_operation(OpKind::Cut, BLADE, 0);
    s.tick(100);
    assert!(!s.compile_running());
    assert_eq!(s.job_status(), JobStatus::NoMachine);
}

#[test]
fn a_job_the_machine_cannot_run_is_blocked_and_says_how_many_errors() {
    let mut s = state();
    assert!(s.set_machine("generic.grbl-pen-plotter"));
    draw_rect(&mut s, p(10.0, 10.0), p(60.0, 50.0), 0);
    s.assign_operation(OpKind::Foil, "generic.foil-transfer@1.0.0", 0);
    s.tick(100);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    match s.job_status() {
        JobStatus::Blocked { errors } => assert!(errors >= 1, "{errors}"),
        other => panic!("expected Blocked, got {other:?}"),
    }
    assert!(!s.context().has_compile, "a blocked job cannot be exported");
}

#[test]
fn the_status_follows_a_job_from_nothing_to_do_through_compiling_to_ready() {
    let mut s = state();
    draw_rect(&mut s, p(10.0, 10.0), p(60.0, 50.0), 0);
    s.tick(100);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    assert_eq!(s.job_status(), JobStatus::NothingToDo);

    s.assign_operation(OpKind::Cut, BLADE, 200);
    assert_eq!(
        s.job_status(),
        JobStatus::OutOfDate,
        "the edit has not compiled yet"
    );
    s.tick(400);
    assert_eq!(s.job_status(), JobStatus::Compiling);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    assert_eq!(s.job_status(), JobStatus::Ready);
}

#[test]
fn a_block_that_cannot_be_previewed_offers_no_picture_inputs() {
    let mut s = state();
    assert!(s.set_machine("generic.grbl-pen-plotter"));
    draw_rect(&mut s, p(10.0, 10.0), p(60.0, 50.0), 0);
    s.assign_operation(OpKind::Foil, "generic.foil-transfer@1.0.0", 0);
    s.tick(100);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    assert!(
        s.preview_inputs().is_none(),
        "the preview shows the real reason, not an endless Rendering"
    );
}

#[test]
fn the_speed_and_force_ranges_stop_at_what_the_machine_can_do() {
    let mut entries = builtin_entries().expect("the built-in library loads");
    let mut slow = entries
        .iter()
        .find_map(|e| match e {
            ps_library::Entry::Profile(p) if p.id == "generic.cutter-small" => Some(p.clone()),
            _ => None,
        })
        .expect("a generic cutter exists");
    slow.id = "acme.slow".to_owned();
    slow.max_speed_mm_s = 40.0;
    slow.force.min_gf = 100.0;
    slow.force.max_gf = 900.0;
    entries.push(ps_library::Entry::Profile(slow));

    let mut s = state_preferring("acme.slow");
    s.replace_libraries(LibrarySnapshot::from_entries(entries));
    assert_eq!(s.current_machine_id().as_deref(), Some("acme.slow"));
    let row = cut_row(&mut s);
    let params = s.row_params(row);
    let speed = params.iter().find(|q| q.name == "speed").expect("speed");
    assert!(
        (speed.max - 40.0).abs() < 1e-9,
        "the slider ends at the machine's top speed, not the tool's: {}",
        speed.max
    );
    let force = params.iter().find(|q| q.name == "force").expect("force");
    assert!((force.min - 100.0).abs() < 1e-9 && (force.max - 900.0).abs() < 1e-9);
    assert!(
        !s.set_row_param(row, "speed", 41.0, 0),
        "a value the machine would clamp is refused"
    );
}
// ---- selecting shapes within an object ----

const CTRL: Mods = Mods {
    command: true,
    shift: false,
    alt: false,
};

const SHIFT: Mods = Mods {
    command: false,
    shift: true,
    alt: false,
};

const CTRL_SHIFT: Mods = Mods {
    command: true,
    shift: true,
    alt: false,
};

/// One object like an imported drawing: a frame with two small squares inside it, which on the
/// mat are `left` at (20..40, 20..40), `right` at (60..80, 20..40) and `frame` at (10..110,
/// 10..110). The frame comes last, so it lies on top of the squares.
fn lettered(s: &mut AppState) -> (ObjectId, [PathKey; 3]) {
    let id = ObjectId::new();
    let keys = ["left", "right", "frame"].map(PathKey::source);
    let geoms = [
        ps_geometry::Path::rect(10.0, 10.0, 20.0, 20.0),
        ps_geometry::Path::rect(50.0, 10.0, 20.0, 20.0),
        ps_geometry::Path::rect(0.0, 0.0, 100.0, 100.0),
    ];
    let paths = keys
        .iter()
        .zip(geoms)
        .map(|(key, geom)| {
            Arc::new(ShapePath {
                key: key.clone(),
                geom,
                fill_rule: FillRule::NonZero,
                style: SourceStyle::default(),
                ops: Vec::new(),
                start_locked: false,
            })
        })
        .collect();
    let add: Box<dyn ModelCommand> = Box::new(AddObject {
        mat: s.mat_id(),
        object: PlacedObject {
            id,
            source: None,
            transform: Affine::translate((10.0, 10.0)),
            group: None,
            attach: None,
            paths: Arc::new(paths),
            locked: false,
            hidden: false,
        },
    });
    assert!(s.commit_all(&[add], 0));
    s.run_command("tool.select", 0);
    (id, keys)
}

const IN_LEFT: (f64, f64) = (30.0, 30.0);
const IN_RIGHT: (f64, f64) = (70.0, 30.0);
const IN_FRAME_ONLY: (f64, f64) = (60.0, 90.0);
const OFF_EVERYTHING: (f64, f64) = (250.0, 250.0);

fn click(s: &mut AppState, (x, y): (f64, f64), mods: Mods) {
    s.canvas_event(CanvasEvent::Press(p(x, y)), mods, 0);
    s.canvas_event(CanvasEvent::Release(p(x, y)), mods, 0);
}

/// A double click as the window delivers it: a click, then the double click ahead of the second
/// press and release.
fn double_click(s: &mut AppState, at: (f64, f64)) {
    click(s, at, NONE);
    s.canvas_event(CanvasEvent::DoubleClick(p(at.0, at.1)), NONE, 0);
    click(s, at, NONE);
}

fn selected_shapes(s: &AppState) -> Vec<PathKey> {
    let mut keys: Vec<PathKey> = s.selection().shapes().map(|(_, k)| k.clone()).collect();
    keys.sort_by_key(ToString::to_string);
    keys
}

/// The row of every operation on a shape.
fn rows_on(s: &AppState, object: ObjectId, key: &PathKey) -> Vec<OpRowId> {
    s.current_mat().objects[&object]
        .paths
        .iter()
        .find(|p| p.key == *key)
        .map(|p| p.ops.iter().map(|o| o.row).collect())
        .unwrap_or_default()
}

#[test]
fn double_clicking_selects_one_shape_and_a_click_then_picks_another() {
    let mut s = state();
    let (id, [left, right, frame]) = lettered(&mut s);
    click(&mut s, IN_LEFT, NONE);
    assert!(!s.has_shape_selection(), "one click takes the whole object");
    double_click(&mut s, IN_LEFT);
    assert_eq!(selected_shapes(&s), vec![left]);
    assert_eq!(s.selection().objects(), vec![id]);
    click(&mut s, IN_RIGHT, NONE);
    assert_eq!(selected_shapes(&s), vec![right]);
    click(&mut s, IN_FRAME_ONLY, NONE);
    assert_eq!(selected_shapes(&s), vec![frame]);
}

#[test]
fn shift_clicking_adds_and_removes_shapes() {
    let mut s = state();
    let (_, [left, right, _]) = lettered(&mut s);
    double_click(&mut s, IN_LEFT);
    click(&mut s, IN_RIGHT, SHIFT);
    assert_eq!(selected_shapes(&s), vec![left.clone(), right.clone()]);
    click(&mut s, IN_LEFT, SHIFT);
    assert_eq!(selected_shapes(&s), vec![right]);
}

#[test]
fn command_clicking_selects_a_shape_without_selecting_the_object_first() {
    let mut s = state();
    let (_, [left, right, _]) = lettered(&mut s);
    click(&mut s, IN_LEFT, CTRL);
    assert_eq!(selected_shapes(&s), vec![left.clone()]);
    click(&mut s, IN_RIGHT, CTRL_SHIFT);
    assert_eq!(selected_shapes(&s), vec![left, right]);
}

#[test]
fn a_command_drag_still_moves_the_whole_object() {
    let mut s = state();
    let (id, _) = lettered(&mut s);
    let before = s.current_mat().objects[&id].transform.as_coeffs()[4];
    s.canvas_event(CanvasEvent::Press(p(30.0, 30.0)), CTRL, 0);
    s.canvas_event(CanvasEvent::Drag(p(55.0, 30.0)), CTRL, 0);
    s.canvas_event(CanvasEvent::Release(p(55.0, 30.0)), CTRL, 0);
    let after = s.current_mat().objects[&id].transform.as_coeffs()[4];
    assert!((after - before - 25.0).abs() < 1e-9);
    assert!(!s.has_shape_selection());
}

#[test]
fn escape_returns_to_the_whole_object_and_clicking_away_clears() {
    let mut s = state();
    let (id, _) = lettered(&mut s);
    double_click(&mut s, IN_LEFT);
    s.canvas_event(CanvasEvent::Escape, NONE, 0);
    assert!(!s.has_shape_selection());
    assert_eq!(s.selection().objects(), vec![id]);
    double_click(&mut s, IN_LEFT);
    click(&mut s, OFF_EVERYTHING, NONE);
    assert!(s.selection().is_empty());
}

#[test]
fn a_box_drag_inside_an_entered_object_takes_the_shapes_it_encloses() {
    let mut s = state();
    let (id, [left, right, _]) = lettered(&mut s);
    double_click(&mut s, IN_FRAME_ONLY);
    let before = s.current_mat().objects[&id].transform;
    s.canvas_event(CanvasEvent::Press(p(15.0, 15.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Drag(p(95.0, 55.0)), NONE, 0);
    s.canvas_event(CanvasEvent::Release(p(95.0, 55.0)), NONE, 0);
    assert_eq!(selected_shapes(&s), vec![left, right]);
    assert_eq!(s.current_mat().objects[&id].transform, before, "no move");
}

#[test]
fn an_object_with_one_path_has_no_shapes_to_enter() {
    let mut s = state();
    rect_at(&mut s, 10.0);
    s.run_command("tool.select", 0);
    double_click(&mut s, (10.0, 10.0));
    assert!(!s.has_shape_selection());
    assert_eq!(s.selection().len(), 1);
}

#[test]
fn shapes_of_one_object_can_be_drawn_in_different_inks() {
    let mut s = state();
    let (id, [left, right, frame]) = lettered(&mut s);
    double_click(&mut s, IN_LEFT);
    assert_eq!(s.selected_path_count(), Some(1));
    let blue = s.assign_operation_row(OpKind::Draw, PEN, 0).unwrap();
    assert!(s.set_row_pen_colour(blue, Some("#1E88E5"), 1));
    click(&mut s, IN_RIGHT, NONE);
    let red = s.assign_operation_row(OpKind::Draw, PEN, 2).unwrap();
    assert!(s.set_row_pen_colour(red, Some("#D32F2F"), 3));
    assert_eq!(rows_on(&s, id, &left), vec![blue]);
    assert_eq!(rows_on(&s, id, &right), vec![red]);
    assert_eq!(rows_on(&s, id, &frame), Vec::<OpRowId>::new());
    let rows = s.operation_rows();
    assert_eq!((rows[0].paths, rows[1].paths), (1, 1));
    assert_eq!(rows[0].ink.as_deref(), Some("#1E88E5"));
    assert_eq!(rows[1].ink.as_deref(), Some("#D32F2F"));
}

#[test]
fn shapes_can_join_and_leave_a_row() {
    let mut s = state();
    let (id, [left, right, frame]) = lettered(&mut s);
    click(&mut s, IN_FRAME_ONLY, NONE);
    let black = s.assign_operation_row(OpKind::Draw, PEN, 0).unwrap();
    double_click(&mut s, IN_LEFT);
    assert!(s.can_remove_selection_from_row(black));
    assert!(!s.can_add_selection_to_row(black));
    assert!(s.remove_selection_from_row(black, 1));
    assert_eq!(rows_on(&s, id, &left), Vec::<OpRowId>::new());
    assert_eq!(rows_on(&s, id, &right), vec![black]);
    assert_eq!(rows_on(&s, id, &frame), vec![black]);
    assert!(!s.can_remove_selection_from_row(black));
    assert!(s.add_selection_to_row(black, 2));
    assert_eq!(rows_on(&s, id, &left), vec![black]);
}

#[test]
fn choosing_a_row_selects_just_the_shapes_it_runs_on() {
    let mut s = state();
    let (id, [left, _, _]) = lettered(&mut s);
    double_click(&mut s, IN_LEFT);
    let row = s.assign_operation_row(OpKind::Draw, PEN, 0).unwrap();
    click(&mut s, OFF_EVERYTHING, NONE);
    assert!(s.select_row_objects(row));
    assert_eq!(selected_shapes(&s), vec![left]);

    click(&mut s, OFF_EVERYTHING, NONE);
    click(&mut s, IN_FRAME_ONLY, NONE);
    let all = s.assign_operation_row(OpKind::Cut, BLADE, 1).unwrap();
    click(&mut s, OFF_EVERYTHING, NONE);
    assert!(s.select_row_objects(all));
    assert!(!s.has_shape_selection(), "every shape: the whole object");
    assert_eq!(s.selection().objects(), vec![id]);
}

#[test]
fn deleting_selected_shapes_leaves_the_rest_of_the_object() {
    let mut s = state();
    let (id, [left, right, frame]) = lettered(&mut s);
    double_click(&mut s, IN_LEFT);
    s.run_command("edit.delete", 1);
    let keys: Vec<PathKey> = s.current_mat().objects[&id]
        .paths
        .iter()
        .map(|p| p.key.clone())
        .collect();
    assert_eq!(keys, vec![right, frame]);
    assert!(s.selection().is_empty());
    s.run_command("edit.undo", 2);
    assert_eq!(s.current_mat().objects[&id].paths[0].key, left);
}

#[test]
fn deleting_every_shape_of_an_object_deletes_the_object() {
    let mut s = state();
    let _ = lettered(&mut s);
    double_click(&mut s, IN_LEFT);
    click(&mut s, IN_RIGHT, SHIFT);
    click(&mut s, IN_FRAME_ONLY, SHIFT);
    s.run_command("edit.delete", 1);
    assert_eq!(objects(&s), 0);
}

#[test]
fn moving_and_copying_leave_a_shape_selection_alone() {
    let mut s = state();
    let (id, _) = lettered(&mut s);
    double_click(&mut s, IN_LEFT);
    let before = s.current_mat().objects[&id].transform;
    assert!(s.selection_bounds().is_none(), "no box or handles");
    assert!(!s.context().has_whole_objects);
    s.run_command("nudge.right", 1);
    s.run_command("edit.duplicate", 2);
    assert!(!s.rotate_selection(90.0, 3));
    assert_eq!(objects(&s), 1);
    assert_eq!(s.current_mat().objects[&id].transform, before);
}

#[test]
fn undoing_away_a_selected_shape_drops_it_from_the_selection() {
    let mut s = state();
    let _ = lettered(&mut s);
    double_click(&mut s, IN_LEFT);
    s.run_command("edit.undo", 1);
    assert!(s.selection().is_empty());
}
