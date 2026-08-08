//! Structured logging for the daemon.
//!
//! Built on `tracing` + `tracing-subscriber`.
//!
//! ## Outputs
//! - **stderr** — always (ANSI colors when the stream is a TTY)
//! - **`{app_dir}/daemon.log`** — append-only file, no ANSI
//!
//! ## Filter
//! Precedence:
//! 1. `AMARCODE_LOG` (daemon-specific)
//! 2. `RUST_LOG` (standard)
//! 3. default: `amarcode_daemon=info`
//!
//! Examples:
//! ```text
//! AMARCODE_LOG=debug
//! AMARCODE_LOG=amarcode_daemon=trace,rusqlite=warn
//! ```
//!
//! Call [`init`] once at process start, after [`crate::Config`] is loaded so
//! the log file path is known. Safe to call only once; a second call returns
//! an error without changing the global subscriber.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    sync::Mutex,
};

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{Config, Error, Result};

/// Default filter when neither `AMARCODE_LOG` nor `RUST_LOG` is set.
pub const DEFAULT_LOG_FILTER: &str = "amarcode_daemon=info";

/// Filename of the daemon log under the app data directory.
pub const LOG_FILE_NAME: &str = "daemon.log";

/// Initialize the global tracing subscriber.
///
/// Creates `config.app_dir` if needed and opens `{app_dir}/daemon.log` for append.
pub fn init(config: &Config) -> Result<()> {
    let filter = env_filter();
    let log_path = log_file_path(&config.app_dir);

    fs::create_dir_all(&config.app_dir).map_err(|err| {
        Error::msg(format!(
            "failed to create app dir {}: {err}",
            config.app_dir.display()
        ))
    })?;

    let file = open_log_file(&log_path).map_err(|err| {
        Error::msg(format!(
            "failed to open log file {}: {err}",
            log_path.display()
        ))
    })?;

    let stderr_layer = fmt::layer()
        .with_writer(io::stderr)
        .with_ansi(io::stderr().is_terminal())
        .with_target(true)
        .with_thread_ids(false)
        .with_level(true);

    let file_layer = fmt::layer()
        .with_writer(Mutex::new(file))
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .with_level(true);

    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init()
        .map_err(|err| Error::msg(format!("failed to install tracing subscriber: {err}")))?;

    tracing::debug!(
        log_path = %log_path.display(),
        filter = %std::env::var("AMARCODE_LOG")
            .or_else(|_| std::env::var("RUST_LOG"))
            .unwrap_or_else(|_| DEFAULT_LOG_FILTER.to_string()),
        "logging initialized"
    );

    Ok(())
}

/// Path to the daemon log file for a given app data directory.
pub fn log_file_path(app_dir: &Path) -> PathBuf {
    app_dir.join(LOG_FILE_NAME)
}

fn env_filter() -> EnvFilter {
    // Prefer daemon-specific override, then RUST_LOG, then a sensible default.
    if let Ok(spec) = std::env::var("AMARCODE_LOG") {
        if !spec.is_empty() {
            return EnvFilter::try_new(spec).unwrap_or_else(|err| {
                eprintln!("invalid AMARCODE_LOG, falling back to default: {err}");
                EnvFilter::new(DEFAULT_LOG_FILTER)
            });
        }
    }

    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER))
}

fn open_log_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}
