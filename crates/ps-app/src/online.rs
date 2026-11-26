//! The app's internet features: the release feed and the signed material index.
//!
//! Both are off until the user opts in, and a request is never made while its feature is off.
//! Requests run on their own thread so the UI never waits, and their results are collected each
//! frame with [`OnlineController::poll`]. Nothing identifying is sent: a request is a plain `GET`.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use ps_library::Network;
use ps_net::{Http, HttpError, MaterialIndex, Net, NetError, PackageEntry, Release};
use semver::Version;

/// Where the release feed is published.
pub const RELEASE_FEED_URL: &str = "https://plotter-studio.github.io/releases/feed.json";
/// Where the signed material index is published.
pub const MATERIAL_INDEX_URL: &str = "https://plotter-studio.github.io/materials/index.json";

/// The public keys that may have signed the index, from the build: a `;`-separated list in the
/// `PLOTTER_STUDIO_INDEX_KEYS` environment variable (the primary key and the backup key). Without
/// it no index is ever accepted.
#[must_use]
pub fn trusted_keys() -> Vec<String> {
    trusted_keys_from(option_env!("PLOTTER_STUDIO_INDEX_KEYS"))
}

/// Splits the key list the build provides.
#[must_use]
pub fn trusted_keys_from(list: Option<&str>) -> Vec<String> {
    list.map(|text| {
        text.split(';')
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(str::to_owned)
            .collect()
    })
    .unwrap_or_default()
}

/// Why a request was not started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OnlineError {
    /// The user has not turned the feature on.
    #[error("this internet feature is turned off in settings")]
    Disabled,
    /// A request of that kind is already running.
    #[error("a request is already running")]
    Busy,
}

/// An HTTP client shared with the request threads.
struct Shared(Arc<dyn Http + Send + Sync>);

impl Http for Shared {
    fn get(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, HttpError> {
        self.0.get(url, max_bytes)
    }
}

enum Message {
    Update(Result<Option<Release>, NetError>),
    Index(Result<MaterialIndex, NetError>),
    Package(Result<(String, Vec<u8>), NetError>),
    Plugin(Result<Vec<u8>, NetError>),
}

/// Where a plugin download is.
#[derive(Debug, PartialEq, Eq)]
enum PluginDownload {
    /// None is running and no result is waiting.
    Idle,
    /// A request is in flight.
    Running,
    /// It finished: the package, or why it failed. Waits to be taken.
    Finished(Result<Vec<u8>, String>),
}

/// Runs the opt-in internet requests off the UI thread.
pub struct OnlineController {
    client: Arc<dyn Http + Send + Sync>,
    current: Version,
    feed_url: String,
    index_url: String,
    keys: Vec<String>,
    network: Network,
    update: Option<Release>,
    index: Option<MaterialIndex>,
    last_error: Option<String>,
    checking: bool,
    refreshing: bool,
    downloading: bool,
    package: Option<(String, Vec<u8>)>,
    plugin: PluginDownload,
    tx: Sender<Message>,
    rx: Receiver<Message>,
}

impl std::fmt::Debug for OnlineController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OnlineController")
            .field("network", &self.network)
            .field("checking", &self.checking)
            .finish_non_exhaustive()
    }
}

impl OnlineController {
    /// A controller over `client`, for an app at version `current`, with both features as
    /// `network` says.
    #[must_use]
    pub fn new(
        client: Arc<dyn Http + Send + Sync>,
        current: Version,
        feed_url: String,
        index_url: String,
        keys: Vec<String>,
        network: Network,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            client,
            current,
            feed_url,
            index_url,
            keys,
            network,
            update: None,
            index: None,
            last_error: None,
            checking: false,
            refreshing: false,
            downloading: false,
            package: None,
            plugin: PluginDownload::Idle,
            tx,
            rx,
        }
    }

    /// The real controller: the real HTTP client and the published feed and index.
    #[must_use]
    pub fn live(network: Network) -> Self {
        let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version::new(0, 0, 0));
        Self::new(
            Arc::new(ps_net::UreqClient::new()),
            current,
            RELEASE_FEED_URL.to_owned(),
            MATERIAL_INDEX_URL.to_owned(),
            trusted_keys(),
            network,
        )
    }

    /// Applies the user's opt-ins as they change.
    pub fn set_network(&mut self, network: Network) {
        self.network = network;
    }

    /// Looks for a newer release in the background.
    ///
    /// # Errors
    ///
    /// [`OnlineError`] when the feature is off or a check is already running.
    pub fn check_for_update(&mut self) -> Result<(), OnlineError> {
        if !self.network.update_check {
            return Err(OnlineError::Disabled);
        }
        if self.checking {
            return Err(OnlineError::Busy);
        }
        self.checking = true;
        let net = Net::new(Shared(Arc::clone(&self.client)), self.network);
        let (url, current, tx) = (self.feed_url.clone(), self.current.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Message::Update(net.check_for_update(&url, &current)));
        });
        Ok(())
    }

    /// Fetches the signed material index in the background.
    ///
    /// # Errors
    ///
    /// [`OnlineError`] when the feature is off or a fetch is already running.
    pub fn refresh_material_index(&mut self) -> Result<(), OnlineError> {
        if !self.network.material_index {
            return Err(OnlineError::Disabled);
        }
        if self.refreshing {
            return Err(OnlineError::Busy);
        }
        self.refreshing = true;
        let net = Net::new(Shared(Arc::clone(&self.client)), self.network);
        let (url, keys, tx) = (self.index_url.clone(), self.keys.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Message::Index(net.fetch_material_index(&url, &keys)));
        });
        Ok(())
    }

    /// Downloads a package listed in the signed index in the background; the bytes are checked
    /// against the signed hash before they are offered by [`take_package`](Self::take_package).
    ///
    /// # Errors
    ///
    /// [`OnlineError`] when the feature is off or a download is already running.
    pub fn download_package(&mut self, entry: &PackageEntry) -> Result<(), OnlineError> {
        if !self.network.material_index {
            return Err(OnlineError::Disabled);
        }
        if self.downloading {
            return Err(OnlineError::Busy);
        }
        self.downloading = true;
        let net = Net::new(Shared(Arc::clone(&self.client)), self.network);
        let (entry, tx) = (entry.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let result = net
                .fetch_package(&entry)
                .map(|bytes| (entry.id.clone(), bytes));
            let _ = tx.send(Message::Package(result));
        });
        Ok(())
    }

    /// Downloads a device plugin from a link the user typed, in the background. This needs no
    /// opt-in: the user asking for this one link is the consent. The bytes are unverified, so
    /// whoever takes them with [`take_plugin`](Self::take_plugin) must check them like a file the
    /// user picked.
    ///
    /// # Errors
    ///
    /// [`OnlineError::Busy`] when a plugin download is already running.
    pub fn download_plugin(&mut self, url: &str) -> Result<(), OnlineError> {
        if self.plugin == PluginDownload::Running {
            return Err(OnlineError::Busy);
        }
        self.plugin = PluginDownload::Running;
        let net = Net::new(Shared(Arc::clone(&self.client)), self.network);
        let (url, tx) = (url.to_owned(), self.tx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Message::Plugin(net.fetch_plugin(&url)));
        });
        Ok(())
    }

    /// The result of a finished plugin download, once: the package, or why it failed.
    pub fn take_plugin(&mut self) -> Option<Result<Vec<u8>, String>> {
        match std::mem::replace(&mut self.plugin, PluginDownload::Idle) {
            PluginDownload::Finished(result) => Some(result),
            // Nothing to take: put the state back as it was.
            unchanged => {
                self.plugin = unchanged;
                None
            }
        }
    }

    /// Whether a plugin download is running.
    #[must_use]
    pub fn plugin_downloading(&self) -> bool {
        self.plugin == PluginDownload::Running
    }

    /// Supplies a verified index without a network request, for tests.
    #[cfg(test)]
    pub(crate) fn use_index_for_tests(&mut self, index: MaterialIndex) {
        self.index = Some(index);
    }

    /// The package a finished download produced, once.
    pub fn take_package(&mut self) -> Option<(String, Vec<u8>)> {
        self.package.take()
    }

    /// Whether a download is running.
    #[must_use]
    pub fn downloading(&self) -> bool {
        self.downloading
    }

    /// Collects finished requests.
    pub fn poll(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Message::Update(result) => {
                    self.checking = false;
                    match result {
                        Ok(found) => {
                            self.update = found;
                            self.last_error = None;
                        }
                        Err(e) => self.last_error = Some(e.to_string()),
                    }
                }
                Message::Package(result) => {
                    self.downloading = false;
                    match result {
                        Ok(found) => {
                            self.package = Some(found);
                            self.last_error = None;
                        }
                        Err(e) => self.last_error = Some(e.to_string()),
                    }
                }
                Message::Plugin(result) => {
                    self.plugin = PluginDownload::Finished(result.map_err(|e| e.to_string()));
                }
                Message::Index(result) => {
                    self.refreshing = false;
                    match result {
                        Ok(index) => {
                            self.index = Some(index);
                            self.last_error = None;
                        }
                        Err(e) => self.last_error = Some(e.to_string()),
                    }
                }
            }
        }
    }

    /// A newer release, if one was found and update checks are still on.
    #[must_use]
    pub fn update(&self) -> Option<&Release> {
        self.update.as_ref().filter(|_| self.network.update_check)
    }

    /// The material index, if fetched and the feature is still on.
    #[must_use]
    pub fn index(&self) -> Option<&MaterialIndex> {
        self.index.as_ref().filter(|_| self.network.material_index)
    }

    /// Whether an update check is running.
    #[must_use]
    pub fn checking(&self) -> bool {
        self.checking
    }

    /// Whether an index fetch is running.
    #[must_use]
    pub fn refreshing_index(&self) -> bool {
        self.refreshing
    }

    /// Why the last request failed, if it did.
    #[must_use]
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

#[cfg(test)]
mod tests;
