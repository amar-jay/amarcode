//! TCP listener and accept loop.
//!
//! - bind to `Config::daemon_addr`
//! - accept connections
//! - spawn a task per socket → `connection::handle`
//! - cooperate with process shutdown (drop listener / cancel tasks)
//!
//! Shared state is `Arc<App>` (or a slimmer `RpcState`).

use std::sync::Arc;

use tokio::net::TcpListener;
use tracing::{error, info, warn};

use crate::{App, Result, rpc::connection};

/// Serve RPC clients on `listener` until Ctrl-C / SIGTERM.
pub async fn run(app: Arc<App>, listener: TcpListener) -> Result<()> {
    let local = listener.local_addr()?;
    info!(%local, "rpc server listening");

    loop {
        tokio::select! {
            biased;

            _ = shutdown_signal() => {
                info!("shutdown signal received, stopping rpc server");
                break;
            }

            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, peer)) => {
                        let app = Arc::clone(&app);
                        tokio::spawn(async move {
                            if let Err(err) = connection::handle(stream, app).await {
                                // Disconnect mid-write is common; keep it quiet unless useful.
                                warn!(%peer, error = %err, "connection ended with error");
                            }
                        });
                    }
                    Err(err) => {
                        // Transient accept errors should not kill the process.
                        error!(error = %err, "accept failed");
                    }
                }
            }
        }
    }

    info!("rpc server stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            error!(error = %err, "failed to install Ctrl-C handler");
            // Fall through to pending so we don't spin.
            std::future::pending::<()>().await
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(err) => {
                error!(error = %err, "failed to install SIGTERM handler");
                std::future::pending::<()>().await
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
