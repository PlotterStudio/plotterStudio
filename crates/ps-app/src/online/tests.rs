//! Ground truth for the app's internet features: the release feed and the signed
//! material index, both off until the user opts in, run off the UI thread so nothing blocks. A
//! fake HTTP client stands in for the network, and a request is never made while the feature is
//! off.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ps_library::Network;
use ps_net::{Http, HttpError};
use semver::Version;

use super::*;

#[derive(Default)]
struct Fake {
    pages: BTreeMap<String, Vec<u8>>,
    asked: Mutex<Vec<String>>,
}

impl Http for Fake {
    fn get(&self, url: &str, _max_bytes: usize) -> Result<Vec<u8>, HttpError> {
        self.asked.lock().unwrap().push(url.to_owned());
        self.pages.get(url).cloned().ok_or(HttpError::Status(404))
    }
}

const FEED: &str = r#"{"schema":1,"releases":[
  {"version":"9.0.0","date":"2030-01-01","notes":"big","url":"https://example.org/9.0.0"}]}"#;

fn on() -> Network {
    Network {
        update_check: true,
        material_index: true,
    }
}

fn online(client: Fake, network: Network) -> (OnlineController, Arc<Fake>) {
    let client = Arc::new(client);
    let controller = OnlineController::new(
        Arc::clone(&client) as Arc<dyn Http + Send + Sync>,
        Version::new(1, 0, 0),
        "https://example.org/feed.json".to_owned(),
        "https://example.org/index.json".to_owned(),
        Vec::new(),
        network,
    );
    (controller, client)
}

fn wait(c: &mut OnlineController, done: impl Fn(&OnlineController) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(c) {
        c.poll();
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn nothing_is_requested_while_the_features_are_off() {
    let (mut c, client) = online(Fake::default(), Network::default());
    assert_eq!(c.check_for_update(), Err(OnlineError::Disabled));
    assert_eq!(c.refresh_material_index(), Err(OnlineError::Disabled));
    c.poll();
    assert_eq!(client.asked.lock().unwrap().len(), 0, "no request was made");
}

#[test]
fn a_newer_release_is_found_in_the_background() {
    let mut fake = Fake::default();
    fake.pages.insert(
        "https://example.org/feed.json".to_owned(),
        FEED.as_bytes().to_vec(),
    );
    let (mut c, _) = online(fake, on());
    assert!(c.update().is_none());
    c.check_for_update().unwrap();
    assert!(c.checking(), "the request is in flight");
    wait(&mut c, |c| !c.checking());
    let release = c.update().expect("an update");
    assert_eq!(release.version, Version::new(9, 0, 0));
    assert_eq!(release.url, "https://example.org/9.0.0");
    assert!(c.last_error().is_none());
}

#[test]
fn an_up_to_date_app_finds_no_update() {
    let mut fake = Fake::default();
    fake.pages.insert(
        "https://example.org/feed.json".to_owned(),
        br#"{"schema":1,"releases":[]}"#.to_vec(),
    );
    let (mut c, _) = online(fake, on());
    c.check_for_update().unwrap();
    wait(&mut c, |c| !c.checking());
    assert!(c.update().is_none());
    assert!(c.last_error().is_none());
}

#[test]
fn a_failed_check_is_reported_and_the_app_carries_on() {
    let (mut c, _) = online(Fake::default(), on());
    c.check_for_update().unwrap();
    wait(&mut c, |c| !c.checking());
    assert!(c.update().is_none());
    assert!(
        c.last_error().unwrap().contains("404"),
        "{:?}",
        c.last_error()
    );
}

#[test]
fn a_second_check_while_one_is_running_is_refused() {
    let (mut c, _) = online(Fake::default(), on());
    c.check_for_update().unwrap();
    assert_eq!(c.check_for_update(), Err(OnlineError::Busy));
    wait(&mut c, |c| !c.checking());
    assert!(c.check_for_update().is_ok(), "fine again afterwards");
}

#[test]
fn turning_the_feature_off_stops_further_requests_and_hides_a_found_update() {
    let mut fake = Fake::default();
    fake.pages.insert(
        "https://example.org/feed.json".to_owned(),
        FEED.as_bytes().to_vec(),
    );
    let (mut c, client) = online(fake, on());
    c.check_for_update().unwrap();
    wait(&mut c, |c| !c.checking());
    assert!(c.update().is_some());
    c.set_network(Network::default());
    assert!(
        c.update().is_none(),
        "an update found under an old opt-in is not shown"
    );
    let before = client.asked.lock().unwrap().len();
    assert_eq!(c.check_for_update(), Err(OnlineError::Disabled));
    assert_eq!(client.asked.lock().unwrap().len(), before);
}

#[test]
fn the_material_index_needs_a_trusted_key() {
    let (mut c, _) = online(Fake::default(), on());
    c.refresh_material_index().unwrap();
    wait(&mut c, |c| !c.refreshing_index());
    assert!(c.index().is_none());
    assert!(
        c.last_error().unwrap().contains("key"),
        "{:?}",
        c.last_error()
    );
}

#[test]
fn the_trusted_keys_come_from_the_build() {
    // Without the build setting there are none, so an unsigned or wrongly signed index is
    // never accepted.
    assert_eq!(trusted_keys_from(None).len(), 0);
    assert_eq!(trusted_keys_from(Some("a;b ; ;c")), vec!["a", "b", "c"]);
}

#[test]
fn the_feed_and_index_links_are_https() {
    assert!(RELEASE_FEED_URL.starts_with("https://"));
    assert!(MATERIAL_INDEX_URL.starts_with("https://"));
}

// ---- downloading a listed package ----

fn listed(url: &str, bytes: &[u8]) -> PackageEntry {
    PackageEntry {
        id: "acme.glitter".into(),
        name: "Glitter card".into(),
        version: Version::new(1, 0, 0),
        kind: "material".into(),
        url: url.into(),
        sha256: ps_library::sha256_hex(bytes),
        size: u64::try_from(bytes.len()).unwrap(),
        license: "CC0-1.0".into(),
        category: String::new(),
        brand: String::new(),
        machines: Vec::new(),
        tools: Vec::new(),
    }
}

#[test]
fn a_package_is_not_downloaded_while_the_feature_is_off() {
    let (mut c, client) = online(Fake::default(), Network::default());
    let entry = listed("https://example.org/p.zip", b"data");
    assert_eq!(c.download_package(&entry), Err(OnlineError::Disabled));
    assert_eq!(client.asked.lock().unwrap().len(), 0);
}

#[test]
fn a_listed_package_is_downloaded_and_checked_against_its_hash() {
    let mut fake = Fake::default();
    fake.pages.insert(
        "https://example.org/p.zip".into(),
        b"package bytes".to_vec(),
    );
    let (mut c, _) = online(fake, on());
    let entry = listed("https://example.org/p.zip", b"package bytes");
    c.download_package(&entry).unwrap();
    assert!(c.downloading());
    wait(&mut c, |c| !c.downloading());
    let (id, bytes) = c.take_package().expect("downloaded");
    assert_eq!(id, "acme.glitter");
    assert_eq!(bytes, b"package bytes");
    assert!(c.take_package().is_none(), "taken once");
}

#[test]
fn bytes_that_do_not_match_the_signed_hash_are_refused() {
    let mut fake = Fake::default();
    fake.pages
        .insert("https://example.org/p.zip".into(), b"tampered".to_vec());
    let (mut c, _) = online(fake, on());
    let entry = listed("https://example.org/p.zip", b"original");
    c.download_package(&entry).unwrap();
    wait(&mut c, |c| !c.downloading());
    assert!(c.take_package().is_none());
    assert!(
        c.last_error().is_some_and(|e| e.contains("acme.glitter")),
        "{:?}",
        c.last_error()
    );
}

#[test]
fn only_one_download_runs_at_a_time() {
    let mut fake = Fake::default();
    fake.pages
        .insert("https://example.org/p.zip".into(), b"x".to_vec());
    let (mut c, _) = online(fake, on());
    let entry = listed("https://example.org/p.zip", b"x");
    c.download_package(&entry).unwrap();
    assert_eq!(c.download_package(&entry), Err(OnlineError::Busy));
    wait(&mut c, |c| !c.downloading());
}

// ---- a plugin from a link the user typed ----

#[test]
fn a_plugin_link_is_downloaded_even_with_the_internet_features_off() {
    let mut fake = Fake::default();
    fake.pages
        .insert("https://example.org/a.psdevice".into(), b"zip".to_vec());
    let (mut c, client) = online(fake, Network::default());
    c.download_plugin("https://example.org/a.psdevice").unwrap();
    assert!(c.plugin_downloading());
    wait(&mut c, |c| !c.plugin_downloading());
    assert_eq!(c.take_plugin(), Some(Ok(b"zip".to_vec())));
    assert_eq!(c.take_plugin(), None, "taken once");
    assert_eq!(client.asked.lock().unwrap().len(), 1);
}

#[test]
fn a_failed_plugin_download_is_reported_and_is_not_confused_with_the_update_check() {
    let (mut c, _) = online(Fake::default(), Network::default());
    c.download_plugin("https://example.org/missing.psdevice")
        .unwrap();
    wait(&mut c, |c| !c.plugin_downloading());
    let Some(Err(why)) = c.take_plugin() else {
        panic!("expected a failure");
    };
    assert!(why.contains("404"), "{why}");
    assert_eq!(c.last_error(), None, "the update row shows nothing");
}

#[test]
fn a_plugin_link_that_is_not_https_is_refused_in_the_background_too() {
    let (mut c, client) = online(Fake::default(), Network::default());
    c.download_plugin("http://example.org/a.psdevice").unwrap();
    wait(&mut c, |c| !c.plugin_downloading());
    assert!(matches!(c.take_plugin(), Some(Err(_))));
    assert!(client.asked.lock().unwrap().is_empty());
}

#[test]
fn only_one_plugin_download_runs_at_a_time() {
    let mut fake = Fake::default();
    fake.pages
        .insert("https://example.org/a.psdevice".into(), b"x".to_vec());
    let (mut c, _) = online(fake, Network::default());
    c.download_plugin("https://example.org/a.psdevice").unwrap();
    assert_eq!(
        c.download_plugin("https://example.org/a.psdevice"),
        Err(OnlineError::Busy)
    );
    wait(&mut c, |c| !c.plugin_downloading());
}
