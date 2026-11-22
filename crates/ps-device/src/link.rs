//! Real byte links: a TCP socket and a serial port, as the blocking transports a
//! WebAssembly plugin reads and writes through the host.
//!
//! Network access is scoped by the host: a link is opened only to an address on the user's own
//! network and a port the machine was approved for, so a plugin can never be pointed at an
//! arbitrary internet host through its transport.

use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

use crate::host::{DeviceId, DiscoveredDevice};
use crate::types::TransportKind;
use crate::wasm::{BlockingTransport, HttpRequest, HttpResponse};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// True for addresses on the user's own network: loopback, private ranges and link-local.
#[must_use]
pub fn is_local_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// A TCP connection to a machine.
#[derive(Debug)]
pub struct TcpTransport {
    stream: TcpStream,
}

impl TcpTransport {
    /// Connects to a machine on the local network.
    ///
    /// # Errors
    ///
    /// A message when the address is not local, the port is not one the machine was approved for,
    /// or the connection fails.
    pub fn connect(addr: SocketAddr, allowed_ports: &[u16]) -> Result<Self, String> {
        if !is_local_address(addr.ip()) {
            return Err(format!(
                "{} is not on the local network, so a machine cannot be there",
                addr.ip()
            ));
        }
        if !allowed_ports.contains(&addr.port()) {
            return Err(format!(
                "port {} is not one this machine was approved for",
                addr.port()
            ));
        }
        let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
            .map_err(|e| format!("cannot connect to {addr}: {e}"))?;
        stream.set_nodelay(true).map_err(|e| e.to_string())?;
        Ok(Self { stream })
    }
}

impl BlockingTransport for TcpTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Tcp
    }

    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        self.stream.write_all(data).map_err(|e| e.to_string())
    }

    fn read(&mut self, max: usize, timeout: Duration) -> Result<Vec<u8>, String> {
        self.stream
            .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))
            .map_err(|e| e.to_string())?;
        let mut buf = vec![0_u8; max.clamp(1, 64 * 1024)];
        match self.stream.read(&mut buf) {
            Ok(0) => Err("the connection was closed by the machine".to_owned()),
            Ok(n) => {
                buf.truncate(n);
                Ok(buf)
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                Ok(Vec::new())
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Checks that `addr` is on the local network and its port was approved.
pub(crate) fn scoped(addr: SocketAddr, allowed_ports: &[u16]) -> Result<(), String> {
    if !is_local_address(addr.ip()) {
        return Err(format!(
            "{} is not on the local network, so a machine cannot be there",
            addr.ip()
        ));
    }
    if !allowed_ports.contains(&addr.port()) {
        return Err(format!(
            "port {} is not one this machine was approved for",
            addr.port()
        ));
    }
    Ok(())
}

/// A UDP exchange with one machine: datagrams go only to it and only its replies are delivered.
#[derive(Debug)]
pub struct UdpTransport {
    socket: UdpSocket,
    machine: SocketAddr,
}

impl UdpTransport {
    /// Opens a socket aimed at a machine on the local network.
    ///
    /// # Errors
    ///
    /// A message when the address is not local, the port is not approved, or the socket fails.
    pub fn connect(addr: SocketAddr, allowed_ports: &[u16]) -> Result<Self, String> {
        scoped(addr, allowed_ports)?;
        let bind = if addr.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let socket = UdpSocket::bind(bind).map_err(|e| e.to_string())?;
        socket
            .connect(addr)
            .map_err(|e| format!("cannot reach {addr}: {e}"))?;
        Ok(Self {
            socket,
            machine: addr,
        })
    }
}

impl BlockingTransport for UdpTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Udp
    }

    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        self.send_datagram(data)
    }

    fn read(&mut self, _max: usize, timeout: Duration) -> Result<Vec<u8>, String> {
        self.recv_datagram(timeout)
    }

    fn send_datagram(&mut self, data: &[u8]) -> Result<(), String> {
        self.socket
            .send(data)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn recv_datagram(&mut self, timeout: Duration) -> Result<Vec<u8>, String> {
        self.socket
            .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))
            .map_err(|e| e.to_string())?;
        let mut buf = vec![0_u8; 64 * 1024];
        match self.socket.recv_from(&mut buf) {
            Ok((n, from)) if from == self.machine => {
                buf.truncate(n);
                Ok(buf)
            }
            Ok(_) => Ok(Vec::new()),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                Ok(Vec::new())
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// The most an HTTP response from a machine may hold.
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// HTTP to one machine: every path is relative to it, and nothing else can be named.
#[derive(Debug)]
pub struct HttpTransport {
    addr: SocketAddr,
}

pub(crate) fn valid_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains("://")
        && path.chars().all(|c| c.is_ascii_graphic())
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

impl HttpTransport {
    /// Aims HTTP requests at a machine on the local network.
    ///
    /// # Errors
    ///
    /// A message when the address is not local or the port is not approved.
    pub fn connect(addr: SocketAddr, allowed_ports: &[u16]) -> Result<Self, String> {
        scoped(addr, allowed_ports)?;
        Ok(Self { addr })
    }

    fn head(&self, request: &HttpRequest) -> Result<String, String> {
        request_head(&self.addr.to_string(), request)
    }
}

/// The request line and headers for `request` to `host`, refusing header injection.
pub(crate) fn request_head(host: &str, request: &HttpRequest) -> Result<String, String> {
    use std::fmt::Write as _;
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n",
        request.method, request.path
    );
    for (name, value) in &request.headers {
        if name.contains(['\r', '\n', ':']) || value.contains(['\r', '\n']) {
            return Err("a header holds a line break".to_owned());
        }
        let _ = write!(head, "{name}: {value}\r\n");
    }
    if !request.body.is_empty() {
        let _ = write!(head, "Content-Length: {}\r\n", request.body.len());
    }
    head.push_str("\r\n");
    Ok(head)
}

impl BlockingTransport for HttpTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Http
    }

    fn write(&mut self, _data: &[u8]) -> Result<(), String> {
        Err("an HTTP link takes requests, not raw bytes".to_owned())
    }

    fn read(&mut self, _max: usize, _timeout: Duration) -> Result<Vec<u8>, String> {
        Err("an HTTP link takes requests, not raw bytes".to_owned())
    }

    fn http(&mut self, request: HttpRequest) -> Result<HttpResponse, String> {
        if !valid_path(&request.path) {
            return Err(format!("{:?} is not a path on the machine", request.path));
        }
        if request.method.is_empty() || !request.method.chars().all(|c| c.is_ascii_uppercase()) {
            return Err("the method must be upper-case letters".to_owned());
        }
        let head = self.head(&request)?;
        let mut stream = TcpStream::connect_timeout(&self.addr, CONNECT_TIMEOUT)
            .map_err(|e| format!("cannot connect to {}: {e}", self.addr))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| e.to_string())?;
        stream
            .write_all(head.as_bytes())
            .map_err(|e| e.to_string())?;
        stream.write_all(&request.body).map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    raw.extend_from_slice(&chunk[..n]);
                    if raw.len() > MAX_RESPONSE_BYTES {
                        return Err("the response is too large".to_owned());
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return Err("the machine did not finish answering".to_owned());
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        parse_response(&raw)
    }
}

pub(crate) fn parse_response(raw: &[u8]) -> Result<HttpResponse, String> {
    let split = find(raw, b"\r\n\r\n").ok_or("the response has no header end")?;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.lines();
    let status_line = lines.next().ok_or("the response is empty")?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| format!("bad status line {status_line:?}"))?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let mut body = raw[split + 4..].to_vec();
    if let Some((_, len)) = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        && let Ok(len) = len.parse::<usize>()
    {
        body.truncate(len);
    }
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

/// A WebSocket to one machine, carrying binary messages as a byte stream.
pub struct WebSocketTransport {
    socket: tungstenite::WebSocket<TcpStream>,
    pending: std::collections::VecDeque<u8>,
}

impl std::fmt::Debug for WebSocketTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSocketTransport").finish_non_exhaustive()
    }
}

impl WebSocketTransport {
    /// Opens a WebSocket to a machine on the local network.
    ///
    /// # Errors
    ///
    /// A message when the address is not local, the port is not approved, or the handshake fails.
    pub fn connect(addr: SocketAddr, allowed_ports: &[u16]) -> Result<Self, String> {
        scoped(addr, allowed_ports)?;
        let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
            .map_err(|e| format!("cannot connect to {addr}: {e}"))?;
        let (socket, _) = tungstenite::client(format!("ws://{addr}/"), stream)
            .map_err(|e| format!("the WebSocket handshake failed: {e}"))?;
        Ok(Self {
            socket,
            pending: std::collections::VecDeque::new(),
        })
    }
}

impl BlockingTransport for WebSocketTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::WebSocket
    }

    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        self.socket
            .send(tungstenite::Message::Binary(data.to_vec().into()))
            .map_err(|e| e.to_string())
    }

    fn read(&mut self, max: usize, timeout: Duration) -> Result<Vec<u8>, String> {
        if self.pending.is_empty() {
            self.socket
                .get_ref()
                .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))
                .map_err(|e| e.to_string())?;
            match self.socket.read() {
                Ok(tungstenite::Message::Binary(bytes)) => self.pending.extend(bytes.iter()),
                Ok(tungstenite::Message::Text(text)) => self.pending.extend(text.as_bytes()),
                Ok(tungstenite::Message::Close(_)) => {
                    return Err("the connection was closed by the machine".to_owned());
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        let n = self.pending.len().min(max);
        Ok(self.pending.drain(..n).collect())
    }
}

/// One endpoint of a USB interface, as far as choosing a pair to talk through is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointInfo {
    /// The interface it belongs to.
    pub interface: u8,
    /// Its address; the top bit is set for IN endpoints.
    pub address: u8,
    /// True for bulk endpoints.
    pub bulk: bool,
}

/// The first interface that has both a bulk OUT and a bulk IN endpoint: `(interface, out, in)`.
#[must_use]
pub fn pick_bulk_endpoints(endpoints: &[EndpointInfo]) -> Option<(u8, u8, u8)> {
    let mut interfaces: Vec<u8> = endpoints.iter().map(|e| e.interface).collect();
    interfaces.dedup();
    interfaces.into_iter().find_map(|interface| {
        let bulk = |input: bool| {
            endpoints
                .iter()
                .find(|e| e.interface == interface && e.bulk && (e.address & 0x80 != 0) == input)
                .map(|e| e.address)
        };
        Some((interface, bulk(false)?, bulk(true)?))
    })
}

/// A USB device the system lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbInfo {
    /// The vendor id.
    pub vendor: u16,
    /// The product id.
    pub product: u16,
    /// The product string, when the device gives one.
    pub name: Option<String>,
    /// The serial number, when the device gives one.
    pub serial: Option<String>,
}

impl UsbInfo {
    /// The host's view of this device.
    #[must_use]
    pub fn to_discovered(&self) -> DiscoveredDevice {
        let ids = format!("{:04x}:{:04x}", self.vendor, self.product);
        let id = match &self.serial {
            Some(serial) => format!("usb:{ids}:{serial}"),
            None => format!("usb:{ids}"),
        };
        DiscoveredDevice {
            id: DeviceId(id),
            name: self
                .name
                .clone()
                .unwrap_or_else(|| format!("USB device {ids}")),
            transport: TransportKind::Usb,
            usb: Some((self.vendor, self.product)),
            address: None,
            serial: self.serial.clone(),
        }
    }
}

/// The USB devices the system lists now; empty when it lists none or cannot say.
#[must_use]
pub fn list_usb_devices() -> Vec<UsbInfo> {
    use nusb::MaybeFuture as _;
    nusb::list_devices()
        .wait()
        .map(|devices| {
            devices
                .map(|d| UsbInfo {
                    vendor: d.vendor_id(),
                    product: d.product_id(),
                    name: d.product_string().map(str::to_owned),
                    serial: d.serial_number().map(str::to_owned),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A USB device talked to through one bulk OUT and one bulk IN endpoint.
pub struct UsbTransport {
    out: nusb::Endpoint<nusb::transfer::Bulk, nusb::transfer::Out>,
    input: nusb::Endpoint<nusb::transfer::Bulk, nusb::transfer::In>,
    waiting: bool,
}

impl std::fmt::Debug for UsbTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UsbTransport").finish_non_exhaustive()
    }
}

/// How long one write may take.
const USB_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

impl UsbTransport {
    /// Opens the device with these ids (and this serial number, when given) and claims its first
    /// interface that has bulk endpoints in both directions.
    ///
    /// # Errors
    ///
    /// A message when the device is absent, cannot be opened, or has no bulk endpoint pair.
    pub fn open(vendor: u16, product: u16, serial: Option<&str>) -> Result<Self, String> {
        use nusb::MaybeFuture as _;
        let info = nusb::list_devices()
            .wait()
            .map_err(|e| e.to_string())?
            .find(|d| {
                d.vendor_id() == vendor
                    && d.product_id() == product
                    && serial.is_none_or(|s| d.serial_number() == Some(s))
            })
            .ok_or_else(|| format!("no USB device {vendor:04x}:{product:04x} is connected"))?;
        let device = info
            .open()
            .wait()
            .map_err(|e| format!("cannot open the USB device: {e}"))?;
        let mut endpoints = Vec::new();
        if let Some(configuration) = device.configurations().next() {
            for interface in configuration.interfaces() {
                if let Some(alt) = interface.alt_settings().next() {
                    for endpoint in alt.endpoints() {
                        endpoints.push(EndpointInfo {
                            interface: interface.interface_number(),
                            address: endpoint.address(),
                            bulk: endpoint.transfer_type() == nusb::descriptors::TransferType::Bulk,
                        });
                    }
                }
            }
        }
        let (interface, out, input) = pick_bulk_endpoints(&endpoints)
            .ok_or("the device has no interface with bulk endpoints in both directions")?;
        let claimed = device
            .detach_and_claim_interface(interface)
            .wait()
            .map_err(|e| format!("cannot claim the USB interface: {e}"))?;
        let out = claimed
            .endpoint::<nusb::transfer::Bulk, nusb::transfer::Out>(out)
            .map_err(|e| e.to_string())?;
        let input = claimed
            .endpoint::<nusb::transfer::Bulk, nusb::transfer::In>(input)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            out,
            input,
            waiting: false,
        })
    }
}

impl BlockingTransport for UsbTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Usb
    }

    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        let mut buffer = self.out.allocate(data.len());
        buffer.extend_from_slice(data);
        self.out.submit(buffer);
        if let Some(done) = self.out.wait_next_complete(USB_WRITE_TIMEOUT) {
            done.status.map_err(|e| e.to_string())
        } else {
            self.out.cancel_all();
            Err("the USB write timed out".to_owned())
        }
    }

    fn read(&mut self, max: usize, timeout: Duration) -> Result<Vec<u8>, String> {
        if !self.waiting {
            let size = self.input.max_packet_size().max(max.min(64 * 1024));
            let size = size.div_ceil(self.input.max_packet_size()) * self.input.max_packet_size();
            let buffer = self.input.allocate(size);
            self.input.submit(buffer);
            self.waiting = true;
        }
        match self
            .input
            .wait_next_complete(timeout.max(Duration::from_millis(1)))
        {
            Some(done) => {
                self.waiting = false;
                done.status.map_err(|e| e.to_string())?;
                let mut bytes = done.buffer.into_vec();
                bytes.truncate(done.actual_len);
                Ok(bytes)
            }
            None => Ok(Vec::new()),
        }
    }
}
/// A serial port, opened 8N1 at a baud rate.
pub struct SerialTransport {
    port: serial2::SerialPort,
}

impl std::fmt::Debug for SerialTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SerialTransport").finish_non_exhaustive()
    }
}

impl SerialTransport {
    /// Opens `name` (such as `COM7` or `/dev/ttyUSB0`) at `baud`.
    ///
    /// # Errors
    ///
    /// A message when the port cannot be opened.
    pub fn open(name: &str, baud: u32) -> Result<Self, String> {
        let port = serial2::SerialPort::open(name, baud)
            .map_err(|e| format!("cannot open {name}: {e}"))?;
        Ok(Self { port })
    }
}

impl BlockingTransport for SerialTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Serial
    }

    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        self.port.write_all(data).map_err(|e| e.to_string())?;
        self.port.flush().map_err(|e| e.to_string())
    }

    fn read(&mut self, max: usize, timeout: Duration) -> Result<Vec<u8>, String> {
        self.port
            .set_read_timeout(timeout.max(Duration::from_millis(1)))
            .map_err(|e| e.to_string())?;
        let mut buf = vec![0_u8; max.clamp(1, 64 * 1024)];
        match self.port.read(&mut buf) {
            Ok(n) => {
                buf.truncate(n);
                Ok(buf)
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                Ok(Vec::new())
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// A serial port the system lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialPortInfo {
    /// The port's name.
    pub name: String,
    /// USB vendor and product ids, when the port is a USB adapter and the system says so.
    pub usb: Option<(u16, u16)>,
    /// The product string, when known.
    pub product: Option<String>,
    /// The adapter's serial number, when known.
    pub serial: Option<String>,
}

impl SerialPortInfo {
    /// The host's view of this port.
    #[must_use]
    pub fn to_discovered(&self) -> DiscoveredDevice {
        DiscoveredDevice {
            id: DeviceId(format!("serial:{}", self.name)),
            name: self.product.clone().unwrap_or_else(|| self.name.clone()),
            transport: TransportKind::Serial,
            usb: self.usb,
            address: Some(self.name.clone()),
            serial: self.serial.clone(),
        }
    }
}

/// What the system says about one serial port. A USB adapter carries its vendor and product ids,
/// product string and serial number, which is how a plugin tells its own machine's port from any
/// other port without opening it.
#[must_use]
pub fn describe_serial_port(port: serialport::SerialPortInfo) -> SerialPortInfo {
    match port.port_type {
        serialport::SerialPortType::UsbPort(usb) => SerialPortInfo {
            name: port.port_name,
            usb: Some((usb.vid, usb.pid)),
            product: usb.product,
            serial: usb.serial_number,
        },
        _ => SerialPortInfo {
            name: port.port_name,
            usb: None,
            product: None,
            serial: None,
        },
    }
}

/// The serial ports the system lists now; an empty list when it lists none or cannot say.
#[must_use]
pub fn list_serial_ports() -> Vec<SerialPortInfo> {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(describe_serial_port)
        .collect()
}

#[cfg(test)]
mod tests;
