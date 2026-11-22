//! Ground truth for ps-net. It exposes exactly two operations: fetch the release
//! feed, and fetch the signed material index and its packages. Both are off until the user opts
//! in. Signatures are real minisign signatures made by the `minisign` crate, an implementation
//! independent of the verifier under test. Nothing here touches the network: a fake client
//! stands in for HTTP.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Cursor;
use std::sync::Mutex;

use minisign::KeyPair;
use ps_library::Network;
use semver::Version;
use sha2::{Digest, Sha256};

use super::*;

// ---- a fake HTTP client ----

#[derive(Default)]
struct Fake {
    pages: BTreeMap<String, Vec<u8>>,
    asked: Mutex<Vec<String>>,
}

impl Fake {
    fn with(mut self, url: &str, body: impl Into<Vec<u8>>) -> Self {
        self.pages.insert(url.to_owned(), body.into());
        self
    }
    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Http for Fake {
    fn get(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, HttpError> {
        self.asked.lock().unwrap().push(url.to_owned());
        let body = self.pages.get(url).ok_or(HttpError::Status(404))?;
        if body.len() > max_bytes {
            return Err(HttpError::TooLarge);
        }
        Ok(body.clone())
    }
}

fn on() -> Network {
    Network {
        update_check: true,
        material_index: true,
    }
}

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

// ---- the privacy gate ----

#[test]
fn everything_is_off_by_default_and_nothing_is_requested() {
    let net = Net::new(Fake::default(), Network::default());
    assert_eq!(
        net.check_for_update("https://example.org/feed.json", &v("1.0.0")),
        Err(NetError::Disabled(Feature::UpdateCheck))
    );
    let key = KeyPair::generate_unencrypted_keypair().unwrap();
    let pk = key.pk.to_box().unwrap().to_string();
    assert_eq!(
        net.fetch_material_index("https://example.org/index.json", &[&pk]),
        Err(NetError::Disabled(Feature::MaterialIndex))
    );
    assert_eq!(
        net.client().asked(),
        Vec::<String>::new(),
        "no request was made"
    );
}

#[test]
fn the_two_opt_ins_are_independent() {
    let feed = r#"{"schema":1,"releases":[]}"#;
    let client = Fake::default().with("https://example.org/feed.json", feed);
    let only_updates = Network {
        update_check: true,
        material_index: false,
    };
    let net = Net::new(client, only_updates);
    assert!(
        net.check_for_update("https://example.org/feed.json", &v("1.0.0"))
            .is_ok()
    );
    let pk = KeyPair::generate_unencrypted_keypair()
        .unwrap()
        .pk
        .to_box()
        .unwrap()
        .to_string();
    assert!(matches!(
        net.fetch_material_index("https://example.org/index.json", &[&pk]),
        Err(NetError::Disabled(Feature::MaterialIndex))
    ));
}

#[test]
fn only_https_urls_are_fetched() {
    let net = Net::new(Fake::default(), on());
    for url in [
        "http://example.org/feed.json",
        "ftp://example.org/feed.json",
        "file:///etc/passwd",
        "example.org/feed.json",
    ] {
        assert!(
            matches!(
                net.check_for_update(url, &v("1.0.0")),
                Err(NetError::InsecureUrl(_))
            ),
            "{url}"
        );
    }
    assert_eq!(net.client().asked(), Vec::<String>::new());
}

// ---- the release feed ----

const FEED: &str = r#"{
  "schema": 1,
  "releases": [
    {"version": "1.0.0", "date": "2026-01-01", "notes": "first", "url": "https://example.org/1.0.0"},
    {"version": "1.2.0", "date": "2026-03-01", "notes": "newer", "url": "https://example.org/1.2.0"},
    {"version": "1.10.0", "date": "2026-05-01", "notes": "tenth", "url": "https://example.org/1.10.0"},
    {"version": "2.0.0-beta.1", "date": "2026-06-01", "notes": "beta", "url": "https://example.org/2.0.0-beta.1"}
  ]
}"#;

#[test]
fn the_newest_stable_release_newer_than_ours_is_offered() {
    let net = Net::new(
        Fake::default().with("https://example.org/feed.json", FEED),
        on(),
    );
    let update = net
        .check_for_update("https://example.org/feed.json", &v("1.2.0"))
        .unwrap()
        .expect("1.10.0 is newer");
    assert_eq!(
        update.version,
        v("1.10.0"),
        "compared as versions, not as text"
    );
    assert_eq!(update.notes, "tenth");
    assert_eq!(update.url, "https://example.org/1.10.0");
}

#[test]
fn prereleases_are_not_offered_to_stable_users() {
    let net = Net::new(
        Fake::default().with("https://example.org/feed.json", FEED),
        on(),
    );
    let update = net
        .check_for_update("https://example.org/feed.json", &v("1.0.0"))
        .unwrap()
        .unwrap();
    assert_eq!(update.version, v("1.10.0"));
}

#[test]
fn someone_already_on_a_prerelease_is_offered_newer_prereleases() {
    let net = Net::new(
        Fake::default().with("https://example.org/feed.json", FEED),
        on(),
    );
    let update = net
        .check_for_update("https://example.org/feed.json", &v("2.0.0-alpha.1"))
        .unwrap()
        .unwrap();
    assert_eq!(update.version, v("2.0.0-beta.1"));
}

#[test]
fn being_up_to_date_is_not_an_error() {
    let net = Net::new(
        Fake::default().with("https://example.org/feed.json", FEED),
        on(),
    );
    assert_eq!(
        net.check_for_update("https://example.org/feed.json", &v("1.10.0")),
        Ok(None)
    );
    assert_eq!(
        net.check_for_update("https://example.org/feed.json", &v("1.11.0")),
        Ok(None)
    );
}

#[test]
fn a_release_link_that_is_not_https_is_dropped() {
    let feed = r#"{"schema":1,"releases":[
        {"version":"3.0.0","date":"d","notes":"n","url":"http://evil.example/3.0.0"},
        {"version":"2.0.0","date":"d","notes":"n","url":"https://example.org/2.0.0"}]}"#;
    let net = Net::new(
        Fake::default().with("https://example.org/feed.json", feed),
        on(),
    );
    let update = net
        .check_for_update("https://example.org/feed.json", &v("1.0.0"))
        .unwrap()
        .unwrap();
    assert_eq!(update.version, v("2.0.0"));
}

#[test]
fn a_malformed_feed_is_reported_not_trusted() {
    for body in [
        "not json",
        r#"{"schema":1}"#,
        r#"{"schema":2,"releases":[]}"#,
    ] {
        let net = Net::new(
            Fake::default().with("https://example.org/feed.json", body),
            on(),
        );
        assert!(
            matches!(
                net.check_for_update("https://example.org/feed.json", &v("1.0.0")),
                Err(NetError::Malformed(_))
            ),
            "{body}"
        );
    }
}

#[test]
fn a_release_with_an_unparseable_version_is_skipped() {
    let feed = r#"{"schema":1,"releases":[
        {"version":"banana","date":"d","notes":"n","url":"https://example.org/b"},
        {"version":"1.5.0","date":"d","notes":"n","url":"https://example.org/1.5.0"}]}"#;
    let net = Net::new(
        Fake::default().with("https://example.org/feed.json", feed),
        on(),
    );
    let update = net
        .check_for_update("https://example.org/feed.json", &v("1.0.0"))
        .unwrap()
        .unwrap();
    assert_eq!(update.version, v("1.5.0"));
}

#[test]
fn a_missing_feed_is_a_network_error() {
    let net = Net::new(Fake::default(), on());
    assert_eq!(
        net.check_for_update("https://example.org/feed.json", &v("1.0.0")),
        Err(NetError::Http(HttpError::Status(404)))
    );
}

// ---- the signed material index ----

struct Signed {
    public_key: String,
    index: Vec<u8>,
    signature: String,
}

fn sign(index: &str) -> Signed {
    let pair = KeyPair::generate_unencrypted_keypair().unwrap();
    let sig = minisign::sign(
        Some(&pair.pk),
        &pair.sk,
        Cursor::new(index.as_bytes()),
        Some("trusted"),
        Some("untrusted"),
    )
    .unwrap();
    Signed {
        public_key: pair.pk.to_box().unwrap().to_string(),
        index: index.as_bytes().to_vec(),
        signature: sig.to_string(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut out = String::new();
    for b in Sha256::digest(bytes) {
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn index_json(package: &[u8]) -> String {
    format!(
        r#"{{"schema":1,"generated":"2026-01-01T00:00:00Z","packages":[
            {{"id":"community.vinyl","name":"Community vinyl","version":"1.2.0","kind":"material",
              "url":"https://packages.example.org/vinyl-1.2.0.zip","sha256":"{}","size":{},
              "license":"CC0-1.0"}}]}}"#,
        sha256_hex(package),
        package.len()
    )
}

fn host_with(s: &Signed, package: &[u8]) -> Net<Fake> {
    let client = Fake::default()
        .with("https://example.org/index.json", s.index.clone())
        .with(
            "https://example.org/index.json.minisig",
            s.signature.clone(),
        )
        .with("https://packages.example.org/vinyl-1.2.0.zip", package);
    Net::new(client, on())
}

#[test]
fn a_correctly_signed_index_is_read() {
    let package = b"PK-fake-package";
    let s = sign(&index_json(package));
    let net = host_with(&s, package);
    let index = net
        .fetch_material_index("https://example.org/index.json", &[&s.public_key])
        .unwrap();
    assert_eq!(index.packages.len(), 1);
    let p = &index.packages[0];
    assert_eq!(p.id, "community.vinyl");
    assert_eq!(p.version, v("1.2.0"));
    assert_eq!(p.license, "CC0-1.0");
    assert_eq!(
        net.client().asked(),
        vec![
            "https://example.org/index.json".to_owned(),
            "https://example.org/index.json.minisig".to_owned()
        ]
    );
}

#[test]
fn an_index_that_was_changed_after_signing_is_refused() {
    let package = b"PK-fake-package";
    let s = sign(&index_json(package));
    let tampered = String::from_utf8(s.index.clone())
        .unwrap()
        .replace("community.vinyl", "community.evil");
    let client = Fake::default()
        .with("https://example.org/index.json", tampered)
        .with(
            "https://example.org/index.json.minisig",
            s.signature.clone(),
        );
    let net = Net::new(client, on());
    assert_eq!(
        net.fetch_material_index("https://example.org/index.json", &[&s.public_key]),
        Err(NetError::BadSignature)
    );
}

#[test]
fn an_index_signed_by_a_different_key_is_refused() {
    let package = b"x";
    let s = sign(&index_json(package));
    let other = KeyPair::generate_unencrypted_keypair()
        .unwrap()
        .pk
        .to_box()
        .unwrap()
        .to_string();
    let net = host_with(&s, package);
    assert_eq!(
        net.fetch_material_index("https://example.org/index.json", &[&other]),
        Err(NetError::BadSignature)
    );
}

#[test]
fn a_missing_signature_is_refused_not_skipped() {
    let s = sign(&index_json(b"x"));
    let client = Fake::default().with("https://example.org/index.json", s.index.clone());
    let net = Net::new(client, on());
    assert!(matches!(
        net.fetch_material_index("https://example.org/index.json", &[&s.public_key]),
        Err(NetError::Http(HttpError::Status(404)))
    ));
}

#[test]
fn a_garbled_signature_or_key_is_refused() {
    let s = sign(&index_json(b"x"));
    let client = Fake::default()
        .with("https://example.org/index.json", s.index.clone())
        .with("https://example.org/index.json.minisig", "garbage");
    let net = Net::new(client, on());
    assert_eq!(
        net.fetch_material_index("https://example.org/index.json", &[&s.public_key]),
        Err(NetError::BadSignature)
    );
    let net = host_with(&s, b"x");
    assert_eq!(
        net.fetch_material_index("https://example.org/index.json", &["not a key"]),
        Err(NetError::BadKey)
    );
}

#[test]
fn a_signed_but_malformed_index_is_reported() {
    let s = sign("{\"schema\":1}");
    let net = host_with(&s, b"x");
    assert!(matches!(
        net.fetch_material_index("https://example.org/index.json", &[&s.public_key]),
        Err(NetError::Malformed(_))
    ));
}

#[test]
fn package_links_that_are_not_https_are_refused() {
    let json = r#"{"schema":1,"generated":"g","packages":[
        {"id":"a","name":"A","version":"1.0.0","kind":"material","url":"http://x/a.zip",
         "sha256":"00","size":1,"license":"MIT"}]}"#;
    let s = sign(json);
    let net = host_with(&s, b"x");
    assert!(matches!(
        net.fetch_material_index("https://example.org/index.json", &[&s.public_key]),
        Err(NetError::InsecureUrl(_))
    ));
}

// ---- packages ----

fn first_entry(net: &Net<Fake>, s: &Signed) -> PackageEntry {
    net.fetch_material_index("https://example.org/index.json", &[&s.public_key])
        .unwrap()
        .packages
        .remove(0)
}

#[test]
fn a_package_whose_bytes_match_the_signed_hash_is_returned() {
    let package = b"PK-fake-package";
    let s = sign(&index_json(package));
    let net = host_with(&s, package);
    let entry = first_entry(&net, &s);
    assert_eq!(net.fetch_package(&entry).unwrap(), package);
}

#[test]
fn a_package_that_does_not_match_the_signed_hash_is_refused() {
    let s = sign(&index_json(b"PK-real-package"));
    let net = host_with(&s, b"PK-other-bytes!");
    let entry = first_entry(&net, &s);
    assert_eq!(
        net.fetch_package(&entry),
        Err(NetError::HashMismatch {
            id: "community.vinyl".to_owned()
        })
    );
}

#[test]
fn a_package_bigger_than_the_index_says_is_never_fully_downloaded() {
    let s = sign(&index_json(b"tiny"));
    let big = vec![0_u8; 10_000];
    let net = host_with(&s, &big);
    let entry = first_entry(&net, &s);
    assert_eq!(
        net.fetch_package(&entry),
        Err(NetError::Http(HttpError::TooLarge))
    );
}

#[test]
fn fetching_a_package_needs_the_opt_in_too() {
    let package = b"PK-fake-package";
    let s = sign(&index_json(package));
    let entry = first_entry(&host_with(&s, package), &s);
    let off = Net::new(host_with(&s, package).into_client(), Network::default());
    assert_eq!(
        off.fetch_package(&entry),
        Err(NetError::Disabled(Feature::MaterialIndex))
    );
}

#[test]
fn the_hash_is_compared_case_insensitively() {
    let package = b"PK-fake-package";
    let json =
        index_json(package).replace(&sha256_hex(package), &sha256_hex(package).to_uppercase());
    let s = sign(&json);
    let net = host_with(&s, package);
    let entry = first_entry(&net, &s);
    assert!(net.fetch_package(&entry).is_ok());
}

// ---- limits ----

#[test]
fn the_feed_and_index_are_size_limited() {
    let big = format!(
        r#"{{"schema":1,"releases":[],"pad":"{}"}}"#,
        "x".repeat(MAX_METADATA_BYTES)
    );
    let net = Net::new(
        Fake::default().with("https://example.org/feed.json", big),
        on(),
    );
    assert_eq!(
        net.check_for_update("https://example.org/feed.json", &v("1.0.0")),
        Err(NetError::Http(HttpError::TooLarge))
    );
}

// ---- the real client ----

#[test]
fn the_real_client_refuses_plain_http_without_connecting() {
    let client = UreqClient::new();
    assert_eq!(
        client.get("http://127.0.0.1:1/never", 10),
        Err(HttpError::Transport("only https is allowed".to_owned()))
    );
}

#[test]
fn either_the_primary_or_the_backup_key_may_have_signed_the_index() {
    let package = b"x";
    let s = sign(&index_json(package));
    let primary = KeyPair::generate_unencrypted_keypair()
        .unwrap()
        .pk
        .to_box()
        .unwrap()
        .to_string();
    let net = host_with(&s, package);
    // The signer is the second (backup) key.
    assert!(
        net.fetch_material_index("https://example.org/index.json", &[&primary, &s.public_key])
            .is_ok()
    );
    // Neither key signed it.
    let other = KeyPair::generate_unencrypted_keypair()
        .unwrap()
        .pk
        .to_box()
        .unwrap()
        .to_string();
    assert_eq!(
        net.fetch_material_index("https://example.org/index.json", &[&primary, &other]),
        Err(NetError::BadSignature)
    );
}

#[test]
fn an_unreadable_key_is_skipped_when_another_is_valid() {
    let package = b"x";
    let s = sign(&index_json(package));
    let net = host_with(&s, package);
    assert!(
        net.fetch_material_index(
            "https://example.org/index.json",
            &["junk", s.public_key.as_str()]
        )
        .is_ok()
    );
}

#[test]
fn the_index_entry_carries_category_brand_machines_and_tools_when_given() {
    let json = r#"{"schema":1,"generated":"g","packages":[
        {"id":"a","name":"A","version":"1.0.0","kind":"material","url":"https://x/a.zip",
         "sha256":"00","size":1,"license":"CC0-1.0","category":"vinyl","brand":"Acme",
         "machines":["generic.cutter-small"],"tools":["generic.drag-blade"]}]}"#;
    let s = sign(json);
    let net = host_with(&s, b"x");
    let index = net
        .fetch_material_index("https://example.org/index.json", &[&s.public_key])
        .unwrap();
    let p = &index.packages[0];
    assert_eq!((p.category.as_str(), p.brand.as_str()), ("vinyl", "Acme"));
    assert_eq!(p.machines, vec!["generic.cutter-small"]);
    assert_eq!(p.tools, vec!["generic.drag-blade"]);
}
