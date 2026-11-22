//! Ground truth for importing drawings into the open project: the same importers,
//! repair, mapping rules and placement as `pstudio export`, but added to the current mat as one
//! undo step instead of making a new document. The original file is kept in the project so a
//! re-import can match its paths again, and the rules used are saved with the project.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ps_library::Settings;
use ps_model::OpKind;

use super::*;
use crate::state::{AppState, Effect};

const RED_SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200mm" height="200mm" viewBox="0 0 200 200">
  <rect id="r1" x="10" y="10" width="40" height="30" fill="none" stroke="#FF0000" stroke-width="0.5"/>
  <rect id="r2" x="60" y="10" width="40" height="30" fill="none" stroke="#0000FF" stroke-width="0.5"/>
</svg>"##;

const DXF: &str = "0\nSECTION\n2\nENTITIES\n0\nLINE\n5\n1A\n8\n0\n62\n1\n10\n0.0\n20\n0.0\n11\n50.0\n21\n0.0\n0\nLINE\n5\n1B\n8\n0\n62\n1\n10\n50.0\n20\n0.0\n11\n50.0\n21\n30.0\n0\nENDSEC\n0\nEOF\n";

fn state() -> AppState {
    AppState::new(Settings::default())
}

fn write(dir: &tempfile::TempDir, name: &str, text: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn an_svg_is_imported_onto_the_current_mat_with_operations_from_the_rules() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "card.svg", RED_SQUARE);
    let mut s = state();
    let report = import_file(&mut s, &path, 0).unwrap();
    // Both rectangles are in one layer, so they arrive as one object with two paths.
    assert_eq!(report.objects, 1);
    let mat = s.current_mat();
    assert_eq!(mat.objects.len(), 1);
    assert_eq!(mat.objects.values().next().unwrap().paths.len(), 2);
    let mut kinds: Vec<_> = mat
        .objects
        .values()
        .flat_map(|o| o.paths.iter())
        .flat_map(|p| p.ops.iter())
        .map(|op| op.kind)
        .collect();
    kinds.sort();
    assert_eq!(kinds, vec![OpKind::Cut, OpKind::Draw]);
    assert_eq!(mat.op_order.len(), 2, "one row per distinct operation");
}

#[test]
fn the_whole_import_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "card.svg", RED_SQUARE);
    let mut s = state();
    import_file(&mut s, &path, 0).unwrap();
    assert_eq!(s.run_command("edit.undo", 0), Effect::None);
    assert_eq!(s.current_mat().objects.len(), 0);
    assert_eq!(s.current_mat().op_order.len(), 0);
    assert_eq!(s.document().assets.len(), 0);
    assert!(!s.can_undo(), "nothing else was recorded");
}

#[test]
fn the_original_file_and_the_rules_are_kept_with_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "card.svg", RED_SQUARE);
    let mut s = state();
    import_file(&mut s, &path, 0).unwrap();
    let doc = s.document();
    assert_eq!(doc.assets.len(), 1);
    let asset = doc.assets.values().next().unwrap();
    assert_eq!(asset.name, "card.svg");
    assert!(
        !doc.rules.is_empty(),
        "the default rules were saved for re-import"
    );
}

#[test]
fn existing_project_rules_are_used_instead_of_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "card.svg", RED_SQUARE);
    let mut s = state();
    // Only red lines are cut; blue ones match no rule.
    let only_red = vec![default_rules().remove(0)];
    s.set_rules(only_red.clone(), 0).unwrap();
    let report = import_file(&mut s, &path, 0).unwrap();
    assert_eq!(*s.document().rules, only_red);
    let with_ops = s
        .current_mat()
        .objects
        .values()
        .flat_map(|o| o.paths.iter())
        .filter(|p| !p.ops.is_empty())
        .count();
    assert_eq!(with_ops, 1);
    assert!(
        !report.diagnostics.is_empty(),
        "the unmatched blue path is reported"
    );
}

#[test]
fn imported_objects_are_selected() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "card.svg", RED_SQUARE);
    let mut s = state();
    import_file(&mut s, &path, 0).unwrap();
    assert_eq!(s.selection().objects().len(), 1);
}

#[test]
fn a_dxf_is_imported_too() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "part.dxf", DXF);
    let mut s = state();
    let report = import_file(&mut s, &path, 0).unwrap();
    assert!(report.objects >= 1, "{report:?}");
    assert!(!s.current_mat().objects.is_empty());
}

#[test]
fn importing_twice_adds_a_second_copy_with_its_own_asset() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "card.svg", RED_SQUARE);
    let mut s = state();
    import_file(&mut s, &path, 0).unwrap();
    import_file(&mut s, &path, 1_000).unwrap();
    assert_eq!(s.current_mat().objects.len(), 2);
    assert_eq!(s.document().assets.len(), 2);
}

#[test]
fn the_imported_job_compiles() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "card.svg", RED_SQUARE);
    let mut s = state();
    import_file(&mut s, &path, 0).unwrap();
    s.tick(1_000);
    assert!(s.wait_for_compile(Duration::from_secs(60)));
    assert_eq!(s.compiled().unwrap().job.mats.len(), 1);
}

#[test]
fn unsupported_and_unreadable_files_are_refused_and_nothing_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = state();
    let txt = write(&dir, "notes.txt", "hello");
    assert!(matches!(
        import_file(&mut s, &txt, 0),
        Err(ImportError::Unsupported(_))
    ));
    assert!(matches!(
        import_file(&mut s, &dir.path().join("missing.svg"), 0),
        Err(ImportError::Io(_))
    ));
    let bad = write(&dir, "bad.svg", "this is not svg");
    assert!(matches!(
        import_file(&mut s, &bad, 0),
        Err(ImportError::Refused(_))
    ));
    assert_eq!(s.current_mat().objects.len(), 0);
    assert!(!s.can_undo());
}

#[test]
fn the_kind_of_a_file_comes_from_its_extension_in_any_case() {
    assert_eq!(kind_of(Path::new("a.SVG")), Some(ImportKind::Svg));
    assert_eq!(kind_of(Path::new("a.svgz")), Some(ImportKind::Svg));
    assert_eq!(kind_of(Path::new("a.Dxf")), Some(ImportKind::Dxf));
    assert_eq!(kind_of(Path::new("a.eps")), None);
    assert_eq!(kind_of(Path::new("noext")), None);
}

#[test]
fn the_default_rules_name_real_library_tools() {
    let s = state();
    let tools: Vec<String> = s
        .tools_for(OpKind::Cut)
        .into_iter()
        .chain(s.tools_for(OpKind::Draw))
        .map(|t| t.reference)
        .collect();
    for rule in default_rules() {
        assert!(
            tools
                .iter()
                .any(|t| t.starts_with(&format!("{}@", rule.assign.tool))),
            "{} is not in the library",
            rule.assign.tool
        );
    }
}

// ---- PDF and AI ----

fn red_square_pdf() -> Vec<u8> {
    use pdf_writer::{Content, Finish, Pdf, Rect, Ref};
    let (catalog, tree, page, stream) = (Ref::new(1), Ref::new(2), Ref::new(3), Ref::new(4));
    let mut pdf = Pdf::new();
    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree).kids([page]).count(1);
    let mut p = pdf.page(page);
    p.media_box(Rect::new(0.0, 0.0, 400.0, 400.0));
    p.parent(tree);
    p.contents(stream);
    p.finish();
    let mut c = Content::new();
    c.set_stroke_rgb(1.0, 0.0, 0.0).set_line_width(1.0);
    c.rect(50.0, 50.0, 100.0, 100.0).stroke();
    pdf.stream(stream, &Content::finish(c));
    pdf.finish()
}

#[test]
fn pdf_and_ai_files_are_recognised_by_extension() {
    assert_eq!(kind_of(Path::new("a.pdf")), Some(ImportKind::Pdf));
    assert_eq!(kind_of(Path::new("a.AI")), Some(ImportKind::Pdf));
    assert_eq!(kind_of(Path::new("a.svg")), Some(ImportKind::Svg));
}

#[test]
fn a_pdf_is_imported_with_its_red_line_assigned_to_the_blade() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("card.pdf");
    std::fs::write(&path, red_square_pdf()).unwrap();
    let mut s = state();
    let report = import_file(&mut s, &path, 0).unwrap();
    assert_eq!(report.objects, 1);
    let mat = s.current_mat();
    let object = mat.objects.values().next().unwrap();
    assert!(
        object.paths[0].ops.iter().any(|op| op.kind == OpKind::Cut),
        "{:?}",
        object.paths[0].ops
    );
    // 100 pt is about 35 mm.
    let b = object.bounds().unwrap();
    assert!((b.width() - 100.0 * 25.4 / 72.0).abs() < 1.0, "{b:?}");
}

#[test]
fn a_file_that_is_not_a_pdf_is_refused_with_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "bad.pdf", "not a pdf");
    let mut s = state();
    let error = import_file(&mut s, &path, 0).unwrap_err();
    assert!(error.to_string().contains("bad.pdf"), "{error}");
}
