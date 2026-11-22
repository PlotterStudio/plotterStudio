//! Toolpath previews for the bundle.
//!
//! Deterministic SVG text: the work area as a frame, then one group per tool slot with one path
//! per contiguous drawn run (tool down to tool up). Travel is not drawn. SVG arcs share the
//! machine's y-down coordinates, so a counter-clockwise arc is simply sweep flag 1.

use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};
use std::fmt::Write;

use ps_core::PtUm;
use ps_job::{MatJob, Step};

use crate::number::um_trimmed;

/// Stroke colours by tool slot, cycled.
const PALETTE: [&str; 6] = [
    "#231F20", "#D62728", "#1F77B4", "#2CA02C", "#9467BD", "#FF7F0E",
];

#[expect(
    clippy::cast_precision_loss,
    reason = "grid coordinates are far below 2^53 µm, where i64 -> f64 is exact"
)]
fn f(v: i64) -> f64 {
    v as f64
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "a rounded grid coordinate on a circle that fits the machine"
)]
fn round_um(v: f64) -> i64 {
    libm::round(v) as i64
}

fn pt(p: PtUm) -> String {
    format!("{} {}", um_trimmed(p.x), um_trimmed(p.y))
}

/// The SVG arc command(s) for one step; a full circle needs two half arcs.
fn arc_commands(from: PtUm, to: PtUm, center: PtUm, ccw: bool) -> String {
    let radius = libm::hypot(f(from.x - center.x), f(from.y - center.y));
    let r = um_trimmed(round_um(radius));
    let start = libm::atan2(f(from.y - center.y), f(from.x - center.x));
    let end = libm::atan2(f(to.y - center.y), f(to.x - center.x));
    let sign = i32::from(ccw);
    if from == to {
        let far = PtUm::new(2 * center.x - from.x, 2 * center.y - from.y);
        return format!(
            "A{r} {r} 0 0 {sign} {}A{r} {r} 0 0 {sign} {}",
            pt(far),
            pt(to)
        );
    }
    let sweep = if ccw { end - start } else { start - end }.rem_euclid(TAU);
    let large = i32::from(sweep > PI);
    format!("A{r} {r} 0 {large} {sign} {}", pt(to))
}

struct Run {
    slot: u8,
    d: String,
}

/// Renders a mat's program as SVG, with the work area `width_um` × `height_um` as the frame.
#[must_use]
pub fn svg_preview(mat: &MatJob, width_um: i64, height_um: i64) -> String {
    let mut at = PtUm::new(0, 0);
    let mut slot = 1_u8;
    let mut current: Option<String> = None;
    let mut runs: Vec<Run> = Vec::new();

    let close = |current: &mut Option<String>, runs: &mut Vec<Run>, slot: u8| {
        if let Some(d) = current.take() {
            runs.push(Run { slot, d });
        }
    };

    for step in &mat.steps {
        match step {
            Step::LoadTool { slot: s, .. } => {
                close(&mut current, &mut runs, slot);
                slot = *s;
            }
            Step::ToolDown { .. } => {
                close(&mut current, &mut runs, slot);
                current = Some(format!("M{}", pt(at)));
            }
            Step::ToolUp => close(&mut current, &mut runs, slot),
            Step::Travel { to } => at = *to,
            Step::Line { to, .. } => {
                if let Some(d) = &mut current {
                    let _ = write!(d, "L{}", pt(*to));
                }
                at = *to;
            }
            Step::Arc {
                to, center, ccw, ..
            } => {
                if let Some(d) = &mut current {
                    d.push_str(&arc_commands(at, *to, *center, *ccw));
                }
                at = *to;
            }
            Step::Pause(_) | Step::Temperature { .. } | Step::Dwell { .. } => {}
        }
    }
    close(&mut current, &mut runs, slot);

    let (w, h) = (um_trimmed(width_um), um_trimmed(height_um));
    let mut out = String::new();
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}mm\" height=\"{h}mm\">\n\
         <rect x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\" fill=\"none\" stroke=\"#999999\" stroke-width=\"0.2\"/>\n"
    );
    let mut by_slot: BTreeMap<u8, Vec<&str>> = BTreeMap::new();
    for run in &runs {
        by_slot.entry(run.slot).or_default().push(&run.d);
    }
    for (slot, paths) in by_slot {
        let colour = PALETTE[usize::from(slot.saturating_sub(1)) % PALETTE.len()];
        let _ = writeln!(
            out,
            "<g id=\"tool-{slot}\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"0.4\" stroke-linecap=\"round\" stroke-linejoin=\"round\">"
        );
        for d in paths {
            let _ = writeln!(out, "<path d=\"{d}\"/>");
        }
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests;
