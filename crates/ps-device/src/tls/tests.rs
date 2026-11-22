//! Ground truth for pinned TLS and declared internet hosts: a machine with a
//! self-signed certificate is pinned on the first approved connection and a changed certificate
//! stops the connection; a plugin reaches an internet host only when its manifest declares it and
//! the user has allowed it; every use is recorded so the machine panel can show it.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;

use super::*;

/// A one-shot HTTPS server with a fresh self-signed certificate. Returns its address and the
/// certificate's pin.
fn serve_once() -> (SocketAddr, String, std::thread::JoinHandle<()>) {
    let key = rcgen::generate_simple_self_signed(vec!["machine.local".to_owned()]).unwrap();
    let der = key.cert.der().to_vec();
    let pin = fingerprint(&der);
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(der)],
            rustls::pki_types::PrivateKeyDer::try_from(key.signing_key.serialize_der()).unwrap(),
        )
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let Ok((tcp, _)) = listener.accept() else {
            return;
        };
        let conn = rustls::ServerConnection::new(Arc::new(config)).unwrap();
        let mut tls = rustls::StreamOwned::new(conn, tcp);
        let mut buf = [0_u8; 2048];
        if tls.read(&mut buf).is_ok() {
            let _ = tls
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
            let _ = tls.flush();
        }
    });
    (addr, pin, handle)
}

fn get() -> HttpRequest {
    HttpRequest {
        method: "GET".to_owned(),
        path: "/status".to_owned(),
        headers: Vec::new(),
        body: Vec::new(),
    }
}

#[test]
fn a_fingerprint_is_the_lowercase_sha256_of_the_certificate() {
    // SHA-256 of the three bytes "abc".
    assert_eq!(
        fingerprint(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn the_first_certificate_is_trusted_and_remembered_and_a_changed_one_is_refused() {
    assert_eq!(decide(None, "aa"), PinDecision::FirstUse("aa".to_owned()));
    assert_eq!(decide(Some("aa"), "aa"), PinDecision::Trust);
    assert_eq!(
        decide(Some("aa"), "bb"),
        PinDecision::Changed {
            expected: "aa".to_owned(),
            got: "bb".to_owned()
        }
    );
}

#[test]
fn the_first_connection_pins_the_machines_certificate_and_serves_the_request() {
    let (addr, pin, server) = serve_once();
    let mut link = PinnedHttps::connect(addr, &[addr.port()], "machine.local", None).unwrap();
    let response = link.http(get()).unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"ok");
    assert_eq!(link.observed_pin().as_deref(), Some(pin.as_str()));
    server.join().unwrap();
}

#[test]
fn a_matching_pin_connects_and_a_different_one_is_refused_with_a_warning() {
    let (addr, pin, server) = serve_once();
    let mut ok = PinnedHttps::connect(addr, &[addr.port()], "machine.local", Some(pin)).unwrap();
    assert_eq!(ok.http(get()).unwrap().status, 200);
    server.join().unwrap();

    let (addr, _, server) = serve_once();
    let wrong = "0".repeat(64);
    let mut bad = PinnedHttps::connect(addr, &[addr.port()], "machine.local", Some(wrong)).unwrap();
    let error = bad.http(get()).unwrap_err();
    assert!(error.contains("certificate"), "{error}");
    let _ = server.join();
}

#[test]
fn a_pinned_link_only_goes_to_the_local_network_and_approved_ports() {
    let public: SocketAddr = "8.8.8.8:443".parse().unwrap();
    assert!(PinnedHttps::connect(public, &[443], "x", None).is_err());
    let local: SocketAddr = "192.168.1.20:443".parse().unwrap();
    assert!(PinnedHttps::connect(local, &[8443], "x", None).is_err());
}

#[test]
fn a_pinned_link_refuses_paths_that_name_another_host() {
    let (addr, _, server) = serve_once();
    let mut link = PinnedHttps::connect(addr, &[addr.port()], "machine.local", None).unwrap();
    let mut request = get();
    request.path = "//evil.example/x".to_owned();
    assert!(link.http(request).is_err());
    drop(server);
}

// ---- declared internet hosts ----

#[derive(Default)]
struct FakeOpener {
    opened: Mutex<Vec<String>>,
}

struct Null;

impl BlockingTransport for Null {
    fn kind(&self) -> TransportKind {
        TransportKind::Http
    }
    fn write(&mut self, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn read(&mut self, _: usize, _: Duration) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
    }
}

impl InternetOpener for FakeOpener {
    fn open(&self, host: &str) -> Result<Box<dyn BlockingTransport>, String> {
        self.opened.lock().unwrap().push(host.to_owned());
        Ok(Box::new(Null))
    }
}

fn gated(declared: &[&str], allowed: &[&str]) -> (InternetAccess, Arc<FakeOpener>) {
    let opener = Arc::new(FakeOpener::default());
    let gate = HostGate::new(
        declared.iter().map(|s| (*s).to_owned()),
        allowed.iter().map(|s| (*s).to_owned()),
    );
    (
        InternetAccess::new(gate, Arc::clone(&opener) as Arc<dyn InternetOpener>),
        opener,
    )
}

#[test]
fn a_declared_and_allowed_host_is_opened_and_recorded_as_in_use() {
    let (access, opener) = gated(&["api.vendor.example"], &["api.vendor.example"]);
    assert!(
        access
            .open("api.vendor.example", TransportKind::Http)
            .is_ok()
    );
    assert_eq!(
        *opener.opened.lock().unwrap(),
        vec!["api.vendor.example".to_owned()]
    );
    assert_eq!(access.in_use(), vec!["api.vendor.example".to_owned()]);
}

#[test]
fn a_declared_host_the_user_has_not_allowed_is_blocked() {
    let (access, opener) = gated(&["api.vendor.example"], &[]);
    let error = access
        .open("api.vendor.example", TransportKind::Http)
        .err()
        .unwrap();
    assert!(error.contains("not been allowed"), "{error}");
    assert!(opener.opened.lock().unwrap().is_empty());
    assert_eq!(access.in_use(), Vec::<String>::new());
}

#[test]
fn an_undeclared_host_is_refused_even_if_the_user_allowed_it() {
    let (access, opener) = gated(&["api.vendor.example"], &["other.example"]);
    let error = access
        .open("other.example", TransportKind::Http)
        .err()
        .unwrap();
    assert!(error.contains("not declared"), "{error}");
    assert!(opener.opened.lock().unwrap().is_empty());
}

#[test]
fn only_https_style_links_are_opened_to_internet_hosts() {
    let (access, _) = gated(&["a.example"], &["a.example"]);
    for kind in [
        TransportKind::Usb,
        TransportKind::Serial,
        TransportKind::Tcp,
        TransportKind::Udp,
    ] {
        assert!(access.open("a.example", kind).is_err(), "{kind:?}");
    }
}

#[test]
fn host_names_are_compared_without_case_and_ip_literals_are_refused() {
    let (access, _) = gated(&["API.Vendor.Example"], &["api.vendor.example"]);
    assert!(
        access
            .open("api.VENDOR.example", TransportKind::Http)
            .is_ok()
    );
    let (access, _) = gated(&["192.168.1.5"], &["192.168.1.5"]);
    let error = access
        .open("192.168.1.5", TransportKind::Http)
        .err()
        .unwrap();
    assert!(error.contains("name"), "{error}");
}

#[test]
fn a_host_in_use_is_listed_once() {
    let (access, _) = gated(&["a.example"], &["a.example"]);
    access.open("a.example", TransportKind::Http).unwrap();
    access.open("a.example", TransportKind::Http).unwrap();
    assert_eq!(access.in_use(), vec!["a.example".to_owned()]);
}
// ---- pinning through the transport factory ----

fn https_device(addr: SocketAddr, serial: &str) -> DiscoveredDevice {
    DiscoveredDevice {
        id: crate::host::DeviceId(format!("net:{addr}")),
        name: "Machine".to_owned(),
        transport: TransportKind::Http,
        usb: None,
        address: Some(format!("https://{addr}")),
        serial: Some(serial.to_owned()),
    }
}

fn book() -> Arc<Mutex<PermissionStore>> {
    Arc::new(Mutex::new(PermissionStore::default()))
}

#[test]
fn the_first_https_connection_pins_the_certificate_in_the_book_and_a_second_with_the_same_one_works()
 {
    use crate::wasm_plugin::TransportFactory as _;
    let (addr, pin, server) = serve_once();
    let pins = book();
    let factory = PinningTransports::new(Arc::clone(&pins), "vendor.net");
    let mut link = factory.open(&https_device(addr, "SN1")).unwrap();
    assert_eq!(link.http(get()).unwrap().status, 200);
    server.join().unwrap();
    assert_eq!(
        pins.lock().unwrap().pin("vendor.net", "SN1"),
        Some(pin.as_str())
    );
}

#[test]
fn a_machine_that_shows_a_different_certificate_later_is_stopped() {
    use crate::wasm_plugin::TransportFactory as _;
    let pins = book();
    pins.lock()
        .unwrap()
        .set_pin("vendor.net", "SN1", &"0".repeat(64));
    let (addr, _, server) = serve_once();
    let factory = PinningTransports::new(Arc::clone(&pins), "vendor.net");
    let mut link = factory.open(&https_device(addr, "SN1")).unwrap();
    let error = link.http(get()).unwrap_err();
    assert!(error.contains("certificate"), "{error}");
    // The old pin is kept: a changed certificate never replaces it silently.
    assert_eq!(
        pins.lock().unwrap().pin("vendor.net", "SN1"),
        Some("0".repeat(64).as_str())
    );
    let _ = server.join();
}

#[test]
fn pins_are_kept_per_device_serial() {
    use crate::wasm_plugin::TransportFactory as _;
    let (addr, pin, server) = serve_once();
    let pins = book();
    pins.lock()
        .unwrap()
        .set_pin("vendor.net", "OTHER", &"1".repeat(64));
    let factory = PinningTransports::new(Arc::clone(&pins), "vendor.net");
    let mut link = factory.open(&https_device(addr, "SN2")).unwrap();
    assert_eq!(link.http(get()).unwrap().status, 200);
    server.join().unwrap();
    assert_eq!(
        pins.lock().unwrap().pin("vendor.net", "SN2"),
        Some(pin.as_str())
    );
    assert_eq!(
        pins.lock().unwrap().pin("vendor.net", "OTHER"),
        Some("1".repeat(64).as_str())
    );
}

#[test]
fn a_plain_http_address_still_goes_to_the_ordinary_http_link() {
    use crate::wasm_plugin::TransportFactory as _;
    let mut device = https_device("127.0.0.1:8080".parse().unwrap(), "SN3");
    device.address = Some("127.0.0.1:8080".to_owned());
    let factory = PinningTransports::new(book(), "vendor.net");
    let link = factory.open(&device).unwrap();
    assert_eq!(link.kind(), TransportKind::Http);
}

#[test]
fn an_https_address_that_is_not_local_is_refused() {
    use crate::wasm_plugin::TransportFactory as _;
    let factory = PinningTransports::new(book(), "vendor.net");
    let device = https_device("8.8.8.8:443".parse().unwrap(), "SN4");
    assert!(factory.open(&device).is_err());
}
