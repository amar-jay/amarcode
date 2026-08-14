//! Process entrypoint for `amarcode-daemon`.
//!
//! Keep this thin:
//! 1. load `Config`
//! 2. init logging (stderr + app-dir file)
//! 3. build `App`
//! 4. run until shutdown signal

use std::path::PathBuf;

use amarcode_daemon::{
    cleanup, logging,
    service_control::{NativeServiceController, ServiceController},
    App, Config,
};
use clap::{Parser, Subcommand};
use tracing::{error, info};

#[derive(Debug, Parser)]
#[command(name = "amarcode-daemon", version, about)]
#[command(propagate_version = true)]
struct Cli {
    /// Defaults to `run` for compatibility with older service definitions.
    #[command(subcommand)]
    command: Option<LifecycleCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
enum LifecycleCommand {
    /// Run the long-lived RPC server in the foreground.
    Run,
    /// Register the current executable as a per-user login service.
    Install,
    /// Stop and unregister the per-user service without deleting user data.
    Uninstall,
    /// Stop and unregister the service, then permanently delete daemon data.
    Purge {
        /// Required acknowledgement for irreversible data deletion.
        #[arg(long)]
        confirm_data_loss: bool,
    },
    /// Start an already-installed service.
    Start,
    /// Stop the installed service gracefully.
    Stop,
    /// Restart an already-installed service.
    Restart,
    /// Report whether the service is installed and running.
    Status {
        /// Emit a stable JSON object for desktop and CLI clients.
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() {
    let command = Cli::parse().command.unwrap_or(LifecycleCommand::Run);
    if command != LifecycleCommand::Run {
        if let Err(error) = run_lifecycle_command(command) {
            eprintln!("amarcode-daemon: {error}");
            std::process::exit(1);
        }
        return;
    }

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

fn run_lifecycle_command(command: LifecycleCommand) -> amarcode_daemon::Result<()> {
    let controller = NativeServiceController;
    match command {
        LifecycleCommand::Run => unreachable!("run is handled by the async entrypoint"),
        LifecycleCommand::Install => {
            let executable = std::env::current_exe()?;
            controller.install(&executable, std::env::var_os("PATH").as_deref())?;
            println!(
                "installed Amarcode per-user service from {}",
                display_path(executable)
            );
        }
        LifecycleCommand::Uninstall => {
            controller.uninstall()?;
            println!("uninstalled Amarcode per-user service");
        }
        LifecycleCommand::Purge { confirm_data_loss } => {
            require_data_loss_confirmation(confirm_data_loss)?;
            cleanup::purge_default(&controller)?;
            println!("removed Amarcode per-user service and daemon data");
        }
        LifecycleCommand::Start => {
            controller.start()?;
            println!("started Amarcode per-user service");
        }
        LifecycleCommand::Stop => {
            controller.stop()?;
            println!("stopped Amarcode per-user service");
        }
        LifecycleCommand::Restart => {
            controller.restart()?;
            println!("restarted Amarcode per-user service");
        }
        LifecycleCommand::Status { json } => {
            let status = controller.status()?;
            if json {
                println!("{}", serde_json::to_string(&status)?);
            } else {
                println!(
                    "installed: {}\nstate: {:?}{}",
                    status.installed,
                    status.state,
                    status
                        .definition_path
                        .as_deref()
                        .map(|path| format!("\ndefinition: {path}"))
                        .unwrap_or_default()
                );
            }
        }
    }
    Ok(())
}

fn require_data_loss_confirmation(confirmed: bool) -> amarcode_daemon::Result<()> {
    if confirmed {
        Ok(())
    } else {
        Err(amarcode_daemon::Error::msg(
            "purge permanently deletes local chats, settings, and logs; retry with --confirm-data-loss",
        ))
    }
}

fn display_path(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

async fn run(config: Config) -> amarcode_daemon::Result<()> {
    let app = App::new(config).await?;
    app.run().await
}

#[cfg(test)]
mod tests {
    use super::{require_data_loss_confirmation, Cli, LifecycleCommand};
    use clap::Parser;

    #[test]
    fn parses_every_lifecycle_command() {
        let cases = [
            ("run", LifecycleCommand::Run),
            ("install", LifecycleCommand::Install),
            ("uninstall", LifecycleCommand::Uninstall),
            ("start", LifecycleCommand::Start),
            ("stop", LifecycleCommand::Stop),
            ("restart", LifecycleCommand::Restart),
        ];
        for (argument, expected) in cases {
            assert_eq!(
                Cli::try_parse_from(["amarcode-daemon", argument])
                    .unwrap()
                    .command,
                Some(expected)
            );
        }
        assert_eq!(
            Cli::try_parse_from(["amarcode-daemon", "purge", "--confirm-data-loss"])
                .unwrap()
                .command,
            Some(LifecycleCommand::Purge {
                confirm_data_loss: true
            })
        );
        assert_eq!(
            Cli::try_parse_from(["amarcode-daemon", "status", "--json"])
                .unwrap()
                .command,
            Some(LifecycleCommand::Status { json: true })
        );
    }

    #[test]
    fn missing_command_preserves_legacy_run_behavior() {
        assert_eq!(
            Cli::try_parse_from(["amarcode-daemon"]).unwrap().command,
            None
        );
    }

    #[test]
    fn purge_requires_runtime_confirmation() {
        assert_eq!(
            Cli::try_parse_from(["amarcode-daemon", "purge"])
                .unwrap()
                .command,
            Some(LifecycleCommand::Purge {
                confirm_data_loss: false
            })
        );
        assert!(require_data_loss_confirmation(false).is_err());
        assert!(require_data_loss_confirmation(true).is_ok());
    }
}
