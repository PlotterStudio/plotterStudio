//! Ground truth for running the server from the settings: it follows the settings, reports how it
//! is doing, and a request from a client really reaches the app.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use ps_library::{Automation, Settings};

use super::*;

fn wake() -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(|| {})
}

fn app_with(automation: Automation) -> App {
    let settings = Settings {
        automation,
        ..Settings::default()
    };
    App::new(settings, "en")
}

fn on(port: u16) -> Automation {
    Automation {
        mcp_enabled: true,
        mcp_port: port,
        mcp_allow_network: false,
        mcp_token: "token-for-tests".to_owned(),
    }
}

fn port_of(status: &McpStatus) -> u16 {
    let McpStatus::Listening { urls } = status else {
        panic!("expected Listening, got {status:?}");
    };
    urls[0]
        .rsplit_once(':')
        .and_then(|(_, rest)| rest.split('/').next())
        .and_then(|p| p.parse().ok())
        .expect("a port in the URL")
}

#[test]
fn the_urls_for_this_computer_only_are_the_loopback_address() {
    let lan = [IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20))];
    assert_eq!(
        endpoint_urls(&lan, 8765, false),
        vec!["http://127.0.0.1:8765/mcp".to_owned()],
        "other addresses are not offered while the network is not allowed"
    );
}

#[test]
fn with_the_network_allowed_every_address_is_listed_before_the_loopback_one() {
    let addresses = [
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)),
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)),
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    ];
    assert_eq!(
        endpoint_urls(&addresses, 9100, true),
        vec![
            "http://192.168.1.20:9100/mcp".to_owned(),
            "http://10.0.0.7:9100/mcp".to_owned(),
            "http://127.0.0.1:9100/mcp".to_owned(),
        ]
    );
}

#[test]
fn a_token_is_sixty_four_hex_characters_and_never_repeats() {
    let a = generate_token();
    let b = generate_token();
    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "{a}");
    assert_ne!(a, b);
}

#[test]
fn nothing_runs_while_the_setting_is_off() {
    let mut app = app_with(Automation::default());
    let mut host = AutomationHost::new();
    host.update(&mut app, &wake(), 0);
    assert_eq!(*app.mcp_status(), McpStatus::Off);
    assert!(host.server.is_none());
}

#[test]
fn turning_it_on_starts_the_server_and_lists_where_to_connect() {
    let mut app = app_with(on(0));
    let mut host = AutomationHost::new();
    host.update(&mut app, &wake(), 0);
    let port = port_of(app.mcp_status());
    assert_ne!(port, 0);
    assert_eq!(
        *app.mcp_status(),
        McpStatus::Listening {
            urls: vec![format!("http://127.0.0.1:{port}/mcp")]
        }
    );
}

#[test]
fn turning_it_on_without_a_token_makes_one_and_keeps_it() {
    let mut app = app_with(Automation {
        mcp_token: String::new(),
        ..on(0)
    });
    let mut host = AutomationHost::new();
    host.update(&mut app, &wake(), 0);
    let token = app.state().settings().automation.mcp_token.clone();
    assert_eq!(token.len(), 64, "a token was generated");
    assert!(
        app.state_mut().take_settings_changed(),
        "and it will be saved"
    );
    host.update(&mut app, &wake(), 0);
    assert_eq!(app.state().settings().automation.mcp_token, token);
}

#[test]
fn turning_it_off_stops_the_server_and_frees_the_port() {
    let mut app = app_with(on(0));
    let mut host = AutomationHost::new();
    host.update(&mut app, &wake(), 0);
    let port = port_of(app.mcp_status());
    app.state_mut()
        .update_settings(|s| s.automation.mcp_enabled = false);
    host.update(&mut app, &wake(), 0);
    assert_eq!(*app.mcp_status(), McpStatus::Off);
    // The listener closes a moment after the server stops; wait for that, not for an instant.
    let gone = (0..40).any(|_| {
        let refused = TcpStream::connect_timeout(
            &SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_millis(100),
        )
        .is_err();
        if !refused {
            std::thread::sleep(Duration::from_millis(50));
        }
        refused
    });
    assert!(gone, "nobody is listening any more");
}

#[test]
fn a_port_that_is_taken_is_reported_and_not_retried_every_frame() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let mut app = app_with(on(port));
    let mut host = AutomationHost::new();
    host.update(&mut app, &wake(), 0);
    let McpStatus::Failed(reason) = app.mcp_status().clone() else {
        panic!("expected Failed, got {:?}", app.mcp_status());
    };
    assert!(reason.contains(&port.to_string()), "{reason}");
    // Frames go by; the failure stands and nothing is retried (this must return at once).
    let started = Instant::now();
    for _ in 0..50 {
        host.update(&mut app, &wake(), 0);
    }
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(matches!(app.mcp_status(), McpStatus::Failed(_)));
    // Choosing another port starts it.
    drop(taken);
    app.state_mut()
        .update_settings(|s| s.automation.mcp_port = 0);
    host.update(&mut app, &wake(), 0);
    assert!(matches!(app.mcp_status(), McpStatus::Listening { .. }));
}

#[test]
fn changing_the_port_restarts_the_server_on_the_new_one() {
    let mut app = app_with(on(0));
    let mut host = AutomationHost::new();
    host.update(&mut app, &wake(), 0);
    let first = port_of(app.mcp_status());
    // Ask for a specific free port this time.
    let free = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap().port()
    };
    app.state_mut()
        .update_settings(|s| s.automation.mcp_port = free);
    host.update(&mut app, &wake(), 0);
    assert_eq!(port_of(app.mcp_status()), free);
    assert_ne!(first, free);
}

#[test]
fn a_client_request_is_answered_by_the_app() {
    let mut app = app_with(on(0));
    let mut host = AutomationHost::new();
    host.update(&mut app, &wake(), 0);
    let port = port_of(app.mcp_status());
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let client = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer token-for-tests\r\n\
             Connection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        raw
    });
    // The window's frames: keep updating until the client has its answer.
    let deadline = Instant::now() + Duration::from_secs(20);
    while !client.is_finished() && Instant::now() < deadline {
        host.update(&mut app, &wake(), 0);
        std::thread::sleep(Duration::from_millis(10));
    }
    let raw = client.join().unwrap();
    assert!(raw.starts_with("HTTP/1.1 200"), "{raw}");
    assert!(
        raw.contains("\"tools\""),
        "the app's tool list came back: {raw}"
    );
}
