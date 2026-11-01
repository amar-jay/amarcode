use super::{now, to_error, Store};
use crate::{
    protocol::rpc::{PromptParams, PromptQueueState, QueuedPrompt},
    Error, Result,
};
use rusqlite::{params, OptionalExtension};

impl Store {
    pub fn recover_prompt_queues(&self) -> Result<()> {
        let connection = self.connection()?;
        connection.execute("INSERT INTO prompt_queue_state(chat_id,paused,reason)
          SELECT DISTINCT chat_id,1,'Daemon restarted. Resume to send waiting messages.' FROM prompt_jobs WHERE status IN ('queued','running')
          ON CONFLICT(chat_id) DO UPDATE SET paused=1,reason=excluded.reason", []).map_err(to_error)?;
        connection.execute("UPDATE prompt_jobs SET status='interrupted',error='Interrupted by daemon restart; not replayed automatically.' WHERE status='running'", []).map_err(to_error)?;
        Ok(())
    }
    pub fn prompt_queue(&self, chat_id: &str) -> Result<PromptQueueState> {
        let connection = self.connection()?;
        let mut state = connection
            .query_row(
                "SELECT paused,reason FROM prompt_queue_state WHERE chat_id=?1",
                [chat_id],
                |r| {
                    Ok(PromptQueueState {
                        paused: r.get(0)?,
                        reason: r.get(1)?,
                        items: Vec::new(),
                    })
                },
            )
            .optional()
            .map_err(to_error)?
            .unwrap_or_default();
        let mut query = connection.prepare("SELECT id,chat_id,agent_id,text,attachment_count,status,created_at,error FROM prompt_jobs WHERE chat_id=?1 AND status NOT IN ('done','removed') ORDER BY sequence").map_err(to_error)?;
        state.items = query
            .query_map([chat_id], |r| {
                Ok(QueuedPrompt {
                    id: r.get(0)?,
                    chat_id: r.get(1)?,
                    agent_id: r.get(2)?,
                    text: r.get(3)?,
                    attachment_count: r.get(4)?,
                    status: r.get(5)?,
                    created_at: r.get(6)?,
                    error: r.get(7)?,
                })
            })
            .map_err(to_error)?
            .collect::<std::result::Result<_, _>>()
            .map_err(to_error)?;
        Ok(state)
    }
    pub fn insert_prompt_job(&self, id: &str, prompt: &PromptParams) -> Result<()> {
        if id.is_empty()
            || id.len() > 128
            || prompt.agent_id.trim().is_empty()
            || prompt.text.len() > 1024 * 1024
            || (prompt.text.trim().is_empty() && prompt.attachments.is_empty())
            || prompt.attachments.len() > 4
        {
            return Err(Error::msg("Invalid queued prompt"));
        }
        let payload = serde_json::to_string(prompt)?;
        let connection = self.connection()?;
        let existing: Option<(String, Option<String>)> = connection
            .query_row(
                "SELECT chat_id,payload_json FROM prompt_jobs WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(to_error)?;
        if let Some((chat, p)) = existing {
            if chat != prompt.chat_id || p.as_ref().is_some_and(|value| value != &payload) {
                return Err(Error::msg("Request ID already belongs to another prompt"));
            }
            return Ok(());
        }
        let (count, bytes):(u32,u64)=connection.query_row("SELECT COUNT(*),COALESCE(SUM(length(CAST(payload_json AS BLOB))),0) FROM prompt_jobs WHERE chat_id=?1 AND status NOT IN ('done','removed')",[&prompt.chat_id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(to_error)?;
        if count >= 16 || bytes.saturating_add(payload.len() as u64) > 64 * 1024 * 1024 {
            return Err(Error::msg(
                "Queue is full (16 messages or 64 MiB). Remove a waiting message first.",
            ));
        }
        connection.execute("INSERT INTO prompt_jobs(id,chat_id,agent_id,text,attachment_count,payload_json,status,created_at) VALUES(?1,?2,?3,?4,?5,?6,'queued',?7)",params![id,prompt.chat_id,prompt.agent_id,prompt.text,prompt.attachments.len(),payload,now()]).map_err(to_error)?;
        Ok(())
    }
    pub fn pause_prompt_queue(&self, chat_id: &str, reason: Option<&str>) -> Result<()> {
        self.connection()?.execute("INSERT INTO prompt_queue_state(chat_id,paused,reason) VALUES(?1,?2,?3) ON CONFLICT(chat_id) DO UPDATE SET paused=excluded.paused,reason=excluded.reason",params![chat_id,reason.is_some(),reason]).map_err(to_error)?;
        Ok(())
    }
    pub fn claim_prompt_job(&self, chat_id: &str) -> Result<Option<(String, PromptParams)>> {
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(to_error)?;
        let job:Option<(String,String)>=tx.query_row("SELECT id,payload_json FROM prompt_jobs WHERE chat_id=?1 AND status='queued' ORDER BY sequence LIMIT 1",[chat_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(to_error)?;
        let result = if let Some((id, payload)) = job {
            let prompt = serde_json::from_str(&payload)?;
            tx.execute("UPDATE prompt_jobs SET status='running' WHERE id=?1", [&id])
                .map_err(to_error)?;
            Some((id, prompt))
        } else {
            None
        };
        tx.commit().map_err(to_error)?;
        Ok(result)
    }
    pub fn finish_prompt_job(&self, id: &str, error: Option<&str>) -> Result<()> {
        self.connection()?.execute("UPDATE prompt_jobs SET status=?2,error=?3,payload_json=CASE WHEN ?3 IS NULL THEN NULL ELSE payload_json END,text=CASE WHEN ?3 IS NULL THEN '' ELSE text END WHERE id=?1",params![id,if error.is_some(){"failed"}else{"done"},error]).map_err(to_error)?;
        Ok(())
    }
    pub fn remove_prompt_job(&self, chat_id: &str, id: &str) -> Result<()> {
        let changed=self.connection()?.execute("UPDATE prompt_jobs SET status='removed',payload_json=NULL,text='',error=NULL WHERE id=?1 AND chat_id=?2 AND status IN ('queued','failed','interrupted')",params![id,chat_id]).map_err(to_error)?;
        if changed == 0 {
            return Err(Error::msg(
                "This message has already started or was removed",
            ));
        }
        Ok(())
    }
    pub fn edit_prompt_job(&self, chat_id: &str, id: &str, text: &str) -> Result<()> {
        let connection = self.connection()?;
        let payload:Option<String>=connection.query_row("SELECT payload_json FROM prompt_jobs WHERE id=?1 AND chat_id=?2 AND status='queued'",params![id,chat_id],|r|r.get(0)).optional().map_err(to_error)?;
        let mut prompt: PromptParams = serde_json::from_str(
            &payload
                .ok_or_else(|| Error::msg("This message has already started or was removed"))?,
        )?;
        if text.trim().is_empty() && prompt.attachments.is_empty() {
            return Err(Error::msg("A queued message cannot be empty"));
        }
        if text.len() > 1024 * 1024 {
            return Err(Error::msg("Queued text exceeds 1 MiB"));
        }
        prompt.text = text.to_owned();
        let payload = serde_json::to_string(&prompt)?;
        let other_bytes: u64 = connection.query_row(
            "SELECT COALESCE(SUM(length(CAST(payload_json AS BLOB))),0) FROM prompt_jobs WHERE chat_id=?1 AND id!=?2 AND status NOT IN ('done','removed')",
            params![chat_id,id], |r|r.get(0),
        ).map_err(to_error)?;
        if other_bytes.saturating_add(payload.len() as u64) > 64 * 1024 * 1024 {
            return Err(Error::msg("Queued messages exceed 64 MiB"));
        }
        connection
            .execute(
                "UPDATE prompt_jobs SET text=?2,payload_json=?3 WHERE id=?1",
                params![id, text, payload],
            )
            .map_err(to_error)?;
        Ok(())
    }
}
