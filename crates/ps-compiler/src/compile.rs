//! The compiler: one pure function from a document snapshot, the libraries, a machine
//! profile and calibration to the job IR.
//!
//! Stages: validate; resolve parameters; prepare geometry per (object, operation); drive-type
//! processing; order blocks, rows and paths; plan tools and pauses; safety checks; estimate; emit
//! steps on the µm grid. Nothing here reads a clock, touches the file system or uses global state.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use ps_core::{
    Diagnostic, Diagnostics, GridError, L10nKey, Location, MatId, PtUm, StableHasher, um_from_mm,
};
use ps_geometry::{Affine, Path, Point, Seg, flatten, path_to_arcs};
use ps_job::{HolderId, Job, MatJob, PauseMsg, PenVariant, SrcRef, Step};
use ps_library::{
    Calibration, Colour, DriveType, LibrarySnapshot, MachineProfile, Origin, Tool, ToolFamily,
};
use ps_model::{Document, Mat, OpKind, Operation, OriginSpec, PlacedObject, ShapePath, validate};

use crate::cache::CompileCache;
use crate::colour::lightness;
use crate::estimate::{EstimateInput, estimate, utilisation_ppm};
use crate::motion::MotionLimits;
use crate::order::{Item, containment, emission_order, improve_sweeps, nearest_neighbour};
use crate::prepare::{Note, PrepareInput, Prepared, Stroke};
use crate::safety::{
    Cutter, Drawn, MachineLimits, Rect, Violation, check_point, check_segment, clamp_force,
    release_edges,
};
use crate::schedule::{Block, Stage, schedule, stage_of};
use crate::simplify::simplify_emits;
use crate::tangential::unwrapped_angles;
use crate::toolplan::{ToolPlan, ToolUse, plan_tools};

/// Tolerance for converting curves to arcs, in mm.
const ARC_TOL: f64 = 0.002;
/// Longest allowed deviation of a flattened curve for machines without arcs, floor in mm.
const MIN_CHORD_TOL: f64 = 0.001;
/// Time allowed for each user pause when estimating, in ms.
const PAUSE_ALLOWANCE_MS: u32 = 30_000;
/// The temperature a heated tool starts from, in °C.
const AMBIENT_C: i32 = 25;
/// Rounds of "plan tools, re-prepare with the real holder offsets" before giving up.
const HOLDER_ROUNDS: usize = 3;

/// Which mats to compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatSelection {
    /// Every mat, in document order.
    All,
    /// One mat, for live preview.
    One(MatId),
}

/// Everything the compiler reads.
#[derive(Debug, Clone, Copy)]
pub struct CompileInput<'a> {
    /// The document snapshot.
    pub doc: &'a Document,
    /// Tools and materials, resolved by reference.
    pub libs: &'a LibrarySnapshot,
    /// The machine profile.
    pub profile: &'a MachineProfile,
    /// Per-machine calibration.
    pub calibration: &'a Calibration,
    /// Which mats to compile.
    pub mats: MatSelection,
}

/// Why compiling failed outright (problems in the design are diagnostics instead).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompileError {
    /// The selected mat is not in the document.
    #[error("mat {0:?} is not in the document")]
    UnknownMat(MatId),
    /// A coordinate does not fit the µm grid.
    #[error("a coordinate on mat {0:?} is outside the machine grid")]
    Coordinate(MatId),
}

struct Work<'a> {
    object: &'a PlacedObject,
    path: &'a ShapePath,
    op: &'a Operation,
    tool: Arc<Tool>,
    params: BTreeMap<String, f64>,
    block: usize,
}

fn family_word(f: ToolFamily) -> &'static str {
    match f {
        ToolFamily::Pen => "pen",
        ToolFamily::Blade => "blade",
        ToolFamily::Foil => "foil tool",
        ToolFamily::Scoring => "scoring stylus",
        ToolFamily::Emboss => "embossing tool",
        ToolFamily::Engrave => "engraving tool",
        ToolFamily::Perforate => "perforating blade",
        ToolFamily::Wheel => "wheel",
        ToolFamily::Heated => "heated tool",
        ToolFamily::Other => "tool",
    }
}

fn grid(mat: MatId) -> impl Fn(GridError) -> CompileError {
    move |_| CompileError::Coordinate(mat)
}

fn to_um(mat: MatId, p: Point) -> Result<PtUm, CompileError> {
    PtUm::from_mm(p.x, p.y).map_err(grid(mat))
}

fn machine_limits(profile: &MachineProfile) -> Result<MachineLimits, GridError> {
    let rect = |r: &ps_library::RectMm| -> Result<Rect, GridError> {
        Ok(Rect {
            x0: um_from_mm(r.x_mm)?,
            y0: um_from_mm(r.y_mm)?,
            x1: um_from_mm(r.x_mm + r.width_mm)?,
            y1: um_from_mm(r.y_mm + r.height_mm)?,
        })
    };
    let keep_outs = profile
        .keep_outs
        .iter()
        .map(rect)
        .collect::<Result<Vec<_>, _>>()?;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "force limits are small non-negative gram-force values"
    )]
    let gf = |v: f64| libm::round(v.max(0.0)) as u32;
    Ok(MachineLimits {
        work_w_um: um_from_mm(profile.work_area.width_mm)?,
        work_h_um: um_from_mm(profile.work_area.height_mm)?,
        keep_outs,
        force_min_gf: gf(profile.force.min_gf),
        force_max_gf: gf(profile.force.max_gf),
    })
}

/// The mat-to-machine transform without the object transform: mirror, origin, then the holder's
/// calibrated offset.
fn machine_transform(mat: &Mat, profile: &MachineProfile, holder_offset: (f64, f64)) -> Affine {
    let mirror = if mat.mirror {
        Affine::new([-1.0, 0.0, 0.0, 1.0, mat.size.width.get(), 0.0])
    } else {
        Affine::IDENTITY
    };
    let custom = match mat.origin {
        OriginSpec::Profile => Affine::IDENTITY,
        OriginSpec::Custom(p) => Affine::translate((-p.x, -p.y)),
    };
    let (w, h) = (profile.work_area.width_mm, profile.work_area.height_mm);
    let corner = match profile.origin {
        Origin::TopLeft => Affine::IDENTITY,
        Origin::TopRight => Affine::new([-1.0, 0.0, 0.0, 1.0, w, 0.0]),
        Origin::BottomLeft => Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, h]),
        Origin::BottomRight => Affine::new([-1.0, 0.0, 0.0, -1.0, w, h]),
    };
    Affine::translate(holder_offset) * corner * custom * mirror
}

fn blade_offset(tool: &Tool, cal: &Calibration) -> f64 {
    cal.blade_offset(&tool.id, tool.geometry.blade_offset_mm.unwrap_or(0.0))
}

/// The tip width in force: the operation's `tip_width` setting, else the tool's own.
fn tip_width(tool: &Tool, params: &BTreeMap<String, f64>) -> Option<f64> {
    params
        .get("tip_width")
        .copied()
        .or(tool.geometry.tip_width_mm)
}

/// How this use of a pen differs from the pen in the library, if it does: another ink colour or a
/// different tip width.
fn pen_variant(op: &Operation, tool: &Tool, params: &BTreeMap<String, f64>) -> Option<PenVariant> {
    tool.pen.as_ref()?;
    let colour = op.tool.pen_colour.clone().filter(|c| {
        tool.pen
            .as_ref()
            .is_none_or(|p| !p.colour.hex.eq_ignore_ascii_case(c))
    });
    let own = tool.geometry.tip_width_mm;
    let tip = params
        .get("tip_width")
        .copied()
        .filter(|w| own.is_none_or(|o| (o - w).abs() > 1e-9));
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a pen width of a few millimetres, rounded to whole micrometres"
    )]
    let tip_width_um = tip.map(|w| (w * 1000.0).round().max(0.0) as u32);
    (colour.is_some() || tip_width_um.is_some()).then_some(PenVariant {
        colour,
        tip_width_um,
    })
}

/// The tool's name with its colour and tip width when they are not its own.
fn variant_label(name: &str, pen: Option<&PenVariant>) -> String {
    let Some(pen) = pen else {
        return name.to_owned();
    };
    let mut out = name.to_owned();
    if let Some(colour) = &pen.colour {
        out.push_str(", ");
        out.push_str(colour);
    }
    if let Some(um) = pen.tip_width_um {
        out.push_str(", ");
        out.push_str(&(f64::from(um) / 1000.0).to_string());
        out.push_str(" mm");
    }
    out
}

fn pen_lightness(op: &Operation, tool: &Tool) -> Option<f64> {
    let rgb = op
        .tool
        .pen_colour
        .as_ref()
        .and_then(|hex| {
            Colour {
                hex: hex.clone(),
                name: String::new(),
            }
            .rgb()
        })
        .or_else(|| tool.pen.as_ref().and_then(|p| p.colour.rgb()))?;
    Some(lightness(rgb))
}

fn input_hash(input: &CompileInput<'_>, mats: &[&Arc<Mat>]) -> ps_core::ContentHash {
    let mut h = StableHasher::new();
    h.write_str("ps-compiler/1");
    h.write_bytes(&serde_json::to_vec(&mats).unwrap_or_default());
    h.write_bytes(&serde_json::to_vec(&*input.doc.settings).unwrap_or_default());
    h.write_bytes(&serde_json::to_vec(input.profile).unwrap_or_default());
    h.write_bytes(&serde_json::to_vec(input.calibration).unwrap_or_default());
    // The versions the library actually resolved, which can differ from what the document asks.
    for m in mats {
        if let Some(r) = &m.material
            && let Ok(found) = input.libs.resolve_material(r)
        {
            h.write_str(&format!("{}@{}", found.item.id, found.item.version));
        }
        for o in m.objects.values() {
            for p in o.paths.iter() {
                for op in &p.ops {
                    if let Ok(found) = input.libs.resolve_tool(&op.tool.tool) {
                        h.write_str(&format!("{}@{}", found.item.id, found.item.version));
                    }
                }
            }
        }
    }
    h.finish()
}

fn loc(mat: MatId, w: &Work<'_>) -> Location {
    Location::object(mat, w.object.id).with_path(w.path.key.clone())
}

/// Compiles the selected mats into a job.
///
/// Problems in the design (a missing tool, a move outside the work area) come back as
/// diagnostics; errors among them block export and, for library or support problems found in
/// validation, stop compilation before any geometry work.
///
/// # Errors
///
/// [`CompileError::UnknownMat`] for a selection that is not in the document;
/// [`CompileError::Coordinate`] when a coordinate does not fit the machine grid.
pub fn compile(input: &CompileInput<'_>) -> Result<Job, CompileError> {
    // A zero-capacity cache keeps nothing, but preparation still runs in parallel.
    compile_cached(input, &mut CompileCache::new(0))
}

/// Compiles like [`compile`], memoising geometry preparation in `cache` so that recompiling after
/// a small edit only re-prepares what changed. The result is identical to an uncached
/// compile.
///
/// # Errors
///
/// As for [`compile`].
pub fn compile_cached(
    input: &CompileInput<'_>,
    cache: &mut CompileCache,
) -> Result<Job, CompileError> {
    let doc = input.doc;
    let selected: Vec<&Arc<Mat>> = match input.mats {
        MatSelection::All => doc.mats.values().collect(),
        MatSelection::One(id) => vec![doc.mat(id).ok_or(CompileError::UnknownMat(id))?],
    };
    let selected_ids: BTreeSet<MatId> = selected.iter().map(|m| m.id).collect();
    let hash = input_hash(input, &selected);

    // Stage 1: validate. Blocking problems stop here.
    let mut diagnostics = Diagnostics::new();
    for d in &validate(doc, input.libs, input.profile) {
        if d.location
            .as_ref()
            .and_then(|l| l.mat)
            .is_none_or(|m| selected_ids.contains(&m))
        {
            diagnostics.push(d.clone());
        }
    }
    if diagnostics.has_errors() {
        diagnostics.sort();
        return Ok(Job {
            mats: Vec::new(),
            diagnostics,
            input_hash: hash,
        });
    }

    let mut mats = Vec::new();
    for mat in selected {
        if let Some(job) = compile_mat(input, mat, &mut diagnostics, cache)? {
            mats.push(job);
        }
    }
    diagnostics.sort();
    Ok(Job {
        mats,
        diagnostics,
        input_hash: hash,
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "the stages read best as one sequence over shared state"
)]
fn compile_mat(
    input: &CompileInput<'_>,
    mat: &Mat,
    diags: &mut Diagnostics,
    cache: &mut CompileCache,
) -> Result<Option<MatJob>, CompileError> {
    let profile = input.profile;
    let cal = input.calibration;

    // Stage 2: collect work and resolve parameters.
    let mut rows: Vec<ps_core::OpRowId> = mat.op_order.iter().copied().collect();
    let mut blocks: Vec<Block> = Vec::new();
    let mut keys: BTreeMap<(usize, OpKind, String), usize> = BTreeMap::new();
    let mut work: Vec<Work<'_>> = Vec::new();
    for object in mat.objects.values().filter(|o| !o.hidden) {
        for path in object.paths.iter() {
            for op in path.ops.iter().filter(|o| o.enabled) {
                let Ok(found) = input.libs.resolve_tool(&op.tool.tool) else {
                    continue;
                };
                let tool = found.item;
                let Ok(resolved) =
                    ps_model::resolve(op, mat, &input.doc.settings, input.libs, profile)
                else {
                    continue;
                };
                for w in &resolved.warnings {
                    diags.push(w.clone().at(Location::object(mat.id, object.id)));
                }
                let params: BTreeMap<String, f64> = resolved
                    .params
                    .iter()
                    .map(|(k, v)| (k.clone(), v.value))
                    .collect();
                let row = rows.iter().position(|r| *r == op.row).unwrap_or_else(|| {
                    rows.push(op.row);
                    rows.len() - 1
                });
                let key = (row, op.kind, tool.lib_ref().to_string());
                let block = *keys.entry(key).or_insert_with(|| {
                    blocks.push(Block {
                        stage: stage_of(op.kind),
                        row: u64::try_from(row).unwrap_or(u64::MAX),
                        tool: tool.id.clone(),
                        lightness: pen_lightness(op, &tool),
                    });
                    blocks.len() - 1
                });
                work.push(Work {
                    object,
                    path,
                    op,
                    tool,
                    params,
                    block,
                });
            }
        }
    }
    if work.is_empty() {
        return Ok(None);
    }

    // Holders, geometry and ordering depend on each other (the holder's calibrated offset moves
    // the geometry, geometry decides the safe order, the order decides the holders), so iterate.
    let mode = input.doc.settings.ordering;
    let usable = |tool: &Tool| -> Vec<HolderId> {
        profile
            .holders_for(tool)
            .map(|h| HolderId::new(h.id.clone()))
            .collect()
    };
    let holder_labels: Vec<(HolderId, String)> = profile
        .holders
        .iter()
        .map(|h| (HolderId::new(h.id.clone()), h.label.clone()))
        .collect();
    let tool_use = |block: usize| -> ToolUse {
        let w = work
            .iter()
            .find(|w| w.block == block)
            .expect("every block has at least one work item");
        let pen = pen_variant(w.op, &w.tool, &w.params);
        ToolUse {
            tool: w.tool.lib_ref(),
            kind: family_word(w.tool.family).to_owned(),
            label: variant_label(&w.tool.name, pen.as_ref()),
            holders: usable(&w.tool),
            pen,
        }
    };
    let plan_for = |order: &[usize]| -> ToolPlan {
        let seq: Vec<ToolUse> = order.iter().map(|&b| tool_use(b)).collect();
        plan_tools(&seq, &holder_labels)
    };
    let holder_map = |plan: &ToolPlan| -> BTreeMap<String, String> {
        plan.table
            .iter()
            .map(|t| (t.tool.id().to_owned(), t.holder.as_str().to_owned()))
            .collect()
    };

    // User run-after locks become edges; contradictory locks are dropped, not fatal.
    let mut locks = lock_edges(&mat.run_after, &rows, &blocks);
    let first = if let Ok(first) = schedule(&blocks, &locks, mode) {
        first
    } else {
        locks.clear();
        let Ok(first) = schedule(&blocks, &[], mode) else {
            return Ok(None);
        };
        first
    };
    let mut plan = plan_for(&first.order);
    let mut holders = holder_map(&plan);
    let mut order = first.order.clone();
    let mut reordered = Vec::new();
    let mut unresolved = Vec::new();
    let mut prepared: Vec<Prepared> = Vec::new();
    for _ in 0..HOLDER_ROUNDS {
        let transforms: Vec<Affine> = work
            .iter()
            .map(|w| {
                let off = holders
                    .get(&w.tool.id)
                    .map(|h| cal.holder_offset(h))
                    .unwrap_or_default();
                machine_transform(mat, profile, (off.x_mm, off.y_mm)) * w.object.transform
            })
            .collect();
        let inputs: Vec<PrepareInput<'_>> = work
            .iter()
            .zip(&transforms)
            .map(|(w, &to_machine)| PrepareInput {
                path: w.path,
                to_machine,
                kind: w.op.kind,
                drive: w.tool.drive,
                tip_width_mm: tip_width(&w.tool, &w.params),
                fill: w.op.fill,
                blade_offset_mm: blade_offset(&w.tool, cal),
                wheel_radius_mm: w.tool.geometry.wheel_radius_mm,
                params: &w.params,
                turn_step_deg: profile.fill_turn_step_deg,
            })
            .collect();
        prepared = cache.prepare_all(&inputs);
        merge_fill_groups(&work, &inputs, &mut prepared);

        // Stage 8.7: release safety edges from the prepared geometry.
        let mut edges = safety_edges(&blocks, &work, &prepared);
        edges.extend(locks.iter().copied());
        let Ok(s) = schedule(&blocks, &edges, mode) else {
            break;
        };
        order = s.order;
        reordered = s.reordered;
        unresolved = s.unresolved;
        plan = plan_for(&order);
        let next = holder_map(&plan);
        if next == holders {
            break;
        }
        holders = next;
    }

    for (before, after) in &reordered {
        diags.push(
            Diagnostic::warning(L10nKey::new("diag-release-order"))
                .with_arg("before", before.to_string())
                .with_arg("after", after.to_string())
                .at(Location {
                    mat: Some(mat.id),
                    ..Location::default()
                }),
        );
    }
    for (before, after) in &unresolved {
        diags.push(
            Diagnostic::warning(L10nKey::new("diag-release-order-locked"))
                .with_arg("before", before.to_string())
                .with_arg("after", after.to_string())
                .at(Location {
                    mat: Some(mat.id),
                    ..Location::default()
                }),
        );
    }
    for id in &plan.unplaced {
        diags.push(
            Diagnostic::error(L10nKey::new("diag-unsupported-tool"))
                .with_arg("tool", id.clone())
                .with_arg("machine", profile.id.clone())
                .at(Location {
                    mat: Some(mat.id),
                    ..Location::default()
                }),
        );
    }
    for (w, p) in work.iter().zip(&prepared) {
        for n in &p.notes {
            diags.push(
                match n {
                    Note::WavyCutAsStroke => {
                        Diagnostic::info(L10nKey::new("diag-wavy-cut-as-stroke"))
                    }
                    Note::MinRadius { radius_mm } => {
                        Diagnostic::warning(L10nKey::new("diag-min-radius"))
                            .with_arg("radius_mm", format!("{radius_mm:.3}"))
                            .with_arg("tool", w.tool.id.clone())
                    }
                }
                .at(loc(mat.id, w)),
            );
        }
    }

    // Emit steps in block order.
    let limits = machine_limits(profile).map_err(grid(mat.id))?;
    let mut em = Emitter {
        mat: mat.id,
        steps: Vec::new(),
        down: false,
        up_dwell_ms: 0,
        at: PtUm::new(0, 0),
        angle: 0,
        moves: BTreeMap::new(),
        reported: BTreeSet::new(),
    };
    if let Some(msg) = &plan.initial_pause {
        em.steps.push(Step::Pause(msg.clone()));
    }
    let mut prev_stage: Option<Stage> = None;
    for (gi, &b) in order.iter().enumerate() {
        let stage = blocks[b].stage;
        if stage == Stage::Overlay && prev_stage != Some(Stage::Overlay) {
            em.up();
            em.steps
                .push(Step::Pause(PauseMsg::new("Place the overlay sheet")));
        } else if prev_stage == Some(Stage::Overlay) && stage != Stage::Overlay {
            em.up();
            em.steps
                .push(Step::Pause(PauseMsg::new("Remove the overlay sheet")));
        }
        prev_stage = Some(stage);

        let Some(first_item) = work.iter().position(|w| w.block == b) else {
            continue;
        };
        let tool = &work[first_item].tool;
        if let Some(sw) = plan.switches.iter().find(|s| s.group == gi) {
            em.up();
            em.steps.push(Step::LoadTool {
                slot: sw.slot,
                holder: sw.holder.clone(),
                pause: sw.pause.clone(),
            });
        }
        if input.doc.settings.pen_check && tool.family == ToolFamily::Pen {
            // A short mark in the corner shows the pen is writing before it starts on the work.
            let mark = Stroke {
                path: Path::polyline(&[Point::new(1.0, 1.0), Point::new(3.0, 1.0)], false),
                tip_ring: None,
                reversible: false,
                tangential: false,
                force_scale: 1.0,
                fixed_start: false,
                group: 0,
            };
            em.stroke(&work[first_item], &mark, profile, cal, &limits, diags)?;
        }
        let heated = tool.drive == DriveType::Heated;
        if heated {
            em.up();
            let celsius = rounded_i32(
                work[first_item]
                    .params
                    .get("temperature")
                    .copied()
                    .unwrap_or(0.0),
            );
            if celsius > 0 {
                em.steps.push(Step::Temperature {
                    celsius,
                    wait: true,
                });
            }
            let warm = rounded_u32(
                work[first_item]
                    .params
                    .get("warmup_ms")
                    .copied()
                    .unwrap_or(0.0),
            );
            if warm > 0 {
                em.steps.push(Step::Dwell { ms: warm });
            }
        }

        let ordered = order_strokes(b, &work, &prepared, em.at, mat.id)?;
        for (wi, stroke) in ordered {
            em.stroke(&work[wi], &stroke, profile, cal, &limits, diags)?;
        }
        em.up();
        if heated {
            em.steps.push(Step::Temperature {
                celsius: 0,
                wait: false,
            });
        }
    }
    em.up();

    let mut est = estimate(&EstimateInput {
        steps: &em.steps,
        limits: MotionLimits {
            max_speed_mm_s: profile.max_speed_mm_s,
            max_travel_speed_mm_s: profile.travel_speed_mm_s(),
            accel_mm_s2: profile.max_accel_mm_s2,
            junction_deviation_mm: profile.junction_deviation_mm,
        },
        tool_up_ms: profile.tool_up_ms,
        tool_down_ms: profile.tool_down_ms,
        heating_rate_c_s: profile.heating_rate_c_s,
        ambient_c: AMBIENT_C,
        time_scale: cal.time_scale,
        pause_allowance_ms: PAUSE_ALLOWANCE_MS,
        accel_by_speed: &profile.accel_by_speed,
        vector_ms_by_speed: &profile.vector_ms_by_speed,
    });
    est.utilisation_ppm = coverage(mat, &work, &prepared)?;

    Ok(Some(MatJob {
        mat: mat.id,
        tool_table: plan.table,
        steps: em.steps,
        estimate: est,
    }))
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "temperatures are small and finite"
)]
fn rounded_i32(v: f64) -> i32 {
    libm::round(v) as i32
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "times are small, finite and clamped at zero"
)]
fn rounded_u32(v: f64) -> u32 {
    libm::round(v.max(0.0)) as u32
}

fn flat_points(path: &Path) -> Vec<PtUm> {
    flatten(path, 0.01)
        .iter()
        .filter_map(|p| PtUm::from_mm(p.x, p.y).ok())
        .collect()
}

/// Fills of the closed paths of one object in one block (row, kind and tool) are one region, so a
/// ring inside a ring is a hole. The group's strokes go to its first path; the others have none.
/// Separate rows stay separate, so an object can be filled more than once.
fn merge_fill_groups(work: &[Work<'_>], inputs: &[PrepareInput<'_>], prepared: &mut [Prepared]) {
    let mut groups: BTreeMap<(ps_core::ObjectId, usize), Vec<usize>> = BTreeMap::new();
    for (i, w) in work.iter().enumerate() {
        if matches!(w.op.kind, OpKind::Fill | OpKind::FoilFill) && w.path.geom.closed {
            groups.entry((w.object.id, w.block)).or_default().push(i);
        }
    }
    for members in groups.values().filter(|m| m.len() > 1) {
        let group: Vec<PrepareInput<'_>> = members.iter().map(|&i| inputs[i]).collect();
        prepared[members[0]] = crate::prepare::prepare_compound(&group);
        for &i in &members[1..] {
            prepared[i] = Prepared::default();
        }
    }
}

/// Edges for the user's run-after locks: every block of the first row runs before every block of
/// the later row.
fn lock_edges(
    locks: &[(ps_core::OpRowId, ps_core::OpRowId)],
    rows: &[ps_core::OpRowId],
    blocks: &[Block],
) -> Vec<crate::schedule::Edge> {
    let position = |id: &ps_core::OpRowId| {
        rows.iter()
            .position(|r| r == id)
            .and_then(|i| u64::try_from(i).ok())
    };
    let mut edges = Vec::new();
    for (first, later) in locks {
        let (Some(a), Some(b)) = (position(first), position(later)) else {
            continue;
        };
        for (i, x) in blocks.iter().enumerate().filter(|(_, x)| x.row == a) {
            for (j, y) in blocks.iter().enumerate().filter(|(_, y)| y.row == b) {
                let _ = (x, y);
                edges.push(crate::schedule::Edge::lock(i, j));
            }
        }
    }
    edges
}

fn safety_edges(
    blocks: &[Block],
    work: &[Work<'_>],
    prepared: &[Prepared],
) -> Vec<crate::schedule::Edge> {
    let mut cutters = Vec::new();
    let mut drawn = Vec::new();
    for (b, block) in blocks.iter().enumerate() {
        let mut rings = Vec::new();
        let mut paths = Vec::new();
        for (w, p) in work.iter().zip(prepared).filter(|(w, _)| w.block == b) {
            let _ = w;
            for s in &p.strokes {
                if let Some(r) = &s.tip_ring {
                    rings.push(r.clone());
                }
                paths.push(flat_points(&s.path));
            }
        }
        if block.stage == Stage::ThroughCut && !rings.is_empty() {
            cutters.push(Cutter { block: b, rings });
        }
        drawn.push(Drawn { block: b, paths });
    }
    let mut edges = release_edges(&cutters, &drawn);
    edges.retain(|e| !only_retraces(e, blocks, work, prepared));
    edges
}

/// True when the only reason for a "must run before" edge is that both blocks through-cut the same
/// outline. Cutting a shape twice is a request, not a release-order problem: neither cut can run
/// "before the shape is freed" by the other.
fn only_retraces(
    edge: &crate::schedule::Edge,
    blocks: &[Block],
    work: &[Work<'_>],
    prepared: &[Prepared],
) -> bool {
    if blocks[edge.before].stage != Stage::ThroughCut
        || blocks[edge.after].stage != Stage::ThroughCut
    {
        return false;
    }
    let of = |block: usize| {
        work.iter()
            .zip(prepared)
            .filter(move |(w, _)| w.block == block)
    };
    let same = |a: &Work<'_>, b: &Work<'_>| a.object.id == b.object.id && a.path.key == b.path.key;
    // Every outline of the later block must be clear of what the earlier one cuts elsewhere.
    of(edge.after).all(|(cut, p)| {
        let cutter = Cutter {
            block: edge.after,
            rings: p
                .strokes
                .iter()
                .filter_map(|s| s.tip_ring.clone())
                .collect(),
        };
        let elsewhere = Drawn {
            block: edge.before,
            paths: of(edge.before)
                .filter(|(w, _)| !same(w, cut))
                .flat_map(|(_, p)| p.strokes.iter().map(|s| flat_points(&s.path)))
                .collect(),
        };
        release_edges(&[cutter], &[elsewhere]).is_empty()
    })
}

fn coverage(mat: &Mat, work: &[Work<'_>], prepared: &[Prepared]) -> Result<u32, CompileError> {
    let rings: Vec<Vec<PtUm>> = work
        .iter()
        .zip(prepared)
        .filter(|(w, _)| matches!(w.op.kind, OpKind::Cut | OpKind::KissCut))
        .flat_map(|(_, p)| p.strokes.iter().filter_map(|s| s.tip_ring.clone()))
        .collect();
    let forest = containment(&rings);
    let covered: u128 = rings
        .iter()
        .enumerate()
        .filter(|(i, _)| forest.parent[*i].is_none())
        .map(|(_, r)| u128::try_from(ps_geometry::ring_area2(r).abs()).unwrap_or(0) / 2)
        .sum();
    let w = u128::try_from(um_from_mm(mat.size.width.get()).map_err(grid(mat.id))?).unwrap_or(0);
    let h = u128::try_from(um_from_mm(mat.size.height.get()).map_err(grid(mat.id))?).unwrap_or(0);
    Ok(utilisation_ppm(covered, w * h))
}

/// How many pair comparisons the tour improvement may spend on one group of strokes.
const IMPROVE_WORK: usize = 20_000_000;
/// The most sweeps of the tour improvement.
const MAX_SWEEPS: usize = 12;

/// Orders one block's strokes: open cut paths first, then closed contours inside-out; everything
/// else nearest-neighbour then 2-opt/Or-opt.
fn order_strokes(
    block: usize,
    work: &[Work<'_>],
    prepared: &[Prepared],
    head: PtUm,
    mat: MatId,
) -> Result<Vec<(usize, Stroke)>, CompileError> {
    let mut all: Vec<(usize, Stroke)> = Vec::new();
    for (wi, (w, p)) in work.iter().zip(prepared).enumerate() {
        if w.block == block {
            all.extend(p.strokes.iter().map(|s| (wi, s.clone())));
        }
    }
    let is_cut = all
        .first()
        .is_some_and(|(wi, _)| matches!(work[*wi].op.kind, OpKind::Cut | OpKind::KissCut));
    let (rings, free): (Vec<_>, Vec<_>) = all
        .into_iter()
        .partition(|(_, s)| is_cut && s.tip_ring.is_some());

    let mut out: Vec<(usize, Stroke)> = Vec::new();
    let items: Vec<Item> = free
        .iter()
        .map(|(_, s)| {
            Ok(Item {
                start: to_um(mat, s.path.start)?,
                end: to_um(mat, s.path.end())?,
                reversible: s.reversible,
            })
        })
        .collect::<Result<_, CompileError>>()?;
    // Pieces that belong together (the letters of a fill) are drawn one after another, nearest
    // piece first; inside a piece the strokes get the usual tour.
    let mut keys: Vec<(usize, u32)> = free.iter().map(|(wi, s)| (*wi, s.group)).collect();
    keys.sort_unstable();
    keys.dedup();
    let mut members: Vec<Vec<usize>> = keys
        .iter()
        .map(|k| {
            free.iter()
                .enumerate()
                .filter(|(_, (wi, s))| (*wi, s.group) == *k)
                .map(|(i, _)| i)
                .collect()
        })
        .collect();
    let mut at = Point::new(
        f64::from(i32::try_from(head.x).unwrap_or(0)) / 1000.0,
        f64::from(i32::try_from(head.y).unwrap_or(0)) / 1000.0,
    );
    while !members.is_empty() {
        let nearest = members
            .iter()
            .enumerate()
            .map(|(g, ids)| {
                let d = ids
                    .iter()
                    .flat_map(|&i| [free[i].1.path.start, free[i].1.path.end()])
                    .map(|p| libm::hypot(p.x - at.x, p.y - at.y))
                    .fold(f64::INFINITY, f64::min);
                (g, d)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(0, |(g, _)| g);
        let ids = members.remove(nearest);
        let sub: Vec<Item> = ids.iter().map(|&i| items[i]).collect();
        let from = to_um(mat, at)?;
        let mut tour = nearest_neighbour(&sub, from);
        // A sweep compares every pair of paths, so the number of sweeps is limited by how much
        // work that is; a tour is usually settled within a few.
        let work = sub.len().saturating_mul(sub.len()).saturating_mul(3).max(1);
        improve_sweeps(
            &sub,
            &mut tour,
            from,
            (IMPROVE_WORK / work).clamp(1, MAX_SWEEPS),
        );
        for v in tour {
            let (wi, s) = &free[ids[v.index]];
            let mut s = s.clone();
            if v.reversed {
                s.path = s.path.reversed();
            }
            at = s.path.end();
            out.push((*wi, s));
        }
    }

    let ring_geom: Vec<Vec<PtUm>> = rings
        .iter()
        .map(|(_, s)| s.tip_ring.clone().unwrap_or_default())
        .collect();
    for i in emission_order(&ring_geom) {
        out.push(rings[i].clone());
    }
    Ok(out)
}

struct Emitter {
    mat: MatId,
    steps: Vec<Step>,
    down: bool,
    /// The preset's wait after lifting the tool, written right after the next `ToolUp`.
    up_dwell_ms: u32,
    at: PtUm,
    angle: i64,
    moves: BTreeMap<ps_core::OpId, u32>,
    reported: BTreeSet<(ps_core::ObjectId, String)>,
}

pub(crate) enum Emit {
    Line(PtUm),
    Arc { to: PtUm, center: PtUm, ccw: bool },
}

impl Emitter {
    fn up(&mut self) {
        if self.down {
            self.steps.push(Step::ToolUp);
            self.down = false;
            if self.up_dwell_ms > 0 {
                self.steps.push(Step::Dwell {
                    ms: self.up_dwell_ms,
                });
            }
        }
    }

    fn segments(&self, path: &Path, arcs: bool, chord_tol: f64) -> Result<Vec<Emit>, CompileError> {
        let mut out = Vec::new();
        if arcs {
            let p = path_to_arcs(path, ARC_TOL);
            for seg in &p.segs {
                match *seg {
                    Seg::Line { to } | Seg::Cubic { to, .. } => {
                        out.push(Emit::Line(to_um(self.mat, to)?));
                    }
                    Seg::Arc { to, center, ccw } => out.push(Emit::Arc {
                        to: to_um(self.mat, to)?,
                        center: to_um(self.mat, center)?,
                        ccw,
                    }),
                }
            }
            if p.closed && p.end() != p.start {
                out.push(Emit::Line(to_um(self.mat, p.start)?));
            }
        } else {
            let mut pts = flatten(path, chord_tol.max(MIN_CHORD_TOL));
            if path.closed
                && let Some(&s) = pts.first()
            {
                pts.push(s);
            }
            for p in pts.iter().skip(1) {
                out.push(Emit::Line(to_um(self.mat, *p)?));
            }
        }
        // The machine spends a fixed time on every vector, so a curve flattened to the chord
        // tolerance is drawn with the fewest straight moves that stay within it.
        let tolerance_um = chord_tol.max(MIN_CHORD_TOL) * 1_000.0;
        Ok(simplify_emits(
            to_um(self.mat, path.start)?,
            out,
            tolerance_um,
        ))
    }

    fn stroke(
        &mut self,
        w: &Work<'_>,
        stroke: &Stroke,
        profile: &MachineProfile,
        cal: &Calibration,
        limits: &MachineLimits,
        diags: &mut Diagnostics,
    ) -> Result<(), CompileError> {
        let segs = self.segments(&stroke.path, profile.arcs, profile.gcode.chord_tolerance_mm)?;
        if segs.is_empty() {
            return Ok(());
        }
        let start = to_um(self.mat, stroke.path.start)?;

        let speed = {
            let s = w
                .params
                .get("speed")
                .copied()
                .unwrap_or(profile.max_speed_mm_s);
            let s = w.params.get("min_speed").map_or(s, |m| s.max(*m));
            s.min(profile.max_speed_mm_s)
        };
        let feed = rounded_u32(speed * 60.0).max(1);
        let requested =
            cal.scaled_force(w.params.get("force").copied().unwrap_or(0.0)) * stroke.force_scale;
        let (force, clamped) = clamp_force(rounded_u32(requested), limits);
        if clamped
            && self
                .reported
                .insert((w.object.id, format!("force:{}", w.op.id)))
        {
            diags.push(
                Diagnostic::warning(L10nKey::new("diag-force-clamped"))
                    .with_arg("requested", rounded_u32(requested).to_string())
                    .with_arg("used", force.to_string())
                    .at(loc(self.mat, w)),
            );
        }

        // A violation anywhere on the stroke is reported once per path.
        let mut prev = start;
        let mut bad = check_point(start, limits);
        for s in &segs {
            let to = match s {
                Emit::Line(to) | Emit::Arc { to, .. } => *to,
            };
            bad = bad.or_else(|| check_segment(prev, to, limits));
            prev = to;
        }
        if let Some(v) = bad
            && self.reported.insert((w.object.id, w.path.key.to_string()))
        {
            let key = match v {
                Violation::OutsideWorkArea => "diag-move-out-of-bounds",
                Violation::InKeepOut => "diag-move-in-keep-out",
            };
            diags.push(Diagnostic::error(L10nKey::new(key)).at(loc(self.mat, w).with_point(start)));
        }

        self.up();
        self.steps.push(Step::Travel { to: start });
        self.steps.push(Step::ToolDown { force_gf: force });
        self.down = true;
        let down_dwell = rounded_u32(w.params.get("down_dwell_ms").copied().unwrap_or(0.0));
        if down_dwell > 0 {
            self.steps.push(Step::Dwell { ms: down_dwell });
        }
        self.up_dwell_ms = rounded_u32(w.params.get("up_dwell_ms").copied().unwrap_or(0.0));
        self.at = start;
        for s in segs {
            let index = {
                let n = self.moves.entry(w.op.id).or_insert(0);
                let i = *n;
                *n += 1;
                i
            };
            let src = SrcRef {
                mat: self.mat,
                object: w.object.id,
                path: w.path.key.clone(),
                op: w.op.id,
                move_index: index,
            };
            match s {
                Emit::Line(to) => {
                    let a = stroke.tangential.then(|| {
                        let a = unwrapped_angles(&[self.at, to], self.angle)[0];
                        self.angle = a;
                        a
                    });
                    self.steps.push(Step::Line {
                        to,
                        feed_mm_min: feed,
                        a_mdeg: a,
                        src,
                    });
                    self.at = to;
                }
                Emit::Arc { to, center, ccw } => {
                    self.steps.push(Step::Arc {
                        to,
                        center,
                        ccw,
                        feed_mm_min: feed,
                        src,
                    });
                    self.at = to;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
