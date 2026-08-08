//! Persistence for the `agent_runs` table.

use rusqlite::params;

use super::{cell_parse, now, to_error, AgentRun, Store};
use crate::{protocol::RunStatus, Result};

impl Store {
    pub fn create_run(&self, run: &AgentRun) -> Result<()> {
        self.connection()?
            .execute(
                "INSERT INTO agent_runs
                 (id,chat_id,agent_id,acp_session_id,status,started_at,finished_at,error_message)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    run.id,
                    run.chat_id,
                    run.agent_id,
                    run.acp_session_id,
                    run.status.as_str(),
                    run.started_at,
                    run.finished_at,
                    run.error_message,
                ],
            )
            .map_err(to_error)?;
        self.touch_chat(&run.chat_id)
    }

    pub fn update_run(
        &self,
        id: &str,
        status: RunStatus,
        acp_session_id: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        let finished_at = status.is_terminal().then(now);
        self.connection()?
            .execute(
                "UPDATE agent_runs SET status=?2, acp_session_id=COALESCE(?3, acp_session_id),
                 finished_at=COALESCE(?4, finished_at), error_message=?5 WHERE id=?1",
                params![id, status.as_str(), acp_session_id, finished_at, error],
            )
            .map_err(to_error)?;
        Ok(())
    }

    pub fn stop_interrupted_runs(&self) -> Result<usize> {
        self.connection()?
            .execute(
                "UPDATE agent_runs SET status=?1, finished_at=?2
                 WHERE status IN (?3, ?4)",
                params![
                    RunStatus::Stopped.as_str(),
                    now(),
                    RunStatus::Starting.as_str(),
                    RunStatus::Running.as_str(),
                ],
            )
            .map_err(to_error)
    }

    pub fn get_run(&self, id: &str) -> Result<Option<AgentRun>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id,chat_id,agent_id,acp_session_id,status,started_at,finished_at,error_message
                 FROM agent_runs WHERE id=?1",
            )
            .map_err(to_error)?;
        let mut rows = statement
            .query_map(params![id], map_run)
            .map_err(to_error)?;
        match rows.next() {
            Some(row) => Ok(Some(row.map_err(to_error)?)),
            None => Ok(None),
        }
    }

    pub fn list_runs_for_chat(&self, chat_id: &str) -> Result<Vec<AgentRun>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id,chat_id,agent_id,acp_session_id,status,started_at,finished_at,error_message
                 FROM agent_runs WHERE chat_id=?1 ORDER BY started_at DESC",
            )
            .map_err(to_error)?;
        let rows = statement
            .query_map(params![chat_id], map_run)
            .map_err(to_error)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(to_error)
    }
}

fn map_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentRun> {
    Ok(AgentRun {
        id: row.get(0)?,
        chat_id: row.get(1)?,
        agent_id: row.get(2)?,
        acp_session_id: row.get(3)?,
        status: cell_parse(row.get(4)?, RunStatus::parse)?,
        started_at: row.get(5)?,
        finished_at: row.get(6)?,
        error_message: row.get(7)?,
    })
}
