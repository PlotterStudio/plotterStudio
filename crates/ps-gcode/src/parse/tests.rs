//! Ground truth for the G-code parser. The dialect is fixed (`ps-gcode/1`): G0, G1,
//! G2/G3, G4 P, G20/G21, G90, M0, M2, M3 S<gf>, M5, T<n> M6, M104/M109, and A for the tangent
//! angle. The device host parses each line once so plugins can map either the text or the parsed
//! form and can never disagree about what a line means. Coordinates stay exact integers (µm); the
//! feed is modal. The parser reads exactly what the writer produces.

use ps_core::PtUm;

use super::*;

fn cmd(text: &str) -> Command {
    let mut state = ParseState::default();
    parse_line(text, &mut state).unwrap()
}

fn p(x: i64, y: i64) -> PtUm {
    PtUm::new(x, y)
}

// ---- single commands ----

#[test]
fn rapid_moves_read_exact_micrometres() {
    assert_eq!(
        cmd("G0 X12.500 Y20.000"),
        Command::Rapid {
            to: p(12_500, 20_000)
        }
    );
    assert_eq!(
        cmd("G0 X-12.345 Y0.007"),
        Command::Rapid { to: p(-12_345, 7) }
    );
    assert_eq!(cmd("G0 X0.000 Y0.000"), Command::Rapid { to: p(0, 0) });
}

#[test]
fn numbers_may_have_fewer_decimals_but_never_more_than_three() {
    assert_eq!(
        cmd("G0 X12 Y1.5"),
        Command::Rapid {
            to: p(12_000, 1_500)
        }
    );
    let mut s = ParseState::default();
    assert!(parse_line("G0 X1.2345 Y0", &mut s).is_err());
}

#[test]
fn a_linear_move_carries_the_modal_feed() {
    let mut s = ParseState::default();
    let first = parse_line("G1 X62.500 Y20.000 F6000", &mut s).unwrap();
    assert_eq!(
        first,
        Command::Linear {
            to: p(62_500, 20_000),
            a_mdeg: None,
            feed_mm_min: 6_000
        }
    );
    let second = parse_line("G1 X62.500 Y70.000", &mut s).unwrap();
    assert_eq!(
        second,
        Command::Linear {
            to: p(62_500, 70_000),
            a_mdeg: None,
            feed_mm_min: 6_000
        }
    );
    let third = parse_line("G1 X0.000 Y0.000 F3000", &mut s).unwrap();
    assert!(matches!(
        third,
        Command::Linear {
            feed_mm_min: 3_000,
            ..
        }
    ));
}

#[test]
fn a_move_before_any_feed_is_an_error() {
    let mut s = ParseState::default();
    assert!(matches!(
        parse_line("G1 X1.000 Y1.000", &mut s),
        Err(ParseError { .. })
    ));
}

#[test]
fn the_tangent_angle_reads_in_milli_degrees() {
    let mut s = ParseState::default();
    let c = parse_line("G1 X10.000 Y0.000 A-45.500 F3000", &mut s).unwrap();
    assert_eq!(
        c,
        Command::Linear {
            to: p(10_000, 0),
            a_mdeg: Some(-45_500),
            feed_mm_min: 3_000
        }
    );
}

#[test]
fn arcs_read_the_relative_centre_and_the_direction() {
    let mut s = ParseState::default();
    let ccw = parse_line("G3 X0.000 Y10.000 I-10.000 J0.000 F3000", &mut s).unwrap();
    assert_eq!(
        ccw,
        Command::Arc {
            to: p(0, 10_000),
            centre_offset: p(-10_000, 0),
            ccw: true,
            feed_mm_min: 3_000
        }
    );
    let cw = parse_line("G2 X10.000 Y0.000 I0.000 J-10.000", &mut s).unwrap();
    assert_eq!(
        cw,
        Command::Arc {
            to: p(10_000, 0),
            centre_offset: p(0, -10_000),
            ccw: false,
            feed_mm_min: 3_000
        }
    );
}

#[test]
fn tool_commands_read_their_values() {
    assert_eq!(cmd("M3 S150"), Command::ToolDown { force: 150 });
    assert_eq!(cmd("M5"), Command::ToolUp);
    assert_eq!(cmd("T2 M6"), Command::ToolChange { slot: 2 });
    assert_eq!(cmd("G4 P80"), Command::Dwell { ms: 80 });
    assert_eq!(
        cmd("M104 S180"),
        Command::Temperature {
            celsius: 180,
            wait: false
        }
    );
    assert_eq!(
        cmd("M109 S200"),
        Command::Temperature {
            celsius: 200,
            wait: true
        }
    );
    assert_eq!(cmd("M2"), Command::End);
}

#[test]
fn setup_codes_are_accepted() {
    assert_eq!(cmd("G21"), Command::Units { millimetres: true });
    assert_eq!(cmd("G20"), Command::Units { millimetres: false });
    assert_eq!(cmd("G90"), Command::Absolute);
}

#[test]
fn pauses_keep_their_message() {
    assert_eq!(
        cmd("M0 (Load: T1 Fine liner black 0.4 into clamp A; T2 Fine-point blade into clamp B)"),
        Command::Pause(
            "Load: T1 Fine liner black 0.4 into clamp A; T2 Fine-point blade into clamp B".into()
        )
    );
    assert_eq!(cmd("M0 (Unload mat)"), Command::Pause("Unload mat".into()));
    assert_eq!(cmd("M0"), Command::Pause(String::new()));
}

#[test]
fn comments_and_blank_lines_are_comments() {
    assert_eq!(
        cmd("; plotter-studio 0.1.0"),
        Command::Comment("plotter-studio 0.1.0".into())
    );
    assert_eq!(cmd(""), Command::Comment(String::new()));
    assert_eq!(cmd("   "), Command::Comment(String::new()));
}

#[test]
fn a_trailing_comment_is_ignored() {
    assert_eq!(cmd("G4 P80 ; settle"), Command::Dwell { ms: 80 });
}

#[test]
fn unknown_codes_are_refused() {
    let mut s = ParseState::default();
    for line in [
        "G28",
        "M104",
        "G1 Z5 F100",
        "M3",
        "G4",
        "T M6",
        "X5",
        "G0 X1",
        "G0 Q1 X0 Y0",
        "M6",
    ] {
        assert!(parse_line(line, &mut s).is_err(), "{line:?}");
    }
}

#[test]
fn the_error_says_which_word_was_wrong() {
    let mut s = ParseState::default();
    let e = parse_line("G28", &mut s).unwrap_err();
    assert!(e.message.contains("G28"), "{}", e.message);
}

// ---- programs ----

const GOLDEN: &str = "\
; plotter-studio 0.1.0
G21
G90
M0 (Load: T1 into clamp A)
T1 M6
G0 X12.500 Y20.000
M3 S150
G4 P80
G1 X62.500 Y20.000 F6000
G1 X62.500 Y70.000
M5
T2 M6
G0 X9.750 Y17.250
M3 S350
G1 X65.250 Y17.250 F3000
M5
G0 X0.000 Y0.000
M0 (Unload mat)
M2
";

#[test]
fn a_program_is_parsed_line_by_line_with_numbers_and_text() {
    let lines = parse_program(GOLDEN).unwrap();
    assert_eq!(lines.len(), 19);
    assert_eq!(lines[0].number, 1);
    assert_eq!(lines[0].text, "; plotter-studio 0.1.0");
    assert_eq!(lines[5].number, 6);
    assert_eq!(
        lines[5].command,
        Command::Rapid {
            to: p(12_500, 20_000)
        }
    );
    assert_eq!(lines[18 - 1].command, Command::Pause("Unload mat".into()));
    assert_eq!(lines.last().unwrap().command, Command::End);
}

#[test]
fn the_feed_stays_modal_across_tool_changes() {
    let lines = parse_program(GOLDEN).unwrap();
    let feeds: Vec<u32> = lines
        .iter()
        .filter_map(|l| {
            if let Command::Linear { feed_mm_min, .. } = l.command {
                Some(feed_mm_min)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(feeds, vec![6_000, 6_000, 3_000]);
}

#[test]
fn the_first_bad_line_is_reported_with_its_number() {
    let e = parse_program("G21\nG90\nG99 X1\nM2\n").unwrap_err();
    assert_eq!(e.line, 3);
}

#[test]
fn an_empty_program_has_no_lines() {
    assert_eq!(parse_program("").unwrap().len(), 0);
}

#[test]
fn crlf_line_endings_are_tolerated() {
    let lines = parse_program("G21\r\nG90\r\nM2\r\n").unwrap();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].text, "G21");
}

#[test]
fn the_end_of_the_program_is_reported_by_its_line() {
    let lines = parse_program(GOLDEN).unwrap();
    assert_eq!(end_line(&lines), Some(19));
    assert_eq!(end_line(&parse_program("G21\n").unwrap()), None);
}

#[test]
fn pause_lines_are_listed_for_the_host() {
    let lines = parse_program(GOLDEN).unwrap();
    let pauses: Vec<(u32, &str)> = pause_lines(&lines).into_iter().collect();
    assert_eq!(
        pauses,
        vec![(4, "Load: T1 into clamp A"), (18, "Unload mat")]
    );
}
