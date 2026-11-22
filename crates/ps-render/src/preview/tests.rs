//! Ground truth for the material preview: the compiled job is rasterised into effect
//! maps at a pixels-per-millimetre scale (ink as capsule coverage at the real tip width, height
//! for score and emboss, kerf for cuts, foil), and composited over the material with lighting.
//! Positions are millimetres; pixel (x, y) covers x/scale .. (x+1)/scale.

use ps_core::{MatId, ObjectId, OpId, PathKey, PtUm};
use ps_job::{HolderId, SrcRef, Step};

use super::*;

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

fn look(slot: u8, effect: Effect, colour: [u8; 3], width: f64) -> ToolLook {
    ToolLook {
        slot,
        effect,
        colour,
        width_mm: width,
    }
}

fn load(slot: u8) -> Step {
    Step::LoadTool {
        slot,
        holder: HolderId::new("a"),
        pause: None,
    }
}

/// A drawn horizontal line from (x0, y) to (x1, y) with the tool in `slot`.
fn line(slot: u8, x0: f64, x1: f64, y: f64) -> Vec<Step> {
    vec![
        load(slot),
        Step::Travel { to: pt(x0, y) },
        Step::ToolDown { force_gf: 100 },
        Step::Line {
            to: pt(x1, y),
            feed_mm_min: 6000,
            a_mdeg: None,
            src: src(),
        },
        Step::ToolUp,
    ]
}

fn maps(steps: &[Step], looks: &[ToolLook]) -> EffectMaps {
    // A 40 x 20 mm mat at 10 px/mm: 400 x 200 pixels.
    EffectMaps::rasterise(steps, looks, 40.0, 20.0, 10.0, usize::MAX)
}

fn px(m: &EffectMaps, x_mm: f64, y_mm: f64) -> Texel {
    m.texel_at(x_mm, y_mm)
}

#[test]
fn the_maps_cover_the_mat_at_the_scale() {
    let m = maps(&[], &[]);
    assert_eq!((m.width(), m.height()), (400, 200));
    assert!((m.px_per_mm() - 10.0).abs() < 1e-12);
}

#[test]
fn a_pen_line_is_covered_for_exactly_the_tip_width() {
    let m = maps(
        &line(1, 10.0, 30.0, 10.0),
        &[look(1, Effect::Ink, [0, 0, 0], 0.4)],
    );
    // 0.4 mm at 10 px/mm is 4 px wide: y = 9.8 .. 10.2 mm.
    assert!(
        px(&m, 20.0, 10.0).ink_alpha > 0.99,
        "{:?}",
        px(&m, 20.0, 10.0)
    );
    assert!(px(&m, 20.0, 10.15).ink_alpha > 0.9);
    assert!(
        px(&m, 20.0, 10.45).ink_alpha < 0.05,
        "outside the tip width"
    );
    assert!(px(&m, 20.0, 9.55).ink_alpha < 0.05);
}

#[test]
fn the_ends_of_a_stroke_are_round() {
    let m = maps(
        &line(1, 10.0, 30.0, 10.0),
        &[look(1, Effect::Ink, [0, 0, 0], 1.0)],
    );
    assert!(
        px(&m, 9.8, 10.0).ink_alpha > 0.9,
        "the cap extends half a width past the start"
    );
    assert!(px(&m, 9.3, 10.0).ink_alpha < 0.1);
    assert!(px(&m, 30.3, 10.0).ink_alpha > 0.5);
}

#[test]
fn nothing_is_drawn_while_the_tool_is_up_or_travelling() {
    let steps = vec![
        load(1),
        Step::Travel { to: pt(10.0, 10.0) },
        Step::Travel { to: pt(30.0, 10.0) },
    ];
    let m = maps(&steps, &[look(1, Effect::Ink, [0, 0, 0], 0.4)]);
    assert!(px(&m, 20.0, 10.0).ink_alpha < 0.01);
}

#[test]
fn ink_takes_the_tools_colour() {
    let m = maps(
        &line(1, 10.0, 30.0, 10.0),
        &[look(1, Effect::Ink, [200, 30, 40], 0.6)],
    );
    let t = px(&m, 20.0, 10.0);
    assert!(
        (t.ink_rgb[0] - 200.0 / 255.0).abs() < 0.01 && (t.ink_rgb[1] - 30.0 / 255.0).abs() < 0.01,
        "{t:?}"
    );
}

#[test]
fn overlapping_strokes_blend_with_the_later_one_on_top() {
    let mut steps = line(1, 10.0, 30.0, 10.0);
    steps.extend(line(2, 15.0, 25.0, 10.0));
    let m = maps(
        &steps,
        &[
            look(1, Effect::Ink, [255, 0, 0], 0.6),
            look(2, Effect::Ink, [0, 0, 255], 0.6),
        ],
    );
    let t = px(&m, 20.0, 10.0);
    assert!(
        t.ink_rgb[2] > 0.9 && t.ink_rgb[0] < 0.1,
        "blue on top: {t:?}"
    );
    assert!(px(&m, 12.0, 10.0).ink_rgb[0] > 0.9, "red alone at the ends");
}

#[test]
fn score_and_deboss_press_down_and_emboss_pushes_up() {
    let steps = {
        let mut s = line(1, 5.0, 15.0, 5.0);
        s.extend(line(2, 5.0, 15.0, 10.0));
        s.extend(line(3, 5.0, 15.0, 15.0));
        s
    };
    let m = maps(
        &steps,
        &[
            look(1, Effect::Score, [0, 0, 0], 0.5),
            look(2, Effect::Emboss, [0, 0, 0], 0.5),
            look(3, Effect::Deboss, [0, 0, 0], 0.5),
        ],
    );
    assert!(px(&m, 10.0, 5.0).height < -0.5, "score");
    assert!(px(&m, 10.0, 10.0).height > 0.5, "emboss");
    assert!(px(&m, 10.0, 15.0).height < -0.5, "deboss");
    assert!(px(&m, 10.0, 7.5).height.abs() < 1e-6, "flat between");
}

#[test]
fn a_through_cut_leaves_a_kerf_and_a_kiss_cut_a_fainter_one() {
    let mut steps = line(1, 5.0, 15.0, 5.0);
    steps.extend(line(2, 5.0, 15.0, 10.0));
    let m = maps(
        &steps,
        &[
            look(1, Effect::Cut, [0, 0, 0], 0.3),
            look(2, Effect::KissCut, [0, 0, 0], 0.3),
        ],
    );
    let through = px(&m, 10.0, 5.0);
    let kiss = px(&m, 10.0, 10.0);
    assert!(through.cut > 0.9 && through.through, "{through:?}");
    assert!(kiss.cut > 0.9 && !kiss.through, "{kiss:?}");
}

#[test]
fn foil_is_a_mask_with_the_foil_colour() {
    let m = maps(
        &line(1, 10.0, 30.0, 10.0),
        &[look(1, Effect::Foil, [212, 175, 55], 1.0)],
    );
    let t = px(&m, 20.0, 10.0);
    assert!(t.foil_alpha > 0.99);
    assert!((t.foil_rgb[0] - 212.0 / 255.0).abs() < 0.01);
}

#[test]
fn arcs_are_followed_not_cut_across() {
    let steps = vec![
        load(1),
        Step::Travel { to: pt(10.0, 10.0) },
        Step::ToolDown { force_gf: 100 },
        Step::Arc {
            to: pt(30.0, 10.0),
            center: pt(20.0, 10.0),
            ccw: false,
            feed_mm_min: 6000,
            src: src(),
        },
        Step::ToolUp,
    ];
    let m = maps(&steps, &[look(1, Effect::Ink, [0, 0, 0], 0.4)]);
    // The half circle of radius 10 about (20, 10) passes through (20, 0) or (20, 20): one of them.
    let top = px(&m, 20.0, 0.1)
        .ink_alpha
        .max(px(&m, 20.0, 19.9).ink_alpha);
    assert!(top > 0.5, "the arc passes through the top or bottom: {top}");
    assert!(px(&m, 20.0, 10.0).ink_alpha < 0.05, "not the chord");
}

#[test]
fn only_the_steps_up_to_the_limit_are_drawn() {
    let steps = line(1, 10.0, 30.0, 10.0);
    let looks = [look(1, Effect::Ink, [0, 0, 0], 0.4)];
    let none = EffectMaps::rasterise(&steps, &looks, 40.0, 20.0, 10.0, 3);
    assert!(
        none.texel_at(20.0, 10.0).ink_alpha < 0.01,
        "stops before the line step"
    );
    let all = maps(&steps, &looks);
    assert!(all.texel_at(20.0, 10.0).ink_alpha > 0.9);
}

#[test]
fn a_tool_with_no_look_draws_nothing() {
    let m = maps(
        &line(7, 10.0, 30.0, 10.0),
        &[look(1, Effect::Ink, [0, 0, 0], 0.4)],
    );
    assert!(px(&m, 20.0, 10.0).ink_alpha < 0.01);
}

#[test]
fn a_texel_outside_the_mat_is_empty() {
    let m = maps(
        &line(1, 10.0, 30.0, 10.0),
        &[look(1, Effect::Ink, [0, 0, 0], 0.4)],
    );
    assert_eq!(px(&m, -5.0, 10.0), Texel::default());
    assert_eq!(px(&m, 500.0, 10.0), Texel::default());
}

// ---- compositing ----

fn base(colour: [u8; 3]) -> BaseMaterial {
    BaseMaterial {
        colour,
        roughness: 0.8,
        metallic: 0.0,
    }
}

fn rgb(image: &Image, x: usize, y: usize) -> [u8; 3] {
    let i = (y * image.width as usize + x) * 4;
    [image.rgba[i], image.rgba[i + 1], image.rgba[i + 2]]
}

#[test]
fn an_empty_job_shows_the_material_colour_lit() {
    let m = maps(&[], &[]);
    let image = composite(&base([240, 235, 220]), &m, &Light::overhead());
    let c = rgb(&image, 200, 100);
    for (got, want) in c.iter().zip([240_u8, 235, 220]) {
        assert!(got.abs_diff(want) <= 12, "{c:?}");
    }
}

#[test]
fn dark_ink_on_light_card_shows_strongly() {
    let m = maps(
        &line(1, 10.0, 30.0, 10.0),
        &[look(1, Effect::Ink, [10, 10, 10], 0.6)],
    );
    let image = composite(&base([240, 240, 240]), &m, &Light::overhead());
    assert!(rgb(&image, 200, 100)[0] < 40, "{:?}", rgb(&image, 200, 100));
}

#[test]
fn light_ink_on_dark_card_looks_faint() {
    let m = maps(
        &line(1, 10.0, 30.0, 10.0),
        &[look(1, Effect::Ink, [255, 255, 255], 0.6)],
    );
    let image = composite(&base([30, 30, 40]), &m, &Light::overhead());
    let c = rgb(&image, 200, 100);
    assert!(c[0] > 60, "the ink shows: {c:?}");
    assert!(c[0] < 200, "but not at full strength: {c:?}");
}

#[test]
fn a_scored_line_is_shaded_by_a_raking_light() {
    let m = maps(
        &line(1, 5.0, 35.0, 10.0),
        &[look(1, Effect::Score, [0, 0, 0], 0.5)],
    );
    let image = composite(&base([200, 200, 200]), &m, &Light::raking(0.0));
    // Light from the left: the two walls of the groove are lit differently.
    let (above, below) = (rgb(&image, 200, 97)[0], rgb(&image, 200, 103)[0]);
    let flat = rgb(&image, 200, 20)[0];
    assert!(
        above != flat || below != flat,
        "the groove changes the shading: {above} {below} {flat}"
    );
}

#[test]
fn a_through_cut_draws_a_dark_kerf() {
    let m = maps(
        &line(1, 5.0, 35.0, 10.0),
        &[look(1, Effect::Cut, [0, 0, 0], 0.3)],
    );
    let image = composite(&base([220, 220, 220]), &m, &Light::overhead());
    assert!(
        rgb(&image, 200, 100)[0] < 120,
        "{:?}",
        rgb(&image, 200, 100)
    );
}

#[test]
fn foil_is_tinted_and_brighter_than_plain_ink() {
    let m = maps(
        &line(1, 10.0, 30.0, 10.0),
        &[look(1, Effect::Foil, [212, 175, 55], 1.0)],
    );
    let image = composite(&base([240, 240, 240]), &m, &Light::overhead());
    let c = rgb(&image, 200, 100);
    assert!(c[0] > c[2] + 40, "gold, not grey: {c:?}");
}

#[test]
fn the_image_is_the_size_of_the_maps_and_opaque() {
    let m = maps(&[], &[]);
    let image = composite(&base([255, 255, 255]), &m, &Light::overhead());
    assert_eq!((image.width, image.height), (400, 200));
    assert!(image.rgba.chunks(4).all(|p| p[3] == 255));
}

#[test]
fn compositing_is_deterministic() {
    let m = maps(
        &line(1, 10.0, 30.0, 10.0),
        &[look(1, Effect::Ink, [0, 0, 0], 0.4)],
    );
    assert_eq!(
        composite(&base([200, 200, 200]), &m, &Light::overhead()),
        composite(&base([200, 200, 200]), &m, &Light::overhead())
    );
}

#[test]
fn the_resolution_steps_down_on_weak_hardware() {
    assert!((adaptive_scale(20.0, Quality::High) - 20.0).abs() < 1e-12);
    assert!((adaptive_scale(20.0, Quality::Medium) - 10.0).abs() < 1e-12);
    assert!((adaptive_scale(20.0, Quality::Low) - 5.0).abs() < 1e-12);
    assert!(
        (adaptive_scale(8.0, Quality::High) - 8.0).abs() < 1e-12,
        "never above the wanted scale"
    );
}

#[test]
fn the_scale_follows_the_zoom_up_to_twenty_pixels_per_millimetre() {
    assert!(
        (scale_for_zoom(0.5) - 1.0).abs() < 1e-12,
        "never below one pixel per mm"
    );
    assert!((scale_for_zoom(6.0) - 6.0).abs() < 1e-12);
    assert!((scale_for_zoom(100.0) - 20.0).abs() < 1e-12);
}
