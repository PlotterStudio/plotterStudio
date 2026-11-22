//! `cargo xtask`: repository chores. `cargo xtask check-boundaries` fails when a crate gains a
//! dependency the design forbids.

mod boundaries;
mod l10n;
mod licenses;
mod package;
mod schemas;
mod workflows;

use std::io::Write;
use std::process::ExitCode;

use anyhow::{Context, Result};
use cargo_metadata::{DependencyKind, MetadataCommand};

use boundaries::{Package, check, check_transitive};

fn workspace_packages() -> Result<Vec<Package>> {
    let metadata = MetadataCommand::new()
        .no_deps()
        .exec()
        .context("running cargo metadata")?;
    Ok(metadata
        .packages
        .iter()
        .map(|p| Package {
            name: p.name.to_string(),
            deps: p
                .dependencies
                .iter()
                .filter(|d| d.kind != DependencyKind::Development)
                .map(|d| d.name.clone())
                .collect(),
        })
        .collect())
}

/// Every package with the names of everything it depends on, directly or not (resolved graph).
fn transitive_packages() -> Result<Vec<Package>> {
    use std::collections::{BTreeMap, BTreeSet};
    let metadata = MetadataCommand::new()
        .exec()
        .context("running cargo metadata")?;
    let resolve = metadata
        .resolve
        .context("cargo metadata gave no resolve graph")?;
    let names: BTreeMap<_, _> = metadata
        .packages
        .iter()
        .map(|p| (p.id.clone(), p.name.to_string()))
        .collect();
    let edges: BTreeMap<_, Vec<_>> = resolve
        .nodes
        .iter()
        .map(|n| {
            let normal = n
                .deps
                .iter()
                .filter(|d| {
                    d.dep_kinds
                        .iter()
                        .any(|k| k.kind != DependencyKind::Development)
                })
                .map(|d| d.pkg.clone())
                .collect();
            (n.id.clone(), normal)
        })
        .collect();
    let mut out = Vec::new();
    for member in &metadata.workspace_members {
        let mut seen = BTreeSet::new();
        let mut stack = vec![member.clone()];
        while let Some(id) = stack.pop() {
            for next in edges.get(&id).into_iter().flatten() {
                if seen.insert(next.clone()) {
                    stack.push(next.clone());
                }
            }
        }
        out.push(Package {
            name: names.get(member).cloned().unwrap_or_default(),
            deps: seen
                .iter()
                .filter_map(|id| names.get(id).cloned())
                .collect(),
        });
    }
    Ok(out)
}

fn main() -> Result<ExitCode> {
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let task = std::env::args().nth(1);
    let root = std::env::current_dir()?;
    if task.as_deref() == Some("schemas") {
        let files = schemas::generate();
        schemas::write_all(&root, &files)?;
        writeln!(out, "wrote {} schema files", files.len())?;
        return Ok(ExitCode::SUCCESS);
    }
    if task.as_deref() == Some("check-schemas") {
        let diffs = schemas::differences(&root, &schemas::generate());
        if diffs.is_empty() {
            writeln!(out, "schemas: ok")?;
            return Ok(ExitCode::SUCCESS);
        }
        for d in &diffs {
            writeln!(err, "{d}")?;
        }
        writeln!(err, "run `cargo xtask schemas` and commit the result")?;
        return Ok(ExitCode::FAILURE);
    }
    if task.as_deref() == Some("check-boundaries") {
        let mut violations = check(&workspace_packages()?);
        violations.extend(check_transitive(&transitive_packages()?));
        if violations.is_empty() {
            writeln!(out, "dependency boundaries: ok")?;
            return Ok(ExitCode::SUCCESS);
        }
        for v in &violations {
            writeln!(err, "{v}")?;
        }
        return Ok(ExitCode::FAILURE);
    }
    if task.as_deref() == Some("l10n-check") {
        let problems = l10n::check_dir(&root)?;
        if problems.is_empty() {
            writeln!(out, "localisation: ok")?;
            return Ok(ExitCode::SUCCESS);
        }
        for p in &problems {
            writeln!(err, "{p}")?;
        }
        return Ok(ExitCode::FAILURE);
    }
    if task.as_deref() == Some("licenses") {
        return licenses_task(&root, std::env::args().any(|a| a == "--notice"));
    }
    if task.as_deref() == Some("package") {
        let files = package::package_all(&root)?;
        for f in &files {
            writeln!(out, "wrote {}", f.display())?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    if task.as_deref() == Some("goldens") {
        return goldens_task(std::env::args().any(|a| a == "--bless"));
    }
    writeln!(
        err,
        "usage: cargo xtask <check-boundaries | schemas | check-schemas | l10n-check | licenses [--notice] | goldens [--bless] | package>"
    )?;
    Ok(ExitCode::from(2))
}

/// Checks every dependency licence against `deny.toml`; with `--notice` also writes
/// `THIRD-PARTY-LICENSES.md`.
fn licenses_task(root: &std::path::Path, write_notice: bool) -> Result<ExitCode> {
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let deny = std::fs::read_to_string(root.join("deny.toml")).context("reading deny.toml")?;
    let allow = licenses::allow_list(&deny).map_err(anyhow::Error::msg)?;
    let metadata = MetadataCommand::new()
        .exec()
        .context("running cargo metadata")?;
    let members: std::collections::BTreeSet<_> = metadata.workspace_members.iter().collect();
    let deps: Vec<licenses::Dependency> = licenses::clarify(
        metadata
            .packages
            .iter()
            .filter(|p| !members.contains(&p.id))
            .map(|p| licenses::Dependency {
                name: p.name.to_string(),
                version: p.version.to_string(),
                license: p.license.clone(),
            })
            .collect(),
    );
    if write_notice {
        std::fs::write(
            root.join("THIRD-PARTY-LICENSES.md"),
            licenses::notice(&deps),
        )?;
        writeln!(
            out,
            "wrote THIRD-PARTY-LICENSES.md for {} packages",
            deps.len()
        )?;
    }
    let problems = licenses::violations(&deps, &allow);
    if problems.is_empty() {
        writeln!(out, "licences: ok ({} packages)", deps.len())?;
        return Ok(ExitCode::SUCCESS);
    }
    for p in &problems {
        writeln!(err, "{p}")?;
    }
    Ok(ExitCode::FAILURE)
}

/// Runs the golden-file tests; `--bless` rewrites the expected files instead of comparing.
fn goldens_task(bless: bool) -> Result<ExitCode> {
    let mut command = std::process::Command::new("cargo");
    command.args(["test", "-p", "ps-cli", "--test", "goldens"]);
    if bless {
        command.env("PS_BLESS", "1");
    }
    let status = command.status().context("running cargo test")?;
    Ok(if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
