//! Persistence for the `agents` table.
//!
//! Methods:
//! - list / upsert agent definitions
//! - sync registry catalog into SQLite
//! - persist host availability
//!
//! No process spawning — resolving executables is `service::agent_manager`.

use std::collections::HashSet;

use rusqlite::params;

use super::{json_string, now, parse_json, to_error, AgentDefinition, Store};
use crate::Result;

impl Store {
    /// Replace registry-managed agents while preserving rows referenced by
    /// historical runs (and any agents still present in the registry set).
    pub fn sync_presets(&self, agents: &[AgentDefinition]) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(to_error)?;
        let registry_ids = agents
            .iter()
            .map(|agent| agent.id.as_str())
            .collect::<HashSet<_>>();

        for agent in agents {
            save_registry_agent_in(&transaction, agent)?;
        }

        let stale_ids = {
            let mut statement = transaction
                .prepare("SELECT id FROM agents WHERE source='registry'")
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

    pub fn save_builtin_agent(&self, agent: &AgentDefinition) -> Result<()> {
        let connection = self.connection()?;
        save_owned_agent_in(&connection, agent, "builtin")
    }

    pub fn set_agent_available(&self, id: &str, available: bool) -> Result<()> {
        self.connection()?
            .execute(
                "UPDATE agents SET available=?2, updated_at=?3 WHERE id=?1",
                params![id, available, now()],
            )
            .map_err(to_error)?;
        Ok(())
    }

    pub fn agents(&self) -> Result<Vec<AgentDefinition>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id,name,command,arguments_json,environment_json,available,created_at,updated_at
                 FROM agents ORDER BY name",
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
                    available: row.get(5)?,
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
                "SELECT id,name,command,arguments_json,environment_json,available,created_at,updated_at
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
                    available: row.get(5)?,
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
            "INSERT INTO agents (id,name,command,arguments_json,environment_json,available,created_at,updated_at,source)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'custom')
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, command=excluded.command,
             arguments_json=excluded.arguments_json, environment_json=excluded.environment_json,
             updated_at=excluded.updated_at",
            params![
                agent.id,
                agent.name,
                agent.command,
                json_string(&agent.arguments)?,
                json_string(&agent.environment)?,
                agent.available,
                agent.created_at,
                now(),
            ],
        )
        .map_err(to_error)?;
    Ok(())
}

/// Registry sync upsert that keeps an already-installed absolute launch path.
fn save_registry_agent_in(
    connection: &rusqlite::Connection,
    agent: &AgentDefinition,
) -> Result<()> {
    connection
        .execute(
            "INSERT INTO agents (id,name,command,arguments_json,environment_json,available,created_at,updated_at,source)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'registry')
             ON CONFLICT(id) DO UPDATE SET
               name=excluded.name,
               command=CASE
                 WHEN substr(agents.command, 1, 1) IN ('/', '\\')
                   OR substr(agents.command, 2, 1) = ':'
                 THEN agents.command
                 ELSE excluded.command
               END,
               arguments_json=CASE
                 WHEN substr(agents.command, 1, 1) IN ('/', '\\')
                   OR substr(agents.command, 2, 1) = ':'
                 THEN agents.arguments_json
                 ELSE excluded.arguments_json
               END,
               environment_json=CASE
                 WHEN substr(agents.command, 1, 1) IN ('/', '\\')
                   OR substr(agents.command, 2, 1) = ':'
                 THEN agents.environment_json
                 ELSE excluded.environment_json
               END,
               source='registry',
               updated_at=excluded.updated_at",
            params![
                agent.id,
                agent.name,
                agent.command,
                json_string(&agent.arguments)?,
                json_string(&agent.environment)?,
                agent.available,
                agent.created_at,
                now(),
            ],
        )
        .map_err(to_error)?;
    Ok(())
}

fn save_owned_agent_in(
    connection: &rusqlite::Connection,
    agent: &AgentDefinition,
    source: &str,
) -> Result<()> {
    connection
        .execute(
            "INSERT INTO agents (id,name,command,arguments_json,environment_json,available,created_at,updated_at,source)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, command=excluded.command,
             arguments_json=excluded.arguments_json, environment_json=excluded.environment_json,
             source=excluded.source, updated_at=excluded.updated_at",
            params![
                agent.id,
                agent.name,
                agent.command,
                json_string(&agent.arguments)?,
                json_string(&agent.environment)?,
                agent.available,
                agent.created_at,
                now(),
                source,
            ],
        )
        .map_err(to_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn agent(id: &str) -> AgentDefinition {
        AgentDefinition {
            id: id.into(),
            name: id.into(),
            command: "example".into(),
            arguments: vec![],
            environment: vec![],
            available: false,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn registry_sync_preserves_builtin_and_custom_agents() {
        let store = Store::open(Path::new(":memory:")).expect("open store");
        store.save_agent(&agent("custom-agent")).expect("custom");
        store
            .save_builtin_agent(&agent("builtin-agent"))
            .expect("builtin");
        store
            .sync_presets(&[agent("registry-agent")])
            .expect("first registry sync");
        store.sync_presets(&[]).expect("empty registry sync");

        assert!(store.get_agent("custom-agent").unwrap().is_some());
        assert!(store.get_agent("builtin-agent").unwrap().is_some());
        assert!(store.get_agent("registry-agent").unwrap().is_none());
    }
}
