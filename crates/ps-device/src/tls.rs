//! Pinned TLS and declared internet hosts.
//!
//! A machine on the local network may use a self-signed certificate. The first approved connection
//! pins that certificate's SHA-256 fingerprint; a later connection that presents a different one
//! is stopped. [`PinnedHttps`] is the HTTPS link to one machine (paths are relative to it, like
//! the plain HTTP link).
//!
//! A plugin reaches an internet host only through [`InternetAccess`]: the host must be named in
//! the plugin's manifest *and* allowed by the user, only HTTPS-style links are opened, IP literals
//! are refused (names only), and every host actually used is recorded so the machine panel can
//! show it.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use sha2::{Digest, Sha256};

use crate::host::DiscoveredDevice;
use crate::link::{is_local_address, parse_response, request_head, scoped, valid_path};
use crate::permissions::PermissionStore;
use crate::types::{DeviceError, TransportKind};
use crate::wasm::{BlockingTransport, HttpRequest, HttpResponse};
use crate::wasm_plugin::{SystemTransports, TransportFactory};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// The lowercase hex SHA-256 of a DER certificate: the pin.
#[must_use]
pub fn fingerprint(der: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(der)
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// What to do with a certificate a machine presented, given the pin stored for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinDecision {
    /// It matches the stored pin.
    Trust,
    /// Nothing is stored yet: pin this one (the first approved connection).
    FirstUse(String),
    /// It differs from the stored pin: stop and warn.
    Changed {
        /// The pinned fingerprint.
        expected: String,
        /// The one presented now.
        got: String,
    },
}

/// Compares an observed fingerprint with the stored pin.
#[must_use]
pub fn decide(stored: Option<&str>, observed: &str) -> PinDecision {
    match stored {
        None => PinDecision::FirstUse(observed.to_owned()),
        Some(pin) if pin.eq_ignore_ascii_case(observed) => PinDecision::Trust,
        Some(pin) => PinDecision::Changed {
            expected: pin.to_owned(),
            got: observed.to_owned(),
        },
    }
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

/// Called with the fingerprint of a certificate pinned for the first time.
type FirstUse = Arc<dyn Fn(&str) + Send + Sync>;

/// Accepts exactly one certificate: the pinned one, or (on first use) whatever is presented, which
/// is then recorded for the caller to store.
struct PinVerifier {
    expected: Option<String>,
    on_first_use: Option<FirstUse>,
    observed: Arc<Mutex<Option<String>>>,
    algorithms: WebPkiSupportedAlgorithms,
}

impl std::fmt::Debug for PinVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinVerifier")
            .field("expected", &self.expected)
            .finish_non_exhaustive()
    }
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let seen = fingerprint(end_entity.as_ref());
        *self.observed.lock().unwrap_or_else(PoisonError::into_inner) = Some(seen.clone());
        match decide(self.expected.as_deref(), &seen) {
            PinDecision::Trust => Ok(ServerCertVerified::assertion()),
            PinDecision::FirstUse(pin) => {
                if let Some(store) = &self.on_first_use {
                    store(&pin);
                }
                Ok(ServerCertVerified::assertion())
            }
            PinDecision::Changed { .. } => Err(rustls::Error::General(
                "the machine's certificate changed since it was pinned".to_owned(),
            )),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// Sends one request over a fresh TLS connection and parses the reply.
fn exchange(
    config: Arc<ClientConfig>,
    addr: SocketAddr,
    server_name: &str,
    head: &str,
    body: &[u8],
) -> Result<HttpResponse, String> {
    let name = ServerName::try_from(server_name.to_owned())
        .map_err(|e| format!("{server_name:?} is not a valid server name: {e}"))?;
    let conn = ClientConnection::new(config, name).map_err(|e| e.to_string())?;
    let tcp = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
        .map_err(|e| format!("cannot connect to {addr}: {e}"))?;
    tcp.set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    let mut tls = rustls::StreamOwned::new(conn, tcp);
    let tls_error = |e: std::io::Error| {
        // A refused certificate surfaces as an I/O error wrapping the TLS one.
        let text = e.to_string();
        if text.to_ascii_lowercase().contains("certificate") {
            format!("the machine's certificate was refused: {text}")
        } else {
            text
        }
    };
    tls.write_all(head.as_bytes()).map_err(tls_error)?;
    tls.write_all(body).map_err(tls_error)?;
    tls.flush().map_err(tls_error)?;
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        match tls.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&chunk[..n]);
                if raw.len() > MAX_RESPONSE_BYTES {
                    return Err("the response is too large".to_owned());
                }
            }
            // Servers that close without a TLS close_notify end the body here.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof && !raw.is_empty() => break,
            Err(e) => return Err(tls_error(e)),
        }
    }
    parse_response(&raw)
}

fn check_request(request: &HttpRequest) -> Result<(), String> {
    if !valid_path(&request.path) {
        return Err(format!("{:?} is not a path on the machine", request.path));
    }
    if request.method.is_empty() || !request.method.chars().all(|c| c.is_ascii_uppercase()) {
        return Err("the method must be upper-case letters".to_owned());
    }
    Ok(())
}

/// HTTPS to one machine on the local network, whose self-signed certificate is pinned.
pub struct PinnedHttps {
    addr: SocketAddr,
    server_name: String,
    config: Arc<ClientConfig>,
    observed: Arc<Mutex<Option<String>>>,
}

impl std::fmt::Debug for PinnedHttps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinnedHttps")
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

impl PinnedHttps {
    /// Aims HTTPS requests at a local machine. `pin` is the fingerprint stored for it, or `None`
    /// on the first approved connection (the presented certificate is then trusted and can be read
    /// back with [`observed_pin`](Self::observed_pin) to store).
    ///
    /// # Errors
    ///
    /// A message when the address is not local, the port is not approved, or TLS cannot be set up.
    pub fn connect(
        addr: SocketAddr,
        allowed_ports: &[u16],
        server_name: &str,
        pin: Option<String>,
    ) -> Result<Self, String> {
        Self::connect_with(addr, allowed_ports, server_name, pin, None)
    }

    /// Like [`connect`](Self::connect), and calls `on_first_use` with the fingerprint when there
    /// was no pin and the machine's certificate was trusted for the first time.
    ///
    /// # Errors
    ///
    /// The same as [`connect`](Self::connect).
    pub fn connect_with(
        addr: SocketAddr,
        allowed_ports: &[u16],
        server_name: &str,
        pin: Option<String>,
        on_first_use: Option<FirstUse>,
    ) -> Result<Self, String> {
        scoped(addr, allowed_ports)?;
        let provider = provider();
        let observed = Arc::new(Mutex::new(None));
        let verifier = PinVerifier {
            expected: pin,
            on_first_use,
            observed: Arc::clone(&observed),
            algorithms: provider.signature_verification_algorithms,
        };
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        Ok(Self {
            addr,
            server_name: server_name.to_owned(),
            config: Arc::new(config),
            observed,
        })
    }

    /// The fingerprint of the certificate the machine presented on the latest connection.
    #[must_use]
    pub fn observed_pin(&self) -> Option<String> {
        self.observed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl BlockingTransport for PinnedHttps {
    fn kind(&self) -> TransportKind {
        TransportKind::Http
    }

    fn write(&mut self, _data: &[u8]) -> Result<(), String> {
        Err("an HTTPS link takes requests, not raw bytes".to_owned())
    }

    fn read(&mut self, _max: usize, _timeout: Duration) -> Result<Vec<u8>, String> {
        Err("an HTTPS link takes requests, not raw bytes".to_owned())
    }

    fn http(&mut self, request: HttpRequest) -> Result<HttpResponse, String> {
        check_request(&request)?;
        let head = request_head(&self.addr.to_string(), &request)?;
        exchange(
            Arc::clone(&self.config),
            self.addr,
            &self.server_name,
            &head,
            &request.body,
        )
    }
}

// ---- pinning through the transport factory ----

/// Opens `https://ip:port` machines with a pinned certificate and everything else as the system
/// does. Pins live in the shared permission book, per plugin and device serial number; the first
/// approved connection stores the machine's certificate, and a different one later is refused.
pub struct PinningTransports {
    pins: Arc<Mutex<PermissionStore>>,
    plugin: String,
}

impl std::fmt::Debug for PinningTransports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinningTransports")
            .field("plugin", &self.plugin)
            .finish_non_exhaustive()
    }
}

impl PinningTransports {
    /// A factory that keeps `plugin`'s pins in `pins`.
    #[must_use]
    pub fn new(pins: Arc<Mutex<PermissionStore>>, plugin: &str) -> Self {
        Self {
            pins,
            plugin: plugin.to_owned(),
        }
    }
}

impl TransportFactory for PinningTransports {
    fn open(&self, device: &DiscoveredDevice) -> Result<Box<dyn BlockingTransport>, DeviceError> {
        let Some(rest) = device
            .address
            .as_deref()
            .and_then(|a| a.strip_prefix("https://"))
            .filter(|_| device.transport == TransportKind::Http)
        else {
            return SystemTransports.open(device);
        };
        let addr: SocketAddr = rest
            .trim_end_matches('/')
            .parse()
            .map_err(|_| DeviceError::new("bad-address", format!("{rest:?} is not host:port")))?;
        let serial = device.serial.clone().unwrap_or_else(|| device.id.0.clone());
        let stored = self
            .pins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pin(&self.plugin, &serial)
            .map(str::to_owned);
        let (pins, plugin, key) = (Arc::clone(&self.pins), self.plugin.clone(), serial);
        let remember: FirstUse = Arc::new(move |fingerprint: &str| {
            pins.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .set_pin(&plugin, &key, fingerprint);
        });
        PinnedHttps::connect_with(
            addr,
            &[addr.port()],
            &addr.ip().to_string(),
            stored,
            Some(remember),
        )
        .map(|t| Box::new(t) as Box<dyn BlockingTransport>)
        .map_err(|m| DeviceError::new("open-failed", m))
    }
}

// ---- declared internet hosts ----

/// Which internet hosts a plugin may reach: those its manifest names that the user allowed.
#[derive(Debug, Clone, Default)]
pub struct HostGate {
    declared: BTreeSet<String>,
    allowed: Arc<Mutex<BTreeSet<String>>>,
}

impl HostGate {
    /// A gate from a manifest's declared hosts and the user's allowed ones (case-insensitive).
    #[must_use]
    pub fn new(
        declared: impl IntoIterator<Item = String>,
        allowed: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            declared: declared
                .into_iter()
                .map(|h| h.to_ascii_lowercase())
                .collect(),
            allowed: Arc::new(Mutex::new(
                allowed
                    .into_iter()
                    .map(|h| h.to_ascii_lowercase())
                    .collect(),
            )),
        }
    }

    /// Allows (or blocks again) a host; a change reaches every clone of this gate.
    pub fn set_allowed(&self, host: &str, allowed: bool) {
        let mut set = self.allowed.lock().unwrap_or_else(PoisonError::into_inner);
        let host = host.to_ascii_lowercase();
        if allowed {
            set.insert(host);
        } else {
            set.remove(&host);
        }
    }

    fn check(&self, host: &str) -> Result<String, String> {
        let host = host.to_ascii_lowercase();
        if host.parse::<IpAddr>().is_ok() || host.is_empty() {
            return Err(format!(
                "{host:?} is not a host name: plugins reach internet hosts by name only"
            ));
        }
        if !self.declared.contains(&host) {
            return Err(format!("{host} is not declared in the plugin's manifest"));
        }
        if !self
            .allowed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&host)
        {
            return Err(format!("{host} has not been allowed by the user"));
        }
        Ok(host)
    }
}

/// Opens a link to an internet host the gate has already approved.
pub trait InternetOpener: Send + Sync {
    /// Opens the link.
    ///
    /// # Errors
    ///
    /// A message when the host cannot be reached.
    fn open(&self, host: &str) -> Result<Box<dyn BlockingTransport>, String>;
}

/// The gate, an opener and the record of which hosts were used.
#[derive(Clone)]
pub struct InternetAccess {
    gate: HostGate,
    opener: Arc<dyn InternetOpener>,
    used: Arc<Mutex<BTreeSet<String>>>,
}

impl std::fmt::Debug for InternetAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InternetAccess")
            .field("gate", &self.gate)
            .finish_non_exhaustive()
    }
}

impl InternetAccess {
    /// Access through `gate`, opening links with `opener`.
    #[must_use]
    pub fn new(gate: HostGate, opener: Arc<dyn InternetOpener>) -> Self {
        Self {
            gate,
            opener,
            used: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }

    /// Opens a link to a declared, allowed host and records that it is in use.
    ///
    /// # Errors
    ///
    /// A message when the host is not declared, not allowed, not a name, the link kind is not an
    /// HTTPS-style one, or the host cannot be reached.
    pub fn open(
        &self,
        host: &str,
        kind: TransportKind,
    ) -> Result<Box<dyn BlockingTransport>, String> {
        if !matches!(kind, TransportKind::Http | TransportKind::WebSocket) {
            return Err(format!("{kind:?} links are not opened to internet hosts"));
        }
        let host = self.gate.check(host)?;
        let link = self.opener.open(&host)?;
        self.used
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(host);
        Ok(link)
    }

    /// Allows or blocks a declared host, taking effect on the next open.
    pub fn set_allowed(&self, host: &str, allowed: bool) {
        self.gate.set_allowed(host, allowed);
    }

    /// The internet hosts used so far, sorted: what the machine panel shows.
    #[must_use]
    pub fn in_use(&self) -> Vec<String> {
        self.used
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }
}

/// HTTPS to a public host, verified against the public web roots and the host name.
#[derive(Debug, Clone, Copy, Default)]
pub struct PublicHttpsOpener;

struct PublicHttps {
    host: String,
    config: Arc<ClientConfig>,
}

impl InternetOpener for PublicHttpsOpener {
    fn open(&self, host: &str) -> Result<Box<dyn BlockingTransport>, String> {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config = ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Box::new(PublicHttps {
            host: host.to_owned(),
            config: Arc::new(config),
        }))
    }
}

impl BlockingTransport for PublicHttps {
    fn kind(&self) -> TransportKind {
        TransportKind::Http
    }

    fn write(&mut self, _data: &[u8]) -> Result<(), String> {
        Err("an HTTPS link takes requests, not raw bytes".to_owned())
    }

    fn read(&mut self, _max: usize, _timeout: Duration) -> Result<Vec<u8>, String> {
        Err("an HTTPS link takes requests, not raw bytes".to_owned())
    }

    fn http(&mut self, request: HttpRequest) -> Result<HttpResponse, String> {
        check_request(&request)?;
        let addr = std::net::ToSocketAddrs::to_socket_addrs(&(self.host.as_str(), 443))
            .map_err(|e| format!("cannot resolve {}: {e}", self.host))?
            .find(|a| !is_local_address(a.ip()))
            .ok_or_else(|| format!("{} resolves only to local addresses", self.host))?;
        let head = request_head(&self.host, &request)?;
        exchange(
            Arc::clone(&self.config),
            addr,
            &self.host,
            &head,
            &request.body,
        )
    }
}

#[cfg(test)]
mod tests;
