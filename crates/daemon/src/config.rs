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

use std::{env, path::PathBuf};

use crate::{Error, Result};

/// Default TCP address for the JSON-line RPC server.
pub const DEFAULT_DAEMON_ADDR: &str = "127.0.0.1:43821";

/// Default SQLite filename inside `app_dir`.
pub const DEFAULT_DB_NAME: &str = "workspace.sqlite3";

/// ACP registry maintained for Amarcode's agent catalog.
pub const DEFAULT_ACP_REGISTRY_SOURCE: &str = "https://github.com/amar-jay/acp-registry.git";

pub const LOCK_FILE_SUFFIX: &str = ".amarcode.lock";

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
        let app_dir = resolve()?;
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

/// Resolve the Amarcode application data directory.
///
/// Uses `AMARCODE_APPDIR` when set, otherwise the platform default.
pub fn resolve() -> Result<PathBuf> {
    if let Some(dir) = env::var_os("AMARCODE_APPDIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }

    resolve_default()
}

/// Resolve the platform-owned Amarcode data directory without honoring
/// overrides. Destructive cleanup uses this as its allowlisted target so an
/// environment variable can never turn `purge` into an arbitrary directory
/// deletion primitive.
pub fn resolve_default() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let home = env::var_os("HOME").ok_or_else(|| Error::msg("HOME is not set"))?;
        Ok(PathBuf::from(home).join(".amarcode"))
    }

    #[cfg(target_os = "windows")]
    {
        let local =
            env::var_os("LOCALAPPDATA").ok_or_else(|| Error::msg("LOCALAPPDATA is not set"))?;
        Ok(PathBuf::from(local).join("amarcode"))
    }

    #[cfg(target_os = "macos")]
    {
        let home = env::var_os("HOME").ok_or_else(|| Error::msg("HOME is not set"))?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("amarcode"))
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let home = env::var_os("HOME").ok_or_else(|| Error::msg("HOME is not set"))?;
        Ok(PathBuf::from(home).join(".amarcode"))
    }
}
