//! Process entrypoint for `amarcode-daemon`.
//!
//! Keep this thin:
//! 1. load `Config`
//! 2. init logging (stderr + app-dir file)
//! 3. build `App`
//! 4. run until shutdown signal

use amarcode_daemon::{logging, App, Config};
use tracing::{error, info};

#[tokio::main]
async fn main() {
    // Config before logging so we know where to write daemon.log.
    // Failures here go to stderr because the subscriber is not up yet.
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("amarcode-daemon: failed to load config: {err}");
            std::process::exit(1);
        }
    };

    if let Err(err) = logging::init(&config) {
        eprintln!("amarcode-daemon: failed to initialize logging: {err}");
        std::process::exit(1);
    }

    info!(
        service = "amarcode-daemon",
        version = env!("CARGO_PKG_VERSION"),
        addr = %config.daemon_addr,
        app_dir = %config.app_dir.display(),
        db_path = %config.db_path.display(),
        log_path = %logging::log_file_path(&config.app_dir).display(),
        "starting"
    );

    if let Err(err) = run(config).await {
        error!(error = %err, "daemon exited with error");
        std::process::exit(1);
    }

    info!("shutdown complete");
}

async fn run(config: Config) -> amarcode_daemon::Result<()> {
    let app = App::new(config).await?;
    app.run().await
}
