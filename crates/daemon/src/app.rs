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
    instance_lock::InstanceLock,
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
    // Declared after the database-backed fields so they are dropped before
    // process ownership is released.
    _instance_lock: InstanceLock,
}

impl App {
    /// Open the store, seed presets, construct managers.
    pub async fn new(config: Config) -> Result<Self> {
        info!(
            app_dir = %config.app_dir.display(),
            db_path = %config.db_path.display(),
            "initializing application"
        );

        // This must happen before opening SQLite or running crash recovery.
        // Otherwise a losing second process could mutate live run state before
        // it discovers the RPC port is already occupied.
        let instance_lock = InstanceLock::acquire(&config.db_path)?;
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
        let sessions = SessionManager::new(
            Arc::clone(&store),
            agents.clone(),
            events.clone(),
            config.app_dir.join("attachments"),
        );

        info!("application initialized");
        Ok(Self {
            config,
            store,
            events,
            agents,
            chats,
            sessions,
            _instance_lock: instance_lock,
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

#[cfg(test)]
mod tests {
    use crate::{
        protocol::RunStatus,
        store::{AgentRun, Chat},
        App, Config,
    };

    #[tokio::test]
    async fn second_app_cannot_stop_runs_owned_by_the_first_app() {
        let app_dir = std::env::temp_dir().join(format!(
            "amarcode-app-instance-lock-{}",
            uuid::Uuid::new_v4()
        ));
        let config = Config {
            db_path: app_dir.join("workspace.sqlite3"),
            app_dir: app_dir.clone(),
            daemon_addr: "127.0.0.1:0".to_owned(),
        };
        let first = App::new(config.clone()).await.expect("start first app");
        first
            .store
            .create_chat(&Chat {
                id: "chat-1".to_owned(),
                workspace_path: "/tmp/workspace".to_owned(),
                title: "Instance lock test".to_owned(),
                created_at: "2026-01-01T00:00:00Z".to_owned(),
                updated_at: "2026-01-01T00:00:00Z".to_owned(),
                archived_at: None,
            })
            .expect("create chat");
        first
            .store
            .create_run(&AgentRun {
                id: "run-1".to_owned(),
                chat_id: "chat-1".to_owned(),
                agent_id: "codex-acp".to_owned(),
                acp_session_id: Some("session-1".to_owned()),
                status: RunStatus::Running,
                started_at: "2026-01-01T00:00:01Z".to_owned(),
                finished_at: None,
                error_message: None,
            })
            .expect("create active run");

        let error = match App::new(config).await {
            Ok(_) => panic!("second app unexpectedly acquired database ownership"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("already owns database"));
        assert_eq!(
            first
                .store
                .get_run("run-1")
                .expect("read active run")
                .expect("active run exists")
                .status,
            RunStatus::Running,
            "the rejected daemon must not run crash recovery"
        );

        drop(first);
        std::fs::remove_dir_all(app_dir).expect("remove test directory");
    }
}
