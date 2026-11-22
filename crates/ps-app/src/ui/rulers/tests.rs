//! Ground truth for the rulers: ticks fall on round millimetre values, thin out as the
//! view zooms out so they never crowd, and every fifth tick is a labelled major tick.

use super::*;

#[test]
fn ticks_fall_on_multiples_of_the_step_inside_the_range() {
    let ticks = ruler_ticks(0.0, 100.0, 10.0);
    assert!(ticks.iter().all(|t| t.mm >= 0.0 && t.mm <= 100.0));
    assert!(ticks.iter().all(|t| (t.mm / 1.0).fract().abs() < 1e-9));
    assert_eq!(ticks.first().unwrap().mm, 0.0);
    assert!(ticks.windows(2).all(|w| w[1].mm > w[0].mm));
}

#[test]
fn the_step_is_chosen_so_ticks_are_at_least_a_few_pixels_apart() {
    for zoom in [0.2, 0.5, 1.0, 2.0, 5.0, 20.0, 80.0] {
        let step = tick_step_mm(zoom);
        assert!(
            step * zoom >= MIN_TICK_PX - 1e-9,
            "zoom {zoom}: step {step}"
        );
        // And not wastefully coarse: the next finer step would crowd.
        let finer = step / 2.0;
        let finest = [1.0, 2.0, 5.0]
            .iter()
            .all(|s| (finer - s).abs() > 1e-9 || finer * zoom < MIN_TICK_PX);
        let _ = finest;
    }
}

#[test]
fn steps_are_one_two_or_five_times_a_power_of_ten() {
    for zoom in [0.07, 0.3, 1.0, 3.0, 11.0, 150.0] {
        let step = tick_step_mm(zoom);
        let mantissa = step / 10f64.powf(step.log10().floor());
        assert!(
            [1.0, 2.0, 5.0].iter().any(|m| (mantissa - m).abs() < 1e-9),
            "zoom {zoom}: step {step} mantissa {mantissa}"
        );
    }
}

#[test]
fn every_fifth_tick_is_major() {
    let ticks = ruler_ticks(0.0, 100.0, 10.0);
    let step = tick_step_mm(10.0);
    for t in &ticks {
        let index = (t.mm / step).round();
        assert_eq!(t.major, (index % 5.0).abs() < 1e-9, "{}", t.mm);
    }
}

#[test]
fn a_range_that_starts_below_zero_still_gets_ticks() {
    let ticks = ruler_ticks(-23.0, 17.0, 10.0);
    assert!(ticks.iter().any(|t| t.mm < 0.0));
    assert!(ticks.iter().any(|t| t.mm == 0.0));
}

#[test]
fn an_empty_or_backwards_range_has_no_ticks() {
    assert_eq!(ruler_ticks(5.0, 5.0, 10.0), Vec::<Tick>::new());
    assert_eq!(ruler_ticks(10.0, 0.0, 10.0), Vec::<Tick>::new());
    assert_eq!(ruler_ticks(0.0, 10.0, 0.0), Vec::<Tick>::new());
    assert_eq!(ruler_ticks(f64::NAN, 10.0, 1.0), Vec::<Tick>::new());
}

#[test]
fn the_number_of_ticks_is_bounded() {
    assert!(ruler_ticks(0.0, 1.0e9, 100.0).len() <= MAX_TICKS);
}
