//! Runtime configuration loaded from the environment.
//!
//! Fields:
//! - `daemon_service_executable` — optional lifecycle CLI override for a
//!   developer-installed service (`AMARCODE_DAEMON_SERVICE_EXECUTABLE`)
//! - `daemon_addr` — TCP bind address (`AMARCODE_DAEMON_ADDR`, default `127.0.0.1:43821`)
//!
//! Logging filter is **not** stored here; see [`crate::logging`] and `AMARCODE_LOG` / `RUST_LOG`.
//!
//! Keep parsing and defaults here; do not open the database or bind sockets.

use std::{path::PathBuf, sync::OnceLock};

/// Default TCP address for the JSON-line RPC server.
pub const DEFAULT_DAEMON_ADDR: &str = "127.0.0.1:43821";

pub const DEFAULT_RELEASE_MANIFEST_URL: &str = "https://amarcode-daemon-distribution.abdelmanan-abdelrahman03.workers.dev/v1/daemon/latest.json";
pub const RELEASE_PUBLIC_KEY_HEX: &str =
    "5ef56cd7772e8c601ca9c5a15378b7088fc558e7edcde73770cbb116d9e255d2";
pub const CURRENT_MANIFEST_FILE: &str = "current-manifest.json";
pub const CURRENT_SIGNATURE_FILE: &str = "current-manifest.json.sig";

/// Daemon configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// A daemon binary used only for short-lived service lifecycle commands.
    /// The desktop application never launches this executable in `run` mode.
    pub daemon_service_executable: Option<PathBuf>,
    pub daemon_addr: String,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

impl Config {
    /// Load config once from environment variables and platform defaults.
    pub fn get() -> &'static Self {
        CONFIG.get_or_init(|| {
            let daemon_addr = std::env::var("AMARCODE_DAEMON_ADDR")
                .unwrap_or_else(|_| DEFAULT_DAEMON_ADDR.to_string());

            let daemon_service_executable = resolve_daemon_service_executable();

            Self {
                daemon_addr,
                daemon_service_executable,
            }
        })
    }
}

fn resolve_daemon_service_executable() -> Option<PathBuf> {
    if let Some(executable) = std::env::var_os("AMARCODE_DAEMON_SERVICE_EXECUTABLE") {
        if !executable.is_empty() {
            return Some(PathBuf::from(executable));
        }
    }

    // A debug build can manage a service previously registered from the
    // workspace binary. It still never owns the long-lived daemon process.
    #[cfg(debug_assertions)]
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    #[cfg(debug_assertions)]
    let candidates = [
        manifest_dir.join("../../../target/debug/amarcode-daemon"),
        manifest_dir.join("../../target/debug/amarcode-daemon"),
    ];

    #[cfg(debug_assertions)]
    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate.canonicalize().unwrap_or(candidate));
        }
    }

    None
}
