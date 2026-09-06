//! Persistence for the `agents` table.
//!
//! Methods:
//! - list / upsert agent definitions
//! - seed preset agents on first boot
//!
//! No process spawning — resolving executables is `service::agent_manager`.

use std::collections::HashSet;

use rusqlite::params;

use super::{json_string, now, parse_json, to_error, AgentDefinition, Store};
use crate::Result;

impl Store {
    /// Replace registry-managed presets while preserving user-created agents
    /// and legacy presets referenced by historical runs.
    pub fn sync_presets(&self, agents: &[AgentDefinition]) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(to_error)?;
        let registry_ids = agents
            .iter()
            .map(|agent| agent.id.as_str())
            .collect::<HashSet<_>>();

        for agent in agents {
            save_agent_in(&transaction, agent)?;
        }

        let stale_ids = {
            let mut statement = transaction
                .prepare("SELECT id FROM agents WHERE is_preset=1")
                .map_err(to_error)?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(to_error)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(to_error)?;
            ids
        };
        for id in stale_ids {
            if !registry_ids.contains(id.as_str()) {
                transaction
                    .execute(
                        "DELETE FROM agents WHERE id=?1 AND NOT EXISTS (SELECT 1 FROM agent_runs WHERE agent_id=?1)",
                        [&id],
                    )
                    .map_err(to_error)?;
            }
        }
        transaction.commit().map_err(to_error)?;
        Ok(())
    }

    pub fn save_agent(&self, agent: &AgentDefinition) -> Result<()> {
        let connection = self.connection()?;
        save_agent_in(&connection, agent)?;
        Ok(())
    }

    pub fn agents(&self) -> Result<Vec<AgentDefinition>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id,name,command,arguments_json,environment_json,is_preset,created_at,updated_at
                 FROM agents ORDER BY is_preset DESC, name",
            )
            .map_err(to_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(AgentDefinition {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    command: row.get(2)?,
                    arguments: parse_json(&row.get::<_, String>(3)?)?,
                    environment: parse_json(&row.get::<_, String>(4)?)?,
                    is_preset: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .map_err(to_error)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(to_error)
    }

    pub fn get_agent(&self, id: &str) -> Result<Option<AgentDefinition>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id,name,command,arguments_json,environment_json,is_preset,created_at,updated_at
                 FROM agents WHERE id=?1",
            )
            .map_err(to_error)?;
        let mut rows = statement
            .query_map([id], |row| {
                Ok(AgentDefinition {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    command: row.get(2)?,
                    arguments: parse_json(&row.get::<_, String>(3)?)?,
                    environment: parse_json(&row.get::<_, String>(4)?)?,
                    is_preset: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .map_err(to_error)?;
        match rows.next() {
            Some(row) => Ok(Some(row.map_err(to_error)?)),
            None => Ok(None),
        }
    }
}

fn save_agent_in(connection: &rusqlite::Connection, agent: &AgentDefinition) -> Result<()> {
    connection
        .execute(
            "INSERT INTO agents (id,name,command,arguments_json,environment_json,is_preset,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, command=excluded.command,
             arguments_json=excluded.arguments_json, environment_json=excluded.environment_json,
             is_preset=excluded.is_preset, updated_at=excluded.updated_at",
            params![
                agent.id,
                agent.name,
                agent.command,
                json_string(&agent.arguments)?,
                json_string(&agent.environment)?,
                agent.is_preset,
                agent.created_at,
                now(),
            ],
        )
        .map_err(to_error)?;
    Ok(())
}
