//! Persistence for the `acp_events` raw traffic log.
//!
//! Rows are the durable form of [`crate::protocol::RpcEnvelope`]. Prefer
//! [`Store::save_acp_envelope`] from `service::session` after each ACP unit.

use rusqlite::params;

use super::{cell_parse, to_error, AcpEvent, Store};
use crate::{
    protocol::{RpcDirection, RpcEnvelope},
    Result,
};

impl Store {
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
}
