//! Ground truth for operation styles: colour is never the only cue, so every
//! operation kind has a line style (solid cut, dashed score, dotted perforate, double line for
//! kiss cut) and an icon, and the styles tell the kinds apart.

use ps_model::{LineStyle, OpKind};

use super::*;

#[test]
fn the_tdd_line_styles_are_used() {
    assert_eq!(style_of(OpKind::Cut), LineStyle::Solid);
    assert_eq!(style_of(OpKind::Score), LineStyle::Dashed);
    assert_eq!(style_of(OpKind::Perforate), LineStyle::Dotted);
    assert_eq!(style_of(OpKind::KissCut), LineStyle::Double);
}

#[test]
fn every_kind_has_an_icon_and_the_icons_differ() {
    let mut names: Vec<&str> = OpKind::ALL.iter().map(|k| icon_name(*k)).collect();
    assert!(names.iter().all(|n| !n.is_empty()));
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), total, "each kind needs its own icon");
}

#[test]
fn every_kind_has_a_colour_that_is_not_pure_black_or_white() {
    for kind in OpKind::ALL {
        let [r, g, b] = colour_of(kind);
        assert!(u16::from(r) + u16::from(g) + u16::from(b) > 60, "{kind:?}");
        assert!(
            u16::from(r) + u16::from(g) + u16::from(b) < 3 * 255 - 60,
            "{kind:?}"
        );
    }
}

#[test]
fn kinds_that_share_a_colour_differ_in_style_or_icon() {
    for a in OpKind::ALL {
        for b in OpKind::ALL {
            if a != b && colour_of(a) == colour_of(b) {
                assert!(
                    style_of(a) != style_of(b) || icon_name(a) != icon_name(b),
                    "{a:?} and {b:?} look alike"
                );
            }
        }
    }
}

#[test]
fn a_solid_line_has_no_dash_pattern() {
    assert_eq!(dash_pattern_px(LineStyle::Solid), Vec::<f64>::new());
}

#[test]
fn dashed_and_dotted_have_on_off_pairs_with_dots_shorter_than_dashes() {
    let dashed = dash_pattern_px(LineStyle::Dashed);
    let dotted = dash_pattern_px(LineStyle::Dotted);
    assert_eq!(dashed.len() % 2, 0);
    assert_eq!(dotted.len() % 2, 0);
    assert!(!dashed.is_empty() && !dotted.is_empty());
    assert!(dotted[0] < dashed[0]);
    assert!(dashed.iter().chain(&dotted).all(|v| *v > 0.0));
}

#[test]
fn a_double_line_is_drawn_as_two_offsets() {
    assert_eq!(parallel_offsets_px(LineStyle::Double), vec![-1.5, 1.5]);
    assert_eq!(parallel_offsets_px(LineStyle::Solid), vec![0.0]);
}

#[test]
fn travel_moves_are_dashed_and_neutral() {
    assert_eq!(travel_style(), LineStyle::Dashed);
}

#[test]
fn a_dash_pattern_converts_to_world_units_at_a_zoom() {
    let world = dash_pattern_mm(LineStyle::Dashed, 4.0);
    let px = dash_pattern_px(LineStyle::Dashed);
    assert_eq!(world.len(), px.len());
    for (w, p) in world.iter().zip(&px) {
        assert!((w - p / 4.0).abs() < 1e-12);
    }
}
