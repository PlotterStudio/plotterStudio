//! Ground truth for tool changes, holders and pauses: each tool maps to a holder; a
//! change between holders needs no pause; a change within a holder pauses with a message naming
//! the tool and clamp; the job starts with one pause listing the initial tool in each holder; two
//! pen holders alternate colours without pauses.

use ps_core::LibRef;
use ps_job::HolderId;

use super::*;

fn tool(id: &str, kind: &str, label: &str, holders: &[&str]) -> ToolUse {
    ToolUse {
        tool: format!("{id}@1.0.0").parse::<LibRef>().unwrap(),
        kind: kind.to_owned(),
        label: label.to_owned(),
        holders: holders.iter().map(|h| HolderId::new(*h)).collect(),
        pen: None,
    }
}

fn labels() -> Vec<(HolderId, String)> {
    vec![
        (HolderId::new("a"), "clamp A".to_owned()),
        (HolderId::new("b"), "clamp B".to_owned()),
    ]
}

fn pen(n: u8) -> ToolUse {
    tool(&format!("pen.p{n}"), "pen", &format!("Pen {n}"), &["a"])
}

fn blade() -> ToolUse {
    tool("generic.blade", "blade", "Blade", &["b"])
}

fn plan_for(seq: &[ToolUse]) -> ToolPlan {
    plan_tools(seq, &labels())
}

#[test]
fn slots_are_numbered_from_one_in_order_of_first_use() {
    let plan = plan_for(&[blade(), pen(1), blade(), pen(2)]);
    let slots: Vec<(u8, String)> = plan
        .table
        .iter()
        .map(|t| (t.slot, t.tool.id().to_owned()))
        .collect();
    assert_eq!(
        slots,
        vec![
            (1, "generic.blade".to_owned()),
            (2, "pen.p1".to_owned()),
            (3, "pen.p2".to_owned())
        ]
    );
}

#[test]
fn a_tool_that_is_used_twice_has_one_table_row() {
    let plan = plan_for(&[pen(1), blade(), pen(1)]);
    assert_eq!(plan.table.len(), 2);
}

#[test]
fn the_job_starts_with_one_pause_listing_the_initial_tool_in_each_holder() {
    let plan = plan_for(&[pen(1), blade()]);
    let msg = plan.initial_pause.as_ref().unwrap().text();
    assert_eq!(
        msg,
        "Load pen: Pen 1 into clamp A\nLoad blade: Blade into clamp B"
    );
}

#[test]
fn a_change_between_holders_needs_no_pause() {
    let plan = plan_for(&[pen(1), blade(), pen(1)]);
    // Group 0 starts with the pen (no switch needed: it is the first), group 1 changes to the
    // blade in the other holder, group 2 back to the pen.
    let switches: Vec<(usize, bool)> = plan
        .switches
        .iter()
        .map(|s| (s.group, s.pause.is_some()))
        .collect();
    assert_eq!(switches, vec![(0, false), (1, false), (2, false)]);
    assert_eq!(plan.tool_changes(), 0);
}

#[test]
fn a_change_within_a_holder_pauses_and_names_the_tool_and_clamp() {
    let plan = plan_for(&[pen(1), pen(2)]);
    // Both pens only fit clamp A; the first is loaded at the start, the second needs a swap.
    let swap = plan.switches.iter().find(|s| s.group == 1).unwrap();
    assert_eq!(swap.holder, HolderId::new("a"));
    assert_eq!(
        swap.pause.as_ref().unwrap().text(),
        "Load pen: Pen 2 into clamp A"
    );
    assert_eq!(plan.tool_changes(), 1);
}

#[test]
fn only_the_first_tool_of_each_holder_is_in_the_initial_pause() {
    let plan = plan_for(&[pen(1), pen(2), blade()]);
    assert_eq!(
        plan.initial_pause.as_ref().unwrap().text(),
        "Load pen: Pen 1 into clamp A\nLoad blade: Blade into clamp B"
    );
}

#[test]
fn two_pen_holders_alternate_colours_without_pauses() {
    let both = |n: u8| {
        tool(
            &format!("pen.p{n}"),
            "pen",
            &format!("Pen {n}"),
            &["a", "b"],
        )
    };
    let plan = plan_for(&[both(1), both(2), both(1), both(2)]);
    assert_eq!(plan.tool_changes(), 0);
    let holders: Vec<&str> = plan.switches.iter().map(|s| s.holder.as_str()).collect();
    assert_eq!(holders, vec!["a", "b", "a", "b"]);
}

#[test]
fn three_colours_in_two_holders_swap_the_one_needed_furthest_ahead() {
    let both = |n: u8| {
        tool(
            &format!("pen.p{n}"),
            "pen",
            &format!("Pen {n}"),
            &["a", "b"],
        )
    };
    // 1 2 3 1 2 : when 3 is needed, holder a has 1 (next used at 3) and b has 2 (next used at 4),
    // so 2 is replaced.
    let plan = plan_for(&[both(1), both(2), both(3), both(1), both(2)]);
    let swap = plan.switches.iter().find(|s| s.group == 2).unwrap();
    assert_eq!(swap.holder.as_str(), "b");
    assert!(swap.pause.is_some());
}

#[test]
fn consecutive_groups_with_the_same_tool_need_no_switch() {
    let plan = plan_for(&[pen(1), pen(1), pen(1)]);
    assert_eq!(plan.switches.len(), 1);
}

#[test]
fn the_table_records_each_tools_holder() {
    let plan = plan_for(&[pen(1), blade()]);
    let holder_of: Vec<(&str, &str)> = plan
        .table
        .iter()
        .map(|t| (t.tool.id(), t.holder.as_str()))
        .collect();
    assert_eq!(holder_of, vec![("pen.p1", "a"), ("generic.blade", "b")]);
    assert_eq!(plan.table[0].label, "Pen 1");
}

#[test]
fn a_tool_with_no_usable_holder_is_reported() {
    let orphan = tool("x.thing", "tool", "Thing", &["zzz"]);
    let plan = plan_for(&[orphan]);
    assert_eq!(plan.unplaced, vec!["x.thing".to_owned()]);
    assert_eq!(plan.table.len(), 0);
    assert_eq!(plan.switches.len(), 0);
}

#[test]
fn an_empty_sequence_plans_nothing() {
    let plan = plan_for(&[]);
    assert_eq!(plan.table.len(), 0);
    assert!(plan.initial_pause.is_none());
    assert_eq!(plan.tool_changes(), 0);
}

#[test]
fn merging_adjacent_groups_with_the_same_tool() {
    let tools = ["pen", "pen", "blade", "pen", "pen"];
    assert_eq!(
        merge_adjacent(&tools),
        vec![vec![0, 1], vec![2], vec![3, 4]]
    );
    assert_eq!(merge_adjacent::<&str>(&[]).len(), 0);
}

#[test]
fn planning_is_deterministic() {
    let both = |n: u8| {
        tool(
            &format!("pen.p{n}"),
            "pen",
            &format!("Pen {n}"),
            &["a", "b"],
        )
    };
    let seq = [both(1), both(2), both(3), both(1), both(2), both(3)];
    assert_eq!(plan_for(&seq), plan_for(&seq));
}
