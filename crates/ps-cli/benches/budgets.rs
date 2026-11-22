//! Performance budgets, measured on generated drawings: a 5 MB SVG imports in under
//! 2 s, a typical mat compiles in under 1 s, and 10,000 paths in under 5 s. Run with
//! `cargo bench -p ps-cli`; the nightly workflow compares each run with the last and opens an issue
//! for a regression of more than 10%.

use std::fmt::Write as _;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use ps_import::{SvgOptions, import_svg};

const MACHINE: &str = "generic.cutter-small";
const RULES: &str = r##"[
  {"match": {"stroke": "#FF0000"}, "assign": {"op": "cut", "tool": "generic.drag-blade"}}
]"##;

/// An SVG of `count` small rectangles on a grid, with a red stroke so a cut is assigned.
fn grid_svg(count: usize, columns: usize, cell_mm: f64) -> String {
    let rows = count.div_ceil(columns);
    let width = cell_mm * f64::from(u32::try_from(columns).unwrap_or(1)) + cell_mm;
    let height = cell_mm * f64::from(u32::try_from(rows).unwrap_or(1)) + cell_mm;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}mm\" height=\"{height}mm\" viewBox=\"0 0 {width} {height}\">\n"
    );
    let side = cell_mm * 0.6;
    for i in 0..count {
        let (col, row) = (i % columns, i / columns);
        let x = cell_mm * f64::from(u32::try_from(col).unwrap_or(0)) + cell_mm * 0.2;
        let y = cell_mm * f64::from(u32::try_from(row).unwrap_or(0)) + cell_mm * 0.2;
        let _ = writeln!(
            svg,
            "  <rect id=\"r{i}\" x=\"{x:.3}\" y=\"{y:.3}\" width=\"{side:.3}\" height=\"{side:.3}\" fill=\"none\" stroke=\"#FF0000\" stroke-width=\"0.3\"/>"
        );
    }
    svg.push_str("</svg>\n");
    svg
}

/// Exports `svg` through the command line, as the app and the CLI do.
fn export(svg: &str) {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("drawing.svg");
    let rules = dir.path().join("rules.json");
    std::fs::write(&input, svg).unwrap();
    std::fs::write(&rules, RULES).unwrap();
    let args: Vec<String> = [
        "pstudio",
        "export",
        &input.to_string_lossy(),
        "--machine",
        MACHINE,
        "--rules",
        &rules.to_string_lossy(),
        "--out",
        &dir.path().to_string_lossy(),
        "--export-time",
        "2026-01-01T00:00:00Z",
        "--format",
        "gcode",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = ps_cli::run(
        &args,
        &mut ps_cli::Io {
            out: &mut out,
            err: &mut err,
            now_unix_secs: 1_700_000_000,
        },
    );
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
}

fn budgets(c: &mut Criterion) {
    let mut group = c.benchmark_group("budgets");
    group
        .sample_size(10)
        .measurement_time(Duration::from_secs(20));

    // A 5 MB drawing: about 42,000 small shapes.
    let five_mb = grid_svg(42_000, 200, 1.5);
    assert!(five_mb.len() > 5_000_000, "{} bytes", five_mb.len());
    group.bench_function("import_5mb_svg (budget 2 s)", |b| {
        b.iter(|| import_svg(five_mb.as_bytes(), &SvgOptions::default()).unwrap());
    });

    // A typical mat: a few hundred shapes. Import, compile and write.
    let typical = grid_svg(300, 20, 12.0);
    group.bench_function("export_typical_mat_300_paths (budget 1 s)", |b| {
        b.iter(|| export(&typical));
    });

    // The large case: 10,000 paths.
    let large = grid_svg(10_000, 100, 2.9);
    group.bench_function("export_10000_paths (budget 5 s)", |b| {
        b.iter(|| export(&large));
    });
    group.finish();
}

criterion_group!(benches, budgets);
criterion_main!(benches);
