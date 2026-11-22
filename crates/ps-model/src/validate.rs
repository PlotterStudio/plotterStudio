//! The validation pass.
//!
//! A cheap check that runs on every document change, separate from the full compiler. It finds
//! layout problems (outside the usable area, in a keep-out or used scrap region, a feature
//! smaller than the tool's minimum, a path with no operation, a tool the machine cannot hold) and
//! library references that do not resolve. Each diagnostic carries a location, so the canvas can
//! draw a badge. Release-order warnings need ordering and come from the compiler instead.

use ps_core::{Diagnostic, Diagnostics, L10nKey, Location, MatId, ObjectId};
use ps_geometry::Rect;
use ps_library::{LibrarySnapshot, MachineProfile, ResolveWarning};

use crate::{Document, Mat, PlacedObject, ShapePath};

/// Tolerance for "touching the edge", in mm.
const EDGE_EPS: f64 = 1e-6;
/// How close a mat size must be to a supported one, in mm.
const MAT_SIZE_TOLERANCE: f64 = 1.0;

fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

fn object_loc(mat: MatId, object: ObjectId) -> Location {
    Location::object(mat, object)
}

fn mat_loc(mat: MatId) -> Location {
    Location {
        mat: Some(mat),
        ..Location::default()
    }
}

fn check_mat_size(mat: &Mat, profile: &MachineProfile, out: &mut Diagnostics) {
    let (w, h) = (mat.size.width.get(), mat.size.height.get());
    let fits =
        |a: f64, b: f64| (w - a).abs() <= MAT_SIZE_TOLERANCE && (h - b).abs() <= MAT_SIZE_TOLERANCE;
    let supported = profile
        .mat_sizes
        .iter()
        .any(|s| fits(s.width_mm, s.height_mm) || fits(s.height_mm, s.width_mm));
    if !supported {
        out.push(
            Diagnostic::warning(L10nKey::new("diag-mat-size-unsupported"))
                .with_arg("width_mm", format!("{w}"))
                .with_arg("height_mm", format!("{h}"))
                .at(mat_loc(mat.id)),
        );
    }
}

fn check_material(mat: &Mat, libs: &LibrarySnapshot, out: &mut Diagnostics) {
    let Some(r) = &mat.material else {
        return;
    };
    match libs.resolve_material(r) {
        Err(_) => out.push(missing(r.id(), mat_loc(mat.id))),
        Ok(found) => {
            if let Some(w) = found.warning {
                out.push(substituted(&w, mat_loc(mat.id)));
            }
        }
    }
}

fn missing(id: &str, loc: Location) -> Diagnostic {
    Diagnostic::error(L10nKey::new("diag-missing-library-entry"))
        .with_arg("id", id)
        .at(loc)
}

fn substituted(w: &ResolveWarning, loc: Location) -> Diagnostic {
    let ResolveWarning::VersionSubstituted { wanted, used } = w;
    Diagnostic::warning(L10nKey::new("diag-version-substituted"))
        .with_arg("wanted", wanted.to_string())
        .with_arg("used", used.to_string())
        .at(loc)
}

fn check_placement(mat: &Mat, obj: &PlacedObject, profile: &MachineProfile, out: &mut Diagnostics) {
    let Some(b) = obj.bounds() else {
        return;
    };
    let (w, h) = (mat.size.width.get(), mat.size.height.get());
    if b.x0 < -EDGE_EPS || b.y0 < -EDGE_EPS || b.x1 > w + EDGE_EPS || b.y1 > h + EDGE_EPS {
        out.push(
            Diagnostic::warning(L10nKey::new("diag-outside-usable-area"))
                .at(object_loc(mat.id, obj.id)),
        );
    }
    for k in &profile.keep_outs {
        let rect = Rect::new(k.x_mm, k.y_mm, k.x_mm + k.width_mm, k.y_mm + k.height_mm);
        if rects_overlap(&b, &rect) {
            out.push(
                Diagnostic::warning(L10nKey::new("diag-in-keep-out"))
                    .at(object_loc(mat.id, obj.id)),
            );
            break;
        }
    }
    for poly in &mat.scrap_regions {
        let bounds = poly.iter().fold(None::<Rect>, |acc, p| {
            Some(acc.map_or(Rect::from_points(*p, *p), |r| r.union_pt(*p)))
        });
        if bounds.is_some_and(|r| rects_overlap(&b, &r)) {
            out.push(
                Diagnostic::warning(L10nKey::new("diag-in-scrap-region"))
                    .at(object_loc(mat.id, obj.id)),
            );
            break;
        }
    }
}

fn check_path(
    mat: &Mat,
    obj: &PlacedObject,
    path: &ShapePath,
    libs: &LibrarySnapshot,
    profile: &MachineProfile,
    out: &mut Diagnostics,
) {
    let loc = object_loc(mat.id, obj.id).with_path(path.key.clone());
    let enabled: Vec<_> = path.ops.iter().filter(|o| o.enabled).collect();
    if enabled.is_empty() {
        out.push(Diagnostic::warning(L10nKey::new("diag-missing-operation")).at(loc));
        return;
    }
    let placed = path.geom.transformed(obj.transform);
    let b = placed.bounds();
    for op in enabled {
        let Ok(found) = libs.resolve_tool(&op.tool.tool) else {
            out.push(missing(op.tool.tool.id(), loc.clone()));
            continue;
        };
        if let Some(w) = &found.warning {
            out.push(substituted(w, loc.clone()));
        }
        let tool = &found.item;
        if profile.holders_for(tool).next().is_none() {
            out.push(
                Diagnostic::error(L10nKey::new("diag-unsupported-tool"))
                    .with_arg("tool", tool.id.clone())
                    .with_arg("machine", profile.id.clone())
                    .at(loc.clone()),
            );
        }
        if let Some(min) = tool.geometry.min_feature_mm {
            let size = if placed.closed {
                b.width().min(b.height())
            } else {
                placed.length()
            };
            if size < min {
                out.push(
                    Diagnostic::warning(L10nKey::new("diag-feature-too-small"))
                        .with_arg("tool", tool.id.clone())
                        .with_arg("min_mm", format!("{min}"))
                        .at(loc.clone()),
                );
            }
        }
    }
}

/// Validates a document against the library and the selected machine.
///
/// The result is sorted most severe first, so it is identical for identical input.
#[must_use]
pub fn validate(doc: &Document, libs: &LibrarySnapshot, profile: &MachineProfile) -> Diagnostics {
    let mut out = Diagnostics::new();
    for mat in doc.mats.values() {
        check_mat_size(mat, profile, &mut out);
        check_material(mat, libs, &mut out);
        for obj in mat.objects.values().filter(|o| !o.hidden) {
            check_placement(mat, obj, profile, &mut out);
            for path in obj.paths.iter() {
                check_path(mat, obj, path, libs, profile, &mut out);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests;
