//! Ground truth for the HTTP transport: what a client on the wire gets back, and what the server
//! refuses to forward to the app. The tests use a real socket and a hand-written HTTP/1.1 client,
//! with the test thread playing the window that serves queued requests.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use ps_library::Settings;
use serde_json::{Value, json};

use super::*;
use crate::ui::App;

const TOKEN: &str = "test-token-0123456789";

fn start(allow_network: bool) -> (HttpServer, Receiver<Request>) {
    start_with(&ServerConfig::new(0, allow_network, TOKEN.to_owned()))
}

fn start_with(config: &ServerConfig) -> (HttpServer, Receiver<Request>) {
    HttpServer::start(config, || {}).expect("the server starts")
}

struct Reply {
    status: u16,
    headers: String,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<String> {
        self.headers.lines().find_map(|l| {
            let (n, v) = l.split_once(':')?;
            n.eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
        })
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.body).expect("a JSON body")
    }
}

/// One HTTP/1.1 request on a fresh connection.
fn http(addr: SocketAddr, method: &str, path: &str, extra: &[(&str, &str)], body: &str) -> Reply {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in extra {
        let _ = write!(head, "{name}: {value}\r\n");
    }
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(b"\r\n").unwrap();
    stream.write_all(body.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let raw = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let (status_line, headers) = head.split_once("\r\n").unwrap_or((head, ""));
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("a status code");
    Reply {
        status,
        headers: headers.to_owned(),
        body: body.to_owned(),
    }
}

fn authed(addr: SocketAddr) -> Vec<(&'static str, String)> {
    vec![
        ("Host", format!("127.0.0.1:{}", addr.port())),
        ("Authorization", format!("Bearer {TOKEN}")),
        ("Content-Type", "application/json".to_owned()),
    ]
}

fn headers_of<'a>(list: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    list.iter().map(|(n, v)| (*n, v.as_str())).collect()
}

/// Sends `message` to the endpoint with valid credentials while serving it on this thread, as the
/// window would, and returns what the client got.
fn exchange(
    server: &HttpServer,
    requests: &Receiver<Request>,
    app: &mut App,
    message: &Value,
) -> Reply {
    let addr = server.local_addr();
    let body = message.to_string();
    std::thread::scope(|scope| {
        let client = scope.spawn(move || {
            let list = authed(addr);
            http(addr, "POST", "/mcp", &headers_of(&list), &body)
        });
        let request = requests
            .recv_timeout(Duration::from_secs(20))
            .expect("the request reaches the app");
        request.serve(app, 0);
        client.join().expect("the client finished")
    })
}

fn app() -> App {
    App::new(Settings::default(), "en")
}

fn initialize() -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
           "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                      "clientInfo": {"name": "t", "version": "0"}}})
}

#[test]
fn a_message_with_the_token_reaches_the_app_and_its_reply_comes_back_as_json() {
    let (server, requests) = start(false);
    let reply = exchange(&server, &requests, &mut app(), &initialize());
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(
        reply.header("Content-Type").as_deref(),
        Some("application/json")
    );
    assert_eq!(
        reply.json()["result"]["serverInfo"]["name"],
        "plotter-studio"
    );
}

#[test]
fn a_tool_call_runs_in_the_app_and_returns_its_result() {
    let (server, requests) = start(false);
    let mut app = app();
    let reply = exchange(
        &server,
        &requests,
        &mut app,
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "query", "arguments": {"what": "document"}}}),
    );
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json()["result"]["isError"], false, "{}", reply.body);
}

#[test]
fn a_notification_is_accepted_without_a_body() {
    let (server, requests) = start(false);
    let reply = exchange(
        &server,
        &requests,
        &mut app(),
        &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    );
    assert_eq!(reply.status, 202);
    assert!(reply.body.is_empty(), "{}", reply.body);
}

#[test]
fn a_malformed_message_gets_a_json_rpc_error_not_an_http_one() {
    let (server, requests) = start(false);
    let addr = server.local_addr();
    let list = authed(addr);
    let reply = std::thread::scope(|scope| {
        let client = scope.spawn(|| http(addr, "POST", "/mcp", &headers_of(&list), "{not json"));
        requests
            .recv_timeout(Duration::from_secs(20))
            .unwrap()
            .serve(&mut app(), 0);
        client.join().unwrap()
    });
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json()["error"]["code"], -32700);
}

#[test]
fn a_request_without_the_right_token_is_refused_and_never_reaches_the_app() {
    let (server, requests) = start(false);
    let addr = server.local_addr();
    let host = format!("127.0.0.1:{}", addr.port());
    for auth in [
        None,
        Some("Bearer wrong"),
        Some("Basic dGVzdDp0ZXN0"),
        Some(TOKEN),
    ] {
        let mut headers = vec![("Host", host.as_str())];
        if let Some(value) = auth {
            headers.push(("Authorization", value));
        }
        let reply = http(addr, "POST", "/mcp", &headers, &initialize().to_string());
        assert_eq!(reply.status, 401, "{auth:?}");
        assert!(
            reply
                .header("WWW-Authenticate")
                .is_some_and(|v| v.starts_with("Bearer")),
            "{auth:?}"
        );
    }
    assert!(requests.try_recv().is_err(), "nothing was forwarded");
}

#[test]
fn while_local_only_a_request_for_another_host_name_is_refused() {
    let (server, requests) = start(false);
    let addr = server.local_addr();
    let auth = format!("Bearer {TOKEN}");
    // What a rebinding attack looks like: the browser sends the attacker's own name.
    let reply = http(
        addr,
        "POST",
        "/mcp",
        &[("Host", "evil.example:8765"), ("Authorization", &auth)],
        &initialize().to_string(),
    );
    assert_eq!(reply.status, 403);
    assert!(requests.try_recv().is_err());
}

#[test]
fn with_the_network_allowed_other_host_names_work_if_the_token_is_right() {
    let (server, requests) = start(true);
    let addr = SocketAddr::from(([127, 0, 0, 1], server.local_addr().port()));
    let body = initialize().to_string();
    let reply = std::thread::scope(|scope| {
        let client = scope.spawn(|| {
            http(
                addr,
                "POST",
                "/mcp",
                &[
                    ("Host", "192.168.1.20:8765"),
                    ("Authorization", &format!("Bearer {TOKEN}")),
                ],
                &body,
            )
        });
        requests
            .recv_timeout(Duration::from_secs(20))
            .unwrap()
            .serve(&mut app(), 0);
        client.join().unwrap()
    });
    assert_eq!(reply.status, 200, "{}", reply.body);
}

#[test]
fn a_web_page_on_another_site_cannot_use_the_server_even_with_the_token() {
    let (server, requests) = start(false);
    let addr = server.local_addr();
    let list = authed(addr);
    for origin in ["https://evil.example", "null", "http://evil.example:8765"] {
        let mut headers = headers_of(&list);
        headers.push(("Origin", origin));
        let reply = http(addr, "POST", "/mcp", &headers, &initialize().to_string());
        assert_eq!(reply.status, 403, "{origin}");
    }
    assert!(requests.try_recv().is_err());
}

#[test]
fn a_page_served_from_this_computer_is_not_turned_away_for_its_origin() {
    let (server, requests) = start(false);
    let addr = server.local_addr();
    let body = initialize().to_string();
    let list = authed(addr);
    let reply = std::thread::scope(|scope| {
        let client = scope.spawn(|| {
            let mut headers = headers_of(&list);
            headers.push(("Origin", "http://localhost:6274"));
            http(addr, "POST", "/mcp", &headers, &body)
        });
        requests
            .recv_timeout(Duration::from_secs(20))
            .unwrap()
            .serve(&mut app(), 0);
        client.join().unwrap()
    });
    assert_eq!(reply.status, 200, "{}", reply.body);
}

#[test]
fn only_post_to_the_mcp_path_is_served() {
    let (server, requests) = start(false);
    let addr = server.local_addr();
    let list = authed(addr);
    let headers = headers_of(&list);
    for method in ["GET", "DELETE", "PUT"] {
        let reply = http(addr, method, "/mcp", &headers, "");
        assert_eq!(reply.status, 405, "{method}");
        assert_eq!(reply.header("Allow").as_deref(), Some("POST"), "{method}");
    }
    assert_eq!(http(addr, "POST", "/other", &headers, "{}").status, 404);
    assert!(requests.try_recv().is_err());
}

#[test]
fn a_message_larger_than_the_limit_is_refused() {
    let (server, requests) = start(false);
    let addr = server.local_addr();
    let list = authed(addr);
    let huge = "x".repeat(usize::try_from(MAX_BODY_BYTES).unwrap() + 10);
    let reply = http(addr, "POST", "/mcp", &headers_of(&list), &huge);
    assert_eq!(reply.status, 413);
    assert!(requests.try_recv().is_err());
}

#[test]
fn a_window_that_does_not_answer_gives_the_client_a_timeout() {
    let mut config = ServerConfig::new(0, false, TOKEN.to_owned());
    config.reply_timeout = Duration::from_millis(200);
    let (server, _requests) = start_with(&config);
    let addr = server.local_addr();
    let list = authed(addr);
    let reply = http(
        addr,
        "POST",
        "/mcp",
        &headers_of(&list),
        &initialize().to_string(),
    );
    assert_eq!(reply.status, 504);
}

#[test]
fn the_server_listens_on_this_computer_only_unless_the_network_is_allowed() {
    let (local, _a) = start(false);
    assert!(local.local_addr().ip().is_loopback());
    let (open, _b) = start(true);
    assert!(open.local_addr().ip().is_unspecified());
}

#[test]
fn a_server_cannot_start_without_a_token() {
    let config = ServerConfig::new(0, false, String::new());
    assert!(HttpServer::start(&config, || {}).is_err());
}

#[test]
fn a_port_that_is_taken_is_reported_and_stopping_the_server_frees_it() {
    let (first, _requests) = start(false);
    let port = first.local_addr().port();
    let taken = HttpServer::start(&ServerConfig::new(port, false, TOKEN.to_owned()), || {});
    assert!(taken.is_err(), "the port is in use");
    drop(first);
    let again = HttpServer::start(&ServerConfig::new(port, false, TOKEN.to_owned()), || {});
    assert!(again.is_ok(), "stopping released the port");
}

#[test]
fn the_token_is_not_printed_by_debug() {
    let config = ServerConfig::new(0, false, TOKEN.to_owned());
    assert!(!format!("{config:?}").contains(TOKEN));
}

#[test]
fn host_and_origin_parsing_handles_ports_and_ipv6() {
    assert_eq!(host_of("localhost:8765"), "localhost");
    assert_eq!(host_of("127.0.0.1"), "127.0.0.1");
    assert_eq!(host_of("[::1]:8765"), "[::1]");
    assert!(is_local_host("LOCALHOST"));
    assert!(!is_local_host("localhost.evil.example"));
    assert_eq!(origin_host("https://example.org:8080"), Some("example.org"));
    assert_eq!(origin_host("http://[::1]:3000/path"), Some("[::1]"));
    assert_eq!(origin_host("null"), None);
}

#[test]
fn the_woken_window_is_told_when_a_request_is_queued() {
    use std::sync::atomic::AtomicUsize;
    let woken = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&woken);
    let (server, requests) =
        HttpServer::start(&ServerConfig::new(0, false, TOKEN.to_owned()), move || {
            counter.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();
    let reply = exchange(&server, &requests, &mut app(), &initialize());
    assert_eq!(reply.status, 200);
    assert_eq!(woken.load(Ordering::Relaxed), 1);
}
