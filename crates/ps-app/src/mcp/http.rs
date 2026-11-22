//! The Streamable HTTP transport of the MCP server.
//!
//! One endpoint, `POST /mcp`, takes one JSON-RPC message per request and answers with the JSON
//! reply (`200`), or with `202 Accepted` and no body when the message was a notification. The
//! server keeps no sessions and makes no server-initiated messages, so `GET` and `DELETE` are
//! refused with `405`, which the protocol allows.
//!
//! Because the tools can drive real hardware, the server is closed by default:
//!
//! - It listens on this computer only unless the user allows the network.
//! - Every request needs `Authorization: Bearer <token>`.
//! - A request is refused when its `Host` is not a local name (while local only) or when it carries
//!   an `Origin` from another site, so a web page cannot reach the server through the browser,
//!   not even by DNS rebinding.
//!
//! The server only moves bytes. Messages go to the window through a channel and are answered
//! between frames, exactly as the stdio transport once did, because the app state belongs to the
//! window's thread.

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use subtle::ConstantTimeEq;
use tiny_http::{Header, Method, Response, Server};

use super::Request;

/// The path the server answers on.
pub const ENDPOINT: &str = "/mcp";

/// The largest message accepted, in bytes. Tool arguments are small; this stops a client from
/// making the app buffer something huge.
const MAX_BODY_BYTES: u64 = 4 * 1024 * 1024;

/// Threads answering requests, so one slow tool call does not hold up a client's next one.
const WORKERS: usize = 4;

/// How often an idle worker looks up from waiting to see whether the server was stopped.
const POLL: Duration = Duration::from_millis(200);

/// How the server listens.
#[derive(Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// The TCP port; 0 lets the system choose one (the tests do).
    pub port: u16,
    /// Listen on every network interface instead of this computer only.
    pub allow_network: bool,
    /// The secret clients send as a bearer token. Must not be empty.
    pub token: String,
    /// How long a request waits for the window to answer before the client gets a `504`.
    pub reply_timeout: Duration,
}

impl ServerConfig {
    /// A configuration with the usual reply timeout.
    #[must_use]
    pub fn new(port: u16, allow_network: bool, token: String) -> Self {
        Self {
            port,
            allow_network,
            token,
            reply_timeout: Duration::from_secs(30),
        }
    }
}

// By hand: the token is a secret and must stay out of logs and panic reports.
impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("port", &self.port)
            .field("allow_network", &self.allow_network)
            .field("token", &"<hidden>")
            .field("reply_timeout", &self.reply_timeout)
            .finish()
    }
}

/// A running server. Dropping it stops listening and waits for its threads to finish.
pub struct HttpServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for HttpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpServer")
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

impl HttpServer {
    /// Starts listening. Requests for the app arrive on the returned receiver; `wake` is called
    /// whenever one is queued so the window can come and serve it (it should ask for a repaint).
    ///
    /// # Errors
    ///
    /// A message when the token is empty or the port cannot be used (for example, it is taken).
    pub fn start(
        config: &ServerConfig,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<(Self, mpsc::Receiver<Request>), String> {
        if config.token.is_empty() {
            return Err("the server needs an access token".to_owned());
        }
        let ip = if config.allow_network {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        };
        let server = bind(SocketAddr::new(ip, config.port))
            .map_err(|e| format!("cannot listen on port {}: {e}", config.port))?;
        let addr = server
            .server_addr()
            .to_ip()
            .ok_or_else(|| "the server has no IP address".to_owned())?;
        let server = Arc::new(server);
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel::<Request>();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let shared = Arc::new(Shared {
            token: config.token.clone(),
            allow_network: config.allow_network,
            reply_timeout: config.reply_timeout,
        });
        let workers = (0..WORKERS)
            .map(|_| {
                let (server, stop, tx, wake, shared) = (
                    Arc::clone(&server),
                    Arc::clone(&stop),
                    tx.clone(),
                    Arc::clone(&wake),
                    Arc::clone(&shared),
                );
                std::thread::spawn(move || worker(&server, &stop, &tx, wake.as_ref(), &shared))
            })
            .collect();
        tracing::info!(%addr, "the automation server is listening");
        Ok((
            Self {
                addr,
                stop,
                workers,
            },
            rx,
        ))
    }

    /// The address the server listens on (with the real port when 0 was asked for).
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for worker in self.workers.drain(..) {
            // A worker that panicked has nothing left to clean up.
            let _ = worker.join();
        }
        tracing::info!(addr = %self.addr, "the automation server stopped");
    }
}

/// How many times binding is tried while the address is still in use, and how long to wait
/// between tries.
const BIND_ATTEMPTS: u32 = 5;
const BIND_RETRY: Duration = Duration::from_millis(100);

/// Opens the listening socket. A server that was just stopped releases its port a moment later, so
/// an address that is in use is tried again for a second or so before it is reported as taken;
/// any other failure is reported at once.
fn bind(addr: SocketAddr) -> Result<Server, String> {
    let mut failure = String::new();
    for _ in 0..BIND_ATTEMPTS {
        match Server::http(addr) {
            Ok(server) => return Ok(server),
            Err(e) => {
                let in_use = e
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::AddrInUse);
                failure = e.to_string();
                if !in_use {
                    break;
                }
                std::thread::sleep(BIND_RETRY);
            }
        }
    }
    Err(failure)
}

/// What every worker needs to judge a request.
struct Shared {
    token: String,
    allow_network: bool,
    reply_timeout: Duration,
}

fn worker(
    server: &Server,
    stop: &AtomicBool,
    tx: &Sender<Request>,
    wake: &(dyn Fn() + Send + Sync),
    shared: &Shared,
) {
    while !stop.load(Ordering::Relaxed) {
        match server.recv_timeout(POLL) {
            Ok(Some(request)) => handle(request, tx, wake, shared),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(error = %e, "the automation server stopped listening");
                break;
            }
        }
    }
}

/// The first header called `name`, whatever the case it was sent in.
fn header_value<'a>(request: &'a tiny_http::Request, name: &'static str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str())
}

/// A plain-text reply with a status.
fn text(status: u16, message: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(message).with_status_code(status)
}

fn with_header<R: Read>(response: Response<R>, name: &str, value: &str) -> Response<R> {
    match Header::from_bytes(name.as_bytes(), value.as_bytes()) {
        Ok(header) => response.with_header(header),
        // Only a name or value that is not ASCII can fail, and every caller passes constants.
        Err(()) => response,
    }
}

/// The host part of a `Host` header or of an origin's authority, without the port: `localhost`,
/// `127.0.0.1` or `[::1]`.
fn host_of(authority: &str) -> &str {
    if authority.starts_with('[') {
        authority
            .find(']')
            .map_or(authority, |end| &authority[..=end])
    } else {
        authority
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host)
    }
}

fn is_local_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "[::1]"
    )
}

/// The host of an `Origin` header (`https://example.org:8080` gives `example.org`); `None` for a
/// value that is not a URL origin, such as the literal `null`.
fn origin_host(origin: &str) -> Option<&str> {
    let (_, rest) = origin.split_once("://")?;
    Some(host_of(rest.split('/').next().unwrap_or(rest)))
}

/// Whether the token in an `Authorization` header is the right one. The comparison takes the same
/// time wherever the two differ.
fn token_matches(header: Option<&str>, token: &str) -> bool {
    let Some(sent) = header.and_then(|h| h.strip_prefix("Bearer ")) else {
        return false;
    };
    sent.as_bytes().ct_eq(token.as_bytes()).into()
}

fn handle(
    mut request: tiny_http::Request,
    tx: &Sender<Request>,
    wake: &(dyn Fn() + Send + Sync),
    shared: &Shared,
) {
    let response = answer(&mut request, tx, wake, shared);
    if let Err(e) = request.respond(response) {
        tracing::debug!(error = %e, "a client went away before its answer");
    }
}

fn answer(
    request: &mut tiny_http::Request,
    tx: &Sender<Request>,
    wake: &(dyn Fn() + Send + Sync),
    shared: &Shared,
) -> Response<std::io::Cursor<Vec<u8>>> {
    // A request for another name than this computer's own is a web page being tricked into
    // talking to the server. Names are not knowable once the network is allowed; the token is
    // what protects the server then.
    let host = header_value(request, "Host").map_or("", host_of);
    if !shared.allow_network && !is_local_host(host) {
        return text(
            403,
            "this server only answers requests addressed to this computer",
        );
    }
    if let Some(origin) = header_value(request, "Origin") {
        let same_site =
            origin_host(origin).is_some_and(|o| is_local_host(o) || o.eq_ignore_ascii_case(host));
        if !same_site {
            return text(403, "requests from other websites are not accepted");
        }
    }
    if !token_matches(header_value(request, "Authorization"), &shared.token) {
        tracing::warn!(from = ?request.remote_addr(), "a request without the right access token");
        return with_header(
            text(401, "a valid access token is required"),
            "WWW-Authenticate",
            "Bearer realm=\"plotter-studio\"",
        );
    }
    let path = request.url().split('?').next().unwrap_or("");
    if path != ENDPOINT {
        return text(404, "the MCP endpoint is /mcp");
    }
    if *request.method() != Method::Post {
        return with_header(
            text(405, "send JSON-RPC messages with POST"),
            "Allow",
            "POST",
        );
    }
    let mut body = Vec::new();
    let read = request
        .as_reader()
        .take(MAX_BODY_BYTES + 1)
        .read_to_end(&mut body);
    if read.is_err() {
        return text(400, "the request body could not be read");
    }
    if u64::try_from(body.len()).is_ok_and(|n| n > MAX_BODY_BYTES) {
        return text(413, "the message is too large");
    }
    let Ok(line) = String::from_utf8(body) else {
        return text(400, "the message is not UTF-8");
    };

    let (reply_tx, reply_rx) = channel::<String>();
    if tx
        .send(Request {
            line,
            reply: reply_tx,
        })
        .is_err()
    {
        return text(503, "the application is shutting down");
    }
    wake();
    match reply_rx.recv_timeout(shared.reply_timeout) {
        Ok(reply) => with_header(
            with_header(
                Response::from_string(reply).with_status_code(200),
                "Content-Type",
                "application/json",
            ),
            "Cache-Control",
            "no-store",
        ),
        // The window served the message and had no reply: it was a notification.
        Err(RecvTimeoutError::Disconnected) => Response::from_string("").with_status_code(202),
        Err(RecvTimeoutError::Timeout) => text(504, "the application did not answer in time"),
    }
}

#[cfg(test)]
mod tests;
