//! Ties every command in the registry to the API tool that does the same thing, or says why no
//! tool is needed. A new command with no entry fails a test, so the window cannot grow a
//! capability the API lacks.

/// How the API covers one registry command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// A tool operation does it.
    Op {
        /// The tool.
        tool: &'static str,
        /// The `op` value.
        op: &'static str,
    },
    /// The command only changes what the window shows, so no tool is needed.
    UiOnly(&'static str),
}

const fn op(tool: &'static str, op: &'static str) -> Coverage {
    Coverage::Op { tool, op }
}

const VIEW: Coverage = Coverage::UiOnly("changes only what the window shows");
const POINTER: Coverage = Coverage::UiOnly("a pointer tool; the same edits are tool operations");

const TABLE: &[(&str, Coverage)] = &[
    ("file.new", op("project", "new")),
    ("file.open", op("project", "open")),
    ("file.import", op("import", "file")),
    ("file.save", op("project", "save")),
    ("file.save-as", op("project", "save")),
    ("file.export", op("export", "files")),
    ("edit.undo", op("edit", "undo")),
    ("edit.redo", op("edit", "redo")),
    ("edit.delete", op("object", "delete")),
    ("edit.duplicate", op("object", "duplicate")),
    (
        "edit.select-all",
        Coverage::UiOnly("ids are explicit; `query` objects lists every one"),
    ),
    ("tool.select", POINTER),
    ("tool.node", POINTER),
    ("tool.rectangle", op("import", "shape")),
    ("tool.ellipse", op("import", "shape")),
    ("tool.pen", op("import", "shape")),
    ("tool.line", op("import", "shape")),
    ("tool.polygon", op("import", "shape")),
    ("tool.star", op("import", "shape")),
    ("tool.arc", op("import", "shape")),
    ("tool.arc-center", op("import", "shape")),
    ("tool.knife", op("object", "split")),
    ("tool.start-point", op("object", "set_start_point")),
    ("tool.guide", op("mat", "add_guide")),
    ("view.snap", VIEW),
    ("view.zoom-fit", VIEW),
    ("view.zoom-in", VIEW),
    ("view.zoom-out", VIEW),
    ("view.true-scale", VIEW),
    ("mode.prepare", VIEW),
    ("mode.preview", VIEW),
    ("mode.export", VIEW),
    ("settings.open", VIEW),
    ("text.open", op("import", "text")),
    ("node.delete", op("object", "node")),
    ("node.to-curve", op("object", "node")),
    ("node.to-line", op("object", "node")),
    ("node.smooth", op("object", "node")),
    ("node.corner", op("object", "node")),
    ("mat.add", op("mat", "add")),
    ("mat.add-roll", op("mat", "add_roll")),
    ("mat.duplicate", op("mat", "duplicate")),
    ("mat.remove", op("mat", "remove")),
    ("palette.open", VIEW),
    ("focus.next-panel", VIEW),
    ("focus.previous-panel", VIEW),
    ("machine.send", op("machine", "send")),
    ("arrange.align-left", op("object", "align")),
    ("arrange.align-right", op("object", "align")),
    ("arrange.align-top", op("object", "align")),
    ("arrange.align-bottom", op("object", "align")),
    ("arrange.align-centre-h", op("object", "align")),
    ("arrange.align-centre-v", op("object", "align")),
    ("arrange.distribute-h", op("object", "distribute")),
    ("arrange.distribute-v", op("object", "distribute")),
    ("arrange.mirror-h", op("object", "mirror")),
    ("arrange.mirror-v", op("object", "mirror")),
    ("arrange.rotate-cw", op("object", "rotate")),
    ("arrange.rotate-ccw", op("object", "rotate")),
    ("nudge.left", op("object", "move")),
    ("nudge.right", op("object", "move")),
    ("nudge.up", op("object", "move")),
    ("nudge.down", op("object", "move")),
    ("nudge.left-big", op("object", "move")),
    ("nudge.right-big", op("object", "move")),
    ("nudge.up-big", op("object", "move")),
    ("nudge.down-big", op("object", "move")),
];

/// How the API covers each document command of `ps-model`: the struct's name, then the tool and
/// operation that issue it. A command nobody mapped fails a test.
const MODEL_TABLE: &[(&str, Coverage)] = &[
    ("AddMat", op("mat", "add")),
    ("RemoveMat", op("mat", "remove")),
    ("AddObject", op("import", "shape")),
    ("RemoveObject", op("object", "delete")),
    ("SetTransform", op("object", "move")),
    ("SetOverride", op("operation", "set_param")),
    ("AssignOperation", op("operation", "assign")),
    ("RemoveOperation", op("operation", "remove")),
    ("SetPenColour", op("operation", "set_pen_colour")),
    ("SetOpFill", op("operation", "set_fill")),
    ("SetOpEnabled", op("operation", "enable")),
    ("SetMatMaterial", op("mat", "set_material")),
    ("SetMirror", op("mat", "set_mirror")),
    ("AddAsset", op("import", "file")),
    ("SetPathGeometry", op("object", "node")),
    ("SetOpOrder", op("operation", "reorder")),
    ("SplitPath", op("object", "split")),
    ("SetStartPoint", op("object", "set_start_point")),
    ("SetRunAfter", op("operation", "lock_after_previous")),
    ("SetGuides", op("mat", "add_guide")),
    ("SetJobSettings", op("operation", "set_job")),
    ("SetRules", op("operation", "set_rules")),
    ("RemovePath", op("object", "delete")),
];

/// How the API covers a `ps-model` document command, or `None` when nobody decided.
#[must_use]
pub fn model_coverage(command: &str) -> Option<Coverage> {
    MODEL_TABLE
        .iter()
        .find(|(name, _)| *name == command)
        .map(|(_, c)| *c)
}

/// Every `ps-model` command name the table decides.
pub fn model_covered() -> impl Iterator<Item = &'static str> {
    MODEL_TABLE.iter().map(|(name, _)| *name)
}

/// How the API covers a registry command, or `None` when nobody decided.
#[must_use]
pub fn coverage(command: &str) -> Option<Coverage> {
    TABLE.iter().find(|(id, _)| *id == command).map(|(_, c)| *c)
}

/// Every command id the table decides.
pub fn covered_ids() -> impl Iterator<Item = &'static str> {
    TABLE.iter().map(|(id, _)| *id)
}
