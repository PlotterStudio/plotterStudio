//! Ground truth for dashing: a path is cut
//! into the pieces a dash pattern keeps, with the pattern given as on, off, on, off lengths.

use ps_geometry::{Path, Point};

use super::*;

fn line(len: f64) -> Path {
    Path::polyline(&[Point::new(0.0, 0.0), Point::new(len, 0.0)], false)
}

fn lengths(pieces: &[Path]) -> Vec<f64> {
    pieces.iter().map(Path::length).collect()
}

#[test]
fn a_dash_pattern_keeps_the_on_pieces() {
    // 10 long with 2 on, 1 off: on 0..2, 3..5, 6..8, 9..10.
    let pieces = dash(&line(10.0), &[2.0, 1.0]);
    let l = lengths(&pieces);
    assert_eq!(l.len(), 4);
    for (got, want) in l.iter().zip([2.0, 2.0, 2.0, 1.0]) {
        assert!((got - want).abs() < 1e-9, "{l:?}");
    }
    assert!((pieces[1].start.x - 3.0).abs() < 1e-9);
}

#[test]
fn no_pattern_or_a_useless_one_keeps_the_path_whole() {
    let whole = dash(&line(10.0), &[]);
    assert_eq!(whole.len(), 1);
    assert!((whole[0].length() - 10.0).abs() < 1e-9);
    for bad in [vec![0.0, 0.0], vec![-1.0, 2.0], vec![f64::NAN, 1.0]] {
        assert_eq!(dash(&line(10.0), &bad).len(), 1, "{bad:?}");
    }
}

#[test]
fn a_path_shorter_than_one_dash_is_one_piece() {
    let pieces = dash(&line(1.0), &[2.0, 1.0]);
    assert_eq!(pieces.len(), 1);
    assert!((pieces[0].length() - 1.0).abs() < 1e-9);
}

#[test]
fn a_path_ending_in_a_gap_loses_its_tail() {
    // 2 on, 3 off over length 4: only 0..2 is kept.
    let pieces = dash(&line(4.0), &[2.0, 3.0]);
    assert_eq!(pieces.len(), 1);
    assert!((pieces[0].length() - 2.0).abs() < 1e-9);
}

#[test]
fn an_odd_pattern_repeats_as_on_off_pairs_would() {
    // [3] means 3 on, 3 off.
    let pieces = dash(&line(9.0), &[3.0]);
    assert_eq!(lengths(&pieces).len(), 2);
}

#[test]
fn dashes_follow_a_bend() {
    let bent = Path::polyline(
        &[
            Point::new(0.0, 0.0),
            Point::new(4.0, 0.0),
            Point::new(4.0, 4.0),
        ],
        false,
    );
    let pieces = dash(&bent, &[3.0, 2.0]); // on 0..3, off 3..5, on 5..8
    let l = lengths(&pieces);
    assert_eq!(l.len(), 2);
    assert!(
        (l[0] - 3.0).abs() < 1e-9 && (l[1] - 3.0).abs() < 1e-9,
        "{l:?}"
    );
    // The second dash starts 1 mm up the vertical leg.
    assert!((pieces[1].start.x - 4.0).abs() < 1e-9 && (pieces[1].start.y - 1.0).abs() < 1e-9);
}

#[test]
fn pattern_pixels_become_millimetres_at_a_zoom() {
    let pattern = dash_for_zoom(&[8.0, 4.0], 4.0);
    assert_eq!(pattern, vec![2.0, 1.0]);
}

#[test]
fn a_huge_number_of_dashes_is_capped() {
    // A tiny pattern over a long path would make millions of pieces.
    let pieces = dash(&line(1_000_000.0), &[0.001, 0.001]);
    assert!(pieces.len() <= MAX_DASHES + 1);
}
