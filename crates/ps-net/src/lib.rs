//! Plotter Studio's only door to the internet.
//!
//! Exactly two operations exist: [`Net::check_for_update`] reads the release feed, and
//! [`Net::fetch_material_index`] plus [`Net::fetch_package`] read the signed material index and
//! its packages. Both are off until the user opts in (the [`Network`] settings), only `https`
//! URLs are fetched, every download is size-limited, the index must carry a valid minisign
//! signature from the key the app trusts, and each package must match the SHA-256 in the signed
//! index. No identifiers, counters or telemetry are sent: a request is a plain `GET`.
//!
//! Only `ps-app` may depend on this crate; an `xtask` check enforces it.

use std::time::Duration;

use minisign_verify::{PublicKey, Signature};
use ps_library::Network;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The most bytes read for a feed or an index.
pub const MAX_METADATA_BYTES: usize = 2 * 1024 * 1024;

/// An opt-in internet feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// The release feed.
    UpdateCheck,
    /// The signed material index and its packages.
    MaterialIndex,
}

/// Why an HTTP request failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HttpError {
    /// The server answered with an error status.
    #[error("the server answered {0}")]
    Status(u16),
    /// The response was larger than allowed.
    #[error("the response was larger than allowed")]
    TooLarge,
    /// The connection failed.
    #[error("network error: {0}")]
    Transport(String),
}

/// Why a network operation did not succeed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    /// The user has not turned this feature on.
    #[error("{0:?} is turned off in settings")]
    Disabled(Feature),
    /// A URL was not `https`.
    #[error("only https links are used: {0}")]
    InsecureUrl(String),
    /// The request failed.
    #[error(transparent)]
    Http(#[from] HttpError),
    /// The document is not what was expected.
    #[error("unexpected document: {0}")]
    Malformed(String),
    /// The trusted public key could not be read.
    #[error("the trusted public key is not valid")]
    BadKey,
    /// The signature is missing, damaged, or does not match the document.
    #[error("the signature does not match")]
    BadSignature,
    /// A package differs from the hash in the signed index.
    #[error("package {id} does not match its signed hash")]
    HashMismatch {
        /// The package id.
        id: String,
    },
}

/// A way to `GET` a URL. The app uses [`UreqClient`]; tests use a fake.
pub trait Http {
    /// Fetches `url`, failing with [`HttpError::TooLarge`] rather than reading past `max_bytes`.
    ///
    /// # Errors
    ///
    /// [`HttpError`] when the request fails.
    fn get(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, HttpError>;
}

/// The real HTTP client (ureq with rustls): https only, bounded time, no cookies or identifiers.
#[derive(Debug)]
pub struct UreqClient {
    agent: ureq::Agent,
}

impl Default for UreqClient {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqClient {
    /// A client with a 30 second overall timeout.
    #[must_use]
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent("PlotterStudio")
            .build()
            .into();
        Self { agent }
    }
}

impl Http for UreqClient {
    fn get(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, HttpError> {
        if !url.starts_with("https://") {
            return Err(HttpError::Transport("only https is allowed".to_owned()));
        }
        let mut response = self.agent.get(url).call().map_err(|e| match e {
            ureq::Error::StatusCode(code) => HttpError::Status(code),
            other => HttpError::Transport(other.to_string()),
        })?;
        let limit = u64::try_from(max_bytes).unwrap_or(u64::MAX);
        response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|e| match e {
                ureq::Error::BodyExceedsLimit(_) => HttpError::TooLarge,
                other => HttpError::Transport(other.to_string()),
            })
    }
}

/// A release newer than the running app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Its version.
    pub version: Version,
    /// Its release date as published.
    pub date: String,
    /// What changed.
    pub notes: String,
    /// Where to get it.
    pub url: String,
}

/// One package in the signed material index.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PackageEntry {
    /// The library id.
    pub id: String,
    /// The display name.
    pub name: String,
    /// The version.
    pub version: Version,
    /// What kind of library it holds.
    pub kind: String,
    /// Where to download it.
    pub url: String,
    /// The SHA-256 of the package, in hex.
    pub sha256: String,
    /// The size in bytes.
    pub size: u64,
    /// The licence it declares.
    pub license: String,
    /// Its category, such as `vinyl`.
    #[serde(default)]
    pub category: String,
    /// Its brand.
    #[serde(default)]
    pub brand: String,
    /// Machine profile ids it is meant for.
    #[serde(default)]
    pub machines: Vec<String>,
    /// Tool ids it uses.
    #[serde(default)]
    pub tools: Vec<String>,
}

/// The signed material index.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MaterialIndex {
    /// When it was generated.
    pub generated: String,
    /// The packages it lists.
    pub packages: Vec<PackageEntry>,
}

#[derive(Deserialize)]
struct RawFeed {
    schema: u32,
    releases: Vec<RawRelease>,
}

#[derive(Deserialize)]
struct RawRelease {
    version: String,
    date: String,
    notes: String,
    url: String,
}

#[derive(Deserialize)]
struct RawIndex {
    schema: u32,
    generated: String,
    packages: Vec<PackageEntry>,
}

/// The network operations, gated by the user's opt-ins.
#[derive(Debug)]
pub struct Net<C> {
    client: C,
    network: Network,
}

fn require_https(url: &str) -> Result<(), NetError> {
    if url.starts_with("https://") {
        Ok(())
    } else {
        Err(NetError::InsecureUrl(url.to_owned()))
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 15)]));
    }
    out
}

impl<C: Http> Net<C> {
    /// Operations over `client`, allowed as `network` says.
    #[must_use]
    pub fn new(client: C, network: Network) -> Self {
        Self { client, network }
    }

    /// The client, for inspection in tests.
    #[must_use]
    pub fn client(&self) -> &C {
        &self.client
    }

    /// Gives the client back.
    #[must_use]
    pub fn into_client(self) -> C {
        self.client
    }

    fn allow(&self, feature: Feature) -> Result<(), NetError> {
        let on = match feature {
            Feature::UpdateCheck => self.network.update_check,
            Feature::MaterialIndex => self.network.material_index,
        };
        if on {
            Ok(())
        } else {
            Err(NetError::Disabled(feature))
        }
    }

    /// Reads the release feed and returns the newest release newer than `current`, if any.
    /// Prereleases are offered only to someone already on one; releases with a link that is not
    /// `https` or a version that does not parse are skipped.
    ///
    /// # Errors
    ///
    /// [`NetError`] when the feature is off, the URL is not https, the request fails, or the feed
    /// is not valid.
    pub fn check_for_update(
        &self,
        feed_url: &str,
        current: &Version,
    ) -> Result<Option<Release>, NetError> {
        self.allow(Feature::UpdateCheck)?;
        require_https(feed_url)?;
        let bytes = self.client.get(feed_url, MAX_METADATA_BYTES)?;
        let feed: RawFeed =
            serde_json::from_slice(&bytes).map_err(|e| NetError::Malformed(e.to_string()))?;
        if feed.schema != 1 {
            return Err(NetError::Malformed(format!(
                "unsupported feed schema {}",
                feed.schema
            )));
        }
        let want_pre = !current.pre.is_empty();
        Ok(feed
            .releases
            .into_iter()
            .filter(|r| require_https(&r.url).is_ok())
            .filter_map(|r| {
                Version::parse(&r.version).ok().map(|version| Release {
                    version,
                    date: r.date,
                    notes: r.notes,
                    url: r.url,
                })
            })
            .filter(|r| (want_pre || r.version.pre.is_empty()) && r.version > *current)
            .max_by(|a, b| a.version.cmp(&b.version)))
    }

    /// Fetches the material index and its `.minisig` signature, verifies the signature against
    /// any of `public_keys` (minisign public key file text or the base64 line; the app embeds a
    /// primary and a backup so the signing key can be rotated), and reads the index.
    ///
    /// # Errors
    ///
    /// [`NetError`] when the feature is off, a URL is not https, a request fails, the key or
    /// signature is invalid, or the signed document is malformed.
    pub fn fetch_material_index(
        &self,
        index_url: &str,
        public_keys: &[impl AsRef<str>],
    ) -> Result<MaterialIndex, NetError> {
        self.allow(Feature::MaterialIndex)?;
        require_https(index_url)?;
        let keys: Vec<PublicKey> = public_keys
            .iter()
            .filter_map(|k| {
                let k = k.as_ref();
                PublicKey::decode(k)
                    .or_else(|_| PublicKey::from_base64(k.trim()))
                    .ok()
            })
            .collect();
        if keys.is_empty() {
            return Err(NetError::BadKey);
        }
        let index = self.client.get(index_url, MAX_METADATA_BYTES)?;
        let signature = self
            .client
            .get(&format!("{index_url}.minisig"), MAX_METADATA_BYTES)?;
        let signature = std::str::from_utf8(&signature)
            .ok()
            .and_then(|text| Signature::decode(text).ok())
            .ok_or(NetError::BadSignature)?;
        if !keys
            .iter()
            .any(|key| key.verify(&index, &signature, false).is_ok())
        {
            return Err(NetError::BadSignature);
        }

        let parsed: RawIndex =
            serde_json::from_slice(&index).map_err(|e| NetError::Malformed(e.to_string()))?;
        if parsed.schema != 1 {
            return Err(NetError::Malformed(format!(
                "unsupported index schema {}",
                parsed.schema
            )));
        }
        for package in &parsed.packages {
            require_https(&package.url)?;
        }
        Ok(MaterialIndex {
            generated: parsed.generated,
            packages: parsed.packages,
        })
    }

    /// Downloads a package listed in a verified index and checks it against the signed hash.
    /// Never reads more than the index says the package holds.
    ///
    /// # Errors
    ///
    /// [`NetError`] when the feature is off, the request fails or is larger than listed, or the
    /// bytes do not match the signed SHA-256.
    pub fn fetch_package(&self, entry: &PackageEntry) -> Result<Vec<u8>, NetError> {
        self.allow(Feature::MaterialIndex)?;
        require_https(&entry.url)?;
        let limit = usize::try_from(entry.size).unwrap_or(usize::MAX);
        let bytes = self.client.get(&entry.url, limit)?;
        let actual = hex(&Sha256::digest(&bytes));
        if actual.eq_ignore_ascii_case(entry.sha256.trim()) {
            Ok(bytes)
        } else {
            Err(NetError::HashMismatch {
                id: entry.id.clone(),
            })
        }
    }
}

#[cfg(test)]
mod tests;
