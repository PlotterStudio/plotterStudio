//! Ground truth for the G-code writer. The golden file is the specification's own example
//! (a pen outline in clamp A, then a cut in clamp B, medium cutter), reproduced byte for byte.
//! The rules under test: UTF-8, LF, no trailing spaces, final newline; coordinates from integer
//! micrometres with three decimals; word order G/M/T, X Y, I J, A, F, S/P; F only when it
//! changes; arcs only when the profile allows them, otherwise chords; header comments carry no
//! timestamps.

use ps_core::{LibRef, MatId, ObjectId, OpId, PathKey, PtUm};
use ps_job::{Estimate, HolderId, MatJob, PauseMsg, SrcRef, Step, ToolEntry};
use ps_library::{LibrarySnapshot, MachineProfile, builtin_entries};

use super::*;

fn libs() -> LibrarySnapshot {
    LibrarySnapshot::from_entries(builtin_entries().unwrap())
}

fn explore3() -> MachineProfile {
    let r: LibRef = "generic.cutter-medium@1.0.0".parse().unwrap();
    (*libs().resolve_profile(&r).unwrap().item).clone()
}

fn p(x: f64, y: f64) -> PtUm {
    PtUm::from_mm(x, y).unwrap()
}

fn src() -> SrcRef {
    SrcRef {
        mat: MatId::at_unix_ms(1),
        object: ObjectId::at_unix_ms(2),
        path: PathKey::source("p"),
        op: OpId::at_unix_ms(3),
        move_index: 0,
    }
}

fn line(x: f64, y: f64, feed: u32) -> Step {
    Step::Line {
        to: p(x, y),
        feed_mm_min: feed,
        a_mdeg: None,
        src: src(),
    }
}

fn load(slot: u8, holder: &str) -> Step {
    Step::LoadTool {
        slot,
        holder: HolderId::new(holder),
        pause: None,
    }
}

fn tool(slot: u8, id: &str, holder: &str, label: &str) -> ToolEntry {
    ToolEntry {
        slot,
        tool: format!("{id}@1.0.0").parse().unwrap(),
        holder: HolderId::new(holder),
        label: label.to_owned(),
        pen: None,
    }
}

fn mat_job(steps: Vec<Step>) -> MatJob {
    MatJob {
        mat: MatId::at_unix_ms(1),
        tool_table: vec![
            tool(
                1,
                "pen.fineliner-black-0.4",
                "generic.holder-a",
                "Fine liner black 0.4",
            ),
            tool(
                2,
                "generic.drag-blade",
                "generic.holder-b",
                "Fine-point blade",
            ),
        ],
        steps,
        estimate: Estimate {
            machine_time_ms: 252_000,
            tool_changes: 0,
            ..Estimate::default()
        },
    }
}

fn material() -> LibRef {
    "community.cardstock-80lb@2.1.0".parse().unwrap()
}

fn context<'a>(profile: &'a MachineProfile, material: Option<&'a LibRef>) -> WriteContext<'a> {
    WriteContext {
        app_version: "0.1.0",
        job_id: "0192f3a4-7c1e-7b2a-9f00-3d5c1a2b4e6f",
        profile,
        mat_index: 1,
        mat_count: 1,
        mat_label: "12x12in standard",
        material,
    }
}

fn example_steps() -> Vec<Step> {
    vec![
        Step::Pause(PauseMsg::new(
            "Load: T1 Fine liner black 0.4 into clamp A; T2 Fine-point blade into clamp B",
        )),
        load(1, "generic.holder-a"),
        Step::Travel { to: p(12.5, 20.0) },
        Step::ToolDown { force_gf: 150 },
        Step::Dwell { ms: 80 },
        line(62.5, 20.0, 6_000),
        line(62.5, 70.0, 6_000),
        Step::ToolUp,
        load(2, "generic.holder-b"),
        Step::Travel { to: p(9.75, 17.25) },
        Step::ToolDown { force_gf: 350 },
        line(65.25, 17.25, 3_000),
        line(65.25, 72.75, 3_000),
        Step::ToolUp,
    ]
}

const GOLDEN: &str = "\
; plotter-studio 0.1.0
; job 0192f3a4-7c1e-7b2a-9f00-3d5c1a2b4e6f
; machine generic.cutter-medium@1.0.0  mat 1/1 12x12in standard
; material community.cardstock-80lb@2.1.0
; T1 pen.fineliner-black-0.4 holder A
; T2 generic.drag-blade holder B
; estimate 252s  tool-changes 0
G21
G90
M0 (Load: T1 Fine liner black 0.4 into clamp A; T2 Fine-point blade into clamp B)
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
G1 X65.250 Y72.750
M5
G0 X0.000 Y0.000
M0 (Unload mat)
M2
";

fn write(steps: Vec<Step>) -> Gcode {
    let profile = explore3();
    let m = material();
    write_mat(&mat_job(steps), &context(&profile, Some(&m)))
}

fn body(g: &Gcode) -> Vec<&str> {
    g.text.lines().filter(|l| !l.starts_with(';')).collect()
}

#[test]
fn the_tdd_example_is_reproduced_byte_for_byte() {
    assert_eq!(write(example_steps()).text, GOLDEN);
}

#[test]
fn output_is_lf_only_with_no_trailing_spaces_and_a_final_newline() {
    let g = write(example_steps());
    assert!(g.text.ends_with('\n'));
    assert!(!g.text.contains('\r'));
    assert!(g.text.lines().all(|l| l == l.trim_end()), "trailing space");
    assert!(!g.text.ends_with("\n\n"));
}

#[test]
fn the_header_carries_no_timestamps() {
    // ISO timestamps contain colons; none of the header comments may.
    let g = write(example_steps());
    let header: Vec<&str> = g.text.lines().take_while(|l| l.starts_with(';')).collect();
    assert_eq!(header.len(), 7);
    assert!(header.iter().all(|l| !l.contains(':')), "{header:?}");
}

#[test]
fn the_material_line_is_left_out_when_there_is_none() {
    let profile = explore3();
    let g = write_mat(&mat_job(example_steps()), &context(&profile, None));
    assert!(!g.text.contains("; material"));
    assert!(g.text.contains("; T1 "));
}

#[test]
fn feed_is_repeated_only_when_it_changes() {
    let g = write(vec![
        Step::Travel { to: p(0.0, 0.0) },
        Step::ToolDown { force_gf: 100 },
        line(10.0, 0.0, 3_000),
        line(10.0, 10.0, 3_000),
        line(0.0, 10.0, 4_500),
        line(0.0, 0.0, 4_500),
        line(5.0, 5.0, 3_000),
    ]);
    let moves: Vec<&str> = body(&g)
        .into_iter()
        .filter(|l| l.starts_with("G1"))
        .collect();
    assert_eq!(
        moves,
        vec![
            "G1 X10.000 Y0.000 F3000",
            "G1 X10.000 Y10.000",
            "G1 X0.000 Y10.000 F4500",
            "G1 X0.000 Y0.000",
            "G1 X5.000 Y5.000 F3000",
        ]
    );
}

#[test]
fn negative_and_small_coordinates_print_from_integers() {
    let g = write(vec![Step::Travel {
        to: PtUm::new(-12_345, 7),
    }]);
    assert!(body(&g).contains(&"G0 X-12.345 Y0.007"));
}

#[test]
fn the_tangent_angle_comes_before_the_feed() {
    let g = write(vec![
        Step::Travel { to: p(0.0, 0.0) },
        Step::ToolDown { force_gf: 100 },
        Step::Line {
            to: p(10.0, 0.0),
            feed_mm_min: 3_000,
            a_mdeg: Some(-45_500),
            src: src(),
        },
    ]);
    assert!(
        body(&g).contains(&"G1 X10.000 Y0.000 A-45.500 F3000"),
        "{:?}",
        body(&g)
    );
}

#[test]
fn arcs_use_g2_and_g3_with_the_centre_relative_to_the_start() {
    let g = write(vec![
        Step::Travel { to: p(10.0, 0.0) },
        Step::ToolDown { force_gf: 100 },
        Step::Arc {
            to: p(0.0, 10.0),
            center: p(0.0, 0.0),
            ccw: true,
            feed_mm_min: 3_000,
            src: src(),
        },
        Step::Arc {
            to: p(10.0, 0.0),
            center: p(0.0, 0.0),
            ccw: false,
            feed_mm_min: 3_000,
            src: src(),
        },
    ]);
    let b = body(&g);
    assert!(
        b.contains(&"G3 X0.000 Y10.000 I-10.000 J0.000 F3000"),
        "{b:?}"
    );
    assert!(b.contains(&"G2 X10.000 Y0.000 I0.000 J-10.000"), "{b:?}");
}

#[test]
fn a_profile_without_arcs_gets_chords_instead() {
    let mut profile = explore3();
    profile.gcode.arcs = false;
    profile.gcode.chord_tolerance_mm = 0.01;
    let m = material();
    let g = write_mat(
        &mat_job(vec![
            Step::Travel { to: p(10.0, 0.0) },
            Step::ToolDown { force_gf: 100 },
            Step::Arc {
                to: p(0.0, 10.0),
                center: p(0.0, 0.0),
                ccw: true,
                feed_mm_min: 3_000,
                src: src(),
            },
        ]),
        &context(&profile, Some(&m)),
    );
    let b = body(&g);
    assert!(
        !b.iter()
            .any(|l| matches!(l.split(' ').next(), Some("G2" | "G3")))
    );
    let chords: Vec<&&str> = b.iter().filter(|l| l.starts_with("G1")).collect();
    assert!(chords.len() > 10, "{}", chords.len());
    assert_eq!(
        chords
            .last()
            .unwrap()
            .split(' ')
            .take(3)
            .collect::<Vec<_>>(),
        vec!["G1", "X0.000", "Y10.000"]
    );
}

#[test]
fn force_is_converted_to_machine_units() {
    let mut profile = explore3();
    profile.force.units_per_gf = 0.5;
    let m = material();
    let g = write_mat(
        &mat_job(vec![Step::ToolDown { force_gf: 150 }]),
        &context(&profile, Some(&m)),
    );
    assert!(body(&g).contains(&"M3 S75"), "{:?}", body(&g));
}

#[test]
fn temperature_steps_use_m104_and_m109() {
    let g = write(vec![
        Step::Temperature {
            celsius: 180,
            wait: false,
        },
        Step::Temperature {
            celsius: 200,
            wait: true,
        },
        Step::Dwell { ms: 1_500 },
    ]);
    let b = body(&g);
    assert!(
        b.contains(&"M104 S180") && b.contains(&"M109 S200") && b.contains(&"G4 P1500"),
        "{b:?}"
    );
}

#[test]
fn a_tool_swap_pause_comes_before_the_tool_change() {
    let g = write(vec![Step::LoadTool {
        slot: 2,
        holder: HolderId::new("generic.holder-a"),
        pause: Some(PauseMsg::new(
            "Load pen: Fine liner, black 0.4 into Clamp A",
        )),
    }]);
    let b = body(&g);
    let at = b
        .iter()
        .position(|l| l.starts_with("M0 (Load pen"))
        .unwrap();
    assert_eq!(b[at + 1], "T2 M6");
}

#[test]
fn pause_text_is_made_safe_for_the_parentheses() {
    let g = write(vec![Step::Pause(PauseMsg::new(
        "Load blade (45°)\ninto clamp B",
    ))]);
    assert!(
        body(&g).contains(&"M0 (Load blade [45°]; into clamp B)"),
        "{:?}",
        body(&g)
    );
}

#[test]
fn a_tool_left_down_is_lifted_before_the_footer() {
    let g = write(vec![
        Step::Travel { to: p(1.0, 1.0) },
        Step::ToolDown { force_gf: 100 },
        line(5.0, 5.0, 3_000),
    ]);
    let b = body(&g);
    let n = b.len();
    assert_eq!(
        &b[n - 4..],
        &["M5", "G0 X0.000 Y0.000", "M0 (Unload mat)", "M2"]
    );
}

#[test]
fn every_line_maps_back_to_the_step_that_made_it() {
    let g = write(example_steps());
    let numbered: Vec<(usize, &str)> = g.text.lines().enumerate().collect();
    assert_eq!(g.lines.len(), numbered.len());
    // The first rapid is step 2 of the program.
    let (at, _) = numbered
        .iter()
        .find(|(_, l)| *l == "G0 X12.500 Y20.000")
        .unwrap();
    assert_eq!(g.lines[*at].step, Some(2));
    // Header and footer lines belong to no step.
    assert_eq!(g.lines[0].step, None);
    assert_eq!(g.lines.last().unwrap().step, None);
    // A drawn move carries its source reference.
    let (at, _) = numbered
        .iter()
        .find(|(_, l)| l.starts_with("G1 X62.500 Y20.000"))
        .unwrap();
    assert!(g.lines[*at].src.is_some());
}

#[test]
fn the_step_to_line_lookup_is_the_inverse_of_the_line_map() {
    let g = write(example_steps());
    assert_eq!(
        g.lines_for_step(2),
        vec![
            g.text
                .lines()
                .position(|l| l == "G0 X12.500 Y20.000")
                .unwrap()
                + 1
        ]
    );
    assert_eq!(g.lines_for_step(999).len(), 0);
}

#[test]
fn pauses_are_listed_with_their_one_based_line_numbers() {
    let g = write(example_steps());
    let rows: Vec<(u32, &str)> = g.pauses.iter().map(|r| (r.line, r.text.as_str())).collect();
    let first = u32::try_from(
        g.text
            .lines()
            .position(|l| l.starts_with("M0 (Load:"))
            .unwrap()
            + 1,
    )
    .unwrap();
    let last =
        u32::try_from(g.text.lines().position(|l| l == "M0 (Unload mat)").unwrap() + 1).unwrap();
    assert_eq!(rows.first().map(|r| r.0), Some(first));
    assert_eq!(rows.last().copied(), Some((last, "Unload mat")));
}

#[test]
fn writing_is_deterministic() {
    // Build the steps once: each `src()` call makes fresh random ids.
    let steps = example_steps();
    assert_eq!(write(steps.clone()), write(steps));
}

#[test]
fn an_empty_program_still_has_a_header_and_footer() {
    let g = write(Vec::new());
    assert!(g.text.starts_with("; plotter-studio 0.1.0\n"));
    assert!(g.text.ends_with("M0 (Unload mat)\nM2\n"));
}
