//! Ground truth for the `.psjob` writer. The reader in `ps-job` verifies layout,
//! hashes and manifest consistency, so a bundle that opens there is correct: the tests write a
//! bundle and open it with that reader. The bundle is deterministic except for `export_time`;
//! nothing is written when any mat fails validation; G-code files are moved into place only when
//! every mat succeeded.

use ps_core::{LibRef, MatId, ObjectId, OpId, PathKey, PtUm};
use ps_job::{Bundle, Estimate, HolderId, MatJob, PauseMsg, SrcRef, Step, ToolEntry};
use ps_library::{LibrarySnapshot, MachineProfile, builtin_entries};

use super::*;

fn profile() -> MachineProfile {
    let libs = LibrarySnapshot::from_entries(builtin_entries().unwrap());
    let r: LibRef = "generic.cutter-medium@1.0.0".parse().unwrap();
    (*libs.resolve_profile(&r).unwrap().item).clone()
}

fn p(x: f64, y: f64) -> PtUm {
    PtUm::from_mm(x, y).unwrap()
}

fn mat_job(far: f64) -> MatJob {
    let src = SrcRef {
        mat: MatId::at_unix_ms(1),
        object: ObjectId::at_unix_ms(2),
        path: PathKey::source("p"),
        op: OpId::at_unix_ms(3),
        move_index: 0,
    };
    MatJob {
        mat: MatId::at_unix_ms(1),
        tool_table: vec![ToolEntry {
            slot: 1,
            tool: "pen.fineliner-black-0.4@1.0.0".parse().unwrap(),
            holder: HolderId::new("generic.holder-a"),
            label: "Fine liner".into(),
            pen: None,
        }],
        steps: vec![
            Step::Pause(PauseMsg::new("Load pen")),
            Step::LoadTool {
                slot: 1,
                holder: HolderId::new("generic.holder-a"),
                pause: None,
            },
            Step::Travel { to: p(10.0, 10.0) },
            Step::ToolDown { force_gf: 120 },
            Step::Line {
                to: p(far, 10.0),
                feed_mm_min: 6_000,
                a_mdeg: None,
                src,
            },
            Step::ToolUp,
        ],
        estimate: Estimate {
            machine_time_ms: 12_345,
            with_changes_ms: 40_000,
            tool_changes: 0,
            ..Estimate::default()
        },
    }
}

fn material() -> LibRef {
    "community.cardstock-80lb@1.0.0".parse().unwrap()
}

fn input<'a>(
    profile: &'a MachineProfile,
    mats: &'a [BundleMat<'a>],
    export_time: &'a str,
) -> BundleInput<'a> {
    BundleInput {
        job_id: "job-1",
        app_version: "0.1.0",
        export_time,
        profile,
        mats,
    }
}

fn one<'a>(job: &'a MatJob, material: Option<&'a LibRef>) -> BundleMat<'a> {
    BundleMat {
        job,
        name: "Mat 1",
        label: "12x12in standard",
        material,
    }
}

#[test]
fn a_written_bundle_opens_in_the_verifying_reader() {
    let (pr, job, m) = (profile(), mat_job(60.0), material());
    let mats = [one(&job, Some(&m))];
    let bytes = write_bundle(&input(&pr, &mats, "2026-11-18T10:00:00Z")).unwrap();
    let bundle = Bundle::from_bytes(&bytes).unwrap();
    let man = &bundle.manifest;
    assert_eq!(man.job_id, "job-1");
    assert_eq!(man.app_version, "0.1.0");
    assert_eq!(man.export_time, "2026-11-18T10:00:00Z");
    assert_eq!(man.dialect, "ps-gcode/1");
    assert_eq!(man.profile.to_string(), "generic.cutter-medium@1.0.0");
    assert_eq!(man.mats.len(), 1);
    let entry = &man.mats[0];
    assert_eq!((entry.index, entry.name.as_str()), (1, "Mat 1"));
    assert_eq!(entry.gcode, "mats/01.gcode");
    assert_eq!(entry.preview.as_deref(), Some("previews/01.svg"));
    assert_eq!(entry.material.as_ref(), Some(&m));
    assert_eq!(entry.tools, job.tool_table);
    assert_eq!(entry.estimate, job.estimate);
}

#[test]
fn the_bundled_gcode_is_exactly_what_the_writer_produces() {
    let (pr, job, m) = (profile(), mat_job(60.0), material());
    let mats = [one(&job, Some(&m))];
    let bytes = write_bundle(&input(&pr, &mats, "t")).unwrap();
    let bundle = Bundle::from_bytes(&bytes).unwrap();
    let expected = write_mat(
        &job,
        &WriteContext {
            app_version: "0.1.0",
            job_id: "job-1",
            profile: &pr,
            mat_index: 1,
            mat_count: 1,
            mat_label: "12x12in standard",
            material: Some(&m),
        },
    );
    assert_eq!(bundle.gcode(1), Some(expected.text.as_str()));
    // The manifest's pause table points at real lines of that file.
    for row in &bundle.manifest.mats[0].pauses {
        let line = bundle
            .gcode(1)
            .unwrap()
            .lines()
            .nth(usize::try_from(row.line).unwrap() - 1)
            .unwrap();
        assert!(line.starts_with("M0 ("), "{line}");
    }
    assert_ne!(bundle.manifest.mats[0].pauses.len(), 0);
}

#[test]
fn the_bundle_carries_a_copy_of_the_profile_and_a_preview() {
    let (pr, job) = (profile(), mat_job(60.0));
    let mats = [one(&job, None)];
    let bundle = Bundle::from_bytes(&write_bundle(&input(&pr, &mats, "t")).unwrap()).unwrap();
    let copy: serde_json::Value = serde_json::from_slice(bundle.profile_json().unwrap()).unwrap();
    assert_eq!(copy["id"], "generic.cutter-medium");
    let svg = std::str::from_utf8(bundle.file("previews/01.svg").unwrap()).unwrap();
    assert!(svg.starts_with("<svg "));
    assert!(svg.contains("<path d=\"M10 10L60 10\"/>"), "{svg}");
}

#[test]
fn several_mats_are_numbered_in_order_and_know_their_position() {
    let (pr, a, b) = (profile(), mat_job(60.0), mat_job(80.0));
    let mats = [one(&a, None), one(&b, None)];
    let bundle = Bundle::from_bytes(&write_bundle(&input(&pr, &mats, "t")).unwrap()).unwrap();
    assert_eq!(bundle.manifest.mats.len(), 2);
    assert_eq!(bundle.manifest.mats[1].gcode, "mats/02.gcode");
    assert!(bundle.gcode(1).unwrap().contains("mat 1/2 "));
    assert!(bundle.gcode(2).unwrap().contains("mat 2/2 "));
    assert!(bundle.gcode(2).unwrap().contains("X80.000"));
}

#[test]
fn the_bundle_is_deterministic_except_for_the_export_time() {
    let (pr, job) = (profile(), mat_job(60.0));
    let mats = [one(&job, None)];
    let a = write_bundle(&input(&pr, &mats, "2026-01-01T00:00:00Z")).unwrap();
    assert_eq!(
        a,
        write_bundle(&input(&pr, &mats, "2026-01-01T00:00:00Z")).unwrap()
    );

    let b = write_bundle(&input(&pr, &mats, "2027-02-02T00:00:00Z")).unwrap();
    assert_ne!(a, b);
    let (ba, bb) = (
        Bundle::from_bytes(&a).unwrap(),
        Bundle::from_bytes(&b).unwrap(),
    );
    assert_eq!(ba.file("mats/01.gcode"), bb.file("mats/01.gcode"));
    assert_eq!(ba.file("previews/01.svg"), bb.file("previews/01.svg"));
    assert_eq!(ba.profile_json(), bb.profile_json());
    assert_eq!(
        ba.manifest.files, bb.manifest.files,
        "file hashes do not depend on the time"
    );
    assert_ne!(ba.manifest.export_time, bb.manifest.export_time);
}

#[test]
fn a_job_that_breaks_the_machine_limits_is_refused_and_names_the_mat() {
    let (pr, good, bad) = (profile(), mat_job(60.0), mat_job(900.0));
    let mats = [one(&good, None), one(&bad, None)];
    match write_bundle(&input(&pr, &mats, "t")).unwrap_err() {
        BundleWriteError::Invalid { mat_index, error } => {
            assert_eq!(mat_index, 2);
            assert_ne!(error.violations.len(), 0);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_bundle_needs_at_least_one_mat() {
    let pr = profile();
    assert_eq!(
        write_bundle(&input(&pr, &[], "t")).unwrap_err(),
        BundleWriteError::NoMats
    );
}

// ---- plain G-code files ----

#[test]
fn gcode_files_are_written_one_per_mat() {
    let dir = tempfile::tempdir().unwrap();
    let (pr, a, b) = (profile(), mat_job(60.0), mat_job(80.0));
    let mats = [one(&a, None), one(&b, None)];
    let files = export_gcode(dir.path(), "job", &input(&pr, &mats, "t")).unwrap();
    let names: Vec<String> = files
        .iter()
        .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["job-01.gcode", "job-02.gcode"]);
    let second = std::fs::read_to_string(&files[1]).unwrap();
    assert!(second.contains("X80.000") && second.ends_with("M2\n"));
    // Nothing else is left behind.
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn a_failed_export_leaves_no_partial_output_and_keeps_old_files() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("job-01.gcode");
    std::fs::write(&old, "old contents").unwrap();
    let (pr, good, bad) = (profile(), mat_job(60.0), mat_job(900.0));
    let mats = [one(&good, None), one(&bad, None)];
    assert!(export_gcode(dir.path(), "job", &input(&pr, &mats, "t")).is_err());
    assert_eq!(std::fs::read_to_string(&old).unwrap(), "old contents");
    let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(entries.len(), 1, "no temporary files remain");
}

#[test]
fn a_successful_export_replaces_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("job-01.gcode"), "old").unwrap();
    let (pr, job) = (profile(), mat_job(60.0));
    let mats = [one(&job, None)];
    export_gcode(dir.path(), "job", &input(&pr, &mats, "t")).unwrap();
    assert!(
        std::fs::read_to_string(dir.path().join("job-01.gcode"))
            .unwrap()
            .starts_with("; plotter-studio")
    );
}

#[test]
fn exporting_to_a_directory_that_does_not_exist_is_an_io_error() {
    let dir = tempfile::tempdir().unwrap();
    let (pr, job) = (profile(), mat_job(60.0));
    let mats = [one(&job, None)];
    let missing = dir.path().join("nope");
    assert!(matches!(
        export_gcode(&missing, "job", &input(&pr, &mats, "t")),
        Err(BundleWriteError::Io(_))
    ));
}
