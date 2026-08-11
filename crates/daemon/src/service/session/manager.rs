//! `SessionManager` — public API for runs, prompts, cancel, and agent replies.

use std::{collections::HashMap, path::Path, sync::Arc};

use serde_json::{json, Value};
use tokio::sync::broadcast;
use tracing::debug;

use crate::{
    acp::AcpClient,
    protocol::{
        AgentEventMethod, AgentRpcMethod, EditorEvent, MessagePartKind, MessageRole, MessageStatus,
        RpcDirection, RpcEnvelope, RunStatus, TurnStatus,
    },
    service::agent_manager::AgentManager,
    store::{AgentRun, Message, MessagePart, Store},
    Error, Result,
};

use super::{
    inbound::spawn_inbound_worker,
    messages::{finalize_message, take_streaming_messages_from_live},
    types::{LiveRun, PendingAgentRequest, PromptResult, SessionInner, ACP_REQUEST_TIMEOUT},
    util::{extract_session_id, extract_stop_reason, normalize_permission_result, timestamp},
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
                prompt_locks: std::sync::Mutex::new(HashMap::new()),
                by_chat: std::sync::Mutex::new(HashMap::new()),
                pending: std::sync::Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Start a new agent run for a chat (spawns ACP, initialize + resume/new session).
    pub fn start_run(
        &self,
        chat_id: &str,
        agent_id: &str,
        session_mode: Option<&str>,
    ) -> Result<AgentRun> {
        let prompt_lock = self.prompt_lock(chat_id)?;
        let _prompt_guard = prompt_lock
            .lock()
            .map_err(|_| Error::msg("prompt lock poisoned"))?;
        self.start_run_locked(chat_id, agent_id, session_mode)
    }

    /// Start a run while the caller holds this chat's prompt lock.
    fn start_run_locked(
        &self,
        chat_id: &str,
        agent_id: &str,
        session_mode: Option<&str>,
    ) -> Result<AgentRun> {
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

        let has_persisted_history = self
            .inner
            .store
            .messages(chat_id)?
            .iter()
            .any(|message| !message.content.trim().is_empty());

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
                // Advertise only the elicitation mode the desktop client can
                // actually render and answer. Agents commonly disable their
                // ask-user flow when this is absent.
                "clientCapabilities": {
                    "elicitation": {
                        "form": {}
                    }
                },
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

        let previous_session_id = self
            .inner
            .store
            .list_runs_for_chat(chat_id)?
            .into_iter()
            .filter(|previous| previous.id != run.id && previous.agent_id == agent_id)
            .find_map(|previous| previous.acp_session_id);
        let session_setup = (|| -> Result<(Option<String>, bool)> {
            if let Some(session_id) = previous_session_id {
                self.emit(EditorEvent::ContextRestoration {
                    chat_id: chat_id.to_owned(),
                    run_id: run.id.clone(),
                    source: "Resuming saved agent session".into(),
                });
                match self.acp_request(
                    &run.id,
                    &client,
                    AgentRpcMethod::ResumeSession,
                    json!({
                        "sessionId": session_id,
                        "cwd": chat.workspace_path,
                        "mcpServers": [],
                    }),
                ) {
                    // ACP may acknowledge a resume without recreating model
                    // context. Durable chat history remains authoritative.
                    Ok(_) => Ok((Some(session_id), has_persisted_history)),
                    Err(error) => {
                        debug!(%chat_id, %agent_id, %error, "ACP session resume unavailable; creating a hydrated session");
                        let value = self.acp_request(
                            &run.id,
                            &client,
                            AgentRpcMethod::CreateSession,
                            json!({
                                "cwd": chat.workspace_path,
                                "mcpServers": [],
                            }),
                        )?;
                        Ok((extract_session_id(&value), has_persisted_history))
                    }
                }
            } else {
                let value = self.acp_request(
                    &run.id,
                    &client,
                    AgentRpcMethod::CreateSession,
                    json!({
                        "cwd": chat.workspace_path,
                        "mcpServers": [],
                    }),
                )?;
                Ok((extract_session_id(&value), has_persisted_history))
            }
        })();
        let (acp_session_id, needs_history_hydration) = match session_setup {
            Ok(value) => value,
            Err(err) => {
                let _ = client.kill();
                let _ = self.fail_run(&run.id, &err.to_string());
                return Err(err);
            }
        };

        // Codex exposes its `request_user_input` tool only in collaboration
        // plan mode. Make that an explicit first-prompt choice rather than a
        // hidden default; other ACP agents keep their own configuration.
        if let Some(mode) = session_mode.filter(|_| {
            resolved
                .command
                .file_name()
                .is_some_and(|name| name == "codex-acp")
        }) {
            if let Err(err) =
                self.configure_codex_session(&run.id, &client, acp_session_id.as_deref(), mode)
            {
                let _ = client.kill();
                let _ = self.fail_run(&run.id, &err.to_string());
                return Err(err);
            }
        }

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
            needs_history_hydration,
            streaming_message_ids: HashMap::new(),
            last_streaming_message_id: None,
            active_user_message_id: None,
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
        session_mode: Option<&str>,
    ) -> Result<PromptResult> {
        let text = text.as_ref().trim();
        if text.is_empty() {
            return Err(Error::msg("prompt text must not be empty"));
        }

        // Keep session selection/startup and the complete ACP turn atomic for
        // this chat. Without this, concurrent RPC connections can both spawn a
        // run or can overwrite the live turn's streaming/message ownership.
        // Other chats use different locks and continue in parallel.
        let prompt_lock = self.prompt_lock(chat_id)?;
        let _prompt_guard = prompt_lock
            .lock()
            .map_err(|_| Error::msg("prompt lock poisoned"))?;

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
            self.start_run_locked(chat_id, agent_id, session_mode)?;
        }

        let (run_id, client, session_id, needs_history_hydration) = {
            let mut guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            let live = guard
                .get_mut(chat_id)
                .ok_or_else(|| Error::msg("no live session after start_run"))?;
            (
                live.run_id.clone(),
                Arc::clone(&live.client),
                live.acp_session_id.clone(),
                std::mem::take(&mut live.needs_history_hydration),
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

        // Mark the turn open before the blocking ACP prompt so subscribers can
        // show working state without waiting for the RPC to return.
        {
            let mut guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            if let Some(live) = guard.get_mut(chat_id) {
                live.active_user_message_id = Some(user_message.id.clone());
            }
        }
        self.emit(EditorEvent::TurnUpdated {
            chat_id: chat_id.to_owned(),
            run_id: run_id.clone(),
            user_message_id: user_message.id.clone(),
            status: TurnStatus::Started,
            stop_reason: None,
            error_message: None,
        });

        // ACP session/prompt: prompt is an array of content blocks.
        let prompt_text = if needs_history_hydration {
            self.emit(EditorEvent::ContextRestoration {
                chat_id: chat_id.to_owned(),
                run_id: run_id.clone(),
                source: "Restoring saved chat context".into(),
            });
            self.hydrated_prompt(chat_id, &user_message.id, text)?
        } else {
            text.to_owned()
        };
        let mut params = json!({
            "prompt": [{ "type": "text", "text": prompt_text }],
        });
        if let Some(sid) = &session_id {
            params
                .as_object_mut()
                .expect("params object")
                .insert("sessionId".into(), json!(sid));
        } else {
            self.finish_turn(
                chat_id,
                &run_id,
                &user_message.id,
                TurnStatus::Failed,
                None,
                Some("live session missing acp_session_id"),
            );
            return Err(Error::msg("live session missing acp_session_id"));
        }

        let prompt_result = match self.acp_request(&run_id, &client, AgentRpcMethod::Prompt, params)
        {
            Ok(value) => value,
            Err(err) => {
                self.finish_turn(
                    chat_id,
                    &run_id,
                    &user_message.id,
                    TurnStatus::Failed,
                    None,
                    Some(&err.to_string()),
                );
                return Err(err);
            }
        };

        // The ACP reader sees notifications and the RPC result in order, but
        // persists notifications on a separate worker. Wait for that worker
        // before finalizing the messages from this turn.
        if let Err(err) = client.sync_inbound(ACP_REQUEST_TIMEOUT) {
            self.finish_turn(
                chat_id,
                &run_id,
                &user_message.id,
                TurnStatus::Failed,
                None,
                Some(&err.to_string()),
            );
            return Err(Error::msg(err.to_string()));
        }

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

        let stop_reason = extract_stop_reason(&prompt_result);
        self.finish_turn(
            chat_id,
            &run_id,
            &user_message.id,
            TurnStatus::Completed,
            stop_reason,
            None,
        );

        Ok(PromptResult {
            run_id,
            chat_id: chat_id.to_owned(),
            agent_id: agent_id.to_owned(),
            user_message_id: user_message.id,
            acp_session_id: session_id,
        })
    }

    fn prompt_lock(&self, chat_id: &str) -> Result<Arc<std::sync::Mutex<()>>> {
        let mut locks = self
            .inner
            .prompt_locks
            .lock()
            .map_err(|_| Error::msg("prompt lock registry poisoned"))?;

        // A waiting caller owns a strong Arc, so pruning dead weak references
        // cannot split callers for the same chat across different locks.
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(chat_id).and_then(std::sync::Weak::upgrade) {
            return Ok(lock);
        }

        let lock = Arc::new(std::sync::Mutex::new(()));
        locks.insert(chat_id.to_owned(), Arc::downgrade(&lock));
        Ok(lock)
    }

    /// Change the active Codex session's collaboration/permission preset.
    pub fn set_session_mode(&self, chat_id: &str, mode: &str) -> Result<()> {
        let (run_id, client, session_id, agent_id) = {
            let guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            let live = guard
                .get(chat_id)
                .ok_or_else(|| Error::msg("no active session for this chat"))?;
            (
                live.run_id.clone(),
                Arc::clone(&live.client),
                live.acp_session_id.clone(),
                live.agent_id.clone(),
            )
        };
        let resolved = self.agents.resolve(&agent_id)?;
        if !resolved
            .command
            .file_name()
            .is_some_and(|name| name == "codex-acp")
        {
            return Err(Error::msg("this agent does not expose Codex session modes"));
        }
        self.configure_codex_session(&run_id, &client, session_id.as_deref(), mode)
    }

    fn configure_codex_session(
        &self,
        run_id: &str,
        client: &AcpClient,
        session_id: Option<&str>,
        mode: &str,
    ) -> Result<()> {
        let session_id = session_id.ok_or_else(|| Error::msg("Codex session has not started"))?;
        let (collaboration_mode, agent_mode) = match mode {
            "plan" => ("plan", Some("agent")),
            "build" => ("default", Some("agent")),
            "ask" => ("default", Some("read-only")),
            _ => return Err(Error::msg("mode must be plan, build, or ask")),
        };
        self.acp_request(
            run_id,
            client,
            AgentRpcMethod::Other("session/set_config_option".to_owned()),
            json!({
                "sessionId": session_id,
                "configId": "collaboration_mode",
                "type": "id",
                "value": collaboration_mode,
            }),
        )?;
        if let Some(agent_mode) = agent_mode {
            self.acp_request(
                run_id,
                client,
                AgentRpcMethod::Other("session/set_config_option".to_owned()),
                json!({
                    "sessionId": session_id,
                    "configId": "mode",
                    "type": "id",
                    "value": agent_mode,
                }),
            )?;
        }
        Ok(())
    }

    /// Provide an isolated fallback when an agent cannot resume its own saved
    /// session. The transcript contains only rows from this chat and excludes
    /// the message about to be sent, which is appended once as the live prompt.
    fn hydrated_prompt(
        &self,
        chat_id: &str,
        current_message_id: &str,
        prompt: &str,
    ) -> Result<String> {
        const MAX_HISTORY_CHARS: usize = 60_000;

        let mut turns = self
            .inner
            .store
            .messages(chat_id)?
            .into_iter()
            .filter(|message| {
                message.id != current_message_id && !message.content.trim().is_empty()
            })
            .filter_map(|message| match message.role {
                MessageRole::User => Some(format!("User: {}", message.content.trim())),
                MessageRole::Assistant => Some(format!("Assistant: {}", message.content.trim())),
                _ => None,
            })
            .collect::<Vec<_>>();

        let mut history = turns.join("\n\n");
        if history.len() > MAX_HISTORY_CHARS {
            while history.len() > MAX_HISTORY_CHARS && !turns.is_empty() {
                turns.remove(0);
                history = turns.join("\n\n");
            }
            history = format!("[Earlier conversation omitted for context length.]\n\n{history}");
        }

        if history.is_empty() {
            return Ok(prompt.to_owned());
        }

        Ok(format!(
            "Continue this conversation for the current chat only. Treat the transcript as prior context; respond to the final user message normally.\n\n<chat-history>\n{history}\n</chat-history>\n\nUser: {prompt}"
        ))
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

        let active_user_message_id = live.active_user_message_id.clone();
        let run_id = live.run_id.clone();

        let _ = self.acp_notify(
            &run_id,
            &live.client,
            AgentRpcMethod::Cancel,
            json!({ "sessionId": live.acp_session_id }),
        );
        // Prefer session/close when the agent advertised it; cancel is enough for now.
        let _ = live.client.kill();

        if let Some(user_message_id) = active_user_message_id {
            self.emit(EditorEvent::TurnUpdated {
                chat_id: chat_id.to_owned(),
                run_id: run_id.clone(),
                user_message_id,
                status: TurnStatus::Cancelled,
                stop_reason: Some("cancelled".into()),
                error_message: None,
            });
        }

        self.inner
            .store
            .update_run(&run_id, RunStatus::Stopped, None, None)?;
        self.emit(EditorEvent::RunUpdated {
            run_id,
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

        // ACP permission replies must use `outcome.optionId`. Translate
        // convenience shapes from the UI/CLI so agents don't abort the turn.
        let result = if pending.method == AgentEventMethod::PermissionRequested.as_str()
            || pending.method == "permission.requested"
        {
            normalize_permission_result(&pending.params, result)
        } else {
            result
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
        // If a prompt turn is open on this run, close it as failed first.
        if let Ok(mut guard) = self.inner.by_chat.lock() {
            let hit = guard.iter_mut().find_map(|(chat_id, live)| {
                if live.run_id == run_id {
                    live.active_user_message_id
                        .take()
                        .map(|user_message_id| (chat_id.clone(), user_message_id))
                } else {
                    None
                }
            });
            if let Some((chat_id, user_message_id)) = hit {
                drop(guard);
                self.emit(EditorEvent::TurnUpdated {
                    chat_id,
                    run_id: run_id.to_owned(),
                    user_message_id,
                    status: TurnStatus::Failed,
                    stop_reason: None,
                    error_message: Some(error.to_owned()),
                });
            }
        }
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

    /// Clear the open turn on the live chat (if it matches) and notify clients.
    fn finish_turn(
        &self,
        chat_id: &str,
        run_id: &str,
        user_message_id: &str,
        status: TurnStatus,
        stop_reason: Option<String>,
        error_message: Option<&str>,
    ) {
        if let Ok(mut guard) = self.inner.by_chat.lock() {
            if let Some(live) = guard.get_mut(chat_id) {
                if live.run_id == run_id
                    && live.active_user_message_id.as_deref() == Some(user_message_id)
                {
                    live.active_user_message_id = None;
                }
            }
        }
        self.emit(EditorEvent::TurnUpdated {
            chat_id: chat_id.to_owned(),
            run_id: run_id.to_owned(),
            user_message_id: user_message_id.to_owned(),
            status,
            stop_reason,
            error_message: error_message.map(str::to_owned),
        });
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
            if let Some(user_message_id) = live.active_user_message_id {
                let _ = self.inner.events.send(EditorEvent::TurnUpdated {
                    chat_id: chat_id.to_owned(),
                    run_id: live.run_id.clone(),
                    user_message_id,
                    status: TurnStatus::Cancelled,
                    stop_reason: Some("replaced".into()),
                    error_message: Some("replaced by new run".into()),
                });
            }
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
