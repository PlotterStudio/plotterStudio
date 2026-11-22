//! Edits that reach an operations row or an object by id rather than through the selection:
//! taking a row's operation off every path, deleting one object, finding a row's objects, and
//! the ink colour of a pen row.

use ps_core::{ObjectId, OpRowId};
use ps_geometry::{FillSpec, Pattern};
use ps_model::{
    Command as ModelCommand, OpKind, RemoveObject, RemoveOperation, SetOpFill, SetPenColour,
};

use super::AppState;

impl AppState {
    /// Takes the row's operation off every path that has it, as one undo step.
    pub fn remove_row(&mut self, row: OpRowId, now_ms: u64) -> bool {
        let cmds: Vec<Box<dyn ModelCommand>> = self
            .row_operations(row)
            .into_iter()
            .map(|(object, path, op)| {
                Box::new(RemoveOperation {
                    mat: self.mat,
                    object,
                    path,
                    op: op.id,
                }) as Box<dyn ModelCommand>
            })
            .collect();
        !cmds.is_empty() && self.commit(&cmds, now_ms)
    }

    /// Selects the objects that carry the row's operation; false when none do.
    pub fn select_row_objects(&mut self, row: OpRowId) -> bool {
        let mut ids: Vec<ObjectId> = Vec::new();
        for (object, _, _) in self.row_operations(row) {
            if !ids.contains(&object) {
                ids.push(object);
            }
        }
        if ids.is_empty() {
            return false;
        }
        self.selection.set_objects(&ids);
        true
    }

    /// Deletes one object of the current mat, whether or not it is selected.
    pub fn delete_object(&mut self, object: ObjectId, now_ms: u64) -> bool {
        let cmd: Box<dyn ModelCommand> = Box::new(RemoveObject {
            mat: self.mat,
            object,
        });
        let done = self.commit(&[cmd], now_ms);
        if done {
            self.selection.retain_existing(|id| id != object);
        }
        done
    }

    /// The ink colour of a pen row as `#RRGGBB`: the one chosen for it, else the pen's own. `None`
    /// for a row whose tool is not a pen.
    #[must_use]
    pub fn row_pen_colour(&self, row: OpRowId) -> Option<String> {
        let ops = self.row_operations(row);
        let (_, _, op) = ops.first()?;
        let tool = self.libs.resolve_tool(&op.tool.tool).ok()?.item;
        let own = tool.pen.as_ref()?.colour.hex.clone();
        Some(op.tool.pen_colour.clone().unwrap_or(own))
    }

    /// Sets the ink colour of a pen row (`None` goes back to the pen's own); false when the row's
    /// tool is not a pen.
    pub fn set_row_pen_colour(&mut self, row: OpRowId, colour: Option<&str>, now_ms: u64) -> bool {
        if self.row_pen_colour(row).is_none() {
            return false;
        }
        let cmds: Vec<Box<dyn ModelCommand>> = self
            .row_operations(row)
            .into_iter()
            .map(|(object, path, op)| {
                Box::new(SetPenColour {
                    mat: self.mat,
                    object,
                    path,
                    op: op.id,
                    colour: colour.map(str::to_owned),
                }) as Box<dyn ModelCommand>
            })
            .collect();
        !cmds.is_empty() && self.commit(&cmds, now_ms)
    }

    /// The fill settings of a fill row: the ones set on it, else the defaults. `None` for a row
    /// that is not a fill.
    #[must_use]
    pub fn row_fill(&self, row: OpRowId) -> Option<FillSpec> {
        let ops = self.row_operations(row);
        let (_, _, op) = ops.first()?;
        if !matches!(op.kind, OpKind::Fill | OpKind::FoilFill) {
            return None;
        }
        Some(op.fill.unwrap_or(FillSpec {
            pattern: Pattern::Parallel,
            angle_deg: 0.0,
            overlap: 0.15,
            outline_pass: false,
        }))
    }

    /// Sets the fill settings of a fill row on every operation in it.
    pub fn set_row_fill(&mut self, row: OpRowId, fill: FillSpec, now_ms: u64) -> bool {
        if self.row_fill(row).is_none() {
            return false;
        }
        let cmds: Vec<Box<dyn ModelCommand>> = self
            .row_operations(row)
            .into_iter()
            .map(|(object, path, op)| {
                Box::new(SetOpFill {
                    mat: self.mat,
                    object,
                    path,
                    op: op.id,
                    fill: Some(fill),
                }) as Box<dyn ModelCommand>
            })
            .collect();
        !cmds.is_empty() && self.commit(&cmds, now_ms)
    }
}
