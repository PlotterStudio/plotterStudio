//! Ground truth for canvas tools. Each tool turns pointer events into commands; tools
//! never edit the document directly. During a drag a tool shows a preview overlay; on release it
//! commits one transaction, giving one undo step per gesture. Results are checked by applying the
//! committed commands to a real document and reading the objects back.

#![expect(
    clippy::many_single_char_names,
    reason = "tests name objects a, b and points p, c in short geometric scenarios"
)]

use std::sync::Arc;

use ps_core::{L10nKey, MatId, ObjectId, PathKey};
use ps_geometry::{Affine, FillRule, Path, Point, Rect};
use ps_model::{
    AddObject, Document, DocumentStore, Mat, MatSize, PlacedObject, ProjectMeta, ShapePath,
    SourceStyle,
};
use ps_render::Pickable;

use super::*;
use crate::commands::Mods;

const NONE: Mods = Mods {
    command: false,
    shift: false,
    alt: false,
};
const SHIFT: Mods = Mods {
    command: false,
    shift: true,
    alt: false,
};
const ALT: Mods = Mods {
    command: false,
    shift: false,
    alt: true,
};

fn p(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

struct World {
    store: DocumentStore,
    mat: MatId,
}

fn world() -> World {
    let mat = Mat::new(MatId::at_unix_ms(1), "Mat", MatSize::inches(12.0, 12.0));
    let id = mat.id;
    let mut store = DocumentStore::new(Document::new(ProjectMeta::new(
        "T",
        "2026-01-01T00:00:00Z",
        "0.1.0",
    )));
    store.apply(&ps_model::AddMat { mat }, 0).unwrap();
    World { store, mat: id }
}

fn commit(world: &mut World, cmds: &[Box<dyn ps_model::Command>]) {
    world
        .store
        .transaction(L10nKey::new("cmd-test"), cmds, 0)
        .unwrap();
}

fn ctx(mat: MatId) -> ToolCtx {
    ToolCtx {
        mat,
        new_object: ObjectId::at_unix_ms(500),
        new_path: PathKey::source("new"),
        zoom: 1.0,
        pickables: Vec::new(),
        selected: Vec::new(),
        sides: 5,
        star_ratio: 0.5,
        edit_paths: Vec::new(),
        guides: Vec::new(),
        snapping: false,
        mat_size: (300.0, 300.0),
    }
}

fn only_object(world: &World) -> Arc<PlacedObject> {
    let doc = world.store.document();
    let mat = doc.mat(world.mat).unwrap();
    assert_eq!(mat.objects.len(), 1, "exactly one object was added");
    Arc::clone(mat.objects.values().next().unwrap())
}

fn world_bounds(o: &PlacedObject) -> Rect {
    o.paths[0].geom.transformed(o.transform).bounds()
}

fn drag(tool: &mut dyn Tool, mods: Mods, ctx: &ToolCtx, from: Point, to: Point) -> ToolOutcome {
    tool.on_event(&CanvasEvent::Press(from), mods, ctx);
    tool.on_event(&CanvasEvent::Drag(to), mods, ctx);
    tool.on_event(&CanvasEvent::Release(to), mods, ctx)
}

// ---- rectangle ----

#[test]
fn dragging_a_rectangle_adds_one_object_with_that_extent() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = RectangleTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, NONE, &c, p(10.0, 20.0), p(40.0, 50.0)) else {
        panic!("expected a commit");
    };
    assert_eq!(cmds.len(), 1);
    commit(&mut w, &cmds);
    let o = only_object(&w);
    let b = world_bounds(&o);
    assert!(
        close(b.x0, 10.0) && close(b.y0, 20.0) && close(b.x1, 40.0) && close(b.y1, 50.0),
        "{b:?}"
    );
    assert_eq!(o.id, c.new_object);
    assert_eq!(o.paths[0].key, c.new_path);
    assert!(
        o.paths[0].ops.is_empty(),
        "no operation until the user assigns one"
    );
    assert!(!o.locked && !o.hidden);
}

#[test]
fn dragging_up_and_left_gives_the_same_rectangle() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = RectangleTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, NONE, &c, p(40.0, 50.0), p(10.0, 20.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let b = world_bounds(&only_object(&w));
    assert!(
        close(b.x0, 10.0) && close(b.y0, 20.0) && close(b.x1, 40.0) && close(b.y1, 50.0),
        "{b:?}"
    );
}

#[test]
fn shift_makes_a_square_from_the_longer_side() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = RectangleTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, SHIFT, &c, p(0.0, 0.0), p(30.0, 10.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let b = world_bounds(&only_object(&w));
    assert!(close(b.width(), 30.0) && close(b.height(), 30.0), "{b:?}");
    assert!(close(b.x0, 0.0) && close(b.y0, 0.0));
}

#[test]
fn a_square_keeps_the_direction_of_the_drag() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = RectangleTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, SHIFT, &c, p(50.0, 50.0), p(40.0, 20.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let b = world_bounds(&only_object(&w));
    assert!(close(b.x1, 50.0) && close(b.y1, 50.0), "{b:?}");
    assert!(close(b.width(), 30.0) && close(b.height(), 30.0));
}

#[test]
fn alt_draws_from_the_centre() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = RectangleTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, ALT, &c, p(50.0, 50.0), p(60.0, 55.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let b = world_bounds(&only_object(&w));
    assert!(
        close(b.x0, 40.0) && close(b.x1, 60.0) && close(b.y0, 45.0) && close(b.y1, 55.0),
        "{b:?}"
    );
}

#[test]
fn a_rectangle_smaller_than_a_tenth_of_a_millimetre_is_cancelled() {
    let c = ctx(world().mat);
    let mut tool = RectangleTool::default();
    assert!(matches!(
        drag(&mut tool, NONE, &c, p(5.0, 5.0), p(5.05, 5.05)),
        ToolOutcome::Cancel
    ));
    assert!(
        matches!(
            drag(&mut tool, NONE, &c, p(5.0, 5.0), p(25.0, 5.0)),
            ToolOutcome::Cancel
        ),
        "no height"
    );
}

#[test]
fn escape_cancels_the_gesture_and_the_release_that_follows_does_nothing() {
    let c = ctx(world().mat);
    let mut tool = RectangleTool::default();
    tool.on_event(&CanvasEvent::Press(p(0.0, 0.0)), NONE, &c);
    tool.on_event(&CanvasEvent::Drag(p(10.0, 10.0)), NONE, &c);
    assert!(matches!(
        tool.on_event(&CanvasEvent::Escape, NONE, &c),
        ToolOutcome::Cancel
    ));
    assert!(matches!(
        tool.on_event(&CanvasEvent::Release(p(10.0, 10.0)), NONE, &c),
        ToolOutcome::Idle
    ));
    let mut out = Vec::new();
    tool.overlay(&mut out);
    assert_eq!(out.len(), 0);
}

#[test]
fn while_dragging_the_tool_previews_the_rectangle() {
    let c = ctx(world().mat);
    let mut tool = RectangleTool::default();
    let mut out = Vec::new();
    tool.overlay(&mut out);
    assert!(out.is_empty(), "nothing before the press");
    assert!(matches!(
        tool.on_event(&CanvasEvent::Press(p(0.0, 0.0)), NONE, &c),
        ToolOutcome::Preview
    ));
    assert!(matches!(
        tool.on_event(&CanvasEvent::Drag(p(10.0, 5.0)), NONE, &c),
        ToolOutcome::Preview
    ));
    tool.overlay(&mut out);
    assert_eq!(out.len(), 1);
    let Overlay::Polyline { points, closed } = &out[0] else {
        panic!("{:?}", out[0])
    };
    assert!(*closed);
    assert_eq!(points.len(), 4);
    let xs: Vec<f64> = points.iter().map(|q| q.x).collect();
    assert!(xs.iter().any(|x| close(*x, 10.0)) && xs.iter().any(|x| close(*x, 0.0)));
}

#[test]
fn a_rectangle_gesture_is_one_undo_step() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = RectangleTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, NONE, &c, p(0.0, 0.0), p(10.0, 10.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    assert!(w.store.undo());
    assert_eq!(w.store.document().mat(w.mat).unwrap().objects.len(), 0);
    assert!(w.store.redo());
    assert_eq!(w.store.document().mat(w.mat).unwrap().objects.len(), 1);
}

#[test]
fn the_tool_can_be_used_again_after_a_commit() {
    let c = ctx(world().mat);
    let mut tool = RectangleTool::default();
    assert!(matches!(
        drag(&mut tool, NONE, &c, p(0.0, 0.0), p(10.0, 10.0)),
        ToolOutcome::Commit(_)
    ));
    assert!(matches!(
        drag(&mut tool, NONE, &c, p(0.0, 0.0), p(10.0, 10.0)),
        ToolOutcome::Commit(_)
    ));
}

// ---- ellipse ----

#[test]
fn dragging_an_ellipse_fills_the_drag_box() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = EllipseTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, NONE, &c, p(0.0, 0.0), p(20.0, 10.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let o = only_object(&w);
    let b = world_bounds(&o);
    assert!(
        close(b.x0, 0.0) && close(b.x1, 20.0) && close(b.y0, 0.0) && close(b.y1, 10.0),
        "{b:?}"
    );
    // Area is pi a b; four cubic arcs approximate it to well under 0.1 %.
    let area = o.paths[0].geom.signed_area().abs();
    assert!(
        (area - std::f64::consts::PI * 10.0 * 5.0).abs() < 0.2,
        "{area}"
    );
    assert!(o.paths[0].geom.closed);
}

#[test]
fn shift_makes_a_circle() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = EllipseTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, SHIFT, &c, p(0.0, 0.0), p(20.0, 8.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let b = world_bounds(&only_object(&w));
    assert!(close(b.width(), 20.0) && close(b.height(), 20.0), "{b:?}");
}

#[test]
fn an_ellipse_from_the_centre_is_centred_on_the_press() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = EllipseTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, ALT, &c, p(50.0, 50.0), p(60.0, 55.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let b = world_bounds(&only_object(&w));
    assert!(
        close(b.x0, 40.0) && close(b.x1, 60.0) && close(b.y0, 45.0) && close(b.y1, 55.0),
        "{b:?}"
    );
}

#[test]
fn a_tiny_ellipse_is_cancelled() {
    let c = ctx(world().mat);
    let mut tool = EllipseTool::default();
    assert!(matches!(
        drag(&mut tool, NONE, &c, p(0.0, 0.0), p(0.01, 0.01)),
        ToolOutcome::Cancel
    ));
}

// ---- select and move ----

fn square(id_ms: u64, x: f64, y: f64) -> (ObjectId, Pickable<ObjectId>) {
    let id = ObjectId::at_unix_ms(id_ms);
    (
        id,
        Pickable {
            id,
            path: Path::rect(x, y, 10.0, 10.0),
            filled: true,
        },
    )
}

fn doc_with(w: &mut World, objects: &[(ObjectId, f64, f64)]) {
    for (id, x, y) in objects {
        let object = PlacedObject {
            id: *id,
            source: None,
            transform: Affine::translate((*x, *y)),
            group: None,
            attach: None,
            paths: Arc::new(vec![Arc::new(ShapePath {
                key: PathKey::source("r"),
                geom: Path::rect(0.0, 0.0, 10.0, 10.0),
                fill_rule: FillRule::NonZero,
                style: SourceStyle::default(),
                ops: vec![],
                start_locked: false,
            })]),
            locked: false,
            hidden: false,
        };
        w.store.apply(&AddObject { mat: w.mat, object }, 0).unwrap();
    }
}

fn select_ctx(
    mat: MatId,
    selected: &[(ObjectId, f64, f64)],
    items: &[Pickable<ObjectId>],
) -> ToolCtx {
    let mut c = ctx(mat);
    c.pickables = items.to_vec();
    c.selected = selected
        .iter()
        .map(|(id, x, y)| (*id, Affine::translate((*x, *y))))
        .collect();
    c
}

#[test]
fn clicking_an_object_selects_it() {
    let (a, pa) = square(1, 0.0, 0.0);
    let (_, pb) = square(2, 20.0, 0.0);
    let c = select_ctx(world().mat, &[], &[pa, pb]);
    let mut tool = SelectTool::default();
    tool.on_event(&CanvasEvent::Press(p(5.0, 5.0)), NONE, &c);
    let out = tool.on_event(&CanvasEvent::Release(p(5.0, 5.0)), NONE, &c);
    assert!(
        matches!(out, ToolOutcome::Select(ref v) if *v == vec![a]),
        "{out:?}"
    );
}

#[test]
fn clicking_empty_space_clears_the_selection() {
    let (_, pa) = square(1, 0.0, 0.0);
    let c = select_ctx(world().mat, &[], &[pa]);
    let mut tool = SelectTool::default();
    tool.on_event(&CanvasEvent::Press(p(100.0, 100.0)), NONE, &c);
    let out = tool.on_event(&CanvasEvent::Release(p(100.0, 100.0)), NONE, &c);
    assert!(
        matches!(out, ToolOutcome::Select(ref v) if v.is_empty()),
        "{out:?}"
    );
}

#[test]
fn shift_clicking_toggles_an_object_in_the_selection() {
    let (_, pa) = square(1, 0.0, 0.0);
    let (b, pb) = square(2, 20.0, 0.0);
    let c = select_ctx(world().mat, &[], &[pa, pb]);
    let mut tool = SelectTool::default();
    tool.on_event(&CanvasEvent::Press(p(25.0, 5.0)), SHIFT, &c);
    let out = tool.on_event(&CanvasEvent::Release(p(25.0, 5.0)), SHIFT, &c);
    assert!(matches!(out, ToolOutcome::Toggle(id) if id == b), "{out:?}");
}

#[test]
fn dragging_a_selected_object_moves_it_by_the_drag() {
    let mut w = world();
    let (a, pa) = square(1, 0.0, 0.0);
    doc_with(&mut w, &[(a, 0.0, 0.0)]);
    let c = select_ctx(w.mat, &[(a, 0.0, 0.0)], &[pa]);
    let mut tool = SelectTool::default();
    tool.on_event(&CanvasEvent::Press(p(5.0, 5.0)), NONE, &c);
    assert!(matches!(
        tool.on_event(&CanvasEvent::Drag(p(15.0, 10.0)), NONE, &c),
        ToolOutcome::Preview
    ));
    let ToolOutcome::Commit(cmds) = tool.on_event(&CanvasEvent::Release(p(15.0, 10.0)), NONE, &c)
    else {
        panic!("expected a commit");
    };
    commit(&mut w, &cmds);
    let o = w.store.document().object(w.mat, a).unwrap().clone();
    let t = o.transform.as_coeffs();
    assert!(close(t[4], 10.0) && close(t[5], 5.0), "{t:?}");
}

#[test]
fn the_move_keeps_an_objects_existing_transform() {
    let mut w = world();
    let (a, pa) = square(1, 30.0, 40.0);
    doc_with(&mut w, &[(a, 30.0, 40.0)]);
    let c = select_ctx(w.mat, &[(a, 30.0, 40.0)], &[pa]);
    let mut tool = SelectTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, NONE, &c, p(35.0, 45.0), p(37.0, 41.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let t = w
        .store
        .document()
        .object(w.mat, a)
        .unwrap()
        .transform
        .as_coeffs();
    assert!(close(t[4], 32.0) && close(t[5], 36.0), "{t:?}");
}

#[test]
fn every_selected_object_moves_together_in_one_undo_step() {
    let mut w = world();
    let (a, pa) = square(1, 0.0, 0.0);
    let (b, pb) = square(2, 20.0, 0.0);
    doc_with(&mut w, &[(a, 0.0, 0.0), (b, 20.0, 0.0)]);
    let c = select_ctx(w.mat, &[(a, 0.0, 0.0), (b, 20.0, 0.0)], &[pa, pb]);
    let mut tool = SelectTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, NONE, &c, p(5.0, 5.0), p(5.0, 15.0)) else {
        panic!();
    };
    assert_eq!(cmds.len(), 2);
    commit(&mut w, &cmds);
    for id in [a, b] {
        let t = w
            .store
            .document()
            .object(w.mat, id)
            .unwrap()
            .transform
            .as_coeffs();
        assert!(close(t[5], 10.0), "{t:?}");
    }
    assert!(w.store.undo());
    for (id, x) in [(a, 0.0), (b, 20.0)] {
        let t = w
            .store
            .document()
            .object(w.mat, id)
            .unwrap()
            .transform
            .as_coeffs();
        assert!(close(t[4], x) && close(t[5], 0.0), "{t:?}");
    }
}

#[test]
fn pressing_an_unselected_object_selects_it_first_and_then_moves_it() {
    let mut w = world();
    let (b, pb) = square(2, 20.0, 0.0);
    doc_with(&mut w, &[(b, 20.0, 0.0)]);
    // Nothing selected when the press arrives.
    let before = select_ctx(w.mat, &[], std::slice::from_ref(&pb));
    let mut tool = SelectTool::default();
    let out = tool.on_event(&CanvasEvent::Press(p(25.0, 5.0)), NONE, &before);
    assert!(
        matches!(out, ToolOutcome::Select(ref v) if *v == vec![b]),
        "{out:?}"
    );
    // The app now reports the new selection with later events.
    let after = select_ctx(w.mat, &[(b, 20.0, 0.0)], &[pb]);
    tool.on_event(&CanvasEvent::Drag(p(33.0, 5.0)), NONE, &after);
    let ToolOutcome::Commit(cmds) =
        tool.on_event(&CanvasEvent::Release(p(33.0, 5.0)), NONE, &after)
    else {
        panic!();
    };
    commit(&mut w, &cmds);
    let t = w
        .store
        .document()
        .object(w.mat, b)
        .unwrap()
        .transform
        .as_coeffs();
    assert!(close(t[4], 28.0), "{t:?}");
}

#[test]
fn a_press_and_release_that_barely_moves_is_a_click_not_a_move() {
    let (_, pa) = square(1, 0.0, 0.0);
    let (a, _) = square(1, 0.0, 0.0);
    let c = select_ctx(world().mat, &[(a, 0.0, 0.0)], &[pa]);
    let mut tool = SelectTool::default();
    tool.on_event(&CanvasEvent::Press(p(5.0, 5.0)), NONE, &c);
    tool.on_event(&CanvasEvent::Drag(p(5.5, 5.5)), NONE, &c);
    let out = tool.on_event(&CanvasEvent::Release(p(5.5, 5.5)), NONE, &c);
    assert!(matches!(out, ToolOutcome::Select(_)), "{out:?}");
}

#[test]
fn the_click_threshold_is_in_screen_pixels() {
    let (a, pa) = square(1, 0.0, 0.0);
    let mut c = select_ctx(world().mat, &[(a, 0.0, 0.0)], &[pa]);
    c.zoom = 10.0;
    let mut tool = SelectTool::default();
    tool.on_event(&CanvasEvent::Press(p(5.0, 5.0)), NONE, &c);
    // 0.5 mm at 10 px/mm is 5 px: a real drag.
    let out = tool.on_event(&CanvasEvent::Release(p(5.5, 5.0)), NONE, &c);
    assert!(matches!(out, ToolOutcome::Commit(_)), "{out:?}");
}

#[test]
fn escape_during_a_move_cancels_it_without_committing() {
    let (a, pa) = square(1, 0.0, 0.0);
    let c = select_ctx(world().mat, &[(a, 0.0, 0.0)], &[pa]);
    let mut tool = SelectTool::default();
    tool.on_event(&CanvasEvent::Press(p(5.0, 5.0)), NONE, &c);
    tool.on_event(&CanvasEvent::Drag(p(15.0, 15.0)), NONE, &c);
    assert!(matches!(
        tool.on_event(&CanvasEvent::Escape, NONE, &c),
        ToolOutcome::Cancel
    ));
    assert!(matches!(
        tool.on_event(&CanvasEvent::Release(p(15.0, 15.0)), NONE, &c),
        ToolOutcome::Idle
    ));
}

#[test]
fn dragging_on_empty_space_draws_a_marquee_and_selects_what_it_encloses() {
    let (a, pa) = square(1, 0.0, 0.0);
    let (_, pb) = square(2, 20.0, 0.0);
    let c = select_ctx(world().mat, &[], &[pa, pb]);
    let mut tool = SelectTool::default();
    tool.on_event(&CanvasEvent::Press(p(-5.0, -5.0)), NONE, &c);
    assert!(matches!(
        tool.on_event(&CanvasEvent::Drag(p(15.0, 15.0)), NONE, &c),
        ToolOutcome::Preview
    ));
    let mut out = Vec::new();
    tool.overlay(&mut out);
    assert!(
        matches!(out[0], Overlay::Marquee { enclosed: true, .. }),
        "{:?}",
        out[0]
    );
    let done = tool.on_event(&CanvasEvent::Release(p(15.0, 15.0)), NONE, &c);
    assert!(
        matches!(done, ToolOutcome::Select(ref v) if *v == vec![a]),
        "{done:?}"
    );
}

#[test]
fn a_marquee_dragged_right_to_left_selects_whatever_it_touches() {
    let (a, pa) = square(1, 0.0, 0.0);
    let c = select_ctx(world().mat, &[], &[pa]);
    let mut tool = SelectTool::default();
    // Left to right across only part of the square encloses nothing.
    tool.on_event(&CanvasEvent::Press(p(-5.0, -5.0)), NONE, &c);
    let none = tool.on_event(&CanvasEvent::Release(p(5.0, 5.0)), NONE, &c);
    assert!(
        matches!(none, ToolOutcome::Select(ref v) if v.is_empty()),
        "{none:?}"
    );
    // Right to left, starting on empty space, over the same part crosses it.
    tool.on_event(&CanvasEvent::Press(p(20.0, 20.0)), NONE, &c);
    let some = tool.on_event(&CanvasEvent::Release(p(5.0, 5.0)), NONE, &c);
    assert!(
        matches!(some, ToolOutcome::Select(ref v) if *v == vec![a]),
        "{some:?}"
    );
}

#[test]
fn a_marquee_with_shift_adds_to_the_selection() {
    let (_, pa) = square(1, 0.0, 0.0);
    let (b, pb) = square(2, 20.0, 0.0);
    let c = select_ctx(world().mat, &[], &[pa, pb]);
    let mut tool = SelectTool::default();
    tool.on_event(&CanvasEvent::Press(p(15.0, -5.0)), SHIFT, &c);
    let out = tool.on_event(&CanvasEvent::Release(p(35.0, 15.0)), SHIFT, &c);
    assert!(
        matches!(out, ToolOutcome::Add(ref v) if *v == vec![b]),
        "{out:?}"
    );
}

// ---- the tool set ----

#[test]
fn every_tool_id_has_a_command_and_a_cursor() {
    for id in ToolId::ALL {
        assert!(id.command_id().starts_with("tool."), "{id:?}");
        let _ = id.cursor();
    }
    assert_eq!(
        ToolId::from_command("tool.rectangle"),
        Some(ToolId::Rectangle)
    );
    assert_eq!(ToolId::from_command("tool.select"), Some(ToolId::Select));
    assert_eq!(ToolId::from_command("file.save"), None);
}

#[test]
fn the_tool_ids_match_the_registered_commands() {
    let registry = crate::Registry::new(&std::collections::BTreeMap::new());
    for id in ToolId::ALL {
        assert!(registry.get(id.command_id()).is_some(), "{id:?}");
    }
}

#[test]
fn a_tool_made_from_its_id_reports_that_id() {
    for id in ToolId::ALL {
        assert_eq!(id.new_tool().id(), id);
    }
}
// ---- line, polygon, star, arc and pen ----

fn only_path_geometry(w: &World) -> ps_geometry::Path {
    only_object(w).paths[0].geom.clone()
}

fn click(tool: &mut dyn Tool, c: &ToolCtx, at: Point) -> ToolOutcome {
    tool.on_event(&CanvasEvent::Press(at), NONE, c);
    tool.on_event(&CanvasEvent::Release(at), NONE, c)
}

#[test]
fn dragging_a_line_adds_an_open_two_point_path() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = LineTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, NONE, &c, p(10.0, 10.0), p(40.0, 30.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let o = only_object(&w);
    let g = &o.paths[0].geom;
    assert!(!g.closed);
    let ends = (o.transform * g.start, o.transform * g.end());
    assert!(close(ends.0.x, 10.0) && close(ends.0.y, 10.0), "{ends:?}");
    assert!(close(ends.1.x, 40.0) && close(ends.1.y, 30.0), "{ends:?}");
}

#[test]
fn shift_snaps_a_line_to_a_multiple_of_forty_five_degrees() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = LineTool::default();
    // Dragged at about 10 degrees: snaps to horizontal, keeping the length.
    let ToolOutcome::Commit(cmds) = drag(&mut tool, SHIFT, &c, p(0.0, 0.0), p(30.0, 5.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let o = only_object(&w);
    let end = o.transform * o.paths[0].geom.end();
    assert!(close(end.y, 0.0), "{end:?}");
    assert!(
        (end.x - (30.0_f64.powi(2) + 25.0).sqrt()).abs() < 0.01,
        "{end:?}"
    );
}

#[test]
fn a_zero_length_line_is_cancelled() {
    let c = ctx(world().mat);
    let mut tool = LineTool::default();
    assert!(matches!(
        drag(&mut tool, NONE, &c, p(5.0, 5.0), p(5.0, 5.05)),
        ToolOutcome::Cancel
    ));
}

#[test]
fn a_polygon_has_the_requested_number_of_sides_inscribed_in_the_drag_radius() {
    let mut w = world();
    let mut c = ctx(w.mat);
    c.sides = 6;
    let mut tool = PolygonTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, NONE, &c, p(50.0, 50.0), p(60.0, 50.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let g = only_path_geometry(&w);
    assert!(g.closed);
    // A regular hexagon of circumradius 10 has area 3 sqrt(3) / 2 * 100.
    let area = g.signed_area().abs();
    assert!(
        (area - 3.0 * 3.0_f64.sqrt() / 2.0 * 100.0).abs() < 0.01,
        "{area}"
    );
    assert_eq!(ps_geometry::node_count(&g), 6);
}

#[test]
fn a_polygon_is_centred_on_the_press() {
    let mut w = world();
    let mut c = ctx(w.mat);
    c.sides = 4;
    let mut tool = PolygonTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, NONE, &c, p(50.0, 50.0), p(60.0, 50.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let b = world_bounds(&only_object(&w));
    assert!(
        close(b.x0.midpoint(b.x1), 50.0) && close(b.y0.midpoint(b.y1), 50.0),
        "{b:?}"
    );
}

#[test]
fn shift_keeps_a_polygon_upright() {
    let mut w = world();
    let mut c = ctx(w.mat);
    c.sides = 3;
    let mut tool = PolygonTool::default();
    // Dragged sideways, but with Shift the first corner is straight up from the centre.
    let ToolOutcome::Commit(cmds) = drag(&mut tool, SHIFT, &c, p(0.0, 0.0), p(10.0, 0.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let o = only_object(&w);
    let first = o.transform * o.paths[0].geom.start;
    assert!(close(first.x, 0.0) && close(first.y, -10.0), "{first:?}");
}

#[test]
fn a_star_alternates_outer_and_inner_points() {
    let mut w = world();
    let mut c = ctx(w.mat);
    c.sides = 5;
    c.star_ratio = 0.5;
    let mut tool = StarTool::default();
    let ToolOutcome::Commit(cmds) = drag(&mut tool, NONE, &c, p(0.0, 0.0), p(10.0, 0.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let g = only_path_geometry(&w);
    assert_eq!(ps_geometry::node_count(&g), 10);
    // Area of an n-point star with outer radius R and inner radius r is n R r sin(pi / n).
    let expected = 5.0 * 10.0 * 5.0 * (std::f64::consts::PI / 5.0).sin();
    assert!(
        (g.signed_area().abs() - expected).abs() < 0.01,
        "{}",
        g.signed_area()
    );
}

#[test]
fn three_clicks_make_an_arc_through_the_third_point() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = ArcTool::default();
    let r = 10.0_f64;
    let mid = r / 2.0_f64.sqrt();
    assert!(matches!(
        click(&mut tool, &c, p(10.0, 0.0)),
        ToolOutcome::Preview
    ));
    assert!(matches!(
        click(&mut tool, &c, p(0.0, 10.0)),
        ToolOutcome::Preview
    ));
    let ToolOutcome::Commit(cmds) = click(&mut tool, &c, p(mid, mid)) else {
        panic!("the third click commits");
    };
    commit(&mut w, &cmds);
    let o = only_object(&w);
    let g = &o.paths[0].geom;
    assert!(!g.closed);
    // A quarter of a circle of radius 10.
    assert!(
        (g.length() - std::f64::consts::PI * r / 2.0).abs() < 1e-6,
        "{}",
        g.length()
    );
    let (a, b) = (o.transform * g.start, o.transform * g.end());
    assert!(
        close(a.x, 10.0) && close(a.y, 0.0) && close(b.x, 0.0) && close(b.y, 10.0),
        "{a:?} {b:?}"
    );
}

#[test]
fn three_collinear_points_make_no_arc() {
    let c = ctx(world().mat);
    let mut tool = ArcTool::default();
    click(&mut tool, &c, p(0.0, 0.0));
    click(&mut tool, &c, p(10.0, 0.0));
    assert!(matches!(
        click(&mut tool, &c, p(5.0, 0.0)),
        ToolOutcome::Cancel
    ));
}

#[test]
fn escape_abandons_a_half_made_arc() {
    let c = ctx(world().mat);
    let mut tool = ArcTool::default();
    click(&mut tool, &c, p(0.0, 0.0));
    assert!(matches!(
        tool.on_event(&CanvasEvent::Escape, NONE, &c),
        ToolOutcome::Cancel
    ));
    let mut overlay = Vec::new();
    tool.overlay(&mut overlay);
    assert_eq!(overlay.len(), 0);
}

#[test]
fn clicking_with_the_pen_places_corner_nodes_and_enter_finishes_an_open_path() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = PenTool::default();
    for at in [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)] {
        assert!(matches!(click(&mut tool, &c, at), ToolOutcome::Preview));
    }
    let ToolOutcome::Commit(cmds) = tool.on_event(&CanvasEvent::Enter, NONE, &c) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let o = only_object(&w);
    let g = &o.paths[0].geom;
    assert!(!g.closed);
    assert_eq!(ps_geometry::node_count(g), 3);
    assert!(
        g.segs
            .iter()
            .all(|s| matches!(s, ps_geometry::Seg::Line { .. }))
    );
    let last = o.transform * g.end();
    assert!(close(last.x, 10.0) && close(last.y, 10.0));
}

#[test]
fn dragging_with_the_pen_pulls_out_symmetric_handles() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = PenTool::default();
    // Press at (0,0), drag to (0,10): the outgoing handle is (0,10) from the node.
    tool.on_event(&CanvasEvent::Press(p(0.0, 0.0)), NONE, &c);
    tool.on_event(&CanvasEvent::Drag(p(0.0, 10.0)), NONE, &c);
    tool.on_event(&CanvasEvent::Release(p(0.0, 10.0)), NONE, &c);
    click(&mut tool, &c, p(20.0, 0.0));
    let ToolOutcome::Commit(cmds) = tool.on_event(&CanvasEvent::Enter, NONE, &c) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let o = only_object(&w);
    let ps_geometry::Seg::Cubic { c1, c2, to } = o.paths[0].geom.segs[0] else {
        panic!("a handle makes the segment a curve");
    };
    let first = o.paths[0].geom.start;
    assert!(
        close(c1.x - first.x, 0.0) && close(c1.y - first.y, 10.0),
        "{c1:?} from {first:?}"
    );
    assert!(
        close(c2.x, to.x) && close(c2.y, to.y),
        "a corner end has no incoming handle"
    );
}

#[test]
fn clicking_the_first_node_closes_the_path() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = PenTool::default();
    for at in [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)] {
        click(&mut tool, &c, at);
    }
    let ToolOutcome::Commit(cmds) = click(&mut tool, &c, p(0.5, 0.5)) else {
        panic!("a click on the first node finishes a closed shape");
    };
    commit(&mut w, &cmds);
    let g = only_path_geometry(&w);
    assert!(g.closed);
    assert_eq!(ps_geometry::node_count(&g), 3);
}

#[test]
fn the_pen_needs_two_nodes_and_escape_discards_everything() {
    let c = ctx(world().mat);
    let mut tool = PenTool::default();
    click(&mut tool, &c, p(0.0, 0.0));
    assert!(matches!(
        tool.on_event(&CanvasEvent::Enter, NONE, &c),
        ToolOutcome::Cancel
    ));
    click(&mut tool, &c, p(0.0, 0.0));
    click(&mut tool, &c, p(5.0, 5.0));
    assert!(matches!(
        tool.on_event(&CanvasEvent::Escape, NONE, &c),
        ToolOutcome::Cancel
    ));
    let mut overlay = Vec::new();
    tool.overlay(&mut overlay);
    assert_eq!(overlay.len(), 0);
}

#[test]
fn the_pen_previews_the_path_so_far_with_a_rubber_band() {
    let c = ctx(world().mat);
    let mut tool = PenTool::default();
    click(&mut tool, &c, p(0.0, 0.0));
    click(&mut tool, &c, p(10.0, 0.0));
    tool.on_event(&CanvasEvent::Hover(p(10.0, 10.0)), NONE, &c);
    let mut overlay = Vec::new();
    tool.overlay(&mut overlay);
    let Some(Overlay::Polyline { points, closed }) = overlay.first() else {
        panic!("{overlay:?}");
    };
    assert!(!closed);
    let last = points.last().unwrap();
    assert!(
        close(last.x, 10.0) && close(last.y, 10.0),
        "the rubber band ends at the pointer"
    );
}

#[test]
fn the_new_tools_have_commands_and_cursors() {
    for (id, command) in [
        (ToolId::Line, "tool.line"),
        (ToolId::Polygon, "tool.polygon"),
        (ToolId::Star, "tool.star"),
        (ToolId::Arc, "tool.arc"),
        (ToolId::Pen, "tool.pen"),
    ] {
        assert_eq!(id.command_id(), command);
        assert_eq!(ToolId::from_command(command), Some(id));
        assert_eq!(id.cursor(), Cursor::Crosshair);
        assert_eq!(id.new_tool().id(), id);
    }
}
// ---- node editing ----

fn node_world(origin: (f64, f64)) -> (World, ToolCtx, ObjectId, PathKey) {
    let mut w = world();
    let id = ObjectId::at_unix_ms(900);
    let key = PathKey::source("n");
    let geom = ps_geometry::Path::polyline(&[p(0.0, 0.0), p(20.0, 0.0), p(20.0, 20.0)], false);
    let object = PlacedObject {
        id,
        source: None,
        transform: Affine::translate(origin),
        group: None,
        attach: None,
        paths: Arc::new(vec![Arc::new(ShapePath {
            key: key.clone(),
            geom: geom.clone(),
            fill_rule: FillRule::NonZero,
            style: SourceStyle::default(),
            ops: vec![],
            start_locked: false,
        })]),
        locked: false,
        hidden: false,
    };
    w.store.apply(&AddObject { mat: w.mat, object }, 0).unwrap();
    let mut c = ctx(w.mat);
    c.selected = vec![(id, Affine::translate(origin))];
    c.edit_paths = vec![EditPath {
        object: id,
        path: key.clone(),
        geom,
        transform: Affine::translate(origin),
    }];
    (w, c, id, key)
}

fn geometry(w: &World, id: ObjectId) -> ps_geometry::Path {
    w.store.document().object(w.mat, id).unwrap().paths[0]
        .geom
        .clone()
}

#[test]
fn pressing_a_node_selects_it() {
    let (_w, c, id, key) = node_world((100.0, 100.0));
    let mut tool = NodeTool::default();
    // The middle node is at (20, 0) in the object, so (120, 100) in the world.
    let out = tool.on_event(&CanvasEvent::Press(p(120.2, 100.1)), NONE, &c);
    let ToolOutcome::SelectNode(item) = out else {
        panic!("{out:?}");
    };
    assert_eq!(item.object, id);
    assert_eq!(item.path, Some(key));
    assert_eq!(item.node, Some(1));
}

#[test]
fn dragging_a_node_moves_it_in_the_objects_own_space() {
    let (mut w, c, id, _) = node_world((100.0, 100.0));
    let mut tool = NodeTool::default();
    tool.on_event(&CanvasEvent::Press(p(120.0, 100.0)), NONE, &c);
    assert!(matches!(
        tool.on_event(&CanvasEvent::Drag(p(125.0, 107.0)), NONE, &c),
        ToolOutcome::Preview
    ));
    let ToolOutcome::Commit(cmds) = tool.on_event(&CanvasEvent::Release(p(125.0, 107.0)), NONE, &c)
    else {
        panic!("a drag commits");
    };
    assert_eq!(cmds.len(), 1);
    commit(&mut w, &cmds);
    let g = geometry(&w, id);
    let moved = ps_geometry::node_position(&g, 1).unwrap();
    assert!(close(moved.x, 25.0) && close(moved.y, 7.0), "{moved:?}");
    let first = ps_geometry::node_position(&g, 0).unwrap();
    assert!(
        close(first.x, 0.0) && close(first.y, 0.0),
        "the other nodes stay"
    );
}

#[test]
fn a_click_on_a_node_without_dragging_changes_nothing_but_the_selection() {
    let (_w, c, _, _) = node_world((0.0, 0.0));
    let mut tool = NodeTool::default();
    tool.on_event(&CanvasEvent::Press(p(20.0, 0.0)), NONE, &c);
    let out = tool.on_event(&CanvasEvent::Release(p(20.0, 0.0)), NONE, &c);
    assert!(matches!(out, ToolOutcome::Idle), "{out:?}");
}

#[test]
fn escape_during_a_node_drag_cancels_it() {
    let (_w, c, _, _) = node_world((0.0, 0.0));
    let mut tool = NodeTool::default();
    tool.on_event(&CanvasEvent::Press(p(20.0, 0.0)), NONE, &c);
    tool.on_event(&CanvasEvent::Drag(p(30.0, 5.0)), NONE, &c);
    assert!(matches!(
        tool.on_event(&CanvasEvent::Escape, NONE, &c),
        ToolOutcome::Cancel
    ));
    assert!(matches!(
        tool.on_event(&CanvasEvent::Release(p(30.0, 5.0)), NONE, &c),
        ToolOutcome::Idle
    ));
}

#[test]
fn while_dragging_the_overlay_shows_the_path_with_the_node_moved() {
    let (_w, c, _, _) = node_world((0.0, 0.0));
    let mut tool = NodeTool::default();
    tool.on_event(&CanvasEvent::Press(p(20.0, 0.0)), NONE, &c);
    tool.on_event(&CanvasEvent::Drag(p(30.0, 5.0)), NONE, &c);
    let mut overlay = Vec::new();
    tool.overlay(&mut overlay);
    let line = overlay
        .iter()
        .find_map(|o| match o {
            Overlay::Polyline { points, .. } => Some(points.clone()),
            _ => None,
        })
        .expect("the edited outline");
    assert!(
        line.iter().any(|q| close(q.x, 30.0) && close(q.y, 5.0)),
        "{line:?}"
    );
}

#[test]
fn the_node_tool_shows_a_handle_on_every_node_of_the_selected_paths() {
    let (_w, c, _, _) = node_world((100.0, 100.0));
    let mut tool = NodeTool::default();
    tool.on_event(&CanvasEvent::Hover(p(0.0, 0.0)), NONE, &c);
    let mut overlay = Vec::new();
    tool.overlay(&mut overlay);
    let Some(Overlay::Handles { points, .. }) = overlay
        .iter()
        .find(|o| matches!(o, Overlay::Handles { .. }))
    else {
        panic!("{overlay:?}");
    };
    assert_eq!(points.len(), 3);
    assert!(
        points
            .iter()
            .any(|q| close(q.x, 120.0) && close(q.y, 100.0))
    );
}

#[test]
fn a_double_click_on_a_segment_inserts_a_node_there() {
    let (mut w, c, id, _) = node_world((100.0, 100.0));
    let mut tool = NodeTool::default();
    // On the first segment, 10 along it: world (110, 100).
    let ToolOutcome::Commit(cmds) =
        tool.on_event(&CanvasEvent::DoubleClick(p(110.0, 100.5)), NONE, &c)
    else {
        panic!("the double click inserts");
    };
    commit(&mut w, &cmds);
    let g = geometry(&w, id);
    assert_eq!(ps_geometry::node_count(&g), 4);
    let new_node = ps_geometry::node_position(&g, 1).unwrap();
    assert!(
        close(new_node.x, 10.0) && close(new_node.y, 0.0),
        "{new_node:?}"
    );
}

#[test]
fn a_double_click_away_from_the_path_inserts_nothing() {
    let (_w, c, _, _) = node_world((0.0, 0.0));
    let mut tool = NodeTool::default();
    assert!(matches!(
        tool.on_event(&CanvasEvent::DoubleClick(p(200.0, 200.0)), NONE, &c),
        ToolOutcome::Idle
    ));
}

#[test]
fn nothing_selected_means_nothing_to_edit() {
    let mut c = ctx(world().mat);
    c.zoom = 1.0;
    let mut tool = NodeTool::default();
    assert!(matches!(
        tool.on_event(&CanvasEvent::Press(p(0.0, 0.0)), NONE, &c),
        ToolOutcome::Idle
    ));
}

#[test]
fn the_node_tool_is_a_regular_tool() {
    assert_eq!(ToolId::Node.command_id(), "tool.node");
    assert_eq!(ToolId::from_command("tool.node"), Some(ToolId::Node));
    assert_eq!(ToolId::Node.new_tool().id(), ToolId::Node);
}

// ---- arc from its centre ----

#[test]
fn centre_start_end_clicks_make_an_arc_about_the_centre() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = ArcCenterTool::default();
    assert!(matches!(
        click(&mut tool, &c, p(0.0, 0.0)),
        ToolOutcome::Preview
    ));
    assert!(matches!(
        click(&mut tool, &c, p(10.0, 0.0)),
        ToolOutcome::Preview
    ));
    let ToolOutcome::Commit(cmds) = click(&mut tool, &c, p(0.0, 10.0)) else {
        panic!("the third click commits");
    };
    commit(&mut w, &cmds);
    let o = only_object(&w);
    let g = &o.paths[0].geom;
    assert!(!g.closed);
    assert!(
        (g.length() - std::f64::consts::PI * 10.0 / 2.0).abs() < 1e-6,
        "a quarter of a circle of radius 10: {}",
        g.length()
    );
    let (a, b) = (o.transform * g.start, o.transform * g.end());
    assert!(
        close(a.x, 10.0) && close(a.y, 0.0) && close(b.x, 0.0) && close(b.y, 10.0),
        "{a:?} {b:?}"
    );
}

#[test]
fn the_end_click_only_gives_the_direction_the_radius_stays() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = ArcCenterTool::default();
    click(&mut tool, &c, p(0.0, 0.0));
    click(&mut tool, &c, p(10.0, 0.0));
    let ToolOutcome::Commit(cmds) = click(&mut tool, &c, p(0.0, 25.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let o = only_object(&w);
    let end = o.transform * o.paths[0].geom.end();
    assert!(close(end.x, 0.0) && close(end.y, 10.0), "{end:?}");
}

#[test]
fn the_arc_goes_the_short_way_round_to_the_end() {
    let mut w = world();
    let c = ctx(w.mat);
    let mut tool = ArcCenterTool::default();
    click(&mut tool, &c, p(0.0, 0.0));
    click(&mut tool, &c, p(10.0, 0.0));
    let ToolOutcome::Commit(cmds) = click(&mut tool, &c, p(0.0, -10.0)) else {
        panic!();
    };
    commit(&mut w, &cmds);
    let o = only_object(&w);
    assert!(
        (o.paths[0].geom.length() - std::f64::consts::PI * 10.0 / 2.0).abs() < 1e-6,
        "{}",
        o.paths[0].geom.length()
    );
}

#[test]
fn a_start_click_on_the_centre_makes_no_arc() {
    let c = ctx(world().mat);
    let mut tool = ArcCenterTool::default();
    click(&mut tool, &c, p(5.0, 5.0));
    click(&mut tool, &c, p(5.0, 5.0));
    assert!(matches!(
        click(&mut tool, &c, p(9.0, 9.0)),
        ToolOutcome::Cancel
    ));
}

#[test]
fn escape_abandons_a_half_made_centre_arc() {
    let c = ctx(world().mat);
    let mut tool = ArcCenterTool::default();
    click(&mut tool, &c, p(0.0, 0.0));
    assert!(matches!(
        tool.on_event(&CanvasEvent::Escape, NONE, &c),
        ToolOutcome::Cancel
    ));
    let mut overlay = Vec::new();
    tool.overlay(&mut overlay);
    assert_eq!(overlay.len(), 0);
}

// ---- the knife ----

fn knife_world() -> (World, ToolCtx, ObjectId, PathKey) {
    let mut w = world();
    let id = ObjectId::at_unix_ms(901);
    let key = PathKey::source("k");
    let geom = ps_geometry::Path::rect(0.0, 0.0, 20.0, 10.0);
    let object = PlacedObject {
        id,
        source: None,
        transform: Affine::translate((100.0, 50.0)),
        group: None,
        attach: None,
        paths: Arc::new(vec![Arc::new(ShapePath {
            key: key.clone(),
            geom: geom.clone(),
            fill_rule: FillRule::NonZero,
            style: SourceStyle::default(),
            ops: vec![],
            start_locked: false,
        })]),
        locked: false,
        hidden: false,
    };
    w.store.apply(&AddObject { mat: w.mat, object }, 0).unwrap();
    let mut c = ctx(w.mat);
    c.selected = vec![(id, Affine::translate((100.0, 50.0)))];
    c.edit_paths = vec![EditPath {
        object: id,
        path: key.clone(),
        geom,
        transform: Affine::translate((100.0, 50.0)),
    }];
    (w, c, id, key)
}

#[test]
fn dragging_a_line_across_the_selected_shape_cuts_it_in_two() {
    let (mut w, c, id, _) = knife_world();
    let mut tool = KnifeTool::default();
    // World x = 108 is 8 mm into the 20 mm wide shape.
    let out = drag(&mut tool, NONE, &c, p(108.0, 40.0), p(108.0, 70.0));
    let ToolOutcome::Commit(cmds) = out else {
        panic!("{out:?}");
    };
    commit(&mut w, &cmds);
    let doc = w.store.document();
    let paths = &doc.object(w.mat, id).unwrap().paths;
    assert_eq!(paths.len(), 2);
    let mut areas: Vec<f64> = paths.iter().map(|p| p.geom.signed_area().abs()).collect();
    areas.sort_by(f64::total_cmp);
    assert!(
        (areas[0] - 80.0).abs() < 1e-6 && (areas[1] - 120.0).abs() < 1e-6,
        "{areas:?}"
    );
    assert_ne!(paths[0].key, paths[1].key);
}

#[test]
fn a_cut_that_misses_the_shape_does_nothing() {
    let (_w, c, _, _) = knife_world();
    let mut tool = KnifeTool::default();
    assert!(matches!(
        drag(&mut tool, NONE, &c, p(300.0, 40.0), p(300.0, 70.0)),
        ToolOutcome::Cancel
    ));
}

#[test]
fn with_nothing_selected_the_knife_cuts_nothing() {
    let (_w, mut c, _, _) = knife_world();
    c.edit_paths.clear();
    let mut tool = KnifeTool::default();
    assert!(matches!(
        drag(&mut tool, NONE, &c, p(108.0, 40.0), p(108.0, 70.0)),
        ToolOutcome::Cancel
    ));
}

#[test]
fn a_click_without_a_drag_is_not_a_cut() {
    let (_w, c, _, _) = knife_world();
    let mut tool = KnifeTool::default();
    tool.on_event(&CanvasEvent::Press(p(108.0, 55.0)), NONE, &c);
    assert!(matches!(
        tool.on_event(&CanvasEvent::Release(p(108.0, 55.0)), NONE, &c),
        ToolOutcome::Cancel
    ));
}

#[test]
fn the_knife_shows_its_line_while_dragging() {
    let (_w, c, _, _) = knife_world();
    let mut tool = KnifeTool::default();
    tool.on_event(&CanvasEvent::Press(p(108.0, 40.0)), NONE, &c);
    assert!(matches!(
        tool.on_event(&CanvasEvent::Drag(p(108.0, 70.0)), NONE, &c),
        ToolOutcome::Preview
    ));
    let mut overlay = Vec::new();
    tool.overlay(&mut overlay);
    assert_eq!(overlay.len(), 1);
}

#[test]
fn the_new_tools_have_commands_cursors_and_ids() {
    for (id, command) in [
        (ToolId::ArcCenter, "tool.arc-center"),
        (ToolId::Knife, "tool.knife"),
    ] {
        assert_eq!(id.command_id(), command);
        assert_eq!(ToolId::from_command(command), Some(id));
        assert_eq!(id.cursor(), Cursor::Crosshair);
        assert_eq!(id.new_tool().id(), id);
    }
}

// ---- guides ----

fn guide_ctx(mat: MatId, guides: Vec<ps_model::Guide>) -> ToolCtx {
    ToolCtx {
        guides,
        zoom: 2.0,
        ..ctx(mat)
    }
}

fn applied(w: &mut World, cmds: &[Box<dyn ps_model::Command>]) -> Vec<ps_model::Guide> {
    commit(w, cmds);
    w.store.document().mat(w.mat).unwrap().guides.to_vec()
}

#[test]
fn clicking_adds_a_horizontal_guide_at_the_height_clicked() {
    let mut w = world();
    let c = guide_ctx(w.mat, Vec::new());
    let mut tool = GuideTool::default();
    let ToolOutcome::Commit(cmds) = click(&mut tool, &c, p(40.0, 25.0)) else {
        panic!("a click commits");
    };
    let guides = applied(&mut w, &cmds);
    assert_eq!(
        guides,
        vec![ps_model::Guide {
            axis: ps_model::GuideAxis::Horizontal,
            at_mm: 25.0
        }]
    );
}

#[test]
fn shift_clicking_adds_a_vertical_guide() {
    let mut w = world();
    let c = guide_ctx(w.mat, Vec::new());
    let mut tool = GuideTool::default();
    tool.on_event(&CanvasEvent::Press(p(40.0, 25.0)), SHIFT, &c);
    let ToolOutcome::Commit(cmds) = tool.on_event(&CanvasEvent::Release(p(40.0, 25.0)), SHIFT, &c)
    else {
        panic!();
    };
    let guides = applied(&mut w, &cmds);
    assert_eq!(
        guides,
        vec![ps_model::Guide {
            axis: ps_model::GuideAxis::Vertical,
            at_mm: 40.0
        }]
    );
}

#[test]
fn clicking_on_a_guide_removes_it() {
    let mut w = world();
    let existing = ps_model::Guide {
        axis: ps_model::GuideAxis::Horizontal,
        at_mm: 25.0,
    };
    let mat = w.mat;
    commit(
        &mut w,
        &[Box::new(ps_model::SetGuides {
            mat,
            guides: vec![existing],
        })],
    );
    let c = guide_ctx(w.mat, vec![existing]);
    let mut tool = GuideTool::default();
    // 1 mm away at 2 px/mm is 2 px: within the click tolerance.
    let ToolOutcome::Commit(cmds) = click(&mut tool, &c, p(40.0, 26.0)) else {
        panic!();
    };
    assert_eq!(applied(&mut w, &cmds), Vec::new());
}

#[test]
fn a_new_guide_keeps_the_existing_ones() {
    let mut w = world();
    let existing = ps_model::Guide {
        axis: ps_model::GuideAxis::Vertical,
        at_mm: 10.0,
    };
    let mat = w.mat;
    commit(
        &mut w,
        &[Box::new(ps_model::SetGuides {
            mat,
            guides: vec![existing],
        })],
    );
    let c = guide_ctx(w.mat, vec![existing]);
    let mut tool = GuideTool::default();
    let ToolOutcome::Commit(cmds) = click(&mut tool, &c, p(80.0, 60.0)) else {
        panic!();
    };
    assert_eq!(applied(&mut w, &cmds).len(), 2);
}

#[test]
fn the_guide_tool_has_a_command_and_cursor() {
    assert_eq!(ToolId::Guide.command_id(), "tool.guide");
    assert_eq!(ToolId::from_command("tool.guide"), Some(ToolId::Guide));
    assert_eq!(ToolId::Guide.cursor(), Cursor::Crosshair);
    assert_eq!(ToolId::Guide.new_tool().id(), ToolId::Guide);
}

// ---- the start-point tool ----

#[test]
fn clicking_near_a_corner_makes_it_the_start_of_the_selected_closed_path() {
    let (mut w, c, id, key) = knife_world();
    let mut tool = StartPointTool::default();
    // The object sits at (100, 50); its far corner in the mat is (120, 60).
    let outcome = tool.on_event(
        &CanvasEvent::Press(Point::new(119.0, 59.0)),
        Mods::default(),
        &c,
    );
    let ToolOutcome::Commit(cmds) = outcome else {
        panic!("a click commits");
    };
    commit(&mut w, &cmds);
    let doc = w.store.document();
    let path = doc
        .object(w.mat, id)
        .unwrap()
        .paths
        .iter()
        .find(|p| p.key == key)
        .cloned()
        .unwrap();
    assert!(path.start_locked);
    assert_eq!(
        path.geom.start,
        Point::new(20.0, 10.0),
        "in the object's own space"
    );
}

#[test]
fn clicking_on_the_middle_of_an_edge_starts_the_path_there_and_hover_shows_that_point() {
    let (mut w, c, id, key) = knife_world();
    let mut tool = StartPointTool::default();
    // The object sits at (100, 50) and is 20 x 10; the middle of its top edge is (110, 50) on the
    // mat. Click a little off the edge.
    let at = Point::new(110.0, 51.0);
    tool.on_event(&CanvasEvent::Hover(at), Mods::default(), &c);
    let mut overlay = Vec::new();
    tool.overlay(&mut overlay);
    assert!(
        matches!(overlay.as_slice(), [Overlay::Marker { at }] if (at.x - 110.0).abs() < 1e-6 && (at.y - 50.0).abs() < 1e-6),
        "the marker sits on the edge, not on a corner: {overlay:?}"
    );
    let ToolOutcome::Commit(cmds) = tool.on_event(&CanvasEvent::Press(at), Mods::default(), &c)
    else {
        panic!("a click commits");
    };
    commit(&mut w, &cmds);
    let doc = w.store.document();
    let path = doc
        .object(w.mat, id)
        .unwrap()
        .paths
        .iter()
        .find(|p| p.key == key)
        .cloned()
        .unwrap();
    assert!(path.start_locked);
    assert!(
        (path.geom.start.x - 10.0).abs() < 1e-6,
        "{:?}",
        path.geom.start
    );
}

#[test]
fn the_start_point_tool_ignores_open_paths_and_empty_selections() {
    let (_w, mut c, _, _) = knife_world();
    let mut tool = StartPointTool::default();
    c.edit_paths[0].geom =
        ps_geometry::Path::polyline(&[Point::new(0.0, 0.0), Point::new(5.0, 0.0)], false);
    assert!(matches!(
        tool.on_event(
            &CanvasEvent::Press(Point::new(100.0, 50.0)),
            Mods::default(),
            &c
        ),
        ToolOutcome::Idle | ToolOutcome::Cancel
    ));
    c.edit_paths.clear();
    assert!(matches!(
        tool.on_event(
            &CanvasEvent::Press(Point::new(100.0, 50.0)),
            Mods::default(),
            &c
        ),
        ToolOutcome::Idle | ToolOutcome::Cancel
    ));
}
// ---- moving with snapping, scaling and rotating from the selection box ----

/// Snapping on, zoomed out far enough that the grid is not in play (only objects are targets).
fn snapping(mut c: ToolCtx) -> ToolCtx {
    c.snapping = true;
    c.zoom = 0.5;
    c
}

fn commit_one(w: &mut World, out: ToolOutcome, id: ObjectId) -> Affine {
    let ToolOutcome::Commit(cmds) = out else {
        panic!("expected a commit, got {out:?}");
    };
    commit(w, &cmds);
    w.store.document().object(w.mat, id).unwrap().transform
}

#[test]
fn a_moving_object_snaps_its_edge_to_another_objects_edge() {
    let mut w = world();
    let (a, pa) = square(1, 0.0, 0.0);
    let (b, pb) = square(2, 50.0, 0.0);
    doc_with(&mut w, &[(a, 0.0, 0.0), (b, 50.0, 0.0)]);
    let c = snapping(select_ctx(w.mat, &[(a, 0.0, 0.0)], &[pa, pb]));
    let mut tool = SelectTool::default();
    // The square is 10 wide; dropping it 36.8 over puts its right edge at 46.8, 3.2 short of the
    // other square's left edge at 50, which is within the snap distance at zoom 1.
    let out = drag(&mut tool, NONE, &c, p(5.0, 5.0), p(41.8, 5.0));
    let t = commit_one(&mut w, out, a).as_coeffs();
    assert!(close(t[4], 40.0), "snapped so the edges meet: {t:?}");
}

#[test]
fn snapping_is_off_when_the_state_says_so_or_alt_is_held() {
    let (a, pa) = square(1, 0.0, 0.0);
    let (b, pb) = square(2, 50.0, 0.0);
    let run = |snap: bool, mods: Mods| {
        let mut w = world();
        doc_with(&mut w, &[(a, 0.0, 0.0), (b, 50.0, 0.0)]);
        let mut c = select_ctx(w.mat, &[(a, 0.0, 0.0)], &[pa.clone(), pb.clone()]);
        c.snapping = snap;
        c.zoom = 0.5;
        let mut tool = SelectTool::default();
        let out = drag(&mut tool, mods, &c, p(5.0, 5.0), p(41.8, 5.0));
        commit_one(&mut w, out, a).as_coeffs()[4]
    };
    assert!(close(run(false, NONE), 36.8), "free");
    assert!(
        close(run(true, Mods { alt: true, ..NONE }), 36.8),
        "alt skips snapping"
    );
    assert!(close(run(true, NONE), 40.0), "and it does snap otherwise");
}

#[test]
fn shift_keeps_a_move_on_one_axis() {
    let mut w = world();
    let (a, pa) = square(1, 0.0, 0.0);
    doc_with(&mut w, &[(a, 0.0, 0.0)]);
    let c = select_ctx(w.mat, &[(a, 0.0, 0.0)], &[pa]);
    let mut tool = SelectTool::default();
    let out = drag(&mut tool, SHIFT, &c, p(5.0, 5.0), p(35.0, 12.0));
    let t = commit_one(&mut w, out, a).as_coeffs();
    assert!(close(t[4], 30.0) && close(t[5], 0.0), "{t:?}");
}

#[test]
fn during_a_drag_the_tool_reports_the_live_move_instead_of_drawing_a_ghost() {
    let (a, pa) = square(1, 0.0, 0.0);
    let c = select_ctx(world().mat, &[(a, 0.0, 0.0)], &[pa]);
    let mut tool = SelectTool::default();
    assert!(tool.live().is_none());
    tool.on_event(&CanvasEvent::Press(p(5.0, 5.0)), NONE, &c);
    tool.on_event(&CanvasEvent::Drag(p(15.0, 8.0)), NONE, &c);
    let (ids, delta) = tool.live().expect("a live move");
    assert_eq!(ids, vec![a]);
    let t = delta.as_coeffs();
    assert!(close(t[4], 10.0) && close(t[5], 3.0), "{t:?}");
    let mut overlay = Vec::new();
    tool.overlay(&mut overlay);
    assert!(
        overlay
            .iter()
            .all(|o| !matches!(o, Overlay::Polyline { closed: true, .. })),
        "no outline copy of the object: {overlay:?}"
    );
    tool.on_event(&CanvasEvent::Escape, NONE, &c);
    assert!(tool.live().is_none());
}

fn big_square(id: u64) -> (ObjectId, Pickable<ObjectId>) {
    let (a, mut pa) = square(id, 0.0, 0.0);
    pa.path = Path::rect(0.0, 0.0, 100.0, 100.0);
    (a, pa)
}

#[test]
fn dragging_a_corner_square_scales_about_the_opposite_corner() {
    let mut w = world();
    let (a, pa) = big_square(1);
    doc_with(&mut w, &[(a, 0.0, 0.0)]);
    let c = select_ctx(w.mat, &[(a, 0.0, 0.0)], &[pa]);
    let mut tool = SelectTool::default();
    // The bottom right square stands 6 px off the corner; grab it there and pull 50 mm out.
    let out = drag(&mut tool, NONE, &c, p(106.0, 106.0), p(156.0, 156.0));
    let t = commit_one(&mut w, out, a).as_coeffs();
    assert!(close(t[0], 1.5) && close(t[3], 1.5), "scaled by 1.5: {t:?}");
    assert!(
        close(t[4], 0.0) && close(t[5], 0.0),
        "the top left stays: {t:?}"
    );
}

#[test]
fn a_side_square_scales_one_axis_only() {
    let mut w = world();
    let (a, pa) = big_square(1);
    doc_with(&mut w, &[(a, 0.0, 0.0)]);
    let c = select_ctx(w.mat, &[(a, 0.0, 0.0)], &[pa]);
    let mut tool = SelectTool::default();
    let out = drag(&mut tool, NONE, &c, p(106.0, 50.0), p(156.0, 70.0));
    let t = commit_one(&mut w, out, a).as_coeffs();
    assert!(close(t[0], 1.5), "{t:?}");
    assert!(close(t[3], 1.0), "the height is untouched: {t:?}");
}

#[test]
fn shift_on_a_corner_scales_proportionally_and_alt_scales_from_the_centre() {
    let (a, pa) = big_square(1);
    let run = |mods: Mods, to: Point| {
        let mut w = world();
        doc_with(&mut w, &[(a, 0.0, 0.0)]);
        let c = select_ctx(w.mat, &[(a, 0.0, 0.0)], std::slice::from_ref(&pa));
        let mut tool = SelectTool::default();
        let out = drag(&mut tool, mods, &c, p(106.0, 106.0), to);
        commit_one(&mut w, out, a).as_coeffs()
    };
    let t = run(SHIFT, p(156.0, 116.0));
    assert!(close(t[0], t[3]), "proportional: {t:?}");
    let t = run(Mods { alt: true, ..NONE }, p(156.0, 156.0));
    // Pulling 50 out of a box 100 wide, from its centre: each half goes from 50 to 100.
    assert!(close(t[0], 2.0), "{t:?}");
    assert!(close(t[4], -50.0), "the centre stays put: {t:?}");
}

#[test]
fn dragging_from_just_outside_a_corner_rotates_about_the_centre() {
    let mut w = world();
    let (a, pa) = big_square(1);
    doc_with(&mut w, &[(a, 0.0, 0.0)]);
    let c = select_ctx(w.mat, &[(a, 0.0, 0.0)], &[pa]);
    let mut tool = SelectTool::default();
    // Just outside the bottom right corner, then a quarter turn about the centre (50, 50): the
    // pointer goes from the lower right to the lower left.
    let out = drag(&mut tool, NONE, &c, p(114.0, 114.0), p(-14.0, 114.0));
    let t = commit_one(&mut w, out, a);
    let to = t * Point::new(100.0, 100.0);
    assert!(close(to.x, 0.0) && close(to.y, 100.0), "{to:?}");
}

#[test]
fn shift_rotates_in_fifteen_degree_steps() {
    let mut w = world();
    let (a, pa) = big_square(1);
    doc_with(&mut w, &[(a, 0.0, 0.0)]);
    let c = select_ctx(w.mat, &[(a, 0.0, 0.0)], &[pa]);
    let mut tool = SelectTool::default();
    // About 20 degrees: it rounds to 15.
    let angle = 20.0_f64.to_radians();
    let from = p(114.0, 114.0);
    let (cx, cy) = (50.0, 50.0);
    let (dx, dy) = (from.x - cx, from.y - cy);
    let to = p(
        cx + dx * angle.cos() - dy * angle.sin(),
        cy + dx * angle.sin() + dy * angle.cos(),
    );
    let out = drag(&mut tool, SHIFT, &c, from, to);
    let t = commit_one(&mut w, out, a).as_coeffs();
    let got = t[1].atan2(t[0]).to_degrees();
    assert!((got - 15.0).abs() < 1e-6, "{got}");
}

#[test]
fn the_pointer_changes_over_the_handles_and_outside_the_corners() {
    let (a, pa) = big_square(1);
    let c = select_ctx(world().mat, &[(a, 0.0, 0.0)], &[pa]);
    let mut tool = SelectTool::default();
    assert_eq!(tool.pointer(), Cursor::Arrow);
    tool.on_event(&CanvasEvent::Hover(p(106.0, 106.0)), NONE, &c);
    assert_eq!(tool.pointer(), Cursor::Resize(Handle::SouthEast));
    tool.on_event(&CanvasEvent::Hover(p(118.0, 118.0)), NONE, &c);
    assert_eq!(tool.pointer(), Cursor::Rotate);
    tool.on_event(&CanvasEvent::Hover(p(50.0, 50.0)), NONE, &c);
    assert_eq!(tool.pointer(), Cursor::Arrow);
}

#[test]
fn a_small_selection_has_corner_squares_only() {
    let handles = visible_handles(Rect::new(0.0, 0.0, 10.0, 10.0), 1.0);
    assert_eq!(handles.len(), 4);
    assert!(handles.iter().all(|h| h.is_corner()));
    assert_eq!(
        visible_handles(Rect::new(0.0, 0.0, 100.0, 100.0), 1.0).len(),
        8
    );
}

#[test]
fn snapping_to_the_grid_draws_no_guide_but_snapping_to_an_object_does() {
    let (a, pa) = square(1, 0.0, 0.0);
    let (b, pb) = square(2, 100.0, 0.0);
    let lines = |dx: f64| {
        let mut w = world();
        doc_with(&mut w, &[(a, 0.0, 0.0), (b, 100.0, 0.0)]);
        let mut c = select_ctx(w.mat, &[(a, 0.0, 0.0)], &[pa.clone(), pb.clone()]);
        c.snapping = true;
        c.zoom = 4.0;
        let mut tool = SelectTool::default();
        // Well below the mat's top edge, so only the sideways snap is in question.
        tool.on_event(&CanvasEvent::Press(p(5.0, 5.0)), NONE, &c);
        tool.on_event(&CanvasEvent::Drag(p(5.0 + dx, 45.3)), NONE, &c);
        let mut out = Vec::new();
        tool.overlay(&mut out);
        out.iter()
            .filter(|o| matches!(o, Overlay::Polyline { .. }))
            .count()
    };
    // 20.3 mm over lands near a grid line, with no object nearby: it snaps, silently.
    assert_eq!(lines(20.3), 0, "the grid is not drawn as a guide");
    // 90.2 mm over puts the right edge on the other square's left edge: that is shown.
    assert!(lines(90.2) > 0, "an object's edge is");
}

#[test]
fn the_pen_previews_a_curve_and_its_handles_once_a_handle_is_pulled() {
    let c = ctx(world().mat);
    let mut tool = PenTool::default();
    // Press at the first node and drag out a handle, then place a second node.
    tool.on_event(&CanvasEvent::Press(p(0.0, 0.0)), NONE, &c);
    tool.on_event(&CanvasEvent::Drag(p(0.0, 20.0)), NONE, &c);
    tool.on_event(&CanvasEvent::Release(p(0.0, 20.0)), NONE, &c);
    click(&mut tool, &c, p(30.0, 0.0));
    let mut overlay = Vec::new();
    tool.overlay(&mut overlay);
    let Some(Overlay::Polyline { points, .. }) = overlay.first() else {
        panic!("{overlay:?}");
    };
    assert!(
        points.len() > 3,
        "a curve is drawn as many short pieces, not one straight line: {} points",
        points.len()
    );
    // The curve bows out towards the handle: some point is well above the straight line y = 0.
    assert!(points.iter().any(|q| q.y > 5.0), "{points:?}");
    let arms = overlay.iter().find_map(|o| match o {
        Overlay::Arms { arms } => Some(arms.len()),
        _ => None,
    });
    assert_eq!(arms, Some(2), "both sides of the pulled handle are shown");
}
