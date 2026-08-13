//! Persistence for the `agents` table.
//!
//! Methods:
//! - list / upsert agent definitions
//! - seed preset agents on first boot
//!
//! No process spawning — resolving executables is `service::agent_manager`.

use rusqlite::params;

use super::{json_string, now, parse_json, to_error, AgentDefinition, Store};
use crate::Result;

impl Store {
    pub fn seed_presets(&self) -> Result<()> {
        let now = now();
        for (id, name, command, arguments) in [
            ("codex-acp", "Codex", "codex-acp", vec![]),
            ("claude-acp", "Claude Code", "claude-agent-acp", vec![]),
            ("copilot-acp", "GitHub Copilot", "copilot", vec!["--acp"]),
            ("grok-acp", "Grok", "grok", vec!["agent", "stdio"]),
        ] {
            self.save_agent(&AgentDefinition {
                id: id.into(),
                name: name.into(),
                command: command.into(),
                arguments: arguments.into_iter().map(String::from).collect(),
                environment: vec![],
                is_preset: true,
                created_at: now.clone(),
                updated_at: now.clone(),
            })?;
        }
        Ok(())
    }

    pub fn save_agent(&self, agent: &AgentDefinition) -> Result<()> {
        let connection = self.connection()?;
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
