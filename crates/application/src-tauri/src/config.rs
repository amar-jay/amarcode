//! Runtime configuration loaded from the environment.
//!
//! Fields:
//! - `daemon_command` — local development override (`AMARCODE_DAEMON_COMMAND`)
//! - `daemon_addr` — TCP bind address (`AMARCODE_DAEMON_ADDR`, default `127.0.0.1:43821`)
//!
//! Logging filter is **not** stored here; see [`crate::logging`] and `AMARCODE_LOG` / `RUST_LOG`.
//!
//! Keep parsing and defaults here; do not open the database or bind sockets.

use std::{path::PathBuf, sync::OnceLock};

/// Default TCP address for the JSON-line RPC server.
pub const DEFAULT_DAEMON_ADDR: &str = "127.0.0.1:43821";

/// Default Daemon command to launch if not running.
pub const DEFAULT_DAEMON_COMMAND: &str = "amarcode-daemon";

pub const DEFAULT_RELEASE_MANIFEST_URL: &str = "https://amarcode-daemon-distribution.abdelmanan-abdelrahman03.workers.dev/v1/daemon/latest.json";
pub const RELEASE_PUBLIC_KEY_HEX: &str =
    "5ef56cd7772e8c601ca9c5a15378b7088fc558e7edcde73770cbb116d9e255d2";
pub const CURRENT_MANIFEST_FILE: &str = "current-manifest.json";
pub const CURRENT_SIGNATURE_FILE: &str = "current-manifest.json.sig";

/// Daemon configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub daemon_command: String,
    pub daemon_addr: String,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

impl Config {
    /// Load config once from environment variables and platform defaults.
    pub fn get() -> &'static Self {
        CONFIG.get_or_init(|| {
            let daemon_addr = std::env::var("AMARCODE_DAEMON_ADDR")
                .unwrap_or_else(|_| DEFAULT_DAEMON_ADDR.to_string());

            let daemon_command = resolve_daemon_command();

            Self {
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
