use crate::models::{AgentDefinition, AgentEvent, SessionSummary};
use chrono::Utc;
use rusqlite::{params, Connection};
use std::{path::Path, sync::Mutex};

pub struct Store(Mutex<Connection>);

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        let connection = Connection::open(path).map_err(|error| error.to_string())?;
        connection.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS agents (
              id TEXT PRIMARY KEY, name TEXT NOT NULL, command TEXT NOT NULL,
              arguments_json TEXT NOT NULL, environment_json TEXT NOT NULL, is_preset INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sessions (
              id TEXT PRIMARY KEY, workspace_path TEXT NOT NULL, agent_id TEXT NOT NULL,
              status TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
              id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL,
              created_at TEXT NOT NULL, event_json TEXT NOT NULL
            );",
        ).map_err(|error| error.to_string())?;
        Ok(Self(Mutex::new(connection)))
    }

    pub fn seed_presets(&self) -> Result<(), String> {
        for agent in [
            AgentDefinition {
                id: "claude-acp".into(),
                name: "Claude Agent ACP".into(),
                command: "claude-agent-acp".into(),
                arguments: vec![],
                environment: vec![],
                is_preset: true,
            },
            AgentDefinition {
                id: "copilot-acp".into(),
                name: "GitHub Copilot ACP".into(),
                command: "copilot".into(),
                arguments: vec!["--acp".into()],
                environment: vec![],
                is_preset: true,
            },
        ] {
            self.save_agent(&agent)?;
        }
        Ok(())
    }

    pub fn save_agent(&self, agent: &AgentDefinition) -> Result<(), String> {
        self.0.lock().map_err(|_| "Database lock poisoned".to_string())?.execute(
            "INSERT INTO agents (id,name,command,arguments_json,environment_json,is_preset) VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name,command=excluded.command,arguments_json=excluded.arguments_json,environment_json=excluded.environment_json,is_preset=excluded.is_preset",
            params![agent.id, agent.name, agent.command, serde_json::to_string(&agent.arguments).unwrap(), serde_json::to_string(&agent.environment).unwrap(), agent.is_preset],
        ).map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn agents(&self) -> Result<Vec<AgentDefinition>, String> {
        let connection = self
            .0
            .lock()
            .map_err(|_| "Database lock poisoned".to_string())?;
        let mut statement = connection.prepare("SELECT id,name,command,arguments_json,environment_json,is_preset FROM agents ORDER BY is_preset DESC,name").map_err(|error| error.to_string())?;
        let result = statement
            .query_map([], |row| {
                Ok(AgentDefinition {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    command: row.get(2)?,
                    arguments: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
                    environment: serde_json::from_str(&row.get::<_, String>(4)?)
                        .unwrap_or_default(),
                    is_preset: row.get(5)?,
                })
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string());
        result
    }

    pub fn create_session(&self, session: &SessionSummary) -> Result<(), String> {
        self.0.lock().map_err(|_| "Database lock poisoned".to_string())?.execute(
            "INSERT INTO sessions (id,workspace_path,agent_id,status,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![session.id, session.workspace_path, session.agent_id, session.status, session.created_at, session.updated_at],
        ).map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn update_session_status(&self, id: &str, status: &str) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "Database lock poisoned".to_string())?
            .execute(
                "UPDATE sessions SET status=?2, updated_at=?3 WHERE id=?1",
                params![id, status, Utc::now().to_rfc3339()],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn sessions(&self) -> Result<Vec<SessionSummary>, String> {
        let connection = self
            .0
            .lock()
            .map_err(|_| "Database lock poisoned".to_string())?;
        let mut statement = connection.prepare("SELECT id,workspace_path,agent_id,status,created_at,updated_at FROM sessions ORDER BY updated_at DESC").map_err(|error| error.to_string())?;
        let result = statement
            .query_map([], |row| {
                Ok(SessionSummary {
                    id: row.get(0)?,
                    workspace_path: row.get(1)?,
                    agent_id: row.get(2)?,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string());
        result
    }

    pub fn save_event(&self, event: &AgentEvent) -> Result<(), String> {
        let session_id = match event {
            AgentEvent::Status { session_id, .. }
            | AgentEvent::Message { session_id, .. }
            | AgentEvent::Activity { session_id, .. }
            | AgentEvent::Request { session_id, .. }
            | AgentEvent::ProtocolError { session_id, .. } => session_id,
        };
        self.0
            .lock()
            .map_err(|_| "Database lock poisoned".to_string())?
            .execute(
                "INSERT INTO events (session_id,created_at,event_json) VALUES (?1,?2,?3)",
                params![
                    session_id,
                    Utc::now().to_rfc3339(),
                    serde_json::to_string(event).unwrap()
                ],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn events(&self, session_id: &str) -> Result<Vec<AgentEvent>, String> {
        let connection = self
            .0
            .lock()
            .map_err(|_| "Database lock poisoned".to_string())?;
        let mut statement = connection
            .prepare("SELECT event_json FROM events WHERE session_id=?1 ORDER BY id")
            .map_err(|error| error.to_string())?;
        let result = statement
            .query_map([session_id], |row| {
                Ok(serde_json::from_str::<AgentEvent>(&row.get::<_, String>(0)?).unwrap())
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string());
        result
    }
}
