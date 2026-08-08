//! Live session / agent-run coordination.
//!
//! The **only** place that joins ACP I/O, SQLite, and the `EditorEvent` bus.
//! `store` and `acp` stay segmented; this module owns ordering.
//!
//! ## Store-first rule
//!
//! For every meaningful ACP outcome (request result or inbound notification):
//!
//! 1. **Persist** to `store` (run/message/parts/`acp_events`)
//! 2. **Then** publish `EditorEvent` / complete the client RPC result
//!
//! Never fan out or return durable claims that SQLite does not yet contain.

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use serde_json::{json, Value};
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::{
    acp::{AcpClient, AcpInbound},
    protocol::{
        AgentEventMethod, AgentRpcMethod, EditorEvent, MessagePartKind, MessageRole, MessageStatus,
        RpcDirection, RpcEnvelope, RunStatus,
    },
    service::agent_manager::AgentManager,
    store::{AgentRun, Message, MessagePart, Store},
    Error, Result,
};

const ACP_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Result of starting a run / sending a prompt.
#[derive(Debug, Clone)]
pub struct PromptResult {
    pub run_id: String,
    pub chat_id: String,
    pub agent_id: String,
    pub user_message_id: String,
    pub acp_session_id: Option<String>,
}

struct LiveRun {
    run_id: String,
    agent_id: String,
    client: Arc<AcpClient>,
    acp_session_id: Option<String>,
    /// Assistant messages currently being streamed, keyed by the upstream ACP
    /// message id. A turn may contain distinct commentary and final-answer
    /// messages, so they must not be collapsed into one row.
    streaming_message_ids: HashMap<String, String>,
    /// Most recent visible assistant message. Thought chunks do not always use
    /// the same ACP message id, so attach them here when possible.
    last_streaming_message_id: Option<String>,
}

/// Pending agent-initiated JSON-RPC request (permission / input).
#[derive(Debug, Clone)]
pub struct PendingAgentRequest {
    pub run_id: String,
    pub chat_id: String,
    pub acp_id: u64,
    pub method: String,
    pub params: Value,
}

/// Shared pieces used by SessionManager methods and inbound worker threads.
struct SessionInner {
    store: Arc<Store>,
    events: broadcast::Sender<EditorEvent>,
    by_chat: Mutex<HashMap<String, LiveRun>>,
    pending: Mutex<HashMap<String, PendingAgentRequest>>,
}

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
                by_chat: Mutex::new(HashMap::new()),
                pending: Mutex::new(HashMap::new()),
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

fn spawn_inbound_worker(
    inner: Arc<SessionInner>,
    run_id: String,
    chat_id: String,
    inbound: std::sync::mpsc::Receiver<AcpInbound>,
) {
    thread::Builder::new()
        .name(format!("acp-inbound-{run_id}"))
        .spawn(move || {
            while let Ok(msg) = inbound.recv() {
                if let Err(err) = handle_inbound(&inner, &run_id, &chat_id, msg) {
                    warn!(%run_id, error = %err, "failed handling ACP inbound");
                }
            }
        })
        .expect("spawn acp inbound worker");
}

fn handle_inbound(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
    msg: AcpInbound,
) -> Result<()> {
    match msg {
        AcpInbound::Notification { event, envelope } => {
            // 1. STORE raw envelope
            inner.store.save_acp_envelope(run_id, &envelope)?;
            // 2. apply product state + 3. EVENTS
            apply_notification(inner, run_id, chat_id, event, &envelope)?;
        }
        AcpInbound::Request { id, method, params } => {
            let envelope = RpcEnvelope {
                direction: RpcDirection::Received,
                method: method.clone(),
                payload: params.clone(),
            };
            inner.store.save_acp_envelope(run_id, &envelope)?;

            let request_id = id.to_string();
            let pending = PendingAgentRequest {
                run_id: run_id.to_owned(),
                chat_id: chat_id.to_owned(),
                acp_id: id,
                method: method.clone(),
                params: params.clone(),
            };
            inner
                .pending
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?
                .insert(request_id.clone(), pending);

            let agent_method = AgentEventMethod::from(method.as_str());
            match agent_method {
                AgentEventMethod::InputRequested => {
                    emit(
                        inner,
                        EditorEvent::QuestionRequired {
                            run_id: run_id.to_owned(),
                            request_id,
                            details: params,
                        },
                    );
                }
                _ => {
                    // Default: treat agent-initiated requests as approvals
                    // (permissions, etc.).
                    emit(
                        inner,
                        EditorEvent::ApprovalRequired {
                            run_id: run_id.to_owned(),
                            request_id,
                            details: params,
                        },
                    );
                }
            }
        }
        AcpInbound::InvalidMessage { error, raw } => {
            warn!(%run_id, %error, %raw, "invalid ACP message");
            let envelope = RpcEnvelope {
                direction: RpcDirection::Received,
                method: "invalid".into(),
                payload: json!({ "error": error, "raw": raw }),
            };
            let _ = inner.store.save_acp_envelope(run_id, &envelope);
        }
        AcpInbound::Disconnected => {
            debug!(%run_id, "ACP disconnected");
            // If this run is still the live one for the chat, mark failed/stopped.
            let still_live = {
                let guard = inner
                    .by_chat
                    .lock()
                    .map_err(|_| Error::msg("session lock poisoned"))?;
                guard
                    .get(chat_id)
                    .map(|live| live.run_id == run_id)
                    .unwrap_or(false)
            };
            if still_live {
                let agent_id = {
                    let mut guard = inner
                        .by_chat
                        .lock()
                        .map_err(|_| Error::msg("session lock poisoned"))?;
                    guard
                        .remove(chat_id)
                        .map(|live| live.agent_id)
                        .unwrap_or_default()
                };
                inner.store.update_run(
                    run_id,
                    RunStatus::Stopped,
                    None,
                    Some("agent disconnected"),
                )?;
                emit(
                    inner,
                    EditorEvent::RunUpdated {
                        run_id: run_id.to_owned(),
                        status: RunStatus::Stopped,
                        error_message: Some("agent disconnected".into()),
                    },
                );
                if !agent_id.is_empty() {
                    emit(
                        inner,
                        EditorEvent::AgentConnectionChanged {
                            agent_id,
                            connected: false,
                            error_message: Some("agent disconnected".into()),
                        },
                    );
                }
            }
        }
        AcpInbound::Barrier(acknowledge) => {
            let _ = acknowledge.send(());
        }
    }
    Ok(())
}

fn apply_notification(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
    event: AgentEventMethod,
    envelope: &RpcEnvelope,
) -> Result<()> {
    match event {
        // Primary ACP streaming path used by Copilot and the official protocol.
        AgentEventMethod::SessionUpdate => {
            apply_session_update(inner, run_id, chat_id, &envelope.payload)?;
        }
        AgentEventMethod::MessageStarted => {
            let message_id = ensure_streaming_message(inner, run_id, chat_id, None)?;
            emit(
                inner,
                EditorEvent::MessageUpdated {
                    message_id,
                    status: MessageStatus::Streaming,
                },
            );
        }
        AgentEventMethod::MessageDelta | AgentEventMethod::ThinkingDelta => {
            let message_id = ensure_streaming_message(inner, run_id, chat_id, None)?;
            if let Some(delta) = extract_text_delta(&envelope.payload) {
                append_text_delta(inner, &message_id, &delta)?;
            }
            emit(
                inner,
                EditorEvent::MessageUpdated {
                    message_id,
                    status: MessageStatus::Streaming,
                },
            );
        }
        AgentEventMethod::MessageCompleted => {
            for message_id in take_streaming_messages(inner, chat_id) {
                finalize_message(inner, &message_id, MessageStatus::Complete)?;
                emit(
                    inner,
                    EditorEvent::MessageUpdated {
                        message_id,
                        status: MessageStatus::Complete,
                    },
                );
            }
        }
        AgentEventMethod::MessageFailed => {
            for message_id in take_streaming_messages(inner, chat_id) {
                finalize_message(inner, &message_id, MessageStatus::Failed)?;
                emit(
                    inner,
                    EditorEvent::MessageUpdated {
                        message_id,
                        status: MessageStatus::Failed,
                    },
                );
            }
        }
        AgentEventMethod::ToolCallStarted
        | AgentEventMethod::ToolCallOutput
        | AgentEventMethod::ToolCallCompleted
        | AgentEventMethod::ToolCallFailed => {
            append_tool_part(inner, run_id, chat_id, &envelope.payload)?;
        }
        AgentEventMethod::SessionEnded => {
            complete_run(inner, run_id, chat_id, RunStatus::Completed, None)?;
        }
        AgentEventMethod::SessionStatusChanged => {
            if let Some(status) = envelope
                .payload
                .get("status")
                .and_then(|v| v.as_str())
                .and_then(|s| RunStatus::parse(s).ok())
            {
                let error = envelope
                    .payload
                    .get("error")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                inner
                    .store
                    .update_run(run_id, status, None, error.as_deref())?;
                emit(
                    inner,
                    EditorEvent::RunUpdated {
                        run_id: run_id.to_owned(),
                        status,
                        error_message: error,
                    },
                );
            }
        }
        AgentEventMethod::FileChanged | AgentEventMethod::FileChangeProposed => {
            if let Some(paths) = envelope.payload.get("paths").and_then(|v| v.as_array()) {
                let paths: Vec<String> = paths
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect();
                if let Ok(Some(chat)) = inner.store.get_chat(chat_id) {
                    emit(
                        inner,
                        EditorEvent::WorkspaceFilesChanged {
                            workspace_path: chat.workspace_path,
                            paths,
                        },
                    );
                }
            }
        }
        AgentEventMethod::PermissionRequested => {
            emit(
                inner,
                EditorEvent::ApprovalRequired {
                    run_id: run_id.to_owned(),
                    request_id: uuid::Uuid::new_v4().to_string(),
                    details: envelope.payload.clone(),
                },
            );
        }
        AgentEventMethod::InputRequested => {
            emit(
                inner,
                EditorEvent::QuestionRequired {
                    run_id: run_id.to_owned(),
                    request_id: uuid::Uuid::new_v4().to_string(),
                    details: envelope.payload.clone(),
                },
            );
        }
        AgentEventMethod::SessionCreated
        | AgentEventMethod::CommandStarted
        | AgentEventMethod::CommandOutput
        | AgentEventMethod::CommandCompleted
        | AgentEventMethod::PlanUpdated
        | AgentEventMethod::ContextUsage
        | AgentEventMethod::Other(_) => {}
    }
    Ok(())
}

/// Handle ACP `session/update` notification params.
fn apply_session_update(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
    payload: &Value,
) -> Result<()> {
    let update = payload.get("update").unwrap_or(payload);
    let kind = update
        .get("sessionUpdate")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match kind {
        "agent_message_chunk" | "user_message_chunk" => {
            let message_id = ensure_streaming_message(
                inner,
                run_id,
                chat_id,
                update.get("messageId").and_then(|value| value.as_str()),
            )?;
            if let Some(delta) = extract_text_delta(update) {
                append_text_delta(inner, &message_id, &delta)?;
            }
            emit(
                inner,
                EditorEvent::MessageUpdated {
                    message_id,
                    status: MessageStatus::Streaming,
                },
            );
        }
        "agent_thought_chunk" => {
            let message_id = ensure_context_message(inner, run_id, chat_id)?;
            if let Some(delta) = extract_text_delta(update) {
                // Store thought as a thinking part + optional text append.
                let ordinal = next_part_ordinal(inner, &message_id)?;
                let mut parts = inner.store.message_parts(&message_id)?;
                parts.push(MessagePart {
                    message_id: message_id.clone(),
                    ordinal,
                    kind: MessagePartKind::Thinking,
                    content_json: json!({ "text": delta }).to_string(),
                });
                inner.store.replace_message_parts(&message_id, &parts)?;
                emit(
                    inner,
                    EditorEvent::MessagePartAdded {
                        message_id: message_id.clone(),
                        ordinal,
                        kind: MessagePartKind::Thinking,
                    },
                );
            }
            emit(
                inner,
                EditorEvent::MessageUpdated {
                    message_id,
                    status: MessageStatus::Streaming,
                },
            );
        }
        "tool_call" | "tool_call_update" => {
            append_tool_part(inner, run_id, chat_id, update)?;
        }
        "available_commands_update" => {
            // Informational; already logged in acp_events.
        }
        _ => {
            debug!(%kind, "unhandled sessionUpdate kind");
        }
    }
    Ok(())
}

fn append_tool_part(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
    payload: &Value,
) -> Result<()> {
    let message_id = ensure_context_message(inner, run_id, chat_id)?;
    let ordinal = next_part_ordinal(inner, &message_id)?;
    let mut parts = inner.store.message_parts(&message_id)?;
    parts.push(MessagePart {
        message_id: message_id.clone(),
        ordinal,
        kind: MessagePartKind::ToolCall,
        content_json: payload.to_string(),
    });
    inner.store.replace_message_parts(&message_id, &parts)?;
    emit(
        inner,
        EditorEvent::MessagePartAdded {
            message_id,
            ordinal,
            kind: MessagePartKind::ToolCall,
        },
    );
    Ok(())
}

fn complete_run(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
    status: RunStatus,
    error: Option<&str>,
) -> Result<()> {
    for message_id in take_streaming_messages(inner, chat_id) {
        finalize_message(inner, &message_id, MessageStatus::Complete)?;
        emit(
            inner,
            EditorEvent::MessageUpdated {
                message_id,
                status: MessageStatus::Complete,
            },
        );
    }
    inner.store.update_run(run_id, status, None, error)?;
    emit(
        inner,
        EditorEvent::RunUpdated {
            run_id: run_id.to_owned(),
            status,
            error_message: error.map(str::to_owned),
        },
    );
    if let Ok(mut guard) = inner.by_chat.lock() {
        if guard
            .get(chat_id)
            .map(|l| l.run_id == run_id)
            .unwrap_or(false)
        {
            if let Some(live) = guard.remove(chat_id) {
                emit(
                    inner,
                    EditorEvent::AgentConnectionChanged {
                        agent_id: live.agent_id,
                        connected: false,
                        error_message: error.map(str::to_owned),
                    },
                );
            }
        }
    }
    Ok(())
}

fn ensure_streaming_message(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
    upstream_message_id: Option<&str>,
) -> Result<String> {
    let mut guard = inner
        .by_chat
        .lock()
        .map_err(|_| Error::msg("session lock poisoned"))?;
    let live = guard
        .get_mut(chat_id)
        .filter(|live| live.run_id == run_id)
        .ok_or_else(|| Error::msg("no live run for streaming message"))?;

    // Older ACP agents omit `messageId`; retain the old single-message
    // behavior for them. Agents such as Copilot provide it, allowing one turn
    // to carry separate commentary and final-answer messages.
    let stream_key = upstream_message_id.unwrap_or("__default__");
    if let Some(id) = live.streaming_message_ids.get(stream_key) {
        let id = id.clone();
        live.last_streaming_message_id = Some(id.clone());
        return Ok(id);
    }

    let now = timestamp();
    let message = Message {
        id: uuid::Uuid::new_v4().to_string(),
        chat_id: chat_id.to_owned(),
        agent_run_id: Some(run_id.to_owned()),
        role: MessageRole::Assistant,
        content: String::new(),
        status: MessageStatus::Streaming,
        created_at: now.clone(),
        updated_at: now,
    };
    inner.store.create_message(&message)?;
    live.streaming_message_ids
        .insert(stream_key.to_owned(), message.id.clone());
    live.last_streaming_message_id = Some(message.id.clone());
    Ok(message.id)
}

/// Associate non-message events such as thinking and tool calls with the
/// latest visible assistant message. ACP does not give these events the same
/// `messageId` as their surrounding commentary/final message.
fn ensure_context_message(inner: &SessionInner, run_id: &str, chat_id: &str) -> Result<String> {
    let last_message_id = {
        let guard = inner
            .by_chat
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?;
        guard
            .get(chat_id)
            .filter(|live| live.run_id == run_id)
            .and_then(|live| live.last_streaming_message_id.clone())
    };
    last_message_id.map_or_else(
        || ensure_streaming_message(inner, run_id, chat_id, None),
        Ok,
    )
}

fn take_streaming_messages(inner: &SessionInner, chat_id: &str) -> Vec<String> {
    let Ok(mut guard) = inner.by_chat.lock() else {
        return Vec::new();
    };
    let Some(live) = guard.get_mut(chat_id) else {
        return Vec::new();
    };
    take_streaming_messages_from_live(live)
}

fn take_streaming_messages_from_live(live: &mut LiveRun) -> Vec<String> {
    live.last_streaming_message_id = None;
    live.streaming_message_ids
        .drain()
        .map(|(_, id)| id)
        .collect()
}

fn append_text_delta(inner: &SessionInner, message_id: &str, delta: &str) -> Result<()> {
    // Load current content from messages list is heavy; update by reading via store messages filter.
    // Simpler: get all messages for chat is not available by id — we only have messages(chat_id).
    // Use replace on content via update_message with accumulated: need current content.
    // Store doesn't have get_message — add lightweight approach: only append via parts + update content from parts rebuild is heavy.
    // Minimal: update_message replaces full content — fetch via listing is wrong.
    // For now append by reading message_parts text parts or use a SQL get.
    // Quick fix: store message content update using parts + content field:
    // We'll get content by scanning is not ideal. Add get via update that concatenates:
    // Actually create_message stores empty; we can keep content in parts only and set content to previous+delta by...
    // Use replace: load parts, find text part, append, also set content field.

    let mut parts = inner.store.message_parts(message_id)?;
    if let Some(text_part) = parts.iter_mut().find(|p| p.kind == MessagePartKind::Text) {
        let mut obj: Value = serde_json::from_str(&text_part.content_json).unwrap_or(json!({}));
        let existing = obj
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let combined = format!("{existing}{delta}");
        obj = json!({ "text": combined });
        text_part.content_json = obj.to_string();
        inner
            .store
            .update_message(message_id, &combined, MessageStatus::Streaming)?;
        inner.store.replace_message_parts(message_id, &parts)?;
    } else {
        parts.push(MessagePart {
            message_id: message_id.to_owned(),
            ordinal: 0,
            kind: MessagePartKind::Text,
            content_json: json!({ "text": delta }).to_string(),
        });
        inner
            .store
            .update_message(message_id, delta, MessageStatus::Streaming)?;
        inner.store.replace_message_parts(message_id, &parts)?;
        emit(
            inner,
            EditorEvent::MessagePartAdded {
                message_id: message_id.to_owned(),
                ordinal: 0,
                kind: MessagePartKind::Text,
            },
        );
    }
    Ok(())
}

fn finalize_message(inner: &SessionInner, message_id: &str, status: MessageStatus) -> Result<()> {
    // Keep existing content; re-read is hard without get_message. Status-only update with empty content would wipe.
    // load parts text:
    let parts = inner.store.message_parts(message_id)?;
    let content = parts
        .iter()
        .filter(|p| p.kind == MessagePartKind::Text)
        .filter_map(|p| {
            serde_json::from_str::<Value>(&p.content_json)
                .ok()
                .and_then(|v| v.get("text")?.as_str().map(str::to_owned))
        })
        .collect::<Vec<_>>()
        .join("");
    inner.store.update_message(message_id, &content, status)?;
    Ok(())
}

fn next_part_ordinal(inner: &SessionInner, message_id: &str) -> Result<i64> {
    let parts = inner.store.message_parts(message_id)?;
    Ok(parts.iter().map(|p| p.ordinal).max().unwrap_or(-1) + 1)
}

fn extract_text_delta(payload: &Value) -> Option<String> {
    // Prefer ACP content block: { "content": { "type": "text", "text": "..." } }
    if let Some(text) = payload
        .pointer("/content/text")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
    {
        return Some(text);
    }
    if let Some(text) = payload
        .get("update")
        .and_then(|u| u.pointer("/content/text"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
    {
        return Some(text);
    }

    payload
        .get("text")
        .or_else(|| payload.get("delta"))
        .or_else(|| payload.get("content"))
        .and_then(|v| {
            if let Some(s) = v.as_str() {
                Some(s.to_owned())
            } else if let Some(obj) = v.as_object() {
                obj.get("text").and_then(|t| t.as_str()).map(str::to_owned)
            } else if let Some(arr) = v.as_array() {
                let text = arr
                    .iter()
                    .filter_map(|item| {
                        item.as_str()
                            .map(str::to_owned)
                            .or_else(|| item.get("text")?.as_str().map(str::to_owned))
                    })
                    .collect::<String>();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            } else {
                None
            }
        })
}

fn emit(inner: &SessionInner, event: EditorEvent) {
    let _ = inner.events.send(event);
}

fn extract_session_id(value: &Value) -> Option<String> {
    value
        .get("sessionId")
        .or_else(|| value.get("session_id"))
        .or_else(|| value.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}
