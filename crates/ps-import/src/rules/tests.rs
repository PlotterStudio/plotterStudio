//! Ground truth for mapping rules. A rule is a predicate on source style and layer plus
//! the operation to assign. Predicates: colour (exact or within a CIEDE2000 tolerance), dash
//! present, stroke width range, layer name glob, filled or open. Rules run in order and the first
//! match wins. Unmatched paths get a warning, never a silent default. `pen:auto` maps to the
//! nearest owned pen by CIEDE2000 after an exact-match pass.

use ps_core::Severity;
use ps_model::{Assign, MappingRule, Match, OpKind, SourceStyle};

use super::*;

fn style() -> SourceStyle {
    SourceStyle::default()
}

fn stroked(rgb: [u8; 3]) -> SourceStyle {
    SourceStyle {
        stroke: Some(rgb),
        ..style()
    }
}

fn rule(matcher: Match, op: OpKind, tool: &str) -> MappingRule {
    MappingRule {
        matcher,
        assign: Assign {
            op,
            tool: tool.to_owned(),
        },
    }
}

fn colour(hex: &str, tolerance: Option<f64>) -> Match {
    Match {
        stroke: Some(hex.to_owned()),
        tolerance_de: tolerance,
        ..Match::default()
    }
}

fn layer(glob: &str) -> Match {
    Match {
        layer: Some(glob.to_owned()),
        ..Match::default()
    }
}

fn pens() -> Vec<OwnedPen> {
    vec![
        OwnedPen {
            tool_id: "pen.red".into(),
            colour: [200, 30, 30],
        },
        OwnedPen {
            tool_id: "pen.green".into(),
            colour: [30, 160, 60],
        },
        OwnedPen {
            tool_id: "pen.black".into(),
            colour: [20, 20, 20],
        },
    ]
}

fn assigned(m: &Mapping) -> (usize, OpKind, &str) {
    match m {
        Mapping::Assigned { rule, op, tool, .. } => (*rule, *op, tool.as_str()),
        other => panic!("expected an assignment, got {other:?}"),
    }
}

// ---- predicates ----

#[test]
fn an_empty_predicate_matches_everything() {
    assert!(matches(&Match::default(), &style()));
    assert!(matches(&Match::default(), &stroked([1, 2, 3])));
}

#[test]
fn an_exact_colour_matches_only_that_colour() {
    let m = colour("#FF0000", None);
    assert!(matches(&m, &stroked([255, 0, 0])));
    assert!(!matches(&m, &stroked([254, 0, 0])));
    assert!(
        !matches(&m, &style()),
        "a path with no colour cannot match a colour rule"
    );
}

#[test]
fn a_tolerance_accepts_nearby_colours_by_ciede2000() {
    let m = colour("#0000FF", Some(5.0));
    assert!(matches(&m, &stroked([10, 10, 250])));
    assert!(!matches(&m, &stroked([255, 0, 0])));
}

#[test]
fn the_colour_of_a_fill_only_path_is_its_fill() {
    let m = colour("#00FF00", None);
    let fill_only = SourceStyle {
        fill: Some([0, 255, 0]),
        ..style()
    };
    assert!(matches(&m, &fill_only));
}

#[test]
fn the_stroke_colour_wins_over_the_fill_colour() {
    let m = colour("#FF0000", None);
    let both = SourceStyle {
        stroke: Some([255, 0, 0]),
        fill: Some([0, 255, 0]),
        ..style()
    };
    assert!(matches(&m, &both));
    assert!(!matches(&colour("#00FF00", None), &both));
}

#[test]
fn a_malformed_colour_never_matches() {
    assert!(!matches(&colour("red", None), &stroked([255, 0, 0])));
}

#[test]
fn layer_names_match_a_glob() {
    let on = |name: &str| SourceStyle {
        layer: Some(name.to_owned()),
        ..style()
    };
    assert!(matches(&layer("cut*"), &on("cut-lines")));
    assert!(!matches(&layer("cut*"), &on("draw")));
    assert!(matches(&layer("*"), &on("anything")));
    assert!(matches(&layer("layer?"), &on("layer1")));
    assert!(!matches(&layer("layer?"), &on("layer10")));
    assert!(matches(&layer("[ab]-layer"), &on("b-layer")));
    assert!(
        !matches(&layer("cut*"), &on("Cut-lines")),
        "globs are case sensitive"
    );
}

#[test]
fn a_layer_rule_does_not_match_a_path_without_a_layer() {
    assert!(!matches(&layer("*"), &style()));
}

#[test]
fn a_malformed_glob_never_matches() {
    let s = SourceStyle {
        layer: Some("a".into()),
        ..style()
    };
    assert!(!matches(&layer("[unclosed"), &s));
}

#[test]
fn filled_and_open_are_told_apart() {
    let filled = SourceStyle {
        fill: Some([0, 0, 0]),
        ..style()
    };
    let wants = |v| Match {
        filled: Some(v),
        ..Match::default()
    };
    assert!(matches(&wants(true), &filled));
    assert!(!matches(&wants(false), &filled));
    assert!(matches(&wants(false), &stroked([1, 1, 1])));
}

#[test]
fn dashes_are_told_apart_from_solid_lines() {
    let dashed = SourceStyle {
        dash: vec![2.0, 1.0],
        ..stroked([0, 0, 255])
    };
    let wants = |v| Match {
        dashed: Some(v),
        ..Match::default()
    };
    assert!(matches(&wants(true), &dashed));
    assert!(!matches(&wants(true), &stroked([0, 0, 255])));
    assert!(matches(&wants(false), &stroked([0, 0, 255])));
}

#[test]
fn stroke_width_ranges_are_inclusive() {
    let w = |mm| SourceStyle {
        stroke_width_mm: Some(mm),
        ..stroked([0, 0, 0])
    };
    let range = Match {
        min_width_mm: Some(0.2),
        max_width_mm: Some(0.5),
        ..Match::default()
    };
    assert!(matches(&range, &w(0.2)));
    assert!(matches(&range, &w(0.5)));
    assert!(!matches(&range, &w(0.19)));
    assert!(!matches(&range, &w(0.51)));
    assert!(!matches(&range, &stroked([0, 0, 0])), "no width, no match");
}

#[test]
fn a_one_sided_width_range_works() {
    let w = |mm| SourceStyle {
        stroke_width_mm: Some(mm),
        ..stroked([0, 0, 0])
    };
    let thick = Match {
        min_width_mm: Some(1.0),
        ..Match::default()
    };
    assert!(matches(&thick, &w(5.0)) && !matches(&thick, &w(0.5)));
    let thin = Match {
        max_width_mm: Some(0.3),
        ..Match::default()
    };
    assert!(matches(&thin, &w(0.1)) && !matches(&thin, &w(0.4)));
}

#[test]
fn every_set_predicate_must_hold() {
    let m = Match {
        stroke: Some("#0000FF".into()),
        dashed: Some(true),
        layer: Some("score*".into()),
        ..Match::default()
    };
    let good = SourceStyle {
        dash: vec![1.0, 1.0],
        layer: Some("score-lines".into()),
        ..stroked([0, 0, 255])
    };
    assert!(matches(&m, &good));
    assert!(!matches(
        &m,
        &SourceStyle {
            dash: vec![],
            ..good.clone()
        }
    ));
    assert!(!matches(
        &m,
        &SourceStyle {
            layer: Some("other".into()),
            ..good.clone()
        }
    ));
    assert!(!matches(
        &m,
        &SourceStyle {
            stroke: Some([0, 0, 254]),
            ..good
        }
    ));
}

// ---- mapping ----

#[test]
fn the_first_matching_rule_wins() {
    let rules = vec![
        rule(colour("#FF0000", None), OpKind::Cut, "blade"),
        rule(Match::default(), OpKind::Draw, "pen"),
        rule(colour("#FF0000", None), OpKind::Score, "stylus"),
    ];
    let m = map_style(&rules, &stroked([255, 0, 0]), &[]);
    assert_eq!(assigned(&m), (0, OpKind::Cut, "blade"));
    let other = map_style(&rules, &stroked([0, 0, 1]), &[]);
    assert_eq!(assigned(&other), (1, OpKind::Draw, "pen"));
}

#[test]
fn a_path_no_rule_matches_is_unmapped() {
    let rules = vec![rule(colour("#FF0000", None), OpKind::Cut, "blade")];
    assert_eq!(
        map_style(&rules, &stroked([0, 255, 0]), &[]),
        Mapping::Unmapped
    );
    assert_eq!(map_style(&[], &style(), &[]), Mapping::Unmapped);
}

#[test]
fn pen_auto_picks_an_exact_colour_first() {
    let rules = vec![rule(Match::default(), OpKind::Draw, "pen:auto")];
    // Pure black is closest to the black pen anyway, but an exact match must win even when a
    // different pen is nearer by CIEDE2000.
    let tricky = vec![
        OwnedPen {
            tool_id: "near".into(),
            colour: [101, 100, 100],
        },
        OwnedPen {
            tool_id: "exact".into(),
            colour: [100, 100, 100],
        },
    ];
    let m = map_style(&rules, &stroked([100, 100, 100]), &tricky);
    assert_eq!(assigned(&m).2, "exact");
}

#[test]
fn pen_auto_otherwise_picks_the_nearest_owned_pen() {
    let rules = vec![rule(Match::default(), OpKind::Draw, "pen:auto")];
    assert_eq!(
        assigned(&map_style(&rules, &stroked([220, 40, 40]), &pens())).2,
        "pen.red"
    );
    assert_eq!(
        assigned(&map_style(&rules, &stroked([40, 170, 70]), &pens())).2,
        "pen.green"
    );
    assert_eq!(
        assigned(&map_style(&rules, &stroked([5, 5, 5]), &pens())).2,
        "pen.black"
    );
}

#[test]
fn pen_auto_reports_the_pen_colour_it_chose() {
    let rules = vec![rule(Match::default(), OpKind::Draw, "pen:auto")];
    match map_style(&rules, &stroked([220, 40, 40]), &pens()) {
        Mapping::Assigned { pen_colour, .. } => assert_eq!(pen_colour, Some([200, 30, 30])),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_explicit_tool_has_no_pen_colour() {
    let rules = vec![rule(Match::default(), OpKind::Cut, "blade")];
    match map_style(&rules, &stroked([1, 2, 3]), &pens()) {
        Mapping::Assigned { pen_colour, .. } => assert_eq!(pen_colour, None),
        other => panic!("{other:?}"),
    }
}

#[test]
fn pen_auto_with_no_owned_pens_is_reported_not_guessed() {
    let rules = vec![rule(Match::default(), OpKind::Draw, "pen:auto")];
    assert_eq!(
        map_style(&rules, &stroked([1, 2, 3]), &[]),
        Mapping::NoOwnedPen
    );
}

#[test]
fn pen_auto_needs_a_colour_to_match() {
    let rules = vec![rule(Match::default(), OpKind::Draw, "pen:auto")];
    assert_eq!(map_style(&rules, &style(), &pens()), Mapping::NoColour);
}

#[test]
fn unmapped_paths_produce_a_warning_each_never_a_silent_default() {
    let rules = vec![rule(colour("#FF0000", None), OpKind::Cut, "blade")];
    let styles = vec![stroked([255, 0, 0]), stroked([0, 255, 0]), style()];
    let (mapped, diags) = map_all(&rules, &styles, &[]);
    assert_eq!(mapped.len(), 3);
    assert!(matches!(mapped[0], Mapping::Assigned { .. }));
    assert_eq!(diags.len(), 2);
    assert!(diags.iter().all(|d| d.severity == Severity::Warning));
    assert!(
        diags
            .iter()
            .all(|d| d.key.as_str() == "diag-import-unmapped")
    );
}

#[test]
fn a_missing_pen_or_colour_is_a_warning_with_its_own_key() {
    let rules = vec![rule(Match::default(), OpKind::Draw, "pen:auto")];
    let (_, diags) = map_all(&rules, &[stroked([1, 2, 3])], &[]);
    assert_eq!(
        diags.iter().next().unwrap().key.as_str(),
        "diag-import-no-owned-pen"
    );
    let (_, diags) = map_all(&rules, &[style()], &pens());
    assert_eq!(
        diags.iter().next().unwrap().key.as_str(),
        "diag-import-no-colour"
    );
}

// ---- validating a rule set ----

#[test]
fn a_good_rule_set_has_no_issues() {
    let rules = vec![
        rule(colour("#FF0000", Some(3.0)), OpKind::Cut, "blade"),
        rule(layer("cut*"), OpKind::Cut, "blade"),
    ];
    assert_eq!(check_rules(&rules).len(), 0);
}

#[test]
fn bad_colours_globs_tolerances_and_ranges_are_reported_with_their_rule() {
    let rules = vec![
        rule(colour("nope", None), OpKind::Cut, "blade"),
        rule(layer("[bad"), OpKind::Cut, "blade"),
        rule(colour("#FF0000", Some(-1.0)), OpKind::Cut, "blade"),
        rule(
            Match {
                min_width_mm: Some(2.0),
                max_width_mm: Some(1.0),
                ..Match::default()
            },
            OpKind::Cut,
            "blade",
        ),
        rule(Match::default(), OpKind::Cut, ""),
    ];
    let issues = check_rules(&rules);
    let at: Vec<usize> = issues.iter().map(|i| i.rule).collect();
    assert_eq!(at, vec![0, 1, 2, 3, 4]);
}
