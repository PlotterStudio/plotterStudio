//! Ground truth for the weeded view: through-cut pieces become separate layers lifted
//! along the view normal, and for vinyl the waste is removed and the liner shows. Pieces come from
//! the closed through-cut contours of the job: a contour encloses a piece when it is nested an odd
//! number of levels deep, and a hole (the inside of an "O") when it is nested an even number.

use ps_core::{MatId, ObjectId, OpId, PathKey, PtUm};
use ps_job::{HolderId, SrcRef, Step};

use super::*;
use crate::preview::{BaseMaterial, Effect, EffectMaps, Light, ToolLook, composite};

const CUT: ToolLook = ToolLook {
    slot: 1,
    effect: Effect::Cut,
    colour: [0, 0, 0],
    width_mm: 0.3,
};

const LINER: [u8; 3] = [250, 250, 250];

fn src() -> SrcRef {
    SrcRef {
        mat: MatId::at_unix_ms(1),
        object: ObjectId::at_unix_ms(2),
        path: PathKey::source("p"),
        op: OpId::at_unix_ms(3),
        move_index: 0,
    }
}

fn pt(x: f64, y: f64) -> PtUm {
    PtUm::from_mm(x, y).unwrap()
}

/// A closed through-cut square.
fn square(x: f64, y: f64, size: f64, overcut: f64) -> Vec<Step> {
    let line = |to: PtUm| Step::Line {
        to,
        feed_mm_min: 6000,
        a_mdeg: None,
        src: src(),
    };
    let mut steps = vec![
        Step::Travel { to: pt(x, y) },
        Step::ToolDown { force_gf: 100 },
        line(pt(x + size, y)),
        line(pt(x + size, y + size)),
        line(pt(x, y + size)),
        line(pt(x, y + overcut)),
    ];
    if overcut == 0.0 {
        steps.pop();
        steps.push(line(pt(x, y)));
    }
    steps.push(Step::ToolUp);
    steps
}

fn job(squares: &[(f64, f64, f64, f64)]) -> Vec<Step> {
    let mut steps = vec![Step::LoadTool {
        slot: 1,
        holder: HolderId::new("b"),
        pause: None,
    }];
    for (x, y, size, overcut) in squares {
        steps.extend(square(*x, *y, *size, *overcut));
    }
    steps
}

/// A 60 x 40 mm mat at 10 px/mm.
fn maps(steps: &[Step]) -> EffectMaps {
    EffectMaps::rasterise(steps, &[CUT], 60.0, 40.0, 10.0, usize::MAX)
}

fn pieces(steps: &[Step]) -> Pieces {
    Pieces::from_steps(steps, &[CUT], 60.0, 40.0, 10.0, usize::MAX)
}

fn base() -> BaseMaterial {
    BaseMaterial {
        colour: [200, 40, 40],
        roughness: 0.5,
        metallic: 0.0,
    }
}

fn weeding(lift_mm: f64, remove_waste: bool) -> Weeding {
    Weeding {
        lift_mm,
        direction: (1.0, 0.0),
        remove_waste,
        liner: LINER,
    }
}

/// A pixel index for a position in millimetres at 10 pixels per millimetre.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "test positions are small and positive"
)]
fn px(mm: f64) -> usize {
    (mm * 10.0) as usize
}

fn rgb(image: &Image, x_mm: f64, y_mm: f64) -> [u8; 3] {
    let (x, y) = (px(x_mm), px(y_mm));
    let i = (y * image.width as usize + x) * 4;
    [image.rgba[i], image.rgba[i + 1], image.rgba[i + 2]]
}

#[test]
fn a_closed_through_cut_encloses_a_piece_and_the_rest_is_waste() {
    let p = pieces(&job(&[(10.0, 10.0, 20.0, 0.0)]));
    assert_eq!(p.piece_at(20.0, 20.0), Some(1));
    assert_eq!(p.piece_at(5.0, 5.0), None, "outside is waste");
    assert_eq!(p.piece_at(45.0, 30.0), None);
    assert_eq!(p.count(), 1);
}

#[test]
fn a_contour_inside_a_piece_makes_a_hole_that_is_waste_again() {
    let p = pieces(&job(&[(10.0, 5.0, 30.0, 0.0), (20.0, 15.0, 10.0, 0.0)]));
    assert!(p.piece_at(12.0, 8.0).is_some(), "the ring is a piece");
    assert_eq!(p.piece_at(25.0, 20.0), None, "the hole is waste");
    // An island inside the hole is a piece again.
    let q = pieces(&job(&[
        (5.0, 2.0, 50.0, 0.0),
        (10.0, 6.0, 40.0, 0.0),
        (20.0, 14.0, 10.0, 0.0),
    ]));
    assert!(q.piece_at(7.0, 4.0).is_some());
    assert_eq!(q.piece_at(12.0, 8.0), None);
    assert!(q.piece_at(25.0, 19.0).is_some());
    assert_eq!(q.count(), 2);
}

#[test]
fn two_separate_contours_are_two_pieces_with_different_ids() {
    let p = pieces(&job(&[(5.0, 5.0, 10.0, 0.0), (30.0, 5.0, 10.0, 0.0)]));
    let (a, b) = (p.piece_at(10.0, 10.0), p.piece_at(35.0, 10.0));
    assert!(a.is_some() && b.is_some() && a != b, "{a:?} {b:?}");
    assert_eq!(p.count(), 2);
}

#[test]
fn a_contour_that_overcuts_past_its_start_is_still_closed() {
    let p = pieces(&job(&[(10.0, 10.0, 20.0, 1.0)]));
    assert_eq!(p.piece_at(20.0, 20.0), Some(1));
}

#[test]
fn an_open_cut_encloses_nothing() {
    let steps = vec![
        Step::LoadTool {
            slot: 1,
            holder: HolderId::new("b"),
            pause: None,
        },
        Step::Travel { to: pt(10.0, 10.0) },
        Step::ToolDown { force_gf: 100 },
        Step::Line {
            to: pt(40.0, 10.0),
            feed_mm_min: 6000,
            a_mdeg: None,
            src: src(),
        },
        Step::ToolUp,
    ];
    let p = pieces(&steps);
    assert_eq!(p.count(), 0);
    assert_eq!(p.piece_at(20.0, 10.5), None);
}

#[test]
fn drawing_tools_never_make_pieces() {
    let pen = ToolLook {
        slot: 1,
        effect: Effect::Ink,
        colour: [0, 0, 0],
        width_mm: 0.4,
    };
    let p = Pieces::from_steps(
        &job(&[(10.0, 10.0, 20.0, 0.0)]),
        &[pen],
        60.0,
        40.0,
        10.0,
        usize::MAX,
    );
    assert_eq!(p.count(), 0);
}

#[test]
fn kiss_cuts_do_not_free_a_piece() {
    let kiss = ToolLook {
        effect: Effect::KissCut,
        ..CUT
    };
    let p = Pieces::from_steps(
        &job(&[(10.0, 10.0, 20.0, 0.0)]),
        &[kiss],
        60.0,
        40.0,
        10.0,
        usize::MAX,
    );
    assert_eq!(p.count(), 0, "a kiss cut leaves the backing whole");
}

#[test]
fn removing_the_waste_shows_the_liner_and_keeps_the_pieces() {
    let steps = job(&[(10.0, 10.0, 20.0, 0.0), (15.0, 15.0, 10.0, 0.0)]);
    let image = weeded(
        &base(),
        &maps(&steps),
        &pieces(&steps),
        &Light::overhead(),
        &weeding(0.0, true),
    );
    assert_eq!(rgb(&image, 3.0, 3.0), LINER, "waste outside is gone");
    assert_eq!(rgb(&image, 20.0, 20.0), LINER, "the hole is gone too");
    let piece = rgb(&image, 12.0, 12.0);
    assert!(
        piece[0] > piece[1] + 80,
        "the piece keeps the material colour: {piece:?}"
    );
}

#[test]
fn keeping_the_waste_leaves_the_material_in_place() {
    let steps = job(&[(10.0, 10.0, 20.0, 0.0)]);
    let image = weeded(
        &base(),
        &maps(&steps),
        &pieces(&steps),
        &Light::overhead(),
        &weeding(0.0, false),
    );
    let waste = rgb(&image, 3.0, 3.0);
    assert!(waste[0] > waste[1] + 80, "{waste:?}");
}

#[test]
fn a_lifted_piece_moves_along_the_lift_direction_and_leaves_the_liner_behind() {
    let steps = job(&[(10.0, 10.0, 20.0, 0.0)]);
    let image = weeded(
        &base(),
        &maps(&steps),
        &pieces(&steps),
        &Light::overhead(),
        &weeding(6.0, true),
    );
    // Lifted 6 mm to the right: its old left strip shows liner, and it now covers 36 mm.
    assert_eq!(rgb(&image, 12.0, 20.0), LINER, "where it was");
    let moved = rgb(&image, 34.0, 20.0);
    assert!(moved[0] > moved[1] + 80, "where it is now: {moved:?}");
}

#[test]
fn a_lifted_piece_casts_a_shadow_on_the_liner() {
    let steps = job(&[(10.0, 10.0, 20.0, 0.0)]);
    let image = weeded(
        &base(),
        &maps(&steps),
        &pieces(&steps),
        &Light::overhead(),
        &weeding(8.0, true),
    );
    // Just past the lifted piece's right edge the liner is darker than the plain liner.
    let shadow = rgb(&image, 38.5, 20.0);
    assert!(shadow[0] < LINER[0], "{shadow:?}");
    assert_eq!(
        rgb(&image, 55.0, 20.0),
        LINER,
        "far away the liner is plain"
    );
}

#[test]
fn a_zero_lift_view_of_a_job_with_no_cuts_is_just_the_composite() {
    let steps = vec![Step::LoadTool {
        slot: 1,
        holder: HolderId::new("b"),
        pause: None,
    }];
    let m = maps(&steps);
    let flat = composite(&base(), &m, &Light::overhead());
    let image = weeded(
        &base(),
        &m,
        &pieces(&steps),
        &Light::overhead(),
        &weeding(0.0, false),
    );
    assert_eq!(image, flat);
}
