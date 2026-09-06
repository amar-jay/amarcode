//! SQLite persistence layer.
//!
//! Open the database, apply `migrations/`, and expose table-focused modules.
//! Prefer: one file per aggregate, short transactional methods, WAL + FKs on.
//!
//! Row types reuse [`crate::protocol`] enums for status/role/kind/direction so
//! there is a single domain vocabulary. SQL still stores TEXT; conversion is
//! only at this boundary (`as_str` / `parse`).

use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
};

use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{
    protocol::{MessageRole, MessageStatus, RpcDirection, RpcEnvelope, RunStatus},
    Error, Result,
};

pub use crate::protocol::{AgentDefinition, Chat, Message, MessagePart};

pub mod agents;
pub mod chats;
pub mod events;
pub mod messages;
pub mod runs;

/// Ordered migrations embedded at compile time (id, SQL).
const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_initial",
        include_str!("../../migrations/0001_initial.sql"),
    ),
    (
        "0002_agents_available",
        include_str!("../../migrations/0002_agents_available.sql"),
    ),
    (
        "0003_chat_session_config",
        include_str!("../../migrations/0003_chat_session_config.sql"),
    ),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRun {
    pub id: String,
    pub chat_id: String,
    pub agent_id: String,
    pub acp_session_id: Option<String>,
    pub status: RunStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error_message: Option<String>,
}

/// Persisted ACP traffic row (DB form of [`RpcEnvelope`] plus run metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpEvent {
    pub id: i64,
    pub agent_run_id: String,
    pub direction: RpcDirection,
    pub method: String,
    pub payload_json: String,
    pub created_at: String,
}

impl AcpEvent {
    /// Build a row ready for insert (`id` is assigned by SQLite).
    pub fn from_envelope(agent_run_id: impl Into<String>, envelope: &RpcEnvelope) -> Result<Self> {
        Ok(Self {
            id: 0,
            agent_run_id: agent_run_id.into(),
            direction: envelope.direction,
            method: envelope.method.clone(),
            payload_json: serde_json::to_string(&envelope.payload).map_err(to_error)?,
            created_at: now(),
        })
    }

    pub fn payload_value(&self) -> Result<serde_json::Value> {
        serde_json::from_str(&self.payload_json).map_err(to_error)
    }

    pub fn to_envelope(&self) -> Result<RpcEnvelope> {
        Ok(RpcEnvelope {
            direction: self.direction,
            method: self.method.clone(),
            payload: self.payload_value()?,
        })
    }
}

/// Thread-safe SQLite store.
pub struct Store(Mutex<Connection>);

impl Store {
    /// Open (or create) the database at `path`, enable WAL/FKs, apply migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(to_error)?;
            }
        }

        let connection = Connection::open(path).map_err(to_error)?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(to_error)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(to_error)?;

        apply_migrations(&connection)?;

        Ok(Self(Mutex::new(connection)))
    }

    pub(crate) fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.0
            .lock()
            .map_err(|_| Error::msg("database lock poisoned"))
    }

    pub(crate) fn touch_chat(&self, chat_id: &str) -> Result<()> {
        self.connection()?
            .execute(
                "UPDATE chats SET updated_at=?2 WHERE id=?1",
                rusqlite::params![chat_id, now()],
            )
            .map_err(to_error)?;
        Ok(())
    }
}

pub(crate) fn now() -> String {
    Utc::now().to_rfc3339()
}

fn apply_migrations(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                id TEXT PRIMARY KEY,
                applied_at TEXT NOT NULL
            ) STRICT;",
        )
        .map_err(to_error)?;

    // Existing databases created before migration tracking already ran 0001.
    if table_exists(connection, "agents")? && !migration_applied(connection, "0001_initial")? {
        record_migration(connection, "0001_initial")?;
    }

    for &(id, sql) in MIGRATIONS {
        if migration_applied(connection, id)? {
            continue;
        }
        connection.execute_batch(sql).map_err(to_error)?;
        record_migration(connection, id)?;
    }
    Ok(())
}

fn table_exists(connection: &Connection, name: &str) -> Result<bool> {
    let exists: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    Ok(exists > 0)
}

fn migration_applied(connection: &Connection, id: &str) -> Result<bool> {
    let exists: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM _migrations WHERE id=?1",
            [id],
            |row| row.get(0),
        )
        .map_err(to_error)?;
    Ok(exists > 0)
}

fn record_migration(connection: &Connection, id: &str) -> Result<()> {
    connection
        .execute(
            "INSERT INTO _migrations (id, applied_at) VALUES (?1, ?2)",
            rusqlite::params![id, now()],
        )
        .map_err(to_error)?;
    Ok(())
}

pub(crate) fn to_error(error: impl std::fmt::Display) -> Error {
    Error::msg(error.to_string())
}

pub(crate) fn json_string<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(to_error)
}

pub(crate) fn parse_json<T: for<'de> Deserialize<'de>>(value: &str) -> rusqlite::Result<T> {
    serde_json::from_str(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

/// Parse a domain enum from a SQL TEXT column inside a row mapper.
pub(crate) fn cell_parse<T, F>(value: String, parse: F) -> rusqlite::Result<T>
where
    F: FnOnce(&str) -> std::result::Result<T, String>,
{
    parse(&value).map_err(|msg| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(Error::msg(msg)),
        )
    })
}

pub(crate) fn map_chat(row: &rusqlite::Row<'_>) -> rusqlite::Result<Chat> {
    Ok(Chat {
        id: row.get(0)?,
        workspace_path: row.get(1)?,
        title: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        archived_at: row.get(5)?,
    })
}

pub(crate) fn map_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        chat_id: row.get(1)?,
        agent_run_id: row.get(2)?,
        role: cell_parse(row.get(3)?, MessageRole::parse)?,
        content: row.get(4)?,
        status: cell_parse(row.get(5)?, MessageStatus::parse)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}
