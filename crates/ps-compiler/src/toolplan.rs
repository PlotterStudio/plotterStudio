//! Tool changes, holders and pauses.
//!
//! Each tool maps to a holder of the profile. The planner tracks which tool is in each holder. A
//! change between holders needs no pause; a change within a holder pauses with a message naming
//! the tool and the clamp. The job starts with one pause that lists the initial tool in each
//! holder. When a holder must be refilled the tool needed furthest in the future is replaced, so
//! two pen holders alternate colours without pauses.

use ps_core::LibRef;
use ps_job::{HolderId, PauseMsg, PenVariant, ToolEntry};

/// One group of the run, as the planner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolUse {
    /// The tool.
    pub tool: LibRef,
    /// What it is, for messages: "pen", "blade", …
    pub kind: String,
    /// Its display name.
    pub label: String,
    /// Holders that take it, in the profile's order of preference.
    pub holders: Vec<HolderId>,
    /// The colour or tip size this use of a pen has instead of the tool's own. Uses that differ
    /// here are different slots, as the pen is swapped between them.
    pub pen: Option<PenVariant>,
}

impl ToolUse {
    /// Whether `other` is the same tool in the same variant, so one slot serves both.
    fn same_slot(&self, other: &Self) -> bool {
        self.tool == other.tool && self.pen == other.pen
    }
}

/// Selecting a tool at the start of a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switch {
    /// Index into the group sequence.
    pub group: usize,
    /// The tool's slot.
    pub slot: u8,
    /// The holder to use.
    pub holder: HolderId,
    /// A message when the user must swap the tool; `None` when it is already loaded.
    pub pause: Option<PauseMsg>,
}

/// The result of planning tool use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPlan {
    /// One row per distinct tool, by slot.
    pub table: Vec<ToolEntry>,
    /// The pause at the start listing what to load, if any tool is used.
    pub initial_pause: Option<PauseMsg>,
    /// Where the active tool changes.
    pub switches: Vec<Switch>,
    /// Ids of tools that no holder of the profile takes.
    pub unplaced: Vec<String>,
}

impl ToolPlan {
    /// The number of pauses where the user swaps a tool during the job.
    #[must_use]
    pub fn tool_changes(&self) -> u32 {
        let n = self.switches.iter().filter(|s| s.pause.is_some()).count();
        u32::try_from(n).unwrap_or(u32::MAX)
    }
}

fn load_text(kind: &str, label: &str, holder: &str) -> String {
    format!("Load {kind}: {label} into {holder}")
}

/// The planner's working state: which tool sits in which holder.
struct Planner<'a> {
    holder_labels: &'a [(HolderId, String)],
    /// Distinct placeable tools, in order of first use; the index is the slot minus one.
    tools: Vec<&'a ToolUse>,
    /// Holder contents as indices into `tools`.
    contents: Vec<(HolderId, Option<usize>)>,
}

impl Planner<'_> {
    fn usable(&self, t: &ToolUse) -> Vec<HolderId> {
        self.holder_labels
            .iter()
            .map(|(id, _)| id)
            .filter(|id| t.holders.contains(id))
            .cloned()
            .collect()
    }

    fn label_of(&self, h: &HolderId) -> String {
        self.holder_labels
            .iter()
            .find(|(id, _)| id == h)
            .map_or_else(|| h.as_str().to_owned(), |(_, l)| l.clone())
    }

    fn tool_index(&self, t: &ToolUse) -> usize {
        self.tools.iter().position(|k| k.same_slot(t)).unwrap_or(0)
    }

    fn slot_of(&self, t: &ToolUse) -> u8 {
        u8::try_from(self.tool_index(t) + 1).unwrap_or(u8::MAX)
    }

    fn held_by(&self, holder: &HolderId) -> Option<usize> {
        self.contents
            .iter()
            .find(|(c, _)| c == holder)
            .and_then(|(_, held)| *held)
    }

    fn put(&mut self, holder: &HolderId, tool: usize) {
        if let Some(slot) = self.contents.iter_mut().find(|(c, _)| c == holder) {
            slot.1 = Some(tool);
        }
    }

    /// Loads each tool, in order of first use, into the first free holder that takes it.
    fn load_initial(&mut self) {
        for ti in 0..self.tools.len() {
            let free = self.usable(self.tools[ti]).into_iter().find(|h| {
                self.contents
                    .iter()
                    .any(|(c, held)| c == h && held.is_none())
            });
            if let Some(h) = free {
                self.put(&h, ti);
            }
        }
    }

    /// The pause listing what to load at the start.
    fn initial_pause(&self, initial: &[(HolderId, usize)]) -> Option<PauseMsg> {
        if initial.is_empty() {
            return None;
        }
        let lines: Vec<String> = initial
            .iter()
            .map(|(h, ti)| {
                let t = self.tools[*ti];
                load_text(&t.kind, &t.label, &self.label_of(h))
            })
            .collect();
        Some(PauseMsg::new(lines.join("\n")))
    }

    /// The holder to refill for `t`: the one whose tool is needed furthest in the future.
    fn swap_target(&self, t: &ToolUse, upcoming: &[&ToolUse]) -> Option<HolderId> {
        let next_use = |held: Option<usize>| -> usize {
            held.map_or(usize::MAX, |held| {
                upcoming
                    .iter()
                    .position(|u| self.tool_index(u) == held)
                    .unwrap_or(usize::MAX - 1)
            })
        };
        let mut best: Option<(usize, HolderId)> = None;
        for h in self.usable(t) {
            let distance = next_use(self.held_by(&h));
            if best.as_ref().is_none_or(|(b, _)| distance > *b) {
                best = Some((distance, h));
            }
        }
        best.map(|(_, h)| h)
    }

    /// Where the active tool changes along the placeable groups.
    fn switches(&mut self, placed: &[(usize, &ToolUse)]) -> Vec<Switch> {
        let mut switches = Vec::new();
        let mut active: Option<usize> = None;
        for (n, &(group, t)) in placed.iter().enumerate() {
            let ti = self.tool_index(t);
            if active == Some(ti) {
                continue;
            }
            active = Some(ti);
            let loaded = self
                .contents
                .iter()
                .find(|(h, held)| *held == Some(ti) && t.holders.contains(h))
                .map(|(h, _)| h.clone());
            let (holder, pause) = if let Some(h) = loaded {
                (h, None)
            } else {
                let upcoming: Vec<&ToolUse> = placed[n + 1..].iter().map(|(_, u)| *u).collect();
                let Some(h) = self.swap_target(t, &upcoming) else {
                    continue;
                };
                self.put(&h, ti);
                let msg = PauseMsg::new(load_text(&t.kind, &t.label, &self.label_of(&h)));
                (h, Some(msg))
            };
            switches.push(Switch {
                group,
                slot: self.slot_of(t),
                holder,
                pause,
            });
        }
        switches
    }
}

/// Plans holders, slots and pauses for a sequence of groups. `holder_labels` names the profile's
/// holders in preference order (the label is what messages call the clamp).
#[must_use]
pub fn plan_tools(seq: &[ToolUse], holder_labels: &[(HolderId, String)]) -> ToolPlan {
    let mut planner = Planner {
        holder_labels,
        tools: Vec::new(),
        contents: holder_labels
            .iter()
            .map(|(id, _)| (id.clone(), None))
            .collect(),
    };

    // Distinct placeable tools in order of first use get slots.
    let mut unplaced: Vec<String> = Vec::new();
    let mut placed: Vec<(usize, &ToolUse)> = Vec::new();
    for (group, t) in seq.iter().enumerate() {
        if planner.usable(t).is_empty() {
            if !unplaced.iter().any(|u| u == t.tool.id()) {
                unplaced.push(t.tool.id().to_owned());
            }
            continue;
        }
        placed.push((group, t));
        if !planner.tools.iter().any(|k| k.same_slot(t)) {
            planner.tools.push(t);
        }
    }

    planner.load_initial();
    let initial: Vec<(HolderId, usize)> = planner
        .contents
        .iter()
        .filter_map(|(h, t)| t.map(|t| (h.clone(), t)))
        .collect();
    let initial_pause = planner.initial_pause(&initial);
    let switches = planner.switches(&placed);

    // The table lists each tool's first holder (the initial one when it has one).
    let table = planner
        .tools
        .iter()
        .enumerate()
        .map(|(ti, t)| ToolEntry {
            slot: planner.slot_of(t),
            tool: t.tool.clone(),
            holder: initial
                .iter()
                .find(|(_, i)| *i == ti)
                .map(|(h, _)| h.clone())
                .or_else(|| planner.usable(t).into_iter().next())
                .unwrap_or_else(|| HolderId::new("")),
            label: t.label.clone(),
            pen: t.pen.clone(),
        })
        .collect();

    ToolPlan {
        table,
        initial_pause,
        switches,
        unplaced,
    }
}

/// Merges runs of adjacent equal tools into groups of indices ("minimise tool changes").
#[must_use]
pub fn merge_adjacent<T: PartialEq>(tools: &[T]) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    for (i, t) in tools.iter().enumerate() {
        match out.last_mut() {
            Some(run) if tools[run[0]] == *t => run.push(i),
            _ => out.push(vec![i]),
        }
    }
    out
}

#[cfg(test)]
mod tests;
