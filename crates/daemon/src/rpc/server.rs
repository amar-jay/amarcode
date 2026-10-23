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

use crate::{
    rpc::{connection, websocket},
    App, Result,
};

/// Exactly one listener is active, selected by Config::transport.
pub enum Listener {
    Tcp(TcpListener),
    WebSocket(websocket::Listener),
}

/// Serve RPC clients until Ctrl-C / SIGTERM.
pub async fn run(app: Arc<App>, listener: Listener) -> Result<()> {
    let (socket, websocket) = match listener {
        Listener::Tcp(socket) => (socket, None),
        Listener::WebSocket(listener) => (listener.socket, Some(listener.state)),
    };
    let local = socket.local_addr()?;
    info!(%local, transport = ?app.config.transport, "rpc server listening");
    let mut clients = tokio::task::JoinSet::new();
    let slots = Arc::new(tokio::sync::Semaphore::new(websocket::MAX_CONNECTIONS));
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => {
                info!("shutdown signal received, stopping rpc server");
                break;
            }
            Some(result) = clients.join_next(), if !clients.is_empty() => {
                if let Err(error) = result { warn!(%error, "RPC client task ended unexpectedly"); }
            }
            accepted = socket.accept() => {
                match accepted {
                    Ok((stream, peer)) => {
                        let permit = if websocket.is_some() {
                            match Arc::clone(&slots).try_acquire_owned() {
                                Ok(permit) => Some(permit),
                                Err(_) => { drop(stream); continue; }
                            }
                        } else { None };
                        let websocket = websocket.clone();
                        let app = Arc::clone(&app);
                        clients.spawn(async move {
                            let _permit = permit;
                            let result = match websocket {
                                Some(state) => state.handle(stream, app, &peer.to_string()).await,
                                None => connection::handle(stream, app).await,
                            };
                            if let Err(error) = result {
                                warn!(%peer, %error, "RPC connection ended");
                            }
                        });
                    }
                    Err(error) => error!(%error, "RPC accept failed"),
                }
            }
        }
    }
    clients.abort_all();
    while clients.join_next().await.is_some() {}
    info!("rpc server stopped");
    Ok(())
}

pub(crate) async fn shutdown_signal() {
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
