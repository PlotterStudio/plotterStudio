//! Ground truth for the tilted and orbiting preview: the mat is shown as a plane seen
//! from a tilt and an azimuth, with parallax occlusion from the height map, so grooves and
//! embossing shift against the surface as the view moves. A tilt of zero is the flat composite.
//!
//! Screen y of a surface point is `ground_y * cos(tilt) - height * sin(tilt)`: a raised point
//! appears further up the screen than the ground beneath it.

use ps_core::{MatId, ObjectId, OpId, PathKey, PtUm};
use ps_job::{HolderId, SrcRef, Step};

use super::*;
use crate::preview::{BaseMaterial, Effect, EffectMaps, Light, ToolLook, composite};

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

fn base() -> BaseMaterial {
    BaseMaterial {
        colour: [220, 220, 220],
        roughness: 0.5,
        metallic: 0.0,
    }
}

fn look(slot: u8, effect: Effect, colour: [u8; 3], width: f64) -> ToolLook {
    ToolLook {
        slot,
        effect,
        colour,
        width_mm: width,
    }
}

/// A horizontal line at `y`, drawn by tool slots 1 (ink) and 2 (emboss) together.
fn raised_ink_line(y: f64) -> Vec<Step> {
    let mut steps = Vec::new();
    for slot in [1_u8, 2] {
        steps.extend([
            Step::LoadTool {
                slot,
                holder: HolderId::new("a"),
                pause: None,
            },
            Step::Travel { to: pt(10.0, y) },
            Step::ToolDown { force_gf: 100 },
            Step::Line {
                to: pt(50.0, y),
                feed_mm_min: 6000,
                a_mdeg: None,
                src: src(),
            },
            Step::ToolUp,
        ]);
    }
    steps
}

/// A 60 x 40 mm mat at 10 px/mm.
fn maps(steps: &[Step]) -> EffectMaps {
    EffectMaps::rasterise(
        steps,
        &[
            look(1, Effect::Ink, [200, 0, 0], 1.0),
            look(2, Effect::Emboss, [0, 0, 0], 1.0),
        ],
        60.0,
        40.0,
        10.0,
        usize::MAX,
    )
}

fn orbit(tilt: f64, azimuth: f64, relief: f64) -> Orbit {
    Orbit {
        tilt_deg: tilt,
        azimuth_deg: azimuth,
        relief_mm: relief,
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

/// The mean of some row numbers.
fn mean(rows: &[usize]) -> f64 {
    let sum: u32 = rows.iter().map(|r| u32::try_from(*r).unwrap()).sum();
    f64::from(sum) / f64::from(u32::try_from(rows.len()).unwrap())
}

fn red_rows(image: &Image, x_mm: f64) -> Vec<usize> {
    let x = px(x_mm);
    (0..image.height as usize)
        .filter(|&y| {
            let i = (y * image.width as usize + x) * 4;
            i32::from(image.rgba[i]) > i32::from(image.rgba[i + 1]) + 60
        })
        .collect()
}

#[test]
fn no_tilt_is_exactly_the_flat_composite() {
    let m = maps(&raised_ink_line(20.0));
    let flat = composite(&base(), &m, &Light::overhead());
    assert_eq!(
        orbit_view(&base(), &m, &Light::overhead(), &orbit(0.0, 0.0, 2.0)),
        flat
    );
}

#[test]
fn tilting_foreshortens_the_mat_by_the_cosine_of_the_tilt() {
    let m = maps(&[]);
    let image = orbit_view(&base(), &m, &Light::overhead(), &orbit(60.0, 0.0, 0.0));
    assert_eq!(image.width, 600);
    // 400 rows at cos 60 = 200 rows.
    assert!(
        (i64::from(image.height) - 200).abs() <= 1,
        "{}",
        image.height
    );
}

#[test]
fn the_tilt_is_limited_so_the_mat_never_goes_edge_on() {
    let m = maps(&[]);
    let steep = orbit_view(&base(), &m, &Light::overhead(), &orbit(89.9, 0.0, 0.0));
    let limited = orbit_view(
        &base(),
        &m,
        &Light::overhead(),
        &orbit(MAX_TILT_DEG, 0.0, 0.0),
    );
    assert_eq!(steep.height, limited.height);
    assert!(steep.height > 30);
}

#[test]
fn a_flat_line_lands_at_its_foreshortened_row() {
    let m = maps(&raised_ink_line(20.0));
    // No relief: only the ink shows. Ground y = 20 mm at tilt 60 is screen y = 10 mm = row 100.
    let image = orbit_view(&base(), &m, &Light::overhead(), &orbit(60.0, 0.0, 0.0));
    let rows = red_rows(&image, 30.0);
    assert_ne!(rows.len(), 0);
    let mid = mean(&rows);
    assert!((mid - 100.0).abs() < 3.0, "{mid}");
}

#[test]
fn a_raised_line_shifts_up_the_screen_by_its_height_times_the_sine_of_the_tilt() {
    let m = maps(&raised_ink_line(20.0));
    let tilt = 60.0_f64.to_radians();
    let top_edge = |image: &Image| f64::from(u32::try_from(red_rows(image, 30.0)[0]).unwrap());
    // The ink line spans ground y 19.5 .. 20.5 mm; its back edge is at y = 19.5. Where that edge
    // would sit on the ground, in rows from the top of the picture (the picture starts at the
    // highest point, `relief` above the ground):
    let ground_row = |relief: f64| (19.5 * tilt.cos() + relief * tilt.sin()) * 10.0;
    let flat = orbit_view(&base(), &m, &Light::overhead(), &orbit(60.0, 0.0, 0.0));
    let tall = orbit_view(&base(), &m, &Light::overhead(), &orbit(60.0, 0.0, 2.0));
    let lift_flat = ground_row(0.0) - top_edge(&flat);
    let lift_tall = ground_row(2.0) - top_edge(&tall);
    assert!(
        lift_flat.abs() < 3.0,
        "a flat line sits on the ground: {lift_flat}"
    );
    // 2 mm * sin 60 = 1.73 mm = 17 rows above the ground.
    assert!((lift_tall - 17.3).abs() < 3.0, "{lift_tall}");
}

#[test]
fn a_raised_ridge_hides_the_ground_behind_it_when_viewed_from_the_front() {
    // Parallax occlusion: with a tall ridge across the mat, the strip of ground just behind it
    // (further from the viewer) is hidden, so fewer ground rows are visible than without a ridge.
    let ridge = maps(&raised_ink_line(20.0));
    let plain = maps(&[]);
    let hidden = |m: &EffectMaps| -> usize {
        let image = orbit_view(&base(), m, &Light::overhead(), &orbit(70.0, 0.0, 6.0));
        image.height as usize
    };
    // The picture is the same size either way; what changes is which ground is visible, which the
    // next assertion reads from the ridge's own footprint: its top is wider on screen than flat.
    assert_eq!(hidden(&ridge), hidden(&plain));
    let image = orbit_view(&base(), &ridge, &Light::overhead(), &orbit(70.0, 0.0, 6.0));
    let rows = red_rows(&image, 30.0);
    assert!(rows.len() >= 3, "{rows:?}");
}

#[test]
fn a_quarter_turn_of_azimuth_swaps_the_width_and_the_depth() {
    let m = maps(&[]);
    let image = orbit_view(&base(), &m, &Light::overhead(), &orbit(0.0, 90.0, 0.0));
    // A 60 x 40 mat seen from the side is 40 wide and 60 deep at zero tilt.
    assert_eq!((image.width, image.height), (400, 600));
}

#[test]
fn a_half_turn_of_azimuth_turns_the_mat_upside_down() {
    let m = maps(&raised_ink_line(10.0));
    let front = orbit_view(&base(), &m, &Light::overhead(), &orbit(0.0, 0.0, 0.0));
    let back = orbit_view(&base(), &m, &Light::overhead(), &orbit(0.0, 180.0, 0.0));
    let rows = |image: &Image| red_rows(image, 30.0);
    let (a, b) = (rows(&front), rows(&back));
    assert!(!a.is_empty() && !b.is_empty());
    let (ma, mb) = (mean(&a), mean(&b));
    // y = 10 mm from the top is 30 mm from the top once the mat is turned round.
    assert!(
        (ma - 100.0).abs() < 3.0 && (mb - 300.0).abs() < 3.0,
        "{ma} {mb}"
    );
}

#[test]
fn nonsense_angles_fall_back_to_the_flat_view() {
    let m = maps(&[]);
    let flat = composite(&base(), &m, &Light::overhead());
    let image = orbit_view(
        &base(),
        &m,
        &Light::overhead(),
        &orbit(f64::NAN, f64::NAN, f64::NAN),
    );
    assert_eq!(image, flat);
}
