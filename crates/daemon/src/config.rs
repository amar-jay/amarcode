//! Runtime configuration loaded from the environment.
//!
//! Fields:
//! - `app_dir` — data directory (`AMARCODE_APPDIR` / platform default)
//! - `daemon_addr` — TCP bind address (`AMARCODE_DAEMON_ADDR`, default `127.0.0.1:43821`)
//! - `db_path` — SQLite file path (default `app_dir/workspace.sqlite3`, or `AMARCODE_STORE_PATH`)
//! - `acp_registry_source` — Git source for the ACP agent registry
//!
//! Logging filter is **not** stored here; see [`crate::logging`] and `AMARCODE_LOG` / `RUST_LOG`.
//!
//! Keep parsing and defaults here; do not open the database or bind sockets.

use std::path::PathBuf;

use crate::{app_dir, Result};

/// Default TCP address for the JSON-line RPC server.
pub const DEFAULT_DAEMON_ADDR: &str = "127.0.0.1:43821";

/// Default SQLite filename inside `app_dir`.
pub const DEFAULT_DB_NAME: &str = "workspace.sqlite3";

/// ACP registry maintained for Amarcode's agent catalog.
pub const DEFAULT_ACP_REGISTRY_SOURCE: &str = "https://github.com/amar-jay/acp-registry.git";

/// Daemon configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub app_dir: PathBuf,
    pub daemon_addr: String,
    pub db_path: PathBuf,
    /// Git URL or local Git repository used to populate the runtime registry
    /// checkout. `None` disables synchronization (primarily useful in tests).
    pub acp_registry_source: Option<String>,
}

impl Config {
    /// Load config from environment variables and platform defaults.
    pub fn from_env() -> Result<Self> {
        let app_dir = app_dir::resolve()?;
        let daemon_addr = std::env::var("AMARCODE_DAEMON_ADDR")
            .unwrap_or_else(|_| DEFAULT_DAEMON_ADDR.to_string());
        let db_path = std::env::var("AMARCODE_STORE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| app_dir.join(DEFAULT_DB_NAME));
        let acp_registry_source = match std::env::var("AMARCODE_ACP_REGISTRY_SOURCE") {
            Ok(value) if value.trim().is_empty() => None,
            Ok(value) => Some(value),
            Err(_) => Some(DEFAULT_ACP_REGISTRY_SOURCE.to_owned()),
        };

        Ok(Self {
            app_dir,
            daemon_addr,
            db_path,
            acp_registry_source,
        })
    }
}
