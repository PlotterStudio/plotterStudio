//! Ground truth for the licence check and notice: every dependency's licence must
//! satisfy the allow-list in `deny.toml`, and the third-party notice lists each package.

use super::*;

fn allow() -> Vec<String> {
    [
        "MIT",
        "Apache-2.0",
        "Apache-2.0 WITH LLVM-exception",
        "BSD-3-Clause",
        "Unicode-3.0",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

#[test]
fn a_single_allowed_licence_passes() {
    assert!(satisfies("MIT", &allow()));
    assert!(!satisfies("GPL-3.0-only", &allow()));
}

#[test]
fn or_needs_one_side_and_and_needs_both() {
    assert!(satisfies("GPL-3.0-only OR MIT", &allow()));
    assert!(!satisfies("GPL-3.0-only OR LGPL-2.1", &allow()));
    assert!(satisfies("MIT AND Unicode-3.0", &allow()));
    assert!(!satisfies("MIT AND GPL-3.0-only", &allow()));
}

#[test]
fn the_older_slash_means_or() {
    assert!(satisfies("MIT/Apache-2.0", &allow()));
    assert!(satisfies("GPL-2.0/MIT", &allow()));
}

#[test]
fn with_exceptions_match_the_listed_combination() {
    assert!(satisfies("Apache-2.0 WITH LLVM-exception", &allow()));
    // The plain licence is listed too, so the exception only adds permissions.
    assert!(satisfies("MIT OR Apache-2.0 WITH LLVM-exception", &allow()));
    let only_plain = vec!["Apache-2.0".to_owned()];
    assert!(satisfies("Apache-2.0 WITH LLVM-exception", &only_plain));
    let only_exception = vec!["Apache-2.0 WITH LLVM-exception".to_owned()];
    assert!(!satisfies("Apache-2.0", &only_exception));
}

#[test]
fn parentheses_group() {
    assert!(satisfies("(MIT OR GPL-3.0-only) AND Unicode-3.0", &allow()));
    assert!(!satisfies("(GPL-3.0-only OR LGPL-2.1) AND MIT", &allow()));
    assert!(satisfies("MIT AND (GPL-3.0-only OR Apache-2.0)", &allow()));
}

#[test]
fn a_package_with_no_licence_is_a_violation() {
    let pkgs = vec![
        Dependency {
            name: "mystery".into(),
            version: "1.0.0".into(),
            license: None,
        },
        Dependency {
            name: "fine".into(),
            version: "2.0.0".into(),
            license: Some("MIT".into()),
        },
    ];
    let violations = violations(&pkgs, &allow());
    assert_eq!(violations.len(), 1);
    assert!(violations[0].contains("mystery"), "{violations:?}");
}

#[test]
fn a_package_whose_licence_is_not_allowed_is_named_with_its_licence() {
    let pkgs = vec![Dependency {
        name: "copyleft".into(),
        version: "0.3.0".into(),
        license: Some("GPL-3.0-only".into()),
    }];
    let v = violations(&pkgs, &allow());
    assert_eq!(
        v,
        vec!["copyleft 0.3.0 is licensed GPL-3.0-only, which deny.toml does not allow".to_owned()]
    );
}

#[test]
fn the_allow_list_is_read_from_deny_toml() {
    let toml = "[licenses]\nallow = [\"MIT\", \"ISC\"]\nconfidence-threshold = 0.9\n";
    assert_eq!(
        allow_list(toml).unwrap(),
        vec!["MIT".to_owned(), "ISC".to_owned()]
    );
    assert!(allow_list("[licenses]\n").is_err());
    assert!(allow_list("not toml [").is_err());
}

#[test]
fn the_notice_lists_each_package_once_sorted_with_its_licence() {
    let pkgs = vec![
        Dependency {
            name: "zeta".into(),
            version: "1.0.0".into(),
            license: Some("MIT".into()),
        },
        Dependency {
            name: "alpha".into(),
            version: "2.1.0".into(),
            license: Some("MIT OR Apache-2.0".into()),
        },
        Dependency {
            name: "alpha".into(),
            version: "2.1.0".into(),
            license: Some("MIT OR Apache-2.0".into()),
        },
    ];
    let notice = notice(&pkgs);
    let a = notice.find("alpha 2.1.0").unwrap();
    let z = notice.find("zeta 1.0.0").unwrap();
    assert!(a < z);
    assert_eq!(notice.matches("alpha 2.1.0").count(), 1);
    assert!(notice.contains("MIT OR Apache-2.0"));
    assert!(notice.starts_with("# Third-party licences"));
    assert!(notice.ends_with('\n'));
}

#[test]
fn a_package_whose_metadata_has_no_licence_can_be_clarified_from_its_licence_file() {
    let pkgs = vec![Dependency {
        name: "vector-text-hershey".into(),
        version: "0.1.1".into(),
        license: None,
    }];
    let clarified = clarify(pkgs);
    assert_eq!(clarified[0].license.as_deref(), Some("LicenseRef-Hershey"));
    assert_eq!(violations(&clarified, &allow()), Vec::<String>::new());
}

#[test]
fn a_clarification_does_not_override_a_declared_licence() {
    let pkgs = vec![Dependency {
        name: "vector-text-hershey".into(),
        version: "0.1.1".into(),
        license: Some("MIT".into()),
    }];
    assert_eq!(clarify(pkgs)[0].license.as_deref(), Some("MIT"));
}

#[test]
fn other_packages_with_no_licence_are_still_violations() {
    let pkgs = vec![Dependency {
        name: "mystery".into(),
        version: "1.0.0".into(),
        license: None,
    }];
    assert_eq!(violations(&clarify(pkgs), &allow()).len(), 1);
}

#[test]
fn the_notice_carries_the_hershey_acknowledgements_when_the_fonts_are_used() {
    let pkgs = vec![Dependency {
        name: "vector-text-hershey".into(),
        version: "0.1.1".into(),
        license: Some("LicenseRef-Hershey".into()),
    }];
    let n = notice(&pkgs);
    assert!(n.contains("Dr. A. V. Hershey"), "{n}");
    assert!(n.contains("James Hurt"), "{n}");
    assert!(!notice(&[]).contains("Hershey"));
}

#[test]
fn the_notice_names_the_bundled_typeface_and_its_licence() {
    let n = notice(&[]);
    assert!(n.contains("Poppins"), "{n}");
    assert!(n.contains("SIL Open Font License"), "{n}");
}
