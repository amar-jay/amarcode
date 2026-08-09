//! `SessionManager` — public API for runs, prompts, cancel, and agent replies.

use std::{
    collections::HashMap,
    path::Path,
    sync::Arc,
};

use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::{
    acp::AcpClient,
    protocol::{
        AgentRpcMethod, EditorEvent, MessagePartKind, MessageRole, MessageStatus, RpcDirection,
        RpcEnvelope, RunStatus,
    },
    service::agent_manager::AgentManager,
    store::{AgentRun, Message, MessagePart, Store},
    Error, Result,
};

use super::{
    inbound::spawn_inbound_worker,
    messages::{finalize_message, take_streaming_messages_from_live},
    types::{
        LiveRun, PendingAgentRequest, PromptResult, SessionInner, ACP_REQUEST_TIMEOUT,
    },
    util::{extract_session_id, timestamp},
};

pub struct SessionManager {
    agents: AgentManager,
    inner: Arc<SessionInner>,
}

impl SessionManager {
    pub fn new(
        store: Arc<Store>,
        agents: AgentManager,
        events: broadcast::Sender<EditorEvent>,
    ) -> Self {
        Self {
            agents,
            inner: Arc::new(SessionInner {
                store,
                events,
                by_chat: std::sync::Mutex::new(HashMap::new()),
                pending: std::sync::Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Start a new agent run for a chat (spawns ACP, initialize + createSession).
    pub fn start_run(&self, chat_id: &str, agent_id: &str) -> Result<AgentRun> {
        let chat = self
            .inner
            .store
            .get_chat(chat_id)?
            .ok_or_else(|| Error::msg(format!("chat not found: {chat_id}")))?;

        self.detach_chat(chat_id);

        let resolved = self.agents.resolve(agent_id)?;
        let now = timestamp();
        let run = AgentRun {
            id: uuid::Uuid::new_v4().to_string(),
            chat_id: chat_id.to_owned(),
            agent_id: agent_id.to_owned(),
            acp_session_id: None,
            status: RunStatus::Starting,
            started_at: now,
            finished_at: None,
            error_message: None,
        };
        self.inner.store.create_run(&run)?;
        self.emit(EditorEvent::RunUpdated {
            run_id: run.id.clone(),
            status: RunStatus::Starting,
            error_message: None,
        });

        let cwd = Path::new(&chat.workspace_path);
        let command = resolved.command.to_string_lossy().into_owned();
        let (client, inbound) = match AcpClient::spawn(
            &command,
            &resolved.arguments,
            &resolved.environment,
            Some(cwd),
        ) {
            Ok(pair) => pair,
            Err(err) => {
                let _ = self.fail_run(&run.id, &err.to_string());
                return Err(err.into());
            }
        };

        let client = Arc::new(client);
        spawn_inbound_worker(
            Arc::clone(&self.inner),
            run.id.clone(),
            chat_id.to_owned(),
            inbound,
        );

        // Real ACP (Copilot, etc.): method names are `initialize` / `session/*`,
        // not a proprietary `agent.*` namespace.
        if let Err(err) = self.acp_request(
            &run.id,
            &client,
            AgentRpcMethod::Initialize,
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": {
                    "name": "amarcode-daemon",
                    "title": "Amarcode Daemon",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        ) {
            let _ = client.kill();
            let _ = self.fail_run(&run.id, &err.to_string());
            return Err(err);
        }

        let session_result = self.acp_request(
            &run.id,
            &client,
            AgentRpcMethod::CreateSession,
            json!({
                "cwd": chat.workspace_path,
                "mcpServers": [],
            }),
        );

        let acp_session_id = match session_result {
            Ok(value) => extract_session_id(&value),
            Err(err) => {
                let _ = client.kill();
                let _ = self.fail_run(&run.id, &err.to_string());
                return Err(err);
            }
        };

        self.inner.store.update_run(
            &run.id,
            RunStatus::Running,
            acp_session_id.as_deref(),
            None,
        )?;
        self.emit(EditorEvent::RunUpdated {
            run_id: run.id.clone(),
            status: RunStatus::Running,
            error_message: None,
        });
        self.emit(EditorEvent::AgentConnectionChanged {
            agent_id: agent_id.to_owned(),
            connected: true,
            error_message: None,
        });

        let live = LiveRun {
            run_id: run.id.clone(),
            agent_id: agent_id.to_owned(),
            client,
            acp_session_id: acp_session_id.clone(),
            streaming_message_ids: HashMap::new(),
            last_streaming_message_id: None,
        };
        self.inner
            .by_chat
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?
            .insert(chat_id.to_owned(), live);

        let mut run = run;
        run.status = RunStatus::Running;
        run.acp_session_id = acp_session_id;
        Ok(run)
    }

    /// Persist a user message and send it to the agent (starts a run if needed).
    pub fn prompt(
        &self,
        chat_id: &str,
        agent_id: &str,
        text: impl AsRef<str>,
    ) -> Result<PromptResult> {
        let text = text.as_ref().trim();
        if text.is_empty() {
            return Err(Error::msg("prompt text must not be empty"));
        }

        let needs_start = {
            let guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            match guard.get(chat_id) {
                None => true,
                Some(live) => live.agent_id != agent_id,
            }
        };
        if needs_start {
            self.start_run(chat_id, agent_id)?;
        }

        let (run_id, client, session_id) = {
            let guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            let live = guard
                .get(chat_id)
                .ok_or_else(|| Error::msg("no live session after start_run"))?;
            (
                live.run_id.clone(),
                Arc::clone(&live.client),
                live.acp_session_id.clone(),
            )
        };

        let now = timestamp();
        let user_message = Message {
            id: uuid::Uuid::new_v4().to_string(),
            chat_id: chat_id.to_owned(),
            agent_run_id: Some(run_id.clone()),
            role: MessageRole::User,
            content: text.to_owned(),
            status: MessageStatus::Complete,
            created_at: now.clone(),
            updated_at: now,
        };
        self.inner.store.create_message(&user_message)?;
        self.inner.store.replace_message_parts(
            &user_message.id,
            &[MessagePart {
                message_id: user_message.id.clone(),
                ordinal: 0,
                kind: MessagePartKind::Text,
                content_json: json!({ "text": text }).to_string(),
            }],
        )?;
        self.emit(EditorEvent::MessageUpdated {
            message_id: user_message.id.clone(),
            status: MessageStatus::Complete,
        });
        self.emit(EditorEvent::ChatUpdated {
            chat_id: chat_id.to_owned(),
        });

        // ACP session/prompt: prompt is an array of content blocks.
        let mut params = json!({
            "prompt": [{ "type": "text", "text": text }],
        });
        if let Some(sid) = &session_id {
            params
                .as_object_mut()
                .expect("params object")
                .insert("sessionId".into(), json!(sid));
        } else {
            return Err(Error::msg("live session missing acp_session_id"));
        }

        let prompt_result = self.acp_request(&run_id, &client, AgentRpcMethod::Prompt, params)?;

        // The ACP reader sees notifications and the RPC result in order, but
        // persists notifications on a separate worker. Wait for that worker
        // before finalizing the messages from this turn.
        client
            .sync_inbound(ACP_REQUEST_TIMEOUT)
            .map_err(|err| Error::msg(err.to_string()))?;

        // Turn finished (stopReason typically end_turn). Finalize any streaming
        // assistant message that arrived via session/update chunks.
        let message_ids = {
            let mut guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            guard
                .get_mut(chat_id)
                .map(take_streaming_messages_from_live)
                .unwrap_or_default()
        };
        for message_id in message_ids {
            finalize_message(&self.inner, &message_id, MessageStatus::Complete)?;
            self.emit(EditorEvent::MessageUpdated {
                message_id,
                status: MessageStatus::Complete,
            });
        }
        let _ = prompt_result;

        Ok(PromptResult {
            run_id,
            chat_id: chat_id.to_owned(),
            agent_id: agent_id.to_owned(),
            user_message_id: user_message.id,
            acp_session_id: session_id,
        })
    }

    /// Cancel the live run for a chat.
    pub fn cancel(&self, chat_id: &str) -> Result<()> {
        let live = {
            let mut guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            guard.remove(chat_id)
        };

        let Some(live) = live else {
            return Err(Error::msg(format!("no live run for chat: {chat_id}")));
        };

        let _ = self.acp_notify(
            &live.run_id,
            &live.client,
            AgentRpcMethod::Cancel,
            json!({ "sessionId": live.acp_session_id }),
        );
        // Prefer session/close when the agent advertised it; cancel is enough for now.
        let _ = live.client.kill();

        self.inner
            .store
            .update_run(&live.run_id, RunStatus::Stopped, None, None)?;
        self.emit(EditorEvent::RunUpdated {
            run_id: live.run_id,
            status: RunStatus::Stopped,
            error_message: None,
        });
        self.emit(EditorEvent::AgentConnectionChanged {
            agent_id: live.agent_id,
            connected: false,
            error_message: None,
        });
        Ok(())
    }

    /// Answer an agent-initiated request (`ApprovalRequired` / `QuestionRequired`).
    pub fn respond_to_agent(&self, request_id: &str, result: Value) -> Result<()> {
        let pending = {
            let mut guard = self
                .inner
                .pending
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            guard.remove(request_id)
        }
        .ok_or_else(|| Error::msg(format!("unknown pending agent request: {request_id}")))?;

        let client = {
            let guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            guard
                .get(&pending.chat_id)
                .map(|live| Arc::clone(&live.client))
                .ok_or_else(|| Error::msg("live session gone for pending request"))?
        };

        let envelope = RpcEnvelope {
            direction: RpcDirection::Sent,
            method: format!("response:{}", pending.method),
            payload: result.clone(),
        };
        self.inner
            .store
            .save_acp_envelope(&pending.run_id, &envelope)?;

        client
            .respond(pending.acp_id, result)
            .map_err(Error::from)?;
        Ok(())
    }

    pub fn respond_error_to_agent(
        &self,
        request_id: &str,
        code: i64,
        message: &str,
        data: Option<Value>,
    ) -> Result<()> {
        let pending = {
            let mut guard = self
                .inner
                .pending
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            guard.remove(request_id)
        }
        .ok_or_else(|| Error::msg(format!("unknown pending agent request: {request_id}")))?;

        let client = {
            let guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            guard
                .get(&pending.chat_id)
                .map(|live| Arc::clone(&live.client))
                .ok_or_else(|| Error::msg("live session gone for pending request"))?
        };

        client
            .respond_error(pending.acp_id, code, message, data)
            .map_err(Error::from)?;
        Ok(())
    }

    /// `(run_id, agent_id, acp_session_id)` for the live chat session, if any.
    pub fn live_run_for_chat(
        &self,
        chat_id: &str,
    ) -> Result<Option<(String, String, Option<String>)>> {
        let guard = self
            .inner
            .by_chat
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?;
        Ok(guard.get(chat_id).map(|live| {
            (
                live.run_id.clone(),
                live.agent_id.clone(),
                live.acp_session_id.clone(),
            )
        }))
    }

    pub fn pending_requests(&self) -> Result<Vec<PendingAgentRequest>> {
        let guard = self
            .inner
            .pending
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?;
        Ok(guard.values().cloned().collect())
    }

    // --- ACP helpers -------------------------------------------------------

    fn acp_request(
        &self,
        run_id: &str,
        client: &AcpClient,
        method: AgentRpcMethod,
        params: Value,
    ) -> Result<Value> {
        let envelope = RpcEnvelope {
            direction: RpcDirection::Sent,
            method: method.as_str().to_owned(),
            payload: params.clone(),
        };
        self.inner.store.save_acp_envelope(run_id, &envelope)?;

        let result = client
            .request(method, params, ACP_REQUEST_TIMEOUT)
            .map_err(Error::from)?;

        let response_envelope = RpcEnvelope {
            direction: RpcDirection::Received,
            method: "rpc.result".into(),
            payload: result.clone(),
        };
        self.inner
            .store
            .save_acp_envelope(run_id, &response_envelope)?;
        Ok(result)
    }

    fn acp_notify(
        &self,
        run_id: &str,
        client: &AcpClient,
        method: AgentRpcMethod,
        params: Value,
    ) -> Result<()> {
        let envelope = RpcEnvelope {
            direction: RpcDirection::Sent,
            method: method.as_str().to_owned(),
            payload: params.clone(),
        };
        self.inner.store.save_acp_envelope(run_id, &envelope)?;
        client.notify(method, params).map_err(Error::from)
    }

    fn fail_run(&self, run_id: &str, error: &str) -> Result<()> {
        self.inner
            .store
            .update_run(run_id, RunStatus::Failed, None, Some(error))?;
        self.emit(EditorEvent::RunUpdated {
            run_id: run_id.to_owned(),
            status: RunStatus::Failed,
            error_message: Some(error.to_owned()),
        });
        Ok(())
    }

    fn detach_chat(&self, chat_id: &str) {
        let removed = self
            .inner
            .by_chat
            .lock()
            .ok()
            .and_then(|mut guard| guard.remove(chat_id));
        if let Some(live) = removed {
            let _ = live.client.kill();
            let _ = self.inner.store.update_run(
                &live.run_id,
                RunStatus::Stopped,
                None,
                Some("replaced by new run"),
            );
            let _ = self.inner.events.send(EditorEvent::RunUpdated {
                run_id: live.run_id,
                status: RunStatus::Stopped,
                error_message: Some("replaced by new run".into()),
            });
        }
    }

    fn emit(&self, event: EditorEvent) {
        let _ = self.inner.events.send(event);
    }
}
