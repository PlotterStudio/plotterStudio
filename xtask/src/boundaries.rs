//! The dependency-boundary rules, as a pure function over the workspace's direct
//! dependencies so they can be tested without running `cargo metadata`.

use std::fmt;

/// A workspace package and the names of its direct (normal and build) dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Package {
    /// The package name.
    pub(crate) name: String,
    /// Its direct dependencies.
    pub(crate) deps: Vec<String>,
}

/// One broken rule.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Violation {
    /// The package that has the dependency.
    pub(crate) krate: String,
    /// The dependency it must not have.
    pub(crate) dependency: String,
    /// Why not.
    pub(crate) rule: &'static str,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} depends on {}: {}",
            self.krate, self.dependency, self.rule
        )
    }
}

/// The crates that build and test with no window, GPU, network or device access.
const CORE: &[&str] = &[
    "ps-core",
    "ps-geometry",
    "ps-job",
    "ps-model",
    "ps-library",
    "ps-import",
    "ps-compiler",
    "ps-gcode",
    "ps-nest",
];

/// What a core crate may never depend on.
const CORE_BANNED: &[&str] = &["egui", "eframe", "winit", "wgpu", "ps-net", "ps-device"];

/// Checks every package against the rules and returns the violations in a stable order.
#[must_use]
pub(crate) fn check(packages: &[Package]) -> Vec<Violation> {
    let mut out = Vec::new();
    for package in packages {
        let is_core = CORE.contains(&package.name.as_str());
        for dep in &package.deps {
            let dep = dep.as_str();
            let rule = if is_core && CORE_BANNED.contains(&dep) {
                Some("a core crate must stay free of GUI, GPU, network and device crates")
            } else if dep == "ps-net" && package.name != "ps-app" {
                Some("only ps-app may depend on ps-net")
            } else if dep == "ps-device" && !matches!(package.name.as_str(), "ps-app" | "ps-cli") {
                Some("only ps-app and ps-cli may depend on ps-device")
            } else {
                None
            };
            if let Some(rule) = rule {
                out.push(Violation {
                    krate: package.name.clone(),
                    dependency: dep.to_owned(),
                    rule,
                });
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// HTTP and TLS client crates: only `ps-net` (and the app through it) may bring these in.
const NETWORK_CLIENTS: &[&str] = &[
    "reqwest",
    "hyper",
    "hyper-util",
    "hyper-rustls",
    "hyper-tls",
    "ureq",
    "native-tls",
    "curl",
    "isahc",
    "surf",
    "attohttpc",
];

/// Checks the full dependency closure of each core crate: none of them may reach an HTTP client,
/// directly or through another crate (a schema validator that resolves remote references would
/// otherwise give the core internet access).
#[must_use]
pub(crate) fn check_transitive(packages: &[Package]) -> Vec<Violation> {
    let mut out: Vec<Violation> = packages
        .iter()
        .filter(|p| CORE.contains(&p.name.as_str()))
        .flat_map(|p| {
            p.deps
                .iter()
                .filter(|d| NETWORK_CLIENTS.contains(&d.as_str()))
                .map(|d| Violation {
                    krate: p.name.clone(),
                    dependency: d.clone(),
                    rule: "a core crate must not reach an HTTP client, even indirectly",
                })
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests;
