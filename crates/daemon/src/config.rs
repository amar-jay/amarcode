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

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::{Error, Result};

/// Default TCP address for the JSON-line RPC server.
pub const DEFAULT_DAEMON_ADDR: &str = "127.0.0.1:43821";

/// Default SQLite filename inside `app_dir`.
pub const DEFAULT_DB_NAME: &str = "workspace.sqlite3";

/// ACP registry maintained for Amarcode's agent catalog.
pub const DEFAULT_ACP_REGISTRY_SOURCE: &str = "https://github.com/amar-jay/acp-registry.git";

pub const LOCK_FILE_SUFFIX: &str = ".amarcode.lock";

/// Must match the Tauri bundle identifier so desktop and daemon data share one
/// platform-owned application directory.
const APP_IDENTIFIER: &str = "com.amarcode.desktop";
const DAEMON_DATA_DIRECTORY: &str = "data";

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
        if env::var_os("AMARCODE_APPDIR")
            .filter(|value| !value.is_empty())
            .is_none()
        {
            migrate_legacy_default(&app_dir)?;
        }
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
    dirs::data_local_dir()
        .map(|root| root.join(APP_IDENTIFIER).join(DAEMON_DATA_DIRECTORY))
        .ok_or_else(|| Error::msg("platform local data directory is unavailable"))
}

fn legacy_default() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    let path = dirs::home_dir().map(|home| home.join(".amarcode"));
    #[cfg(target_os = "windows")]
    let path = env::var_os("LOCALAPPDATA").map(|root| PathBuf::from(root).join("amarcode"));
    #[cfg(target_os = "macos")]
    let path = dirs::home_dir().map(|home| home.join("Library/Application Support/amarcode"));
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    let path = dirs::home_dir().map(|home| home.join(".amarcode"));

    path
}

fn migrate_legacy_default(destination: &Path) -> Result<()> {
    let Some(source) = legacy_default() else {
        return Ok(());
    };
    migrate_directory(&source, destination)
}

fn migrate_directory(source: &Path, destination: &Path) -> Result<()> {
    if destination.exists() || !source.exists() {
        return Ok(());
    }
    let parent = destination.parent().ok_or_else(|| {
        Error::msg(format!(
            "application data directory has no parent: {}",
            destination.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        Error::msg(format!(
            "failed to create application data directory {}: {error}",
            parent.display()
        ))
    })?;
    fs::rename(&source, destination).map_err(|error| {
        Error::msg(format!(
            "failed to migrate Amarcode data from {} to {}: {error}",
            source.display(),
            destination.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::migrate_directory;

    #[test]
    fn migrates_legacy_data_into_shared_app_directory() {
        let root = std::env::temp_dir().join(format!(
            "amarcode-config-migration-{}",
            uuid::Uuid::new_v4()
        ));
        let source = root.join("legacy");
        let destination = root.join("com.amarcode.desktop/data");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("workspace.sqlite3"), b"existing data").unwrap();

        migrate_directory(&source, &destination).unwrap();

        assert!(!source.exists());
        assert_eq!(
            std::fs::read(destination.join("workspace.sqlite3")).unwrap(),
            b"existing data"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
