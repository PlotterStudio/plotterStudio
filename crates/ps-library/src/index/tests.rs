//! Ground truth for the community materials index: `index.json` lists id, version,
//! category, brand, machines, tools, sha256 and download URL; the app searches it locally and
//! checks each downloaded package against its sha256 (the signature check lives in `ps-net`).

use std::collections::BTreeMap;

use super::*;

fn e(id: &str, version: &str, category: &str, brand: &str, machines: &[&str]) -> IndexEntry {
    IndexEntry {
        id: id.to_owned(),
        version: version.parse().unwrap(),
        name: id.to_owned(),
        category: category.to_owned(),
        brand: brand.to_owned(),
        machines: machines.iter().map(|m| (*m).to_owned()).collect(),
        tools: vec!["generic.drag-blade".to_owned()],
        sha256: sha256_hex(id.as_bytes()),
        url: format!("https://example.test/{id}.zip"),
    }
}

fn index() -> Index {
    Index {
        schema_version: 1,
        entries: vec![
            e(
                "community.kraft",
                "1.0.0",
                "paper",
                "Generic",
                &["generic.cutter-medium"],
            ),
            e(
                "community.glitter",
                "2.0.0",
                "paper",
                "Sparkle",
                &["generic.cutter-medium", "generic.cutter-large"],
            ),
            e(
                "community.htv-black",
                "1.1.0",
                "vinyl",
                "Siser",
                &["generic.cutter-large"],
            ),
        ],
    }
}

#[test]
fn the_index_round_trips_through_json() {
    let i = index();
    let text = serde_json::to_string(&i).unwrap();
    assert_eq!(serde_json::from_str::<Index>(&text).unwrap(), i);
}

#[test]
fn sha256_matches_the_published_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn a_package_must_match_its_hash() {
    let entry = e("community.kraft", "1.0.0", "paper", "Generic", &[]);
    assert!(entry.verify(b"community.kraft"));
    assert!(!entry.verify(b"community.kraft "));
    assert!(!entry.verify(b""));
}

#[test]
fn search_is_local_and_case_insensitive() {
    let i = index();
    let ids = |q: &Query| {
        i.search(q)
            .iter()
            .map(|x| x.id.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&Query::text("GLIT")), ["community.glitter"]);
    assert_eq!(ids(&Query::text("siser")), ["community.htv-black"]);
    assert_eq!(ids(&Query::text("")).len(), 3);
}

#[test]
fn search_filters_by_machine_and_category() {
    let i = index();
    let q = Query {
        machine: Some("generic.cutter-large".to_owned()),
        ..Query::default()
    };
    assert_eq!(i.search(&q).len(), 2);
    let q = Query {
        machine: Some("generic.cutter-large".to_owned()),
        category: Some("vinyl".to_owned()),
        ..Query::default()
    };
    assert_eq!(
        i.search(&q)
            .iter()
            .map(|x| x.id.as_str())
            .collect::<Vec<_>>(),
        ["community.htv-black"]
    );
}

#[test]
fn updates_are_entries_newer_than_what_is_installed() {
    let i = index();
    let installed: BTreeMap<String, Version> = [
        ("community.kraft".to_owned(), "1.0.0".parse().unwrap()),
        ("community.glitter".to_owned(), "1.5.0".parse().unwrap()),
    ]
    .into();
    let updates: Vec<&str> = i
        .updates(&installed)
        .iter()
        .map(|x| x.id.as_str())
        .collect();
    assert_eq!(updates, ["community.glitter"]);
}

#[test]
fn only_https_download_urls_are_acceptable() {
    let mut entry = e("a", "1.0.0", "paper", "x", &[]);
    assert!(entry.has_safe_url());
    entry.url = "http://example.test/a.zip".to_owned();
    assert!(!entry.has_safe_url());
    entry.url = "file:///etc/passwd".to_owned();
    assert!(!entry.has_safe_url());
}

#[test]
fn newer_schemas_are_refused() {
    let text = r#"{"schema_version": 9, "entries": []}"#;
    assert!(Index::from_json(text).is_err());
    assert!(Index::from_json(r#"{"schema_version": 1, "entries": []}"#).is_ok());
    assert!(Index::from_json("nope").is_err());
}
