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
    Error, Result,
    protocol::{MessagePartKind, MessageRole, MessageStatus, RpcDirection, RpcEnvelope, RunStatus},
};

pub mod agents;
pub mod chats;
pub mod events;
pub mod messages;
pub mod runs;

/// Ordered migrations embedded at compile time (filename order).
const MIGRATIONS: &[&str] = &[include_str!("../../migrations/0001_initial.sql")];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub id: String,
    pub name: String,
    pub command: String,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub is_preset: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub workspace_path: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub chat_id: String,
    pub agent_run_id: Option<String>,
    pub role: MessageRole,
    pub content: String,
    pub status: MessageStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePart {
    pub message_id: String,
    pub ordinal: i64,
    pub kind: MessagePartKind,
    pub content_json: String,
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

        for sql in MIGRATIONS {
            connection.execute_batch(sql).map_err(to_error)?;
        }

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
