//! Ground truth for the real byte links: a TCP link talks to a socket on loopback,
//! reads give up at their timeout with nothing rather than blocking, and a link is only opened to
//! an address and port the machine was approved for. Serial ports cannot be opened without a
//! device, so what is checked is that asking for one that is not there fails cleanly.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, UdpSocket};
use std::time::{Duration, Instant};

use super::*;
use crate::types::TransportKind;
use crate::wasm::BlockingTransport;
use crate::wasm::HttpRequest;

fn echo_server() -> (SocketAddr, std::thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        conn.write_all(b"hello\n").unwrap();
        let mut got = Vec::new();
        let mut buf = [0_u8; 64];
        loop {
            match conn.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    got.extend_from_slice(&buf[..n]);
                    if got.ends_with(b"\n") {
                        break;
                    }
                }
            }
        }
        got
    });
    (addr, handle)
}

#[test]
fn a_tcp_link_reads_and_writes_bytes() {
    let (addr, server) = echo_server();
    let mut link = TcpTransport::connect(addr, &[addr.port()]).unwrap();
    assert_eq!(link.kind(), TransportKind::Tcp);
    let mut got = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while got.len() < 6 && Instant::now() < deadline {
        got.extend(link.read(64, Duration::from_millis(200)).unwrap());
    }
    assert_eq!(got, b"hello\n");
    link.write(b"G21\n").unwrap();
    assert_eq!(server.join().unwrap(), b"G21\n");
}

#[test]
fn a_read_with_nothing_to_read_returns_empty_at_the_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mut link = TcpTransport::connect(addr, &[addr.port()]).unwrap();
    let started = Instant::now();
    let bytes = link.read(64, Duration::from_millis(100)).unwrap();
    assert_eq!(bytes, Vec::<u8>::new());
    assert!(started.elapsed() >= Duration::from_millis(90));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_port_the_machine_was_not_approved_for_is_refused_before_connecting() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let error = TcpTransport::connect(addr, &[addr.port().wrapping_add(1)]).unwrap_err();
    assert!(error.contains("port"), "{error}");
}

#[test]
fn an_address_that_is_not_the_machines_is_refused() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    // Only loopback, private-range and link-local addresses are machines on the user's network.
    let internet: SocketAddr = "8.8.8.8:80".parse().unwrap();
    assert!(TcpTransport::connect(internet, &[80]).is_err());
    assert!(is_local_address(addr.ip()));
    assert!(is_local_address("192.168.1.20".parse().unwrap()));
    assert!(is_local_address("10.0.0.5".parse().unwrap()));
    assert!(is_local_address("172.16.4.1".parse().unwrap()));
    assert!(is_local_address("169.254.1.1".parse().unwrap()));
    assert!(!is_local_address("172.32.0.1".parse().unwrap()));
    assert!(!is_local_address("8.8.8.8".parse().unwrap()));
    assert!(is_local_address("::1".parse().unwrap()));
    assert!(is_local_address("fe80::1".parse().unwrap()));
    assert!(is_local_address("fd00::1".parse().unwrap()));
    assert!(!is_local_address("2001:4860:4860::8888".parse().unwrap()));
}

#[test]
fn connecting_to_nothing_fails_with_a_message() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let error = TcpTransport::connect(addr, &[addr.port()]).unwrap_err();
    assert_ne!(error, "");
}

#[test]
fn a_closed_connection_reads_as_an_error_not_as_endless_nothing() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mut link = TcpTransport::connect(addr, &[addr.port()]).unwrap();
    let (server_side, _) = listener.accept().unwrap();
    drop(server_side);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match link.read(8, Duration::from_millis(50)) {
            Err(message) => {
                assert!(message.contains("closed"), "{message}");
                break;
            }
            Ok(_) => assert!(Instant::now() < deadline, "never saw the close"),
        }
    }
}

#[test]
fn opening_a_serial_port_that_is_not_there_fails_cleanly() {
    let error = SerialTransport::open("definitely-not-a-port", 115_200).unwrap_err();
    assert_ne!(error, "");
}

#[test]
fn listing_serial_ports_does_not_fail_without_any() {
    // Whatever this machine has, listing must not panic, and each entry has a name.
    for port in list_serial_ports() {
        assert_ne!(port.name, "");
    }
}

#[test]
fn a_serial_port_that_is_not_usb_carries_no_usb_ids() {
    for port_type in [
        serialport::SerialPortType::PciPort,
        serialport::SerialPortType::BluetoothPort,
        serialport::SerialPortType::Unknown,
    ] {
        let info = describe_serial_port(serialport::SerialPortInfo {
            port_name: "COM1".into(),
            port_type,
        });
        assert_eq!(info.name, "COM1");
        assert_eq!(info.usb, None);
        assert_eq!(info.serial, None);
    }
}

#[test]
fn a_serial_port_is_described_as_a_discovered_device() {
    let port = SerialPortInfo {
        name: "COM7".into(),
        usb: Some((0x1a86, 0x7523)),
        product: Some("USB-SERIAL CH340".into()),
        serial: Some("A1".into()),
    };
    let d = port.to_discovered();
    assert_eq!(d.id.0, "serial:COM7");
    assert_eq!(d.transport, TransportKind::Serial);
    assert_eq!(d.usb, Some((0x1a86, 0x7523)));
    assert_eq!(d.name, "USB-SERIAL CH340");
    let bare = SerialPortInfo {
        name: "/dev/ttyS0".into(),
        usb: None,
        product: None,
        serial: None,
    };
    assert_eq!(bare.to_discovered().name, "/dev/ttyS0");
}

// ---- UDP, HTTP and WebSocket ----

#[test]
fn a_udp_link_sends_and_receives_datagrams_with_the_machine_only() {
    let machine = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = machine.local_addr().unwrap();
    let mut link = UdpTransport::connect(addr, &[addr.port()]).unwrap();
    assert_eq!(link.kind(), TransportKind::Udp);
    link.send_datagram(b"ping").unwrap();
    let mut buf = [0_u8; 16];
    machine
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let (n, from) = machine.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"ping");
    machine.send_to(b"pong", from).unwrap();
    assert_eq!(link.recv_datagram(Duration::from_secs(5)).unwrap(), b"pong");
    // A stranger's datagram is not delivered.
    let stranger = UdpSocket::bind("127.0.0.1:0").unwrap();
    stranger.send_to(b"spam", from).unwrap();
    assert_eq!(
        link.recv_datagram(Duration::from_millis(100)).unwrap(),
        Vec::<u8>::new()
    );
}

#[test]
fn a_udp_link_is_scoped_like_a_tcp_one() {
    let internet: SocketAddr = "8.8.8.8:53".parse().unwrap();
    assert!(UdpTransport::connect(internet, &[53]).is_err());
    let local: SocketAddr = "127.0.0.1:9".parse().unwrap();
    assert!(
        UdpTransport::connect(local, &[10])
            .unwrap_err()
            .contains("port")
    );
}

fn http_server(reply: &'static str) -> (SocketAddr, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut got = Vec::new();
        let mut buf = [0_u8; 512];
        loop {
            let n = conn.read(&mut buf).unwrap();
            got.extend_from_slice(&buf[..n]);
            if got.windows(4).any(|w| w == b"\r\n\r\n") && (n < buf.len()) {
                break;
            }
        }
        conn.write_all(reply.as_bytes()).unwrap();
        String::from_utf8_lossy(&got).into_owned()
    });
    (addr, handle)
}

#[test]
fn an_http_link_makes_requests_relative_to_the_machine() {
    let (addr, server) = http_server(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
    );
    let mut link = HttpTransport::connect(addr, &[addr.port()]).unwrap();
    assert_eq!(link.kind(), TransportKind::Http);
    let response = link
        .http(HttpRequest {
            method: "GET".into(),
            path: "/status".into(),
            headers: vec![("X-Test".into(), "1".into())],
            body: Vec::new(),
        })
        .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"ok");
    let seen = server.join().unwrap();
    assert!(seen.starts_with("GET /status HTTP/1.1"), "{seen}");
    assert!(seen.to_lowercase().contains("x-test: 1"), "{seen}");
}

#[test]
fn an_http_path_must_stay_on_the_machine() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mut link = HttpTransport::connect(addr, &[addr.port()]).unwrap();
    for path in [
        "http://evil.example/x",
        "//evil.example/x",
        "no-slash",
        "/a b",
    ] {
        let result = link.http(HttpRequest {
            method: "GET".into(),
            path: path.into(),
            headers: Vec::new(),
            body: Vec::new(),
        });
        assert!(result.is_err(), "{path}");
    }
}

#[test]
fn a_websocket_link_exchanges_binary_messages() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let message = ws.read().unwrap();
        ws.send(tungstenite::Message::Binary(
            [b"echo:".as_slice(), &message.into_data()].concat().into(),
        ))
        .unwrap();
    });
    let mut link = WebSocketTransport::connect(addr, &[addr.port()]).unwrap();
    assert_eq!(link.kind(), TransportKind::WebSocket);
    link.write(b"hello").unwrap();
    let mut got = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while got.is_empty() && Instant::now() < deadline {
        got = link.read(64, Duration::from_millis(100)).unwrap();
    }
    assert_eq!(got, b"echo:hello");
    server.join().unwrap();
}

#[test]
fn a_websocket_to_the_internet_is_refused() {
    let internet: SocketAddr = "8.8.8.8:80".parse().unwrap();
    assert!(WebSocketTransport::connect(internet, &[80]).is_err());
}

// ---- USB ----

fn ep(interface: u8, address: u8, bulk: bool) -> EndpointInfo {
    EndpointInfo {
        interface,
        address,
        bulk,
    }
}

#[test]
fn the_first_interface_with_a_bulk_in_and_a_bulk_out_endpoint_is_used() {
    let found = pick_bulk_endpoints(&[
        ep(0, 0x81, false), // an interrupt endpoint: not usable
        ep(1, 0x02, true),
        ep(1, 0x83, true),
        ep(2, 0x04, true),
        ep(2, 0x85, true),
    ]);
    assert_eq!(found, Some((1, 0x02, 0x83)));
}

#[test]
fn an_interface_with_only_one_direction_is_not_enough() {
    assert_eq!(pick_bulk_endpoints(&[ep(0, 0x01, true)]), None);
    assert_eq!(pick_bulk_endpoints(&[ep(0, 0x81, true)]), None);
    assert_eq!(pick_bulk_endpoints(&[]), None);
}

#[test]
fn opening_a_usb_device_that_is_not_there_fails_cleanly() {
    let error = UsbTransport::open(0xFFFF, 0xFFFE, None).unwrap_err();
    assert_ne!(error, "");
}

#[test]
fn listing_usb_devices_does_not_fail_without_any() {
    for d in list_usb_devices() {
        assert!(d.to_discovered().usb.is_some());
    }
}

#[test]
fn a_usb_device_is_described_as_a_discovered_device() {
    let d = UsbInfo {
        vendor: 0x1a86,
        product: 0x7523,
        name: Some("CH340".into()),
        serial: Some("S1".into()),
    };
    let found = d.to_discovered();
    assert_eq!(found.id.0, "usb:1a86:7523:S1");
    assert_eq!(found.usb, Some((0x1a86, 0x7523)));
    assert_eq!(found.name, "CH340");
    assert_eq!(found.transport, TransportKind::Usb);
    let bare = UsbInfo {
        vendor: 1,
        product: 2,
        name: None,
        serial: None,
    };
    assert_eq!(bare.to_discovered().id.0, "usb:0001:0002");
    assert_eq!(bare.to_discovered().name, "USB device 0001:0002");
}
