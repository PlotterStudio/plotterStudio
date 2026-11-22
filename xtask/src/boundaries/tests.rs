//! Ground truth for the dependency-boundary rules: a core crate must not gain a
//! dependency on egui, winit, wgpu, ps-net or ps-device; no crate except ps-app may depend on
//! ps-net; no crate except ps-app and ps-cli may depend on ps-device.

use super::*;

fn pkg(name: &str, deps: &[&str]) -> Package {
    Package {
        name: name.to_owned(),
        deps: deps.iter().map(|d| (*d).to_owned()).collect(),
    }
}

fn found(packages: &[Package]) -> Vec<(String, String)> {
    check(packages)
        .into_iter()
        .map(|v| (v.krate, v.dependency))
        .collect()
}

#[test]
fn the_allowed_shape_passes() {
    let packages = [
        pkg("ps-core", &["blake3", "serde"]),
        pkg("ps-geometry", &["ps-core", "kurbo"]),
        pkg("ps-model", &["ps-core", "ps-geometry"]),
        pkg("ps-library", &["ps-core"]),
        pkg("ps-render", &["ps-core", "wgpu", "lyon"]),
        pkg("ps-net", &["ps-library", "ureq"]),
        pkg("ps-device", &["ps-core", "ps-job", "wasmtime"]),
        pkg("ps-cli", &["ps-model", "ps-device", "clap"]),
        pkg(
            "ps-app",
            &[
                "ps-net",
                "ps-device",
                "ps-render",
                "eframe",
                "egui",
                "winit",
            ],
        ),
    ];
    assert_eq!(found(&packages), Vec::<(String, String)>::new());
}

#[test]
fn a_core_crate_may_not_use_a_gui_or_gpu_crate() {
    for banned in ["egui", "eframe", "winit", "wgpu"] {
        let packages = [pkg("ps-geometry", &["ps-core", banned])];
        assert_eq!(
            found(&packages),
            vec![("ps-geometry".to_owned(), banned.to_owned())],
            "{banned}"
        );
    }
}

#[test]
fn every_core_crate_is_covered() {
    for name in [
        "ps-core",
        "ps-geometry",
        "ps-job",
        "ps-model",
        "ps-library",
        "ps-import",
        "ps-compiler",
        "ps-gcode",
        "ps-nest",
    ] {
        let packages = [pkg(name, &["wgpu"])];
        assert_eq!(check(&packages).len(), 1, "{name}");
    }
}

#[test]
fn a_core_crate_may_not_use_the_network_or_device_crates() {
    let packages = [pkg("ps-compiler", &["ps-net", "ps-device"])];
    let mut got = found(&packages);
    got.sort();
    assert_eq!(
        got,
        vec![
            ("ps-compiler".to_owned(), "ps-device".to_owned()),
            ("ps-compiler".to_owned(), "ps-net".to_owned()),
        ]
    );
}

#[test]
fn only_the_app_may_depend_on_the_network_crate() {
    for krate in [
        "ps-cli",
        "ps-render",
        "ps-device",
        "ps-library",
        "someone-else",
    ] {
        let packages = [pkg(krate, &["ps-net"])];
        assert!(
            found(&packages).contains(&(krate.to_owned(), "ps-net".to_owned())),
            "{krate}"
        );
    }
    assert_eq!(found(&[pkg("ps-app", &["ps-net"])]), vec![]);
}

#[test]
fn only_the_app_and_the_cli_may_depend_on_the_device_crate() {
    assert_eq!(found(&[pkg("ps-app", &["ps-device"])]), vec![]);
    assert_eq!(found(&[pkg("ps-cli", &["ps-device"])]), vec![]);
    for krate in ["ps-render", "ps-library", "ps-job", "ps-net"] {
        let packages = [pkg(krate, &["ps-device"])];
        assert!(
            found(&packages).contains(&(krate.to_owned(), "ps-device".to_owned())),
            "{krate}"
        );
    }
}

#[test]
fn the_renderer_may_use_the_gpu_crate() {
    assert_eq!(found(&[pkg("ps-render", &["wgpu"])]), vec![]);
}

#[test]
fn the_hardware_crates_outside_the_plotter_studio_set_are_not_judged() {
    let packages = [
        pkg("vendor-driver", &["tokio"]),
        pkg("vendor-harness", &["vendor-driver"]),
    ];
    assert_eq!(found(&packages), vec![]);
}

#[test]
fn each_violation_says_which_rule_it_broke() {
    let violations = check(&[pkg("ps-geometry", &["wgpu"]), pkg("ps-job", &["ps-net"])]);
    assert_eq!(violations.len(), 2);
    assert!(
        violations
            .iter()
            .any(|v| v.to_string().contains("ps-geometry") && v.to_string().contains("wgpu"))
    );
    assert!(violations.iter().any(|v| v.to_string().contains("ps-net")));
}

#[test]
fn violations_come_out_in_a_stable_order() {
    let a = check(&[
        pkg("ps-model", &["winit", "egui"]),
        pkg("ps-core", &["wgpu"]),
    ]);
    let b = check(&[
        pkg("ps-core", &["wgpu"]),
        pkg("ps-model", &["egui", "winit"]),
    ]);
    assert_eq!(a, b);
}

// ---- no network client reachable from the core ----

fn closure(name: &str, deps: &[&str]) -> Package {
    pkg(name, deps)
}

#[test]
fn a_core_crate_that_reaches_an_http_client_through_another_crate_is_caught() {
    for client in [
        "reqwest",
        "hyper",
        "ureq",
        "native-tls",
        "hyper-rustls",
        "curl",
    ] {
        let packages = [closure("ps-library", &["serde", "jsonschema", client])];
        assert_eq!(
            check_transitive(&packages)
                .into_iter()
                .map(|v| (v.krate, v.dependency))
                .collect::<Vec<_>>(),
            vec![("ps-library".to_owned(), client.to_owned())],
            "{client}"
        );
    }
}

#[test]
fn the_app_and_the_net_crate_may_reach_http_clients() {
    let packages = [
        closure("ps-net", &["ureq", "rustls"]),
        closure("ps-app", &["ureq", "rustls"]),
        closure("ps-core", &["blake3", "serde"]),
    ];
    assert_eq!(check_transitive(&packages), Vec::new());
}
