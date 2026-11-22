//! The G-code writer.
//!
//! A thin, deterministic serialiser of one mat's program: it makes no geometric decisions. The
//! output is UTF-8 with LF line endings, no trailing spaces and a final newline; coordinates are
//! printed from integer micrometres; words follow a fixed order (G/M/T, X Y, I J, A, F, S/P); the
//! feed is repeated only when it changes. The header comments carry no timestamps, so output
//! stays byte-identical between runs.

use ps_core::{LibRef, PtUm};
use ps_job::{MatJob, PauseRow, SrcRef, Step};
use ps_library::MachineProfile;

use crate::linearise::linearise;
use crate::number::{mdeg, pause_text, um};

/// What the header and the profile-dependent rules need besides the program.
#[derive(Debug, Clone, Copy)]
pub struct WriteContext<'a> {
    /// The app version, for the first header line.
    pub app_version: &'a str,
    /// The job id.
    pub job_id: &'a str,
    /// The machine profile the job was compiled for.
    pub profile: &'a MachineProfile,
    /// This mat's 1-based position in the job.
    pub mat_index: u32,
    /// How many mats the job has.
    pub mat_count: u32,
    /// A short description of the mat, such as `12x12in standard`.
    pub mat_label: &'a str,
    /// The material, when one is set.
    pub material: Option<&'a LibRef>,
}

/// Where one line of the file came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineInfo {
    /// Index of the step in the mat's program, or `None` for header and footer lines.
    pub step: Option<usize>,
    /// The source reference of a drawn move.
    pub src: Option<SrcRef>,
}

/// A written file and its line map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gcode {
    /// The file text.
    pub text: String,
    /// One entry per line of `text`, in order.
    pub lines: Vec<LineInfo>,
    /// The pauses in the file, with 1-based line numbers.
    pub pauses: Vec<PauseRow>,
}

impl Gcode {
    /// The 1-based numbers of the lines written for a step.
    #[must_use]
    pub fn lines_for_step(&self, step: usize) -> Vec<usize> {
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.step == Some(step))
            .map(|(i, _)| i + 1)
            .collect()
    }
}

struct Out {
    text: String,
    lines: Vec<LineInfo>,
    pauses: Vec<PauseRow>,
    /// Where the tool is, for arc centres relative to the start.
    at: PtUm,
    /// The last feed written, so it is repeated only when it changes.
    feed: Option<u32>,
    /// Whether the tool is down.
    down: bool,
}

impl Out {
    fn push(&mut self, line: &str, step: Option<usize>, src: Option<&SrcRef>) {
        self.text.push_str(line);
        self.text.push('\n');
        self.lines.push(LineInfo {
            step,
            src: src.cloned(),
        });
    }

    fn pause(&mut self, message: &str, step: Option<usize>) {
        let text = pause_text(message);
        let line = u32::try_from(self.lines.len() + 1).unwrap_or(u32::MAX);
        self.push(&format!("M0 ({text})"), step, None);
        self.pauses.push(PauseRow { line, text });
    }
}

fn holder_letter(profile: &MachineProfile, holder: &str) -> String {
    profile.holders.iter().find(|h| h.id == holder).map_or_else(
        || holder.to_owned(),
        |h| {
            h.label
                .strip_prefix("Clamp ")
                .unwrap_or(&h.label)
                .to_owned()
        },
    )
}

fn header(out: &mut Out, mat: &MatJob, ctx: &WriteContext<'_>) {
    out.push(&format!("; plotter-studio {}", ctx.app_version), None, None);
    out.push(&format!("; job {}", ctx.job_id), None, None);
    out.push(
        &format!(
            "; machine {}@{}  mat {}/{} {}",
            ctx.profile.id, ctx.profile.version, ctx.mat_index, ctx.mat_count, ctx.mat_label
        ),
        None,
        None,
    );
    if let Some(m) = ctx.material {
        out.push(&format!("; material {m}"), None, None);
    }
    for t in &mat.tool_table {
        out.push(
            &format!(
                "; T{} {} holder {}",
                t.slot,
                t.tool.id(),
                holder_letter(ctx.profile, t.holder.as_str())
            ),
            None,
            None,
        );
    }
    let seconds = (mat.estimate.machine_time_ms + 500) / 1_000;
    out.push(
        &format!(
            "; estimate {seconds}s  tool-changes {}",
            mat.estimate.tool_changes
        ),
        None,
        None,
    );
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a force in machine units is a small non-negative number"
)]
fn machine_force(profile: &MachineProfile, gf: u32) -> u64 {
    libm::round(profile.force.to_machine_units(f64::from(gf)).max(0.0)) as u64
}

impl Out {
    /// The ` F<feed>` word, or nothing when the feed has not changed.
    fn feed_word(&mut self, wanted: u32) -> String {
        if self.feed == Some(wanted) {
            String::new()
        } else {
            self.feed = Some(wanted);
            format!(" F{wanted}")
        }
    }

    fn arc(&mut self, arc: &ArcMove<'_>, profile: &MachineProfile, step: Option<usize>) {
        let ArcMove {
            to,
            center,
            ccw,
            feed_mm_min,
            src,
        } = *arc;
        if profile.gcode.arcs {
            let word = if ccw { "G3" } else { "G2" };
            let f = self.feed_word(feed_mm_min);
            let (i, j) = (center.x - self.at.x, center.y - self.at.y);
            self.push(
                &format!(
                    "{word} X{} Y{} I{} J{}{f}",
                    um(to.x),
                    um(to.y),
                    um(i),
                    um(j)
                ),
                step,
                Some(src),
            );
        } else {
            let tol = profile.gcode.chord_tolerance_mm;
            for chord in linearise(self.at, to, center, ccw, tol) {
                let f = self.feed_word(feed_mm_min);
                self.push(
                    &format!("G1 X{} Y{}{f}", um(chord.x), um(chord.y)),
                    step,
                    Some(src),
                );
            }
        }
        self.at = to;
    }

    fn step(&mut self, i: usize, step: &Step, profile: &MachineProfile) {
        let n = Some(i);
        match step {
            Step::Pause(msg) => self.pause(msg.text(), n),
            Step::LoadTool { slot, pause, .. } => {
                if let Some(msg) = pause {
                    self.pause(msg.text(), n);
                }
                self.push(&format!("T{slot} M6"), n, None);
            }
            Step::Temperature { celsius, wait } => {
                let word = if *wait { "M109" } else { "M104" };
                self.push(&format!("{word} S{celsius}"), n, None);
            }
            Step::Dwell { ms } => self.push(&format!("G4 P{ms}"), n, None),
            Step::ToolDown { force_gf } => {
                self.down = true;
                self.push(
                    &format!("M3 S{}", machine_force(profile, *force_gf)),
                    n,
                    None,
                );
            }
            Step::ToolUp => {
                self.down = false;
                self.push("M5", n, None);
            }
            Step::Travel { to } => {
                self.push(&format!("G0 X{} Y{}", um(to.x), um(to.y)), n, None);
                self.at = *to;
            }
            Step::Line {
                to,
                feed_mm_min,
                a_mdeg,
                src,
            } => {
                let a = a_mdeg.map_or_else(String::new, |a| format!(" A{}", mdeg(a)));
                let f = self.feed_word(*feed_mm_min);
                self.push(
                    &format!("G1 X{} Y{}{a}{f}", um(to.x), um(to.y)),
                    n,
                    Some(src),
                );
                self.at = *to;
            }
            Step::Arc {
                to,
                center,
                ccw,
                feed_mm_min,
                src,
            } => {
                let arc = ArcMove {
                    to: *to,
                    center: *center,
                    ccw: *ccw,
                    feed_mm_min: *feed_mm_min,
                    src,
                };
                self.arc(&arc, profile, n);
            }
        }
    }
}

/// An arc step's fields, gathered so the arc writer takes one argument.
#[derive(Clone, Copy)]
struct ArcMove<'a> {
    to: PtUm,
    center: PtUm,
    ccw: bool,
    feed_mm_min: u32,
    src: &'a SrcRef,
}

/// Writes one mat's program as G-code.
#[must_use]
pub fn write_mat(mat: &MatJob, ctx: &WriteContext<'_>) -> Gcode {
    let mut out = Out {
        text: String::new(),
        lines: Vec::new(),
        pauses: Vec::new(),
        at: PtUm::new(0, 0),
        feed: None,
        down: false,
    };
    header(&mut out, mat, ctx);
    out.push("G21", None, None);
    out.push("G90", None, None);

    for (i, step) in mat.steps.iter().enumerate() {
        out.step(i, step, ctx.profile);
    }

    if out.down {
        out.push("M5", None, None);
    }
    out.push("G0 X0.000 Y0.000", None, None);
    out.pause("Unload mat", None);
    out.push("M2", None, None);

    Gcode {
        text: out.text,
        lines: out.lines,
        pauses: out.pauses,
    }
}

#[cfg(test)]
mod tests;
