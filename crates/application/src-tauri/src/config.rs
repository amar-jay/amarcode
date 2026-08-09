//! Runtime configuration loaded from the environment.
//!
//! Fields:
//! - `app_dir` — data directory (`AMARCODE_APPDIR` / platform default)
//! - `daemon_command` — launcher executable (`AMARCODE_DAEMON_COMMAND`, default `amarcode-daemon`)
//! - `daemon_addr` — TCP bind address (`AMARCODE_DAEMON_ADDR`, default `127.0.0.1:43821`)
//! - `db_path` — SQLite file path (default `app_dir/workspace.sqlite3`, or `AMARCODE_STORE_PATH`)
//!
//! Logging filter is **not** stored here; see [`crate::logging`] and `AMARCODE_LOG` / `RUST_LOG`.
//!
//! Keep parsing and defaults here; do not open the database or bind sockets.

use std::{path::PathBuf, sync::OnceLock};

/// Default TCP address for the JSON-line RPC server.
pub const DEFAULT_DAEMON_ADDR: &str = "127.0.0.1:43821";

/// Default Daemon command to launch if not running.
pub const DEFAULT_DAEMON_COMMAND: &str = "amarcode-daemon";

/// Daemon configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub app_dir: PathBuf,
    pub daemon_command: String,
    pub daemon_addr: String,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

impl Config {
    /// Load config once from environment variables and platform defaults.
    pub fn get() -> &'static Self {
        CONFIG.get_or_init(|| {
            let app_dir = std::env::var_os("AMARCODE_APPDIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));

            let daemon_addr = std::env::var("AMARCODE_DAEMON_ADDR")
                .unwrap_or_else(|_| DEFAULT_DAEMON_ADDR.to_string());

            let daemon_command = resolve_daemon_command();

            Self {
                app_dir,
                daemon_addr,
                daemon_command,
            }
        })
    }
}

fn resolve_daemon_command() -> String {
    if let Some(command) = std::env::var_os("AMARCODE_DAEMON_COMMAND") {
        if !command.is_empty() {
            return command.to_string_lossy().into_owned();
        }
    }

		// temporary hack: try to find the daemon binary in the target/debug directory relative to this crate
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest_dir.join("../../../target/debug/amarcode-daemon"),
        manifest_dir.join("../../target/debug/amarcode-daemon"),
    ];

    for candidate in candidates {
        if candidate.exists() {
            return candidate
                .canonicalize()
                .unwrap_or(candidate)
                .to_string_lossy()
                .into_owned();
        }
    }

    DEFAULT_DAEMON_COMMAND.into()
}
