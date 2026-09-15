//! Persistence for the selectively retained `acp_events` raw traffic log.
//!
//! Rows are the durable form of [`crate::protocol::RpcEnvelope`]. Prefer
//! [`Store::save_acp_envelope`] from `service::session` for durable milestones.

use chrono::{Duration, Utc};
use rusqlite::params;

use super::{cell_parse, to_error, AcpEvent, Store};
use crate::{
    protocol::{RpcDirection, RpcEnvelope},
    Result,
};

impl Store {
    pub fn daemon_config(&self) -> amarcode_protocol::rpc::DaemonConfigResult {
        amarcode_protocol::rpc::DaemonConfigResult {
            store_acp_events: self.acp_event_recording_enabled(),
            acp_event_retention_days: self.acp_event_retention_days(),
        }
    }

    pub fn set_daemon_config(
        &self,
        store_acp_events: bool,
        retention_days: u32,
    ) -> Result<amarcode_protocol::rpc::DaemonConfigResult> {
        if !(1..=365).contains(&retention_days) {
            return Err(crate::Error::msg(
                "ACP event retention must be between 1 and 365 days",
            ));
        }
        self.connection()?
            .execute(
                "UPDATE daemon_config SET store_acp_events=?1, acp_event_retention_days=?2 WHERE id=1",
                params![store_acp_events, retention_days],
            )
            .map_err(to_error)?;
        self.set_cached_acp_event_config(store_acp_events, retention_days);
        self.prune_acp_events()?;
        Ok(self.daemon_config())
    }

    pub fn prune_acp_events(&self) -> Result<usize> {
        let cutoff =
            (Utc::now() - Duration::days(i64::from(self.acp_event_retention_days()))).to_rfc3339();
        self.connection()?
            .execute("DELETE FROM acp_events WHERE created_at < ?1", [cutoff])
            .map_err(to_error)
    }

    pub fn save_acp_event(&self, event: &AcpEvent) -> Result<i64> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO acp_events (agent_run_id,direction,method,payload_json,created_at)
                 VALUES (?1,?2,?3,?4,?5)",
                params![
                    event.agent_run_id,
                    event.direction.as_str(),
                    event.method,
                    event.payload_json,
                    event.created_at,
                ],
            )
            .map_err(to_error)?;
        Ok(connection.last_insert_rowid())
    }

    /// Store-first helper: map envelope → row and insert.
    pub fn save_acp_envelope(&self, agent_run_id: &str, envelope: &RpcEnvelope) -> Result<i64> {
        if !self.acp_event_recording_enabled() {
            return Ok(0);
        }
        let now = Utc::now().timestamp();
        if self.should_prune_acp_events(now) {
            self.prune_acp_events()?;
        }
        let event = AcpEvent::from_envelope(agent_run_id, envelope)?;
        self.save_acp_event(&event)
    }

    pub fn acp_events(&self, agent_run_id: &str) -> Result<Vec<AcpEvent>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id,agent_run_id,direction,method,payload_json,created_at
                 FROM acp_events WHERE agent_run_id=?1 ORDER BY id",
            )
            .map_err(to_error)?;
        let rows = statement
            .query_map(params![agent_run_id], |row| {
                Ok(AcpEvent {
                    id: row.get(0)?,
                    agent_run_id: row.get(1)?,
                    direction: cell_parse(row.get(2)?, RpcDirection::parse)?,
                    method: row.get(3)?,
                    payload_json: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(to_error)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(to_error)
    }

    /// Return all raw ACP traffic owned by every run in a chat.
    /// Event ids are globally monotonic, so this preserves durable chronology
    /// even when a chat contains multiple runs.
    pub fn acp_events_for_chat(&self, chat_id: &str) -> Result<Vec<AcpEvent>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT e.id,e.agent_run_id,e.direction,e.method,e.payload_json,e.created_at
                 FROM acp_events e
                 INNER JOIN agent_runs r ON r.id=e.agent_run_id
                 WHERE r.chat_id=?1
                 ORDER BY e.id",
            )
            .map_err(to_error)?;
        let rows = statement
            .query_map(params![chat_id], map_event)
            .map_err(to_error)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(to_error)
    }
}

fn map_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<AcpEvent> {
    Ok(AcpEvent {
        id: row.get(0)?,
        agent_run_id: row.get(1)?,
        direction: cell_parse(row.get(2)?, RpcDirection::parse)?,
        method: row.get(3)?,
        payload_json: row.get(4)?,
        created_at: row.get(5)?,
    })
}

#[cfg(test)]
mod activity_tests {
    use serde_json::json;

    use super::*;
    use crate::{
        protocol::{AgentDefinition, Chat, RpcDirection, RpcEnvelope, RunStatus},
        store::AgentRun,
    };

    #[test]
    fn chat_events_include_all_of_its_runs_and_exclude_other_chats() {
        let store = Store::open(std::path::Path::new(":memory:")).expect("open store");
        store
            .set_daemon_config(true, 7)
            .expect("enable ACP event recording");
        store
            .save_agent(&AgentDefinition {
                id: "agent-1".into(),
                name: "Agent".into(),
                command: "agent".into(),
                arguments: vec![],
                environment: vec![],
                available: true,
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
            })
            .expect("save agent");
        for id in ["chat-1", "chat-2"] {
            store
                .create_chat(&Chat {
                    id: id.into(),
                    workspace_path: "/tmp/workspace".into(),
                    title: id.into(),
                    created_at: "2026-01-01T00:00:00Z".into(),
                    updated_at: "2026-01-01T00:00:00Z".into(),
                    archived_at: None,
                })
                .expect("save chat");
        }
        for (id, chat_id) in [
            ("run-1", "chat-1"),
            ("run-2", "chat-1"),
            ("run-3", "chat-2"),
        ] {
            store
                .create_run(&AgentRun {
                    id: id.into(),
                    chat_id: chat_id.into(),
                    agent_id: "agent-1".into(),
                    acp_session_id: None,
                    status: RunStatus::Completed,
                    started_at: "2026-01-01T00:00:00Z".into(),
                    finished_at: Some("2026-01-01T00:00:01Z".into()),
                    error_message: None,
                })
                .expect("save run");
        }
        for run_id in ["run-1", "run-3", "run-2"] {
            store
                .save_acp_envelope(
                    run_id,
                    &RpcEnvelope {
                        direction: RpcDirection::Received,
                        method: "session/update".into(),
                        payload: json!({ "run": run_id }),
                    },
                )
                .expect("save event");
        }

        let events = store.acp_events_for_chat("chat-1").expect("chat events");
        assert_eq!(
            events
                .iter()
                .map(|event| event.agent_run_id.as_str())
                .collect::<Vec<_>>(),
            vec!["run-1", "run-2"]
        );
        assert!(events[0].id < events[1].id);
    }
}

#[cfg(test)]
mod config_tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn acp_event_recording_defaults_off_and_config_persists() {
        let directory =
            std::env::temp_dir().join(format!("amarcode-daemon-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).expect("create test directory");
        let path = directory.join("store.sqlite3");
        let store = Store::open(&path).expect("open store");

        assert!(!store.daemon_config().store_acp_events);
        assert_eq!(store.daemon_config().acp_event_retention_days, 7);
        let skipped = store
            .save_acp_envelope(
                "missing-run",
                &RpcEnvelope {
                    direction: RpcDirection::Received,
                    method: "session/update".into(),
                    payload: json!({"large": "diagnostic payload"}),
                },
            )
            .expect("disabled recording is a no-op");
        assert_eq!(skipped, 0);

        store.set_daemon_config(true, 30).expect("update config");
        drop(store);
        let reopened = Store::open(&path).expect("reopen store");
        assert!(reopened.daemon_config().store_acp_events);
        assert_eq!(reopened.daemon_config().acp_event_retention_days, 30);

        drop(reopened);
        std::fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn vacuum_reports_reclaimed_database_pages() {
        let directory =
            std::env::temp_dir().join(format!("amarcode-vacuum-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).expect("create test directory");
        let path = directory.join("store.sqlite3");
        let store = Store::open(&path).expect("open store");
        store
            .connection()
            .expect("connection")
            .execute_batch(
                "CREATE TABLE vacuum_fixture (value BLOB);
                 INSERT INTO vacuum_fixture VALUES (zeroblob(2097152));
                 DELETE FROM vacuum_fixture;",
            )
            .expect("create free pages");

        let result = store.vacuum_database().expect("vacuum database");
        assert!(result.reclaimed_bytes > 0);
        assert!(result.after_bytes < result.before_bytes);

        drop(store);
        std::fs::remove_dir_all(directory).expect("remove test directory");
    }
}
