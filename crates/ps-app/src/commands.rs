//! The command registry, shortcuts and palette.
//!
//! One registry backs menus, shortcuts, context menus and the command palette, so every action
//! is reachable from the keyboard. Default shortcuts follow vector editors, with the command key
//! being Cmd on macOS and Ctrl elsewhere. Users can rebind shortcuts; conflicts are reported when
//! assigning. The palette searches every command by its label in the current language.

use std::collections::BTreeMap;

/// The operating system family, for how shortcuts are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Windows.
    Windows,
    /// macOS.
    MacOs,
    /// Linux and other Unix.
    Linux,
}

impl Platform {
    /// The platform this build runs on.
    #[must_use]
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// The modifier keys held during a key press. `command` is Cmd on macOS and Ctrl elsewhere.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    /// The command key.
    pub command: bool,
    /// Shift.
    pub shift: bool,
    /// Alt (Option on macOS).
    pub alt: bool,
}

/// A key with modifiers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Shortcut {
    /// The command key (Cmd on macOS, Ctrl elsewhere).
    pub command: bool,
    /// Shift.
    pub shift: bool,
    /// Alt.
    pub alt: bool,
    /// The key, in canonical case (`Z`, `Left`, `F6`).
    pub key: String,
}

const NAMED_KEYS: [&str; 15] = [
    "Left",
    "Right",
    "Up",
    "Down",
    "Delete",
    "Escape",
    "Enter",
    "Tab",
    "Space",
    "Backspace",
    "Equals",
    "Minus",
    "Home",
    "End",
    "Comma",
];

fn canonical_key(token: &str) -> Option<String> {
    let mut chars = token.chars();
    let first = chars.next()?;
    if chars.next().is_none() {
        return first
            .is_ascii_alphanumeric()
            .then(|| first.to_ascii_uppercase().to_string());
    }
    let lower = token.to_ascii_lowercase();
    if let Some(number) = lower.strip_prefix('f')
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
    {
        return Some(format!("F{number}"));
    }
    NAMED_KEYS
        .iter()
        .find(|name| name.eq_ignore_ascii_case(token))
        .map(|name| (*name).to_owned())
}

impl Shortcut {
    /// Reads `Mod+Shift+Z`-style text. `Mod`, `Ctrl`, `Cmd` and `Command` all mean the command
    /// key; case and modifier order do not matter.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let tokens: Vec<&str> = text.split('+').map(str::trim).collect();
        let (key, modifiers) = tokens.split_last()?;
        let mut shortcut = Self {
            command: false,
            shift: false,
            alt: false,
            key: canonical_key(key)?,
        };
        for m in modifiers {
            match m.to_ascii_lowercase().as_str() {
                "mod" | "ctrl" | "cmd" | "command" => shortcut.command = true,
                "shift" => shortcut.shift = true,
                "alt" | "option" => shortcut.alt = true,
                _ => return None,
            }
        }
        Some(shortcut)
    }

    fn text(&self, command_name: &str) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.command {
            parts.push(command_name);
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(&self.key);
        parts.join("+")
    }

    /// The text shown to the user, with the platform's name for the command key.
    #[must_use]
    pub fn display(&self, platform: Platform) -> String {
        self.text(match platform {
            Platform::MacOs => "Cmd",
            Platform::Windows | Platform::Linux => "Ctrl",
        })
    }

    /// The platform-independent text saved in settings.
    #[must_use]
    pub fn to_setting(&self) -> String {
        self.text("Mod")
    }

    /// Whether a key press with these modifiers is this shortcut.
    #[must_use]
    pub fn matches(&self, mods: Mods, key: &str) -> bool {
        self.command == mods.command
            && self.shift == mods.shift
            && self.alt == mods.alt
            && self.key.eq_ignore_ascii_case(key)
    }
}

/// What a command needs before it can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Needs {
    Nothing,
    Selection,
    Undo,
    Redo,
    Project,
    Compile,
    Device,
}

/// The facts command availability depends on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "a set of independent yes/no facts about the app, read one at a time"
)]
pub struct Context {
    /// Something is selected.
    pub has_selection: bool,
    /// There is something to undo.
    pub can_undo: bool,
    /// There is something to redo.
    pub can_redo: bool,
    /// A project is open.
    pub has_project: bool,
    /// A compiled job is ready.
    pub has_compile: bool,
    /// A machine is connected and idle.
    pub device_ready: bool,
}

/// One action the user can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// A stable id such as `file.save`.
    pub id: &'static str,
    /// The localisation key of its label.
    pub label_key: &'static str,
    /// The shortcut it ships with, as setting text.
    pub default_shortcut: Option<&'static str>,
    needs: Needs,
}

const fn cmd(
    id: &'static str,
    label_key: &'static str,
    default_shortcut: Option<&'static str>,
    needs: Needs,
) -> Command {
    Command {
        id,
        label_key,
        default_shortcut,
        needs,
    }
}

const COMMANDS: &[Command] = &[
    cmd("file.new", "cmd-file-new", Some("Mod+N"), Needs::Nothing),
    cmd("file.open", "cmd-file-open", Some("Mod+O"), Needs::Nothing),
    cmd(
        "file.import",
        "cmd-file-import",
        Some("Mod+I"),
        Needs::Nothing,
    ),
    cmd("file.save", "cmd-file-save", Some("Mod+S"), Needs::Project),
    cmd(
        "file.save-as",
        "cmd-file-save-as",
        Some("Mod+Shift+S"),
        Needs::Project,
    ),
    cmd(
        "file.export",
        "cmd-file-export",
        Some("Mod+E"),
        Needs::Compile,
    ),
    cmd("edit.undo", "cmd-edit-undo", Some("Mod+Z"), Needs::Undo),
    cmd(
        "edit.redo",
        "cmd-edit-redo",
        Some("Mod+Shift+Z"),
        Needs::Redo,
    ),
    cmd(
        "edit.delete",
        "cmd-edit-delete",
        Some("Delete"),
        Needs::Selection,
    ),
    cmd(
        "edit.duplicate",
        "cmd-edit-duplicate",
        Some("Mod+D"),
        Needs::Selection,
    ),
    cmd(
        "edit.select-all",
        "cmd-edit-select-all",
        Some("Mod+A"),
        Needs::Project,
    ),
    cmd("tool.select", "cmd-tool-select", Some("V"), Needs::Nothing),
    cmd("tool.node", "cmd-tool-node", Some("A"), Needs::Nothing),
    cmd(
        "tool.rectangle",
        "cmd-tool-rectangle",
        Some("R"),
        Needs::Nothing,
    ),
    cmd(
        "tool.ellipse",
        "cmd-tool-ellipse",
        Some("E"),
        Needs::Nothing,
    ),
    cmd("tool.pen", "cmd-tool-pen", Some("P"), Needs::Nothing),
    cmd("tool.line", "cmd-tool-line", Some("L"), Needs::Nothing),
    cmd(
        "tool.polygon",
        "cmd-tool-polygon",
        Some("G"),
        Needs::Nothing,
    ),
    cmd("tool.star", "cmd-tool-star", Some("S"), Needs::Nothing),
    cmd("tool.arc", "cmd-tool-arc", Some("C"), Needs::Nothing),
    cmd(
        "tool.arc-center",
        "cmd-tool-arc-center",
        Some("Shift+C"),
        Needs::Nothing,
    ),
    cmd("tool.knife", "cmd-tool-knife", Some("K"), Needs::Nothing),
    cmd(
        "tool.start-point",
        "cmd-tool-start-point",
        None,
        Needs::Nothing,
    ),
    cmd(
        "tool.guide",
        "cmd-tool-guide",
        Some("Mod+Shift+G"),
        Needs::Nothing,
    ),
    cmd("view.snap", "cmd-view-snap", None, Needs::Nothing),
    cmd(
        "view.zoom-fit",
        "cmd-view-zoom-fit",
        Some("Mod+0"),
        Needs::Nothing,
    ),
    cmd(
        "view.zoom-in",
        "cmd-view-zoom-in",
        Some("Mod+Equals"),
        Needs::Nothing,
    ),
    cmd(
        "view.zoom-out",
        "cmd-view-zoom-out",
        Some("Mod+Minus"),
        Needs::Nothing,
    ),
    cmd(
        "view.true-scale",
        "cmd-view-true-scale",
        Some("Mod+1"),
        Needs::Nothing,
    ),
    cmd(
        "mode.prepare",
        "cmd-mode-prepare",
        Some("Mod+Alt+1"),
        Needs::Nothing,
    ),
    cmd(
        "mode.preview",
        "cmd-mode-preview",
        Some("Mod+Alt+2"),
        Needs::Nothing,
    ),
    cmd(
        "mode.export",
        "cmd-mode-export",
        Some("Mod+Alt+3"),
        Needs::Nothing,
    ),
    cmd(
        "settings.open",
        "cmd-settings-open",
        Some("Mod+Comma"),
        Needs::Nothing,
    ),
    cmd("text.open", "cmd-text-open", None, Needs::Nothing),
    cmd("node.delete", "cmd-node-delete", None, Needs::Selection),
    cmd("node.to-curve", "cmd-node-to-curve", None, Needs::Selection),
    cmd("node.to-line", "cmd-node-to-line", None, Needs::Selection),
    cmd("node.smooth", "cmd-node-smooth", None, Needs::Selection),
    cmd("node.corner", "cmd-node-corner", None, Needs::Selection),
    cmd("mat.add", "cmd-mat-add", None, Needs::Project),
    cmd("mat.add-roll", "cmd-mat-add-roll", None, Needs::Project),
    cmd("mat.duplicate", "cmd-mat-duplicate", None, Needs::Project),
    cmd("mat.remove", "cmd-mat-remove", None, Needs::Project),
    cmd(
        "palette.open",
        "cmd-palette-open",
        Some("Mod+K"),
        Needs::Nothing,
    ),
    cmd(
        "focus.next-panel",
        "cmd-focus-next-panel",
        Some("F6"),
        Needs::Nothing,
    ),
    cmd(
        "focus.previous-panel",
        "cmd-focus-previous-panel",
        Some("Shift+F6"),
        Needs::Nothing,
    ),
    cmd(
        "machine.send",
        "cmd-machine-send",
        Some("Mod+Shift+M"),
        Needs::Device,
    ),
    cmd(
        "arrange.align-left",
        "cmd-arrange-align-left",
        None,
        Needs::Selection,
    ),
    cmd(
        "arrange.align-right",
        "cmd-arrange-align-right",
        None,
        Needs::Selection,
    ),
    cmd(
        "arrange.align-top",
        "cmd-arrange-align-top",
        None,
        Needs::Selection,
    ),
    cmd(
        "arrange.align-bottom",
        "cmd-arrange-align-bottom",
        None,
        Needs::Selection,
    ),
    cmd(
        "arrange.align-centre-h",
        "cmd-arrange-align-centre-h",
        None,
        Needs::Selection,
    ),
    cmd(
        "arrange.align-centre-v",
        "cmd-arrange-align-centre-v",
        None,
        Needs::Selection,
    ),
    cmd(
        "arrange.distribute-h",
        "cmd-arrange-distribute-h",
        None,
        Needs::Selection,
    ),
    cmd(
        "arrange.distribute-v",
        "cmd-arrange-distribute-v",
        None,
        Needs::Selection,
    ),
    cmd(
        "arrange.mirror-h",
        "cmd-arrange-mirror-h",
        None,
        Needs::Selection,
    ),
    cmd(
        "arrange.mirror-v",
        "cmd-arrange-mirror-v",
        None,
        Needs::Selection,
    ),
    cmd(
        "arrange.rotate-cw",
        "cmd-arrange-rotate-cw",
        None,
        Needs::Selection,
    ),
    cmd(
        "arrange.rotate-ccw",
        "cmd-arrange-rotate-ccw",
        None,
        Needs::Selection,
    ),
    cmd(
        "nudge.left",
        "cmd-nudge-left",
        Some("Left"),
        Needs::Selection,
    ),
    cmd(
        "nudge.right",
        "cmd-nudge-right",
        Some("Right"),
        Needs::Selection,
    ),
    cmd("nudge.up", "cmd-nudge-up", Some("Up"), Needs::Selection),
    cmd(
        "nudge.down",
        "cmd-nudge-down",
        Some("Down"),
        Needs::Selection,
    ),
    cmd(
        "nudge.left-big",
        "cmd-nudge-left-big",
        Some("Shift+Left"),
        Needs::Selection,
    ),
    cmd(
        "nudge.right-big",
        "cmd-nudge-right-big",
        Some("Shift+Right"),
        Needs::Selection,
    ),
    cmd(
        "nudge.up-big",
        "cmd-nudge-up-big",
        Some("Shift+Up"),
        Needs::Selection,
    ),
    cmd(
        "nudge.down-big",
        "cmd-nudge-down-big",
        Some("Shift+Down"),
        Needs::Selection,
    ),
];

/// The offset in millimetres a nudge command moves the selection by, for a base `step`;
/// `None` for any other command. The `-big` variants (Shift) move ten times as far.
#[must_use]
pub fn nudge_distance_mm(id: &str, step: f64) -> Option<(f64, f64)> {
    let name = id.strip_prefix("nudge.")?;
    let (direction, factor) = match name.strip_suffix("-big") {
        Some(d) => (d, 10.0),
        None => (name, 1.0),
    };
    let d = step * factor;
    match direction {
        "left" => Some((-d, 0.0)),
        "right" => Some((d, 0.0)),
        "up" => Some((0.0, -d)),
        "down" => Some((0.0, d)),
        _ => None,
    }
}

/// Why a shortcut could not be assigned.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssignError {
    /// No such command.
    #[error("no such command")]
    UnknownCommand,
    /// Another command already uses the shortcut.
    #[error("the shortcut is already used by {with}")]
    Conflict {
        /// The id of the command that has it.
        with: &'static str,
    },
}

/// A palette result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The command id.
    pub id: &'static str,
    /// Its label in the current language.
    pub label: String,
    /// Its shortcut, if bound.
    pub shortcut: Option<Shortcut>,
    /// Whether it can run now.
    pub enabled: bool,
}

/// All commands with the user's shortcut bindings applied.
#[derive(Debug, Clone)]
pub struct Registry {
    bindings: BTreeMap<&'static str, Option<Shortcut>>,
}

impl Registry {
    /// The default registry with the user's overrides (`id -> shortcut text`, empty to unbind)
    /// applied. Overrides for unknown commands or with unreadable text are ignored.
    #[must_use]
    pub fn new(overrides: &BTreeMap<String, String>) -> Self {
        let mut bindings: BTreeMap<&'static str, Option<Shortcut>> = COMMANDS
            .iter()
            .map(|c| (c.id, c.default_shortcut.and_then(Shortcut::parse)))
            .collect();
        for (id, text) in overrides {
            let Some(command) = COMMANDS.iter().find(|c| c.id == id) else {
                continue;
            };
            if text.trim().is_empty() {
                bindings.insert(command.id, None);
            } else if let Some(shortcut) = Shortcut::parse(text) {
                bindings.insert(command.id, Some(shortcut));
            }
        }
        Self { bindings }
    }

    /// Every command, in menu order.
    #[must_use]
    pub fn all(&self) -> &'static [Command] {
        COMMANDS
    }

    /// A command by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&'static Command> {
        COMMANDS.iter().find(|c| c.id == id)
    }

    /// The shortcut bound to a command now.
    #[must_use]
    pub fn shortcut_of(&self, id: &str) -> Option<Shortcut> {
        self.bindings.get(id).cloned().flatten()
    }

    /// The command a shortcut runs.
    #[must_use]
    pub fn find_by_shortcut(&self, shortcut: &Shortcut) -> Option<&'static Command> {
        COMMANDS
            .iter()
            .find(|c| self.bindings.get(c.id).and_then(Option::as_ref) == Some(shortcut))
    }

    /// Shortcuts bound to more than one command, with the commands sharing each.
    #[must_use]
    pub fn conflicts(&self) -> Vec<(Shortcut, Vec<&'static str>)> {
        let mut by_shortcut: BTreeMap<Shortcut, Vec<&'static str>> = BTreeMap::new();
        for (id, shortcut) in &self.bindings {
            if let Some(s) = shortcut {
                by_shortcut.entry(s.clone()).or_default().push(id);
            }
        }
        by_shortcut
            .into_iter()
            .filter(|(_, ids)| ids.len() > 1)
            .collect()
    }

    /// Binds a shortcut (or unbinds with `None`).
    ///
    /// # Errors
    ///
    /// [`AssignError`] when the command is unknown or another command already has the shortcut;
    /// nothing changes then.
    pub fn assign(&mut self, id: &str, shortcut: Option<Shortcut>) -> Result<(), AssignError> {
        let command = self.get(id).ok_or(AssignError::UnknownCommand)?;
        if let Some(s) = &shortcut
            && let Some(other) = self.find_by_shortcut(s)
            && other.id != command.id
        {
            return Err(AssignError::Conflict { with: other.id });
        }
        self.bindings.insert(command.id, shortcut);
        Ok(())
    }

    /// Restores a command's default shortcut.
    pub fn reset(&mut self, id: &str) {
        if let Some(command) = self.get(id) {
            self.bindings.insert(
                command.id,
                command.default_shortcut.and_then(Shortcut::parse),
            );
        }
    }

    /// The bindings that differ from the defaults, as saved in settings.
    #[must_use]
    pub fn overrides(&self) -> BTreeMap<String, String> {
        COMMANDS
            .iter()
            .filter_map(|c| {
                let default = c.default_shortcut.and_then(Shortcut::parse);
                let now = self.bindings.get(c.id).cloned().flatten();
                (now != default).then(|| {
                    (
                        c.id.to_owned(),
                        now.map(|s| s.to_setting()).unwrap_or_default(),
                    )
                })
            })
            .collect()
    }

    /// Whether a command can run in `ctx`.
    #[must_use]
    pub fn is_enabled(&self, id: &str, ctx: &Context) -> bool {
        self.get(id).is_some_and(|c| match c.needs {
            Needs::Nothing => true,
            Needs::Selection => ctx.has_selection,
            Needs::Undo => ctx.can_undo,
            Needs::Redo => ctx.can_redo,
            Needs::Project => ctx.has_project,
            Needs::Compile => ctx.has_compile,
            Needs::Device => ctx.device_ready && ctx.has_compile,
        })
    }

    /// Searches every command by label. `label` turns a command id into its text in the current
    /// language. Prefix matches rank above substring matches, shorter labels above longer ones,
    /// and commands that cannot run now come after those that can.
    #[must_use]
    pub fn search(&self, query: &str, label: &dyn Fn(&str) -> String, ctx: &Context) -> Vec<Hit> {
        let needle = query.trim().to_lowercase();
        let mut scored: Vec<(bool, u8, usize, usize, Hit)> = Vec::new();
        for (index, command) in COMMANDS.iter().enumerate() {
            let text = label(command.id);
            let lower = text.to_lowercase();
            let rank = if needle.is_empty() || lower.starts_with(&needle) {
                0
            } else if lower.contains(&needle) {
                1
            } else {
                continue;
            };
            let enabled = self.is_enabled(command.id, ctx);
            scored.push((
                !enabled,
                rank,
                lower.chars().count(),
                index,
                Hit {
                    id: command.id,
                    label: text,
                    shortcut: self.shortcut_of(command.id),
                    enabled,
                },
            ));
        }
        if needle.is_empty() {
            scored.sort_by_key(|(disabled, _, _, index, _)| (*disabled, *index));
        } else {
            scored.sort_by_key(|(disabled, rank, len, index, _)| (*disabled, *rank, *len, *index));
        }
        scored.into_iter().map(|(.., hit)| hit).collect()
    }
}

#[cfg(test)]
mod tests;
