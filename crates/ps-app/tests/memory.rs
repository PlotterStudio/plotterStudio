//! The memory target: under 1.5 GB resident for the 10,000-path stress project with
//! the material preview open. The project is imported and compiled as the app does, and the
//! preview is built at the scale a 4K canvas asks for when the whole mat is in view.

use std::fmt::Write as _;
use std::time::Duration;

use ps_app::{AppState, PreviewView, import_file};
use ps_library::Settings;

const LIMIT_BYTES: usize = 1_500_000_000;

/// 10,000 small red rectangles on a grid: every one gets a cut.
fn stress_svg() -> String {
    let (count, columns, cell) = (10_000_usize, 100_usize, 2.9_f64);
    let mut svg = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"300mm\" height=\"300mm\" viewBox=\"0 0 300 300\">\n",
    );
    for i in 0..count {
        let x = cell * f64::from(u32::try_from(i % columns).unwrap_or(0)) + 0.5;
        let y = cell * f64::from(u32::try_from(i / columns).unwrap_or(0)) + 0.5;
        let _ = writeln!(
            svg,
            "<rect x=\"{x:.3}\" y=\"{y:.3}\" width=\"1.7\" height=\"1.7\" fill=\"none\" stroke=\"#FF0000\" stroke-width=\"0.3\"/>"
        );
    }
    svg.push_str("</svg>\n");
    svg
}

#[test]
fn the_ten_thousand_path_project_with_the_preview_open_stays_under_one_and_a_half_gigabytes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("stress.svg");
    std::fs::write(&file, stress_svg()).unwrap();

    let mut state = AppState::new(Settings::default());
    import_file(&mut state, &file, 0).expect("the stress drawing imports");
    state.tick(1_000);
    assert!(state.wait_for_compile(Duration::from_secs(120)), "compiles");

    // A 4K canvas with the whole 12 x 12 in mat in view is about 7 px per millimetre.
    let preview = state
        .preview(7.0, usize::MAX)
        .expect("a compiled job previews");
    // Every view the user can pick is built once, as clicking through them would.
    let flat = preview.render(&PreviewView::Flat);
    let tilted = preview.render(&PreviewView::Tilted {
        tilt_deg: 55.0,
        azimuth_deg: 30.0,
    });
    let weeded = preview.render(&PreviewView::Weeded { lift_mm: 6.0 });
    assert!(flat.width > 0 && tilted.width > 0 && weeded.width > 0);

    let used = memory_stats::memory_stats().expect("the platform reports memory use");
    assert!(
        used.physical_mem < LIMIT_BYTES,
        "{} MB resident with the stress project and preview open",
        used.physical_mem / 1_000_000
    );
    drop((preview, flat, tilted, weeded));
}
