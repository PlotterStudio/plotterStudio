//! Ground truth for text: outline text is shaped and turned into closed paths in
//! millimetres with y pointing down and the baseline at y = 0; single-stroke text is open paths for
//! pens. The expected values are properties any correct font gives: a letter "o" has an outer and
//! an inner contour, doubling the size doubles every dimension, a second line sits one line height
//! lower, and the crossbar of a "T" is above its stem's foot.

use epaint_default_fonts::UBUNTU_LIGHT;
use ps_geometry::Rect;

use super::*;

fn style(size: f64) -> TextStyle {
    TextStyle {
        size_mm: size,
        ..TextStyle::default()
    }
}

fn bounds_of(paths: &[Path]) -> Rect {
    let mut it = paths.iter().map(Path::bounds);
    let first = it.next().expect("some paths");
    it.fold(first, |a, b| a.union(b))
}

fn outline(text: &str, size: f64) -> TextOutline {
    outline_text(UBUNTU_LIGHT, text, &style(size)).expect("outlines")
}

#[test]
fn an_empty_string_or_only_spaces_has_no_contours() {
    assert_eq!(outline("", 10.0).paths.len(), 0);
    assert_eq!(outline("   ", 10.0).paths.len(), 0);
}

#[test]
fn letters_have_the_number_of_contours_their_shape_has() {
    assert_eq!(outline("I", 10.0).paths.len(), 1);
    assert_eq!(outline("l", 10.0).paths.len(), 1);
    assert_eq!(outline("o", 10.0).paths.len(), 2, "outer and counter");
    assert_eq!(outline("i", 10.0).paths.len(), 2, "stem and dot");
    assert_eq!(outline("8", 10.0).paths.len(), 3, "outer and two counters");
}

#[test]
fn every_contour_is_closed() {
    assert!(outline("Hello", 10.0).paths.iter().all(|p| p.closed));
}

#[test]
fn sizes_scale_every_dimension() {
    let small = bounds_of(&outline("Hamburg", 10.0).paths);
    let large = bounds_of(&outline("Hamburg", 20.0).paths);
    assert!((large.width() - 2.0 * small.width()).abs() < 1e-6);
    assert!((large.height() - 2.0 * small.height()).abs() < 1e-6);
}

#[test]
fn the_baseline_is_y_zero_and_up_is_negative() {
    let b = bounds_of(&outline("I", 10.0).paths);
    assert!(
        b.y1.abs() < 0.1,
        "the foot of an I sits on the baseline: {b:?}"
    );
    assert!(
        b.y0 < -5.0 && b.y0 > -10.0,
        "its top is a cap height above: {b:?}"
    );
}

#[test]
fn a_capital_is_about_seven_tenths_of_the_size_tall() {
    // Cap height is 0.65 to 0.75 of the em in text faces.
    let b = bounds_of(&outline("H", 100.0).paths);
    assert!(b.height() > 60.0 && b.height() < 80.0, "{}", b.height());
}

#[test]
fn text_runs_left_to_right_from_x_zero() {
    let one = bounds_of(&outline("I", 10.0).paths);
    let two = bounds_of(&outline("II", 10.0).paths);
    assert!(one.x0 >= 0.0 && one.x0 < 2.0);
    assert!(
        two.width() > one.width() * 1.9 - 1e-9,
        "the second letter is beside the first"
    );
}

#[test]
fn the_advance_is_reported_for_layout() {
    let one = outline("I", 10.0);
    let two = outline("II", 10.0);
    assert!(
        (two.width - 2.0 * one.width).abs() < 0.05,
        "{} vs {}",
        two.width,
        one.width
    );
}

#[test]
fn letter_spacing_adds_a_gap_after_each_letter() {
    let base = outline("III", 10.0);
    let spaced = outline_text(
        UBUNTU_LIGHT,
        "III",
        &TextStyle {
            letter_spacing_mm: 2.0,
            ..style(10.0)
        },
    )
    .unwrap();
    assert!(
        (spaced.width - base.width - 6.0).abs() < 1e-6,
        "{} {}",
        spaced.width,
        base.width
    );
}

#[test]
fn a_second_line_sits_one_line_height_lower() {
    let first = bounds_of(&outline("I", 10.0).paths);
    let both = outline("I\nI", 10.0);
    assert_eq!(both.paths.len(), 2);
    let second = both.paths[1].bounds();
    assert!(
        (second.y0 - first.y0 - 12.0).abs() < 0.05,
        "1.2 x the size by default"
    );
    assert!((both.height - 22.0).abs() < 0.5 || both.height > 12.0);
}

#[test]
fn the_line_height_is_adjustable() {
    let tight = outline_text(
        UBUNTU_LIGHT,
        "I\nI",
        &TextStyle {
            line_height: 1.0,
            ..style(10.0)
        },
    )
    .unwrap();
    let gap = tight.paths[1].bounds().y0 - tight.paths[0].bounds().y0;
    assert!((gap - 10.0).abs() < 0.05, "{gap}");
}

#[test]
fn lines_can_be_centred_and_right_aligned() {
    let text = "IIII\nI";
    let left = outline_text(UBUNTU_LIGHT, text, &style(10.0)).unwrap();
    let centre = outline_text(
        UBUNTU_LIGHT,
        text,
        &TextStyle {
            align: Align::Centre,
            ..style(10.0)
        },
    )
    .unwrap();
    let right = outline_text(
        UBUNTU_LIGHT,
        text,
        &TextStyle {
            align: Align::Right,
            ..style(10.0)
        },
    )
    .unwrap();
    let last = |o: &TextOutline| o.paths.last().unwrap().bounds();
    let (l, c, r) = (last(&left), last(&centre), last(&right));
    assert!(c.x0 > l.x0 + 1.0, "the short line moves right when centred");
    assert!(r.x0 > c.x0 + 1.0, "and further when right-aligned");
    // Right-aligned, the short line ends where the long line ends.
    let long_end = right.paths[..4]
        .iter()
        .map(|p| p.bounds().x1)
        .fold(f64::MIN, f64::max);
    assert!((long_end - r.x1).abs() < 1.0, "{long_end} {}", r.x1);
}

#[test]
fn a_space_moves_the_next_letter() {
    let tight = bounds_of(&outline("II", 10.0).paths);
    let spaced = bounds_of(&outline("I I", 10.0).paths);
    assert!(spaced.width() > tight.width() + 1.0);
}

#[test]
fn something_that_is_not_a_font_is_refused() {
    assert!(matches!(
        outline_text(b"not a font", "A", &style(10.0)),
        Err(TextError::BadFont)
    ));
}

#[test]
fn a_size_that_is_not_positive_and_finite_is_refused() {
    for bad in [0.0, -5.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            outline_text(UBUNTU_LIGHT, "A", &style(bad)),
            Err(TextError::BadSize)
        ));
    }
}

#[test]
fn characters_the_font_lacks_leave_a_gap_rather_than_failing() {
    // The font has no glyph for this private-use character; the text still comes out.
    let o = outline("I\u{F8FF}I", 10.0);
    assert_eq!(o.paths.len(), 2);
}

#[test]
fn outlines_follow_the_curves_of_the_glyph() {
    // An "o" is round: its outer contour has a bounding box close to square for a light face.
    let o = outline("o", 100.0);
    let b = o.paths[0].bounds();
    assert!(b.width() > 40.0 && b.height() > 40.0);
    assert!(
        o.paths[0]
            .segments()
            .any(|(_, s)| matches!(s, ps_geometry::Seg::Cubic { .. })),
        "curves stay curves"
    );
}

#[test]
fn the_same_text_gives_identical_paths() {
    assert_eq!(outline("Plotter", 12.0), outline("Plotter", 12.0));
}

// ---- single-stroke fonts ----

fn strokes(text: &str, font: StrokeFont, cap: f64) -> Vec<Path> {
    stroke_text(text, font, cap).expect("strokes")
}

#[test]
fn single_stroke_text_is_open_paths() {
    let s = strokes("Hello", StrokeFont::RomanSimplex, 10.0);
    assert_ne!(s.len(), 0);
    assert!(s.iter().all(|p| !p.closed));
}

#[test]
fn an_h_is_three_strokes_in_a_simplex_font() {
    assert_eq!(strokes("H", StrokeFont::RomanSimplex, 10.0).len(), 3);
    assert_eq!(strokes("I", StrokeFont::RomanSimplex, 10.0).len(), 1);
}

#[test]
fn the_cap_height_is_what_was_asked_for() {
    let b = bounds_of(&strokes("H", StrokeFont::RomanSimplex, 14.0));
    assert!((b.height() - 14.0).abs() < 1e-6, "{}", b.height());
}

#[test]
fn single_stroke_text_stands_on_the_baseline_with_up_negative() {
    let b = bounds_of(&strokes("H", StrokeFont::RomanSimplex, 10.0));
    assert!(b.y1.abs() < 1e-6, "the foot is at y = 0: {b:?}");
    assert!((b.y0 + 10.0).abs() < 1e-6);
}

#[test]
fn the_bar_of_a_t_is_above_its_foot() {
    let s = strokes("T", StrokeFont::RomanSimplex, 10.0);
    let bar = s
        .iter()
        .find(|p| p.bounds().height() < 1e-6)
        .expect("a horizontal stroke");
    let stem = s
        .iter()
        .find(|p| p.bounds().width() < 1e-6)
        .expect("a vertical stroke");
    assert!(
        bar.bounds().y0 < stem.bounds().y1 - 5.0,
        "the bar is near the top"
    );
}

#[test]
fn letters_advance_left_to_right() {
    let one = bounds_of(&strokes("H", StrokeFont::RomanSimplex, 10.0));
    let two = bounds_of(&strokes("HH", StrokeFont::RomanSimplex, 10.0));
    assert!(two.width() > 1.8 * one.width());
}

#[test]
fn each_stroke_font_renders_something() {
    for font in StrokeFont::ALL {
        assert!(!strokes("Ab", font, 10.0).is_empty(), "{font:?}");
    }
}

#[test]
fn stroke_text_has_lines_and_rejects_bad_sizes() {
    let two = strokes("H\nH", StrokeFont::RomanSimplex, 10.0);
    let first = two[0].bounds().y0;
    let last = two.last().unwrap().bounds().y0;
    assert!(last > first + 10.0, "the second line is lower");
    assert!(stroke_text("H", StrokeFont::RomanSimplex, 0.0).is_err());
    assert!(stroke_text("H", StrokeFont::RomanSimplex, f64::NAN).is_err());
}

#[test]
fn an_empty_stroke_text_is_empty() {
    assert_eq!(
        strokes("", StrokeFont::RomanSimplex, 10.0),
        Vec::<Path>::new()
    );
    assert_eq!(
        strokes(" ", StrokeFont::RomanSimplex, 10.0),
        Vec::<Path>::new()
    );
}
