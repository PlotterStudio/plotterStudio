//! Running the automation server from the settings.
//!
//! [`AutomationHost`] is what the window calls once a frame: it starts, stops or restarts the HTTP
//! server when the settings change, and serves the requests clients have queued. It reports how it
//! is doing as an [`McpStatus`] for the settings window to show, including the addresses to
//! connect to.

use std::fmt::Write as _;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use super::{ENDPOINT, HttpServer, Request, ServerConfig};
use crate::ui::App;

/// How the automation server is doing, as the settings window shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum McpStatus {
    /// Turned off.
    #[default]
    Off,
    /// Listening; these are the addresses a client connects to.
    Listening {
        /// Full URLs, such as `http://192.168.1.20:8765/mcp`.
        urls: Vec<String>,
    },
    /// Turned on but could not start, with the reason.
    Failed(String),
}

/// The URLs a client connects to. On this computer only, that is the loopback address; with the
/// network allowed, every address the computer has on a network, then the loopback one.
#[must_use]
pub fn endpoint_urls(addresses: &[IpAddr], port: u16, allow_network: bool) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    if allow_network {
        urls.extend(
            addresses
                .iter()
                .filter(|a| !a.is_loopback())
                .map(|a| format!("http://{a}:{port}{ENDPOINT}")),
        );
    }
    urls.push(format!("http://127.0.0.1:{port}{ENDPOINT}"));
    urls
}

/// This computer's IPv4 addresses on its networks, loopback excluded.
#[must_use]
pub fn local_addresses() -> Vec<IpAddr> {
    if_addrs::get_if_addrs()
        .map(|interfaces| {
            interfaces
                .into_iter()
                .filter(|i| !i.is_loopback())
                .map(|i| i.ip())
                .filter(IpAddr::is_ipv4)
                .collect()
        })
        .unwrap_or_default()
}

/// A new access token: 256 random bits as 64 hexadecimal characters.
///
/// # Panics
///
/// If the operating system has no source of randomness, which is a broken system and not
/// something the app can work around (guessable tokens would be worse than stopping).
#[must_use]
pub fn generate_token() -> String {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).expect("the operating system provides random numbers");
    bytes
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            // Writing to a String cannot fail.
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// Keeps the server in line with the settings.
#[derive(Debug, Default)]
pub struct AutomationHost {
    server: Option<HttpServer>,
    requests: Option<Receiver<Request>>,
    /// What was last started (or tried), so a failed start is not retried every frame.
    applied: Option<ServerConfig>,
}

impl AutomationHost {
    /// A host with nothing running.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Brings the server in line with the settings, then serves the requests clients have queued.
    /// `wake` is called when a request arrives, so the window can ask for a repaint and answer it.
    pub fn update(&mut self, app: &mut App, wake: &Arc<dyn Fn() + Send + Sync>, now_ms: u64) {
        let mut automation = app.state().settings().automation.clone();
        if automation.mcp_enabled && automation.mcp_token.is_empty() {
            // A settings file edited by hand: never run without a token.
            let token = generate_token();
            app.state_mut()
                .update_settings(|s| s.automation.mcp_token.clone_from(&token));
            automation.mcp_token = token;
        }
        let desired = automation.mcp_enabled.then(|| {
            ServerConfig::new(
                automation.mcp_port,
                automation.mcp_allow_network,
                automation.mcp_token.clone(),
            )
        });
        if desired != self.applied {
            self.restart(app, desired, wake);
        }
        if let Some(requests) = &self.requests {
            while let Ok(request) = requests.try_recv() {
                request.serve(app, now_ms);
            }
        }
    }

    fn restart(
        &mut self,
        app: &mut App,
        desired: Option<ServerConfig>,
        wake: &Arc<dyn Fn() + Send + Sync>,
    ) {
        // Stop first: the new server may want the very port the old one is releasing.
        self.server = None;
        self.requests = None;
        self.applied.clone_from(&desired);
        let Some(config) = desired else {
            app.set_mcp_status(McpStatus::Off);
            return;
        };
        let wake = Arc::clone(wake);
        match HttpServer::start(&config, move || wake()) {
            Ok((server, requests)) => {
                let urls = endpoint_urls(
                    &local_addresses(),
                    server.local_addr().port(),
                    config.allow_network,
                );
                app.set_mcp_status(McpStatus::Listening { urls });
                self.server = Some(server);
                self.requests = Some(requests);
            }
            Err(reason) => {
                tracing::warn!(%reason, "the automation server could not start");
                app.set_mcp_status(McpStatus::Failed(reason));
            }
        }
    }
}

#[cfg(test)]
mod tests;
