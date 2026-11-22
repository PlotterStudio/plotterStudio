//! The arrange section of the inspector: numeric position and size from any of nine
//! anchors, rotate and mirror, align and distribute for several objects, and grid and circular
//! arrays. Every change is a command, so each is one undo step.

use super::App;
use crate::arrange::Anchor;

pub(super) const ANCHOR_KEYS: [(Anchor, &str, &str); 9] = [
    (Anchor::TopLeft, "arrange-anchor-top-left", "↖"),
    (Anchor::Top, "arrange-anchor-top", "↑"),
    (Anchor::TopRight, "arrange-anchor-top-right", "↗"),
    (Anchor::Left, "arrange-anchor-left", "←"),
    (Anchor::Centre, "arrange-anchor-centre", "•"),
    (Anchor::Right, "arrange-anchor-right", "→"),
    (Anchor::BottomLeft, "arrange-anchor-bottom-left", "↙"),
    (Anchor::Bottom, "arrange-anchor-bottom", "↓"),
    (Anchor::BottomRight, "arrange-anchor-bottom-right", "↘"),
];

/// A labelled number field in the display units; true when the user changed it.
fn field(ui: &mut egui::Ui, label: &str, value: &mut f64, speed: f64) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let name = ui.label(label);
        let drag = ui.add(egui::DragValue::new(value).speed(speed));
        changed = drag.changed();
        drag.labelled_by(name.id);
    });
    changed
}

/// The node commands in one menu, so the control bar stays one row however many there are.
pub(super) fn node_buttons(ui: &mut egui::Ui, app: &mut App, now_ms: u64) {
    let label = app.loc.tr("node-menu");
    let button = ui.button(format!("{label} ▾"));
    let said = label.clone();
    button.widget_info(move || {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, said.clone())
    });
    egui::Popup::menu(&button).show(|ui| {
        for id in [
            "node.delete",
            "node.to-curve",
            "node.to-line",
            "node.smooth",
            "node.corner",
        ] {
            if ui.button(app.loc.command_label(id)).clicked() {
                app.run(id, now_ms);
                ui.close();
            }
        }
    });
}
/// The fields and button that make a grid array of the selection.
pub(super) fn grid_form(ui: &mut egui::Ui, app: &mut App, now_ms: u64) {
    let fmt = app.format();
    {
        let (columns_label, rows_label) = (
            app.loc.tr("arrange-grid-columns"),
            app.loc.tr("arrange-grid-rows"),
        );
        let mut columns = f64::from(app.array_columns);
        let mut rows = f64::from(app.array_rows);
        if field(ui, &columns_label, &mut columns, 0.1) {
            app.array_columns = to_count(columns);
        }
        if field(ui, &rows_label, &mut rows, 0.1) {
            app.array_rows = to_count(rows);
        }
        let mut dx = fmt.display_from_mm(app.array_dx);
        let mut dy = fmt.display_from_mm(app.array_dy);
        if field(ui, &app.loc.tr("arrange-grid-spacing-x"), &mut dx, 0.5) {
            app.array_dx = fmt.mm_from_display(dx);
        }
        if field(ui, &app.loc.tr("arrange-grid-spacing-y"), &mut dy, 0.5) {
            app.array_dy = fmt.mm_from_display(dy);
        }
        if ui.button(app.loc.tr("arrange-grid-make")).clicked() {
            app.state.grid_array_selection(
                app.array_columns,
                app.array_rows,
                app.array_dx,
                app.array_dy,
                now_ms,
            );
        }
    }
}

/// The fields and button that make a circular array of the selection.
pub(super) fn circle_form(ui: &mut egui::Ui, app: &mut App, now_ms: u64) {
    {
        let mut count = f64::from(app.circle_count);
        if field(ui, &app.loc.tr("arrange-circle-count"), &mut count, 0.1) {
            app.circle_count = to_count(count);
        }
        field(
            ui,
            &app.loc.tr("arrange-circle-degrees"),
            &mut app.circle_degrees,
            1.0,
        );
        if ui.button(app.loc.tr("arrange-circle-make")).clicked() {
            app.state
                .circular_array_selection(app.circle_count, app.circle_degrees, now_ms);
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a small rounded non-negative count from a number field"
)]
fn to_count(value: f64) -> u32 {
    value.round().clamp(1.0, 1_000.0) as u32
}
