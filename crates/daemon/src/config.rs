//! Runtime configuration loaded from the environment.
//!
//! Fields:
//! - `transport`: `tcp` (default) or `websocket` (Clap `--transport`)
//! - `app_dir`: data directory (`AMARCODE_APPDIR` / platform default)
//! - `daemon_addr`: bind address (`AMARCODE_DAEMON_ADDR`), defaulting by transport
//! - `db_path`: SQLite file path (default `app_dir/workspace.sqlite3`, or `AMARCODE_STORE_PATH`)
//! - `acp_registry_source`: Git source for the ACP agent registry
//!
//! Logging filter is **not** stored here; see [`crate::logging`] and `AMARCODE_LOG` / `RUST_LOG`.
//!
//! Keep parsing and defaults here; do not open the database or bind sockets.

use std::{env, net::SocketAddr, path::PathBuf, sync::OnceLock};

use crate::{Error, Result};

/// Default TCP address for the JSON-line RPC server.
pub const DEFAULT_DAEMON_ADDR: &str = "127.0.0.1:43821";

/// Default WebSocket address.
pub const DEFAULT_WS_DAEMON_ADDR: &str = "127.0.0.1:43721";

/// Default SQLite filename inside `app_dir`.
pub const DEFAULT_DB_NAME: &str = "workspace.sqlite3";

/// ACP registry maintained for Amarcode's agent catalog.
pub const DEFAULT_ACP_REGISTRY_SOURCE: &str = "https://github.com/amar-jay/acp-registry.git";

pub const LOCK_FILE_SUFFIX: &str = ".amarcode.lock";

/// Must match the Tauri bundle identifier so desktop and daemon data share one
/// platform-owned application directory.
const APP_IDENTIFIER: &str = "com.amarcode.desktop";
const DAEMON_DATA_DIRECTORY: &str = "data";

/// RPC transport selected by the single --transport CLI flag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum DaemonTransport {
    #[default]
    Tcp,
    #[value(name = "websocket")]
    WebSocket,
}

impl DaemonTransport {
    pub fn default_addr(self) -> &'static str {
        match self {
            Self::Tcp => DEFAULT_DAEMON_ADDR,
            Self::WebSocket => DEFAULT_WS_DAEMON_ADDR,
        }
    }
}

/// Daemon configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub app_dir: PathBuf,
    pub transport: DaemonTransport,
    pub daemon_addr: String,
    pub db_path: PathBuf,
    /// PEM certificate chain for WSS (AMARCODE_WS_TLS_CERT).
    pub ws_tls_cert: Option<PathBuf>,
    /// Matching PEM private key (AMARCODE_WS_TLS_KEY).
    pub ws_tls_key: Option<PathBuf>,
    /// Allowed browser origins, comma-separated in AMARCODE_WS_ALLOW_ORIGIN.
    pub ws_allowed_origins: Vec<String>,
    /// Git URL or local Git repository used to populate the runtime registry
    /// checkout. `None` disables synchronization (primarily useful in tests).
    pub acp_registry_source: Option<String>,
}

static CONFIG: OnceLock<Result<Config>> = OnceLock::new();

impl Config {
    /// Load and validate once per process. Environment changes require a restart.
    /// Initialization failures are cached as well, so callers see one consistent result.
    pub fn get(transport: DaemonTransport) -> Result<&'static Self> {
        let config = CONFIG
            .get_or_init(|| Self::load_from_env(transport))
            .as_ref()
            .map_err(|error| Error::msg(error.to_string()))?;
        if config.transport != transport {
            return Err(Error::msg(
                "Daemon configuration was already initialized for a different transport",
            ));
        }
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.daemon_addr.is_empty() {
            return Err(Error::msg("AMARCODE_DAEMON_ADDR must not be empty"));
        }
        // Validate the bind address for either transport, retaining the string in config.
        let address: SocketAddr = self.daemon_addr.parse().map_err(|_| Error::msg(
            "AMARCODE_DAEMON_ADDR must be an IP address and port, e.g. 127.0.0.1:43822 or [::1]:43822"
        ))?;
        if self.transport == DaemonTransport::Tcp {
            return Ok(());
        }
        if self.ws_tls_cert.is_some() != self.ws_tls_key.is_some() {
            return Err(Error::msg(
                "WebSocket TLS requires both AMARCODE_WS_TLS_CERT and AMARCODE_WS_TLS_KEY",
            ));
        }
        if !address.ip().is_loopback() && self.ws_tls_cert.is_none() {
            return Err(Error::msg(
                "Non-loopback WebSocket listeners require TLS certificates",
            ));
        }
        for origin in &self.ws_allowed_origins {
            let url = reqwest::Url::parse(origin)
                .map_err(|_| Error::msg("Invalid WebSocket allowed origin"))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.origin().ascii_serialization() != *origin
            {
                return Err(Error::msg("WebSocket allowed origins must be exact http(s) origins, e.g. https://app.example.com (no path or wildcard)"));
            }
        }
        Ok(())
    }

    fn load_from_env(transport: DaemonTransport) -> Result<Self> {
        let app_dir = resolve()?;
        let db_path =
            env_path("AMARCODE_STORE_PATH").unwrap_or_else(|| app_dir.join(DEFAULT_DB_NAME));
        let acp_registry_source = env_text("AMARCODE_ACP_REGISTRY_SOURCE")?
            .unwrap_or_else(|| DEFAULT_ACP_REGISTRY_SOURCE.to_owned());
        let mut config = Self {
            app_dir,
            transport,
            daemon_addr: env_text("AMARCODE_DAEMON_ADDR")?
                .filter(|address| !address.is_empty())
                .unwrap_or_else(|| transport.default_addr().to_owned()),
            db_path,
            acp_registry_source: (!acp_registry_source.is_empty()).then_some(acp_registry_source),
            ws_tls_cert: None,
            ws_tls_key: None,
            ws_allowed_origins: Vec::new(),
        };
        if transport == DaemonTransport::WebSocket {
            config.ws_tls_cert = env_path("AMARCODE_WS_TLS_CERT");
            config.ws_tls_key = env_path("AMARCODE_WS_TLS_KEY");
            config.ws_allowed_origins = env_text("AMARCODE_WS_ALLOW_ORIGIN")?
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|origin| !origin.is_empty())
                .map(str::to_owned)
                .collect();
        }
        config.validate()?;
        Ok(config)
    }
}

fn env_text(name: &str) -> Result<Option<String>> {
    env::var_os(name)
        .map(|value| {
            value
                .into_string()
                .map(|value| value.trim().to_owned())
                .map_err(|_| Error::msg(format!("{name} must contain valid UTF-8")))
        })
        .transpose()
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Resolve the Amarcode application data directory.
///
/// Uses `AMARCODE_APPDIR` when set, otherwise the platform default.
pub fn resolve() -> Result<PathBuf> {
    if let Some(dir) = env_path("AMARCODE_APPDIR") {
        return Ok(dir);
    }

    resolve_default()
}

/// Resolve the platform-owned Amarcode data directory without honoring
/// overrides. Destructive cleanup uses this as its allowlisted target so an
/// environment variable can never turn `purge` into an arbitrary directory
/// deletion primitive.
pub fn resolve_default() -> Result<PathBuf> {
    dirs::data_local_dir()
        .map(|root| root.join(APP_IDENTIFIER).join(DAEMON_DATA_DIRECTORY))
        .ok_or_else(|| Error::msg("platform local data directory is unavailable"))
}
