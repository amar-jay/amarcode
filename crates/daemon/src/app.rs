//! Application root: owns shared state and drives the process lifetime.
//!
//! `App` wires together:
//! - `Config` / app data directory
//! - `store` (SQLite)
//! - `service` managers (agents, chats, sessions)
//! - `rpc::server` (TCP listener)
//!
//! `run` binds the listener, accepts connections, and shuts down cleanly on
//! signal. Business logic stays in `service` and `store`, not here.

use std::sync::Arc;

use tokio::{net::TcpListener, sync::broadcast};
use tracing::info;

use crate::{
    protocol::EditorEvent,
    rpc,
    service::{AgentManager, ChatManager, SessionManager},
    store::Store,
    Config, Result,
};

/// Capacity of the in-process event fan-out bus.
const EVENT_BUS_CAPACITY: usize = 256;

/// Top-level daemon process state.
pub struct App {
    pub config: Config,
    pub store: Arc<Store>,
    pub events: broadcast::Sender<EditorEvent>,
    pub agents: AgentManager,
    pub chats: ChatManager,
    pub sessions: SessionManager,
}

impl App {
    /// Open the store, seed presets, construct managers.
    pub async fn new(config: Config) -> Result<Self> {
        info!(
            app_dir = %config.app_dir.display(),
            db_path = %config.db_path.display(),
            "initializing application"
        );

        let store = Arc::new(Store::open(&config.db_path)?);
        store.seed_presets()?;
        let stopped = store.stop_interrupted_runs()?;
        if stopped > 0 {
            info!(count = stopped, "marked interrupted agent runs as stopped");
        }

        let (events, _) = broadcast::channel(EVENT_BUS_CAPACITY);
        let tools_dir = config.app_dir.join("tools");
        std::fs::create_dir_all(&tools_dir).ok();

        let agents = AgentManager::new(Arc::clone(&store), tools_dir);
        let chats = ChatManager::new(Arc::clone(&store), events.clone());
        let sessions = SessionManager::new(Arc::clone(&store), agents.clone(), events.clone());

        info!("application initialized");
        Ok(Self {
            config,
            store,
            events,
            agents,
            chats,
            sessions,
        })
    }

    /// Bind TCP and serve until shutdown.
    pub async fn run(self) -> Result<()> {
        let addr = self.config.daemon_addr.clone();
        info!(%addr, "run starting");

        let listener = TcpListener::bind(&addr).await.map_err(|err| {
            crate::Error::msg(format!("failed to bind rpc listener on {addr}: {err}"))
        })?;

        let app = Arc::new(self);
        rpc::server::run(app, listener).await?;

        info!("run finished");
        Ok(())
    }
}
