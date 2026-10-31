//! Process entrypoint for `amarcode-daemon`.
//!
//! Keep this thin:
//! 1. load `Config`
//! 2. init logging (stderr + app-dir file)
//! 3. build `App`
//! 4. run until shutdown signal

// Task Scheduler launches this binary directly. Do not allocate a console;
// explicit CLI invocations can still attach to their existing parent console.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;

use amarcode_daemon::{
    cleanup,
    config::DaemonTransport,
    logging,
    service_control::{NativeServiceController, ServiceController},
    App, Config,
};
use clap::{Parser, Subcommand};
use tracing::{error, info};

#[derive(Debug, Parser)]
#[command(name = "amarcode-daemon", version, about)]
#[command(propagate_version = true)]
struct Cli {
    /// RPC transport; WebSocket address and TLS settings are read from the environment.
    #[arg(long, value_enum, global = true)]
    transport: Option<DaemonTransport>,
    /// Start the shared daemon with public remote access.
    #[arg(long, global = true)]
    remote: bool,
    /// Run remote mode on authenticated loopback without a public tunnel.
    #[arg(long, global = true)]
    no_tunnel: bool,
    /// Run in the foreground when no subcommand is supplied.
    #[command(subcommand)]
    command: Option<LifecycleCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
enum LifecycleCommand {
    /// Run the long-lived RPC server in the foreground.
    Run,
    /// Open an interactive shell on a remote daemon (URL and access token).
    Ssh {
        /// Remote daemon ws:// loopback or wss:// public URL; prompted if omitted.
        url: Option<String>,
        /// Private token file; otherwise AMARCODE_WS_TOKEN_FILE or a hidden prompt.
        #[arg(long)]
        token_file: Option<PathBuf>,
    },
    /// Resume a durable update job in an independent native service/task.
    UpdateSupervisor {
        #[arg(long)]
        release_root: PathBuf,
    },
    /// Manage desktop first installation, repair, and update compatibility.
    Bootstrap {
        #[command(subcommand)]
        command: amarcode_daemon::update::bootstrap::Command,
    },
    /// Open Cloudflare access to an already-running authenticated instance.
    Tunnel,
    /// Enable/disable authenticated access to this instance; restart to apply.
    Access {
        #[arg(value_enum)]
        mode: AccessMode,
    },
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
enum AccessMode {
    Enable,
    Disable,
}

fn main() {
    #[cfg(windows)]
    amarcode_daemon::windows::attach_parent_console();
    let cli = Cli::parse();
    if cli.no_tunnel && !cli.remote {
        eprintln!("--no-tunnel requires --remote; ssh opens a remote shell");
        std::process::exit(2);
    }
    let command = cli.command.unwrap_or(LifecycleCommand::Run);
    // Set the service environment before Tokio creates any worker threads.
    #[cfg(windows)]
    if matches!(
        &command,
        LifecycleCommand::Run | LifecycleCommand::UpdateSupervisor { .. }
    ) {
        if let Err(error) = amarcode_daemon::windows::prepare_environment() {
            eprintln!("amarcode-daemon: {error}");
            std::process::exit(1);
        }
    }
    let isolated = std::env::var_os("AMARCODE_REMOTE_ROOT").is_some();
    let transport = cli.transport.unwrap_or(if cli.remote && isolated {
        DaemonTransport::WebSocket
    } else {
        DaemonTransport::Tcp
    });
    let remote = if cli.remote {
        if command != LifecycleCommand::Run
            || (!isolated && transport != DaemonTransport::Tcp)
            || (isolated && transport != DaemonTransport::WebSocket)
        {
            eprintln!(
                "amarcode-daemon: --remote requires run; standard instances use TCP with an additional authenticated WebSocket listener"
            );
            std::process::exit(2);
        }
        match amarcode_daemon::remote::prepare(!cli.no_tunnel) {
            Ok(setup) => Some(setup),
            Err(error) => {
                eprintln!("amarcode-daemon: {error}");
                std::process::exit(1);
            }
        }
    } else if command == LifecycleCommand::Tunnel {
        match amarcode_daemon::remote::prepare(true) {
            Ok(setup) => Some(setup),
            Err(error) => {
                eprintln!("amarcode-daemon: {error}");
                std::process::exit(1);
            }
        }
    } else if command == LifecycleCommand::Run && transport == DaemonTransport::Tcp {
        match amarcode_daemon::remote::configured() {
            Ok(setup) => setup,
            Err(error) => {
                eprintln!("amarcode-daemon: {error}");
                std::process::exit(1);
            }
        }
    } else {
        None
    };
    let no_tunnel = cli.no_tunnel || !cli.remote;
    async_main(command, transport, remote, no_tunnel);
}

#[tokio::main]
async fn async_main(
    command: LifecycleCommand,
    transport: DaemonTransport,
    remote: Option<amarcode_daemon::remote::Setup>,
    no_tunnel: bool,
) {
    if let LifecycleCommand::Ssh { url, token_file } = command {
        let result = amarcode_daemon::shell::connect(url, token_file).await;
        match result {
            Ok(code) => std::process::exit(code),
            Err(error) => {
                eprintln!("amarcoded ssh: {error}");
                std::process::exit(1);
            }
        }
    }

    if let LifecycleCommand::Bootstrap { ref command } = command {
        if let Err(error) = amarcode_daemon::update::bootstrap::run(command.clone()).await {
            eprintln!("amarcode bootstrap: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let LifecycleCommand::UpdateSupervisor { ref release_root } = command {
        if let Err(error) = amarcode_daemon::update::supervisor::run(release_root).await {
            eprintln!("amarcode update supervisor: {error}");
            std::process::exit(1);
        }
        return;
    }
    if command == LifecycleCommand::Tunnel {
        let outcome = match Config::get(DaemonTransport::Tcp) {
            Ok(config) => {
                amarcode_daemon::remote::tunnel(
                    config.clone(),
                    remote.expect("prepared tunnel setup"),
                )
                .await
            }
            Err(error) => Err(error),
        };
        if let Err(error) = outcome {
            eprintln!("amarcode-daemon: {error}");
            std::process::exit(1);
        }
        return;
    }
    if command != LifecycleCommand::Run {
        if let Err(error) = run_lifecycle_command(command) {
            eprintln!("amarcode-daemon: {error}");
            std::process::exit(1);
        }
        return;
    }

    // Config before logging so we know where to write daemon.log.
    // Failures here go to stderr because the subscriber is not up yet.
    let config = match Config::get(transport) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("amarcode-daemon: failed to load config: {err}");
            std::process::exit(1);
        }
    };

    let quiet_remote = remote.is_some();
    let logging = if quiet_remote {
        logging::init_remote(config)
    } else {
        logging::init(config)
    };
    if let Err(err) = logging {
        eprintln!("amarcode-daemon: failed to initialize logging: {err}");
        std::process::exit(1);
    }

    info!(
        service = "amarcode-daemon",
        version = env!("CARGO_PKG_VERSION"),
        transport = ?config.transport,
        addr = %config.daemon_addr,
        app_dir = %config.app_dir.display(),
        db_path = %config.db_path.display(),
        log_path = %logging::log_file_path(&config.app_dir).display(),
        "starting"
    );

    let outcome = match remote {
        Some(setup) if config.transport == DaemonTransport::Tcp => {
            amarcode_daemon::remote::run_shared(config.clone(), setup, no_tunnel).await
        }
        Some(setup) => amarcode_daemon::remote::run(config.clone(), setup, no_tunnel).await,
        None => run(config.clone()).await,
    };
    if let Err(err) = outcome {
        error!(error = %err, "daemon exited with error");
        if quiet_remote {
            eprintln!("amarcode-daemon: {err}");
        }
        std::process::exit(1);
    }

    info!("shutdown complete");
}

fn run_lifecycle_command(command: LifecycleCommand) -> amarcode_daemon::Result<()> {
    let controller = NativeServiceController;
    match command {
        LifecycleCommand::Run
        | LifecycleCommand::Ssh { .. }
        | LifecycleCommand::Tunnel
        | LifecycleCommand::UpdateSupervisor { .. }
        | LifecycleCommand::Bootstrap { .. } => {
            unreachable!("run is handled by the async entrypoint")
        }
        LifecycleCommand::Access { mode } => {
            amarcode_daemon::remote::configure_access(mode == AccessMode::Enable)?;
        }
        LifecycleCommand::Install => {
            let executable = std::env::current_exe()?;
            controller.install(&executable, std::env::var_os("PATH").as_deref())?;
            println!(
                "installed Amarcode per-user service from {}",
                display_path(executable)
            );
        }
        LifecycleCommand::Uninstall => {
            amarcode_daemon::service_control::remove_update_supervisor()?;
            controller.uninstall()?;
            println!("uninstalled Amarcode per-user service");
        }
        LifecycleCommand::Purge { confirm_data_loss } => {
            require_data_loss_confirmation(confirm_data_loss)?;
            amarcode_daemon::service_control::remove_update_supervisor()?;
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
            (
                "ssh",
                LifecycleCommand::Ssh {
                    url: None,
                    token_file: None,
                },
            ),
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
    fn bootstrap_cli_accepts_the_approved_update_version_as_a_value() {
        use amarcode_daemon::update::bootstrap::Command;
        assert_eq!(
            Cli::try_parse_from([
                "amarcode-daemon",
                "bootstrap",
                "start-update",
                "--version",
                "1.2.3"
            ])
            .unwrap()
            .command,
            Some(LifecycleCommand::Bootstrap {
                command: Command::StartUpdate {
                    version: "1.2.3".into()
                }
            })
        );
        assert!(Cli::try_parse_from(["amarcode-daemon", "bootstrap", "start-update"]).is_err());
        for (arg, command) in [("ensure", Command::Ensure), ("install", Command::Install)] {
            assert_eq!(
                Cli::try_parse_from(["amarcode-daemon", "bootstrap", arg])
                    .unwrap()
                    .command,
                Some(LifecycleCommand::Bootstrap { command })
            );
        }
    }

    #[test]
    fn transport_flag_defaults_to_tcp_and_accepts_websocket() {
        use amarcode_daemon::config::DaemonTransport;
        assert_eq!(
            Cli::try_parse_from(["amarcode-daemon", "run"])
                .unwrap()
                .transport
                .unwrap_or_default(),
            DaemonTransport::Tcp
        );
        for args in [
            vec!["amarcode-daemon", "run", "--transport", "websocket"],
            vec!["amarcode-daemon", "--transport", "websocket", "run"],
        ] {
            assert_eq!(
                Cli::try_parse_from(args).unwrap().transport,
                Some(DaemonTransport::WebSocket)
            );
        }
        assert!(Cli::try_parse_from(["amarcode-daemon", "run", "--transport", "udp"]).is_err());
    }

    #[test]
    fn missing_command_defaults_to_run() {
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
