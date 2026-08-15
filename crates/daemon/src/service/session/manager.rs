//! `SessionManager` — public API for runs, prompts, cancel, and agent replies.

mod agent_requests;
mod lifecycle;
mod prompt;
mod transport;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde_json::{json, Value};
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::{
    acp::AcpClient,
    protocol::{
        AgentEventMethod, AgentRpcMethod, EditorEvent, MessagePartKind, MessageRole, MessageStatus,
        RpcDirection, RpcEnvelope, RunStatus, TurnStatus,
    },
    service::agent_manager::AgentManager,
    service::attachments::AttachmentStore,
    store::{AgentRun, Message, MessagePart, Store},
    Error, Result,
};

use super::{
    inbound::spawn_inbound_worker,
    messages::{
        finalize_message, remove_pending_requests_for_run, take_streaming_messages_from_live,
    },
    session_config::{configure_session, SessionConfiguration},
    types::{
        LiveRun, PendingAgentRequest, PromptResult, SessionInner, ACP_PROMPT_IDLE_TIMEOUT,
        ACP_PROMPT_TOTAL_TIMEOUT, ACP_REQUEST_TIMEOUT,
    },
    util::{extract_session_id, extract_stop_reason, normalize_permission_result, timestamp},
};

pub struct SessionManager {
    agents: AgentManager,
    attachments: AttachmentStore,
    inner: Arc<SessionInner>,
}

impl SessionManager {
    pub fn new(
        store: Arc<Store>,
        agents: AgentManager,
        events: broadcast::Sender<EditorEvent>,
        attachments_dir: PathBuf,
    ) -> Self {
        Self {
            agents,
            attachments: AttachmentStore::new(attachments_dir),
            inner: Arc::new(SessionInner {
                store,
                events,
                prompt_locks: std::sync::Mutex::new(HashMap::new()),
                by_chat: std::sync::Mutex::new(HashMap::new()),
                pending: std::sync::Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn get_attachment(&self, chat_id: &str, attachment_id: &str) -> Result<(String, String)> {
        self.inner
            .store
            .get_chat(chat_id)?
            .ok_or_else(|| Error::msg(format!("chat not found: {chat_id}")))?;
        self.attachments.read(chat_id, attachment_id)
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
        // Ownership begins as soon as the ACP process exists, not only after
        // initialization. This lets legitimate startup requests through while
        // still giving inbound workers an exact run-id ownership check.
        self.inner
            .by_chat
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?
            .insert(
                chat_id.to_owned(),
                LiveRun {
                    run_id: run.id.clone(),
                    agent_id: agent_id.to_owned(),
                    client: Arc::clone(&client),
                    acp_session_id: None,
                    supports_images: false,
                    session_configuration: SessionConfiguration::default(),
                    needs_history_hydration: false,
                    streaming_message_ids: HashMap::new(),
                    last_streaming_message_id: None,
                    active_user_message_id: None,
                },
            );
        spawn_inbound_worker(
            Arc::clone(&self.inner),
            run.id.clone(),
            chat_id.to_owned(),
            inbound,
        );

        // Real ACP (Copilot, etc.): method names are `initialize` / `session/*`,
        // not a proprietary `agent.*` namespace.
        let initialize_response = match self.acp_request(
            &run.id,
            &client,
            AgentRpcMethod::Initialize,
            json!({
                "protocolVersion": 1,
                // Advertise only the elicitation mode the desktop client can
                // actually render and answer. Agents commonly disable their
                // ask-user flow when this is absent.
                "clientCapabilities": {
                    "session": {
                        "configOptions": {
                            "boolean": {}
                        }
                    },
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
            Ok(value) => value,
            Err(err) => {
                self.remove_live_run(chat_id, &run.id);
                let _ = client.kill();
                let _ = self.fail_run(&run.id, &err.to_string());
                return Err(err);
            }
        };
        let supports_images = initialize_response
            .pointer("/agentCapabilities/promptCapabilities/image")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if let Some(live) = self
            .inner
            .by_chat
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?
            .get_mut(chat_id)
        {
            live.supports_images = supports_images;
        }

        let previous_session_id = self
            .inner
            .store
            .list_runs_for_chat(chat_id)?
            .into_iter()
            .filter(|previous| previous.id != run.id && previous.agent_id == agent_id)
            .find_map(|previous| previous.acp_session_id);
        let session_setup = (|| -> Result<(Option<String>, bool, SessionConfiguration)> {
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
                    Ok(value) => Ok((
                        Some(session_id),
                        has_persisted_history,
                        SessionConfiguration::from_response(&value),
                    )),
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
                        Ok((
                            extract_session_id(&value),
                            has_persisted_history,
                            SessionConfiguration::from_response(&value),
                        ))
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
                Ok((
                    extract_session_id(&value),
                    has_persisted_history,
                    SessionConfiguration::from_response(&value),
                ))
            }
        })();
        let (acp_session_id, needs_history_hydration, mut session_configuration) =
            match session_setup {
                Ok(value) => value,
                Err(err) => {
                    self.remove_live_run(chat_id, &run.id);
                    let _ = client.kill();
                    let _ = self.fail_run(&run.id, &err.to_string());
                    return Err(err);
                }
            };

        if let (Some(mode), Some(session_id)) = (session_mode, acp_session_id.as_deref()) {
            match configure_session(
                session_id,
                &mut session_configuration,
                mode,
                |method, params| {
                    self.acp_request(
                        &run.id,
                        &client,
                        AgentRpcMethod::Other(method.to_owned()),
                        params,
                    )
                },
            ) {
                Ok(true) => {}
                Ok(false) => {
                    debug!(%agent_id, %mode, "agent does not advertise a compatible session mode; retaining its default");
                }
                Err(err) => {
                    self.remove_live_run(chat_id, &run.id);
                    let _ = client.kill();
                    let _ = self.fail_run(&run.id, &err.to_string());
                    return Err(err);
                }
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

        let mut live_runs = self
            .inner
            .by_chat
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?;
        let live = live_runs
            .get_mut(chat_id)
            .filter(|live| live.run_id == run.id)
            .ok_or_else(|| Error::msg("agent disconnected during session startup"))?;
        live.acp_session_id = acp_session_id.clone();
        live.session_configuration = session_configuration;
        live.needs_history_hydration = needs_history_hydration;
        drop(live_runs);

        let mut run = run;
        run.status = RunStatus::Running;
        run.acp_session_id = acp_session_id;
        Ok(run)
    }

    fn emit(&self, event: EditorEvent) {
        let _ = self.inner.events.send(event);
    }
}

#[cfg(all(test, unix))]
mod tests;
