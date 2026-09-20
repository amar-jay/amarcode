use std::{collections::HashMap, path::PathBuf, sync::Arc};

use agent_client_protocol::{
    schema::{
        v1::{
            AgentCapabilities, CancelNotification, CloseSessionRequest, CloseSessionResponse,
            ContentBlock, ContentChunk, DeleteSessionRequest, DeleteSessionResponse,
            Implementation, InitializeRequest, InitializeResponse, ListSessionsRequest,
            ListSessionsResponse, LoadSessionRequest, LoadSessionResponse, MessageId,
            NewSessionRequest, NewSessionResponse, PromptCapabilities, PromptRequest,
            PromptResponse, ResumeSessionRequest, ResumeSessionResponse, SessionCapabilities,
            SessionCloseCapabilities, SessionConfigOption, SessionConfigOptionCategory,
            SessionConfigOptionValue, SessionDeleteCapabilities, SessionId, SessionInfo,
            SessionListCapabilities, SessionNotification, SessionResumeCapabilities, SessionUpdate,
            SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, SetSessionModeRequest,
            SetSessionModeResponse, StopReason, TextContent,
        },
        ProtocolVersion,
    },
    Agent, Client, ConnectTo, ConnectionTo, Error, Responder, Stdio,
};
use reqwest::Client as HttpClient;
use tokio::sync::{watch, Mutex};
use uuid::Uuid;

use crate::{
    config::Config,
    persistence::{PersistedSession, SessionStore},
    provider::{self, Completion, ModelTurn},
};

#[derive(Clone)]
struct Runtime {
    config: Arc<Config>,
    http: HttpClient,
    sessions: Arc<Mutex<HashMap<SessionId, Session>>>,
    store: Option<Arc<SessionStore>>,
}

struct Session {
    cwd: PathBuf,
    history: Vec<serde_json::Value>,
    mode: String,
    active_turn: Option<ActiveTurn>,
    permissions: Arc<crate::tools::PermissionState>,
}

impl Session {
    fn from_persisted(saved: PersistedSession) -> Self {
        Self {
            cwd: saved.cwd,
            history: saved.history,
            mode: saved.mode,
            active_turn: None,
            permissions: Arc::new(crate::tools::PermissionState::default()),
        }
    }

    fn persisted(&self, session_id: &SessionId) -> PersistedSession {
        PersistedSession {
            session_id: session_id.to_string(),
            cwd: self.cwd.clone(),
            history: self.history.clone(),
            mode: self.mode.clone(),
            updated_at: crate::persistence::now_seconds(),
        }
    }
}

struct ActiveTurn {
    id: Uuid,
    cancel: watch::Sender<bool>,
    history_len: usize,
}

pub async fn serve(config: Config) -> agent_client_protocol::Result<()> {
    build_agent(Runtime::new(config))
        .connect_to(Stdio::new())
        .await
}

impl Runtime {
    fn new(config: Config) -> Self {
        let store = config.persistence.enabled.then(|| {
            Arc::new(SessionStore::new(
                config.session_store_path(),
                config.persistence.ttl_seconds,
            ))
        });
        let saved = store
            .as_ref()
            .and_then(|store| match store.load() {
                Ok(sessions) => Some(sessions),
                Err(error) => {
                    eprintln!("amarcode-acp: persistence error: {error}");
                    None
                }
            })
            .unwrap_or_default();
        let sessions = saved
            .into_iter()
            .map(|(id, saved)| (id, Session::from_persisted(saved)))
            .collect();
        Self {
            config: Arc::new(config),
            http: HttpClient::new(),
            sessions: Arc::new(Mutex::new(sessions)),
            store,
        }
    }

    async fn initialize(&self, request: InitializeRequest) -> InitializeResponse {
        let protocol_version = match request.protocol_version {
            ProtocolVersion::V1 => ProtocolVersion::V1,
            _ => ProtocolVersion::V1,
        };
        let mut session_capabilities =
            SessionCapabilities::new().close(SessionCloseCapabilities::new());
        if self.store.is_some() {
            session_capabilities = session_capabilities
                .list(SessionListCapabilities::new())
                .delete(SessionDeleteCapabilities::new())
                .resume(SessionResumeCapabilities::new());
        }
        InitializeResponse::new(protocol_version)
            .agent_capabilities(
                AgentCapabilities::new()
                    .load_session(self.store.is_some())
                    .prompt_capabilities(PromptCapabilities::new().image(true))
                    .session_capabilities(session_capabilities),
            )
            .agent_info(
                Implementation::new(self.config.name.clone(), env!("CARGO_PKG_VERSION"))
                    .title(self.config.title()),
            )
    }

    async fn new_session(&self, request: NewSessionRequest) -> NewSessionResponse {
        let session_id = SessionId::new(Uuid::new_v4().to_string());
        let session = Session {
            cwd: request.cwd,
            history: Vec::new(),
            mode: "ask".into(),
            active_turn: None,
            permissions: Arc::new(crate::tools::PermissionState::default()),
        };
        self.persist(&session_id, &session);
        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), session);
        NewSessionResponse::new(session_id).config_options(self.config_options("ask"))
    }

    async fn set_mode(
        &self,
        session_id: &SessionId,
        mode: &str,
    ) -> agent_client_protocol::Result<()> {
        if !matches!(mode, "ask" | "code" | "plan") {
            return Err(Error::invalid_params().data("unsupported session mode"));
        }
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| Error::invalid_params().data("unknown session"))?;
        if session.mode != mode {
            for message in &mut session.history {
                if let Some(message) = message.as_object_mut() {
                    message.remove("reasoning_details");
                }
            }
        }
        session.mode = mode.to_owned();
        self.persist(session_id, session);
        Ok(())
    }

    async fn cancel(&self, session_id: &SessionId) {
        let sessions = self.sessions.lock().await;
        if let Some(turn) = sessions
            .get(session_id)
            .and_then(|session| session.active_turn.as_ref())
        {
            let _ = turn.cancel.send(true);
        }
    }

    async fn close(&self, session_id: &SessionId) -> agent_client_protocol::Result<()> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .remove(session_id)
            .ok_or_else(|| Error::invalid_params().data("unknown session"))?;
        if let Some(turn) = session.active_turn {
            let _ = turn.cancel.send(true);
        }
        Ok(())
    }

    async fn restore(
        &self,
        session_id: &SessionId,
        cwd: &std::path::Path,
    ) -> agent_client_protocol::Result<String> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| Error::invalid_request().data("session persistence is disabled"))?;
        let saved = store
            .load()
            .map_err(|error| Error::internal_error().data(error))?
            .remove(session_id)
            .ok_or_else(|| Error::invalid_params().data("unknown or expired session"))?;
        if saved.cwd != cwd {
            return Err(Error::invalid_params().data("session working directory does not match"));
        }
        let mode = saved.mode.clone();
        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), Session::from_persisted(saved));
        Ok(mode)
    }

    async fn list_sessions(
        &self,
        request: &ListSessionsRequest,
    ) -> agent_client_protocol::Result<ListSessionsResponse> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| Error::invalid_request().data("session persistence is disabled"))?;
        if request.cursor.is_some() {
            return Err(Error::invalid_params().data("session list cursor is not supported"));
        }
        let mut sessions = store
            .load()
            .map_err(|error| Error::internal_error().data(error))?
            .into_values()
            .filter(|session| request.cwd.as_ref().is_none_or(|cwd| cwd == &session.cwd))
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(ListSessionsResponse::new(
            sessions
                .into_iter()
                .map(|session| {
                    let title = session_title(&session.history);
                    SessionInfo::new(session.session_id, session.cwd).title(title)
                })
                .collect(),
        ))
    }

    async fn delete_session(&self, session_id: &SessionId) -> agent_client_protocol::Result<()> {
        if let Some(session) = self.sessions.lock().await.remove(session_id) {
            if let Some(turn) = session.active_turn {
                let _ = turn.cancel.send(true);
            }
        }
        let deleted = self
            .store
            .as_ref()
            .ok_or_else(|| Error::invalid_request().data("session persistence is disabled"))?
            .delete(session_id)
            .map_err(|error| Error::internal_error().data(error))?;
        if !deleted {
            return Err(Error::invalid_params().data("unknown or expired session"));
        }
        Ok(())
    }

    fn persist(&self, session_id: &SessionId, session: &Session) {
        let Some(store) = &self.store else { return };
        if let Err(error) = store.upsert(session.persisted(session_id)) {
            eprintln!("amarcode-acp: persistence error: {error}");
        }
    }

    fn config_options(&self, mode: &str) -> Vec<SessionConfigOption> {
        vec![
            SessionConfigOption::select(
                "mode",
                "Session mode",
                mode.to_owned(),
                vec![
                    agent_client_protocol::schema::v1::SessionConfigSelectOption::new("ask", "Ask"),
                    agent_client_protocol::schema::v1::SessionConfigSelectOption::new(
                        "code", "Code",
                    ),
                    agent_client_protocol::schema::v1::SessionConfigSelectOption::new(
                        "plan", "Plan",
                    ),
                ],
            )
            .category(SessionConfigOptionCategory::Mode),
            SessionConfigOption::select(
                "model",
                "Model",
                self.config.provider.model.clone(),
                vec![
                    agent_client_protocol::schema::v1::SessionConfigSelectOption::new(
                        self.config.provider.model.clone(),
                        self.config.provider.model.clone(),
                    ),
                ],
            )
            .category(SessionConfigOptionCategory::Model),
        ]
    }

    async fn begin_turn(
        &self,
        request: &PromptRequest,
    ) -> agent_client_protocol::Result<(
        Uuid,
        PathBuf,
        String,
        Vec<serde_json::Value>,
        Arc<crate::tools::PermissionState>,
        watch::Receiver<bool>,
    )> {
        let prompt = provider_prompt_content(&request.prompt)?;
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(&request.session_id)
            .ok_or_else(|| Error::invalid_params().data("unknown session"))?;
        if session.active_turn.is_some() {
            return Err(Error::invalid_request().data("session already has an active turn"));
        }
        let history_len = session.history.len();
        session
            .history
            .push(serde_json::json!({ "role": "user", "content": prompt }));
        let history = session.history.clone();
        let turn_id = Uuid::new_v4();
        let (cancel, cancellation) = watch::channel(false);
        session.active_turn = Some(ActiveTurn {
            id: turn_id,
            cancel,
            history_len,
        });
        Ok((
            turn_id,
            session.cwd.clone(),
            session.mode.clone(),
            history,
            Arc::clone(&session.permissions),
            cancellation,
        ))
    }

    async fn finish_turn(
        &self,
        session_id: &SessionId,
        turn_id: Uuid,
        history: Option<Vec<serde_json::Value>>,
    ) {
        let mut sessions = self.sessions.lock().await;
        let Some(session) = sessions.get_mut(session_id) else {
            return;
        };
        if session
            .active_turn
            .as_ref()
            .is_none_or(|active| active.id != turn_id)
        {
            return;
        }
        let active = session.active_turn.take().expect("checked active turn");
        if let Some(history) = history {
            session.history = history;
            self.persist(session_id, session);
        } else {
            session.history.truncate(active.history_len);
        }
    }
}

fn build_agent(runtime: Runtime) -> impl agent_client_protocol::ConnectTo<Client> {
    let initialize_runtime = runtime.clone();
    let new_session_runtime = runtime.clone();
    let load_session_runtime = runtime.clone();
    let resume_session_runtime = runtime.clone();
    let list_sessions_runtime = runtime.clone();
    let delete_session_runtime = runtime.clone();
    let set_mode_runtime = runtime.clone();
    let set_config_runtime = runtime.clone();
    let prompt_runtime = runtime.clone();
    let cancel_runtime = runtime.clone();
    let close_runtime = runtime;

    Agent
        .builder()
        .name("amarcode-acp")
        .on_receive_request(
            async move |request: InitializeRequest, responder, _connection| {
                responder.respond(initialize_runtime.initialize(request).await)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: NewSessionRequest, responder, _connection| {
                responder.respond(new_session_runtime.new_session(request).await)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: LoadSessionRequest,
                        responder,
                        connection: ConnectionTo<Client>| {
                let result = async {
                    let mode = load_session_runtime
                        .restore(&request.session_id, &request.cwd)
                        .await?;
                    let sessions = load_session_runtime.sessions.lock().await;
                    let session = sessions.get(&request.session_id).ok_or_else(|| {
                        Error::internal_error().data("restored session disappeared")
                    })?;
                    replay_history(&connection, &request.session_id, &session.history)?;
                    Ok(LoadSessionResponse::new()
                        .config_options(load_session_runtime.config_options(&mode)))
                }
                .await;
                responder.respond_with_result(result)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ResumeSessionRequest, responder, _connection| {
                let result = resume_session_runtime
                    .restore(&request.session_id, &request.cwd)
                    .await
                    .map(|mode| {
                        ResumeSessionResponse::new()
                            .config_options(resume_session_runtime.config_options(&mode))
                    });
                responder.respond_with_result(result)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ListSessionsRequest, responder, _connection| {
                responder.respond_with_result(list_sessions_runtime.list_sessions(&request).await)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: DeleteSessionRequest, responder, _connection| {
                let result = delete_session_runtime
                    .delete_session(&request.session_id)
                    .await
                    .map(|()| DeleteSessionResponse::new());
                responder.respond_with_result(result)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: SetSessionModeRequest, responder, _connection| {
                let result = set_mode_runtime
                    .set_mode(&request.session_id, &request.mode_id.to_string())
                    .await
                    .map(|()| SetSessionModeResponse::new());
                responder.respond_with_result(result)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: SetSessionConfigOptionRequest, responder, _connection| {
                let result = match (request.config_id.to_string().as_str(), request.value) {
                    ("mode", SessionConfigOptionValue::ValueId { value }) => {
                        let mode = value.to_string();
                        set_config_runtime
                            .set_mode(&request.session_id, &mode)
                            .await
                            .map(|()| {
                                SetSessionConfigOptionResponse::new(
                                    set_config_runtime.config_options(&mode),
                                )
                            })
                    }
                    ("model", SessionConfigOptionValue::ValueId { value })
                        if value.to_string() == set_config_runtime.config.provider.model =>
                    {
                        let sessions = set_config_runtime.sessions.lock().await;
                        let result = sessions
                            .get(&request.session_id)
                            .ok_or_else(|| Error::invalid_params().data("unknown session"))
                            .map(|session| {
                                SetSessionConfigOptionResponse::new(
                                    set_config_runtime.config_options(&session.mode),
                                )
                            });
                        result
                    }
                    _ => Err(Error::invalid_params().data("unknown configuration option")),
                };
                responder.respond_with_result(result)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: PromptRequest,
                        responder: Responder<PromptResponse>,
                        connection: ConnectionTo<Client>| {
                let (turn_id, cwd, mode, history, permissions, cancellation) =
                    match prompt_runtime.begin_turn(&request).await {
                        Ok(turn) => turn,
                        Err(error) => return responder.respond_with_error(error),
                    };
                let runtime = prompt_runtime.clone();
                let session_id = request.session_id;
                let request_cancellation = responder.cancellation();
                let message_id =
                    agent_client_protocol::schema::v1::MessageId::new(Uuid::new_v4().to_string());
                connection.clone().spawn(async move {
                    let outcome = tokio::select! {
                        outcome = run_agent_turn(
                            &runtime, history, cwd, mode, permissions, session_id.clone(), message_id,
                            connection, cancellation,
                        ) => outcome,
                        _ = request_cancellation.cancelled() => {
                            runtime.cancel(&session_id).await;
                            runtime.finish_turn(&session_id, turn_id, None).await;
                            return responder.respond_with_error(Error::request_cancelled());
                        }
                    };
                    match outcome {
                        Ok(Completion::Completed(history)) => {
                            runtime
                                .finish_turn(&session_id, turn_id, Some(history))
                                .await;
                            responder.respond(PromptResponse::new(StopReason::EndTurn))
                        }
                        Ok(Completion::Cancelled) => {
                            runtime.finish_turn(&session_id, turn_id, None).await;
                            responder.respond(PromptResponse::new(StopReason::Cancelled))
                        }
                        Err(error) => {
                            runtime.finish_turn(&session_id, turn_id, None).await;
                            responder.respond_with_error(Error::internal_error().data(error))
                        }
                    }
                })?;
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            async move |notification: CancelNotification, _connection| {
                cancel_runtime.cancel(&notification.session_id).await;
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: CloseSessionRequest, responder, _connection| {
                let result = close_runtime
                    .close(&request.session_id)
                    .await
                    .map(|()| CloseSessionResponse::new());
                responder.respond_with_result(result)
            },
            agent_client_protocol::on_receive_request!(),
        )
}

#[expect(
    clippy::too_many_arguments,
    reason = "turn execution requires explicit protocol, workspace, model-history, and cancellation state"
)]
async fn run_agent_turn(
    runtime: &Runtime,
    mut history: Vec<serde_json::Value>,
    cwd: PathBuf,
    mode: String,
    permissions: Arc<crate::tools::PermissionState>,
    session_id: SessionId,
    mut message_id: agent_client_protocol::schema::v1::MessageId,
    connection: ConnectionTo<Client>,
    cancellation: watch::Receiver<bool>,
) -> Result<Completion<Vec<serde_json::Value>>, String> {
    const MAX_TOOL_ROUNDS: usize = 16;
    let mut search_group: Option<SearchPresentationGroup> = None;
    for _ in 0..MAX_TOOL_ROUNDS {
        let completion = match provider::stream_completion(
            &runtime.http,
            &runtime.config,
            &history,
            &mode,
            provider::StreamContext {
                session_id: session_id.clone(),
                message_id,
                connection: connection.clone(),
                cancellation: cancellation.clone(),
            },
        )
        .await
        {
            Ok(completion) => completion,
            Err(error) => {
                finish_active_search_group(&connection, &session_id, search_group.take(), true);
                return Err(error);
            }
        };
        let Completion::Completed(mut turn) = completion else {
            finish_active_search_group(&connection, &session_id, search_group.take(), true);
            return Ok(Completion::Cancelled);
        };
        normalize_tool_ids(&mut turn);
        history.push(provider::assistant_message(&turn));
        if turn.tool_calls.is_empty() {
            finish_active_search_group(&connection, &session_id, search_group.take(), false);
            return Ok(Completion::Completed(history));
        }
        for call in &turn.tool_calls {
            if *cancellation.borrow() {
                finish_active_search_group(&connection, &session_id, search_group.take(), true);
                return Ok(Completion::Cancelled);
            }
            if crate::tools::is_search_tool(call) {
                if search_group.is_none() {
                    let group_id = format!("search-group-{}", Uuid::new_v4());
                    crate::tools::begin_search_group(&connection, &session_id, &group_id);
                    search_group = Some(SearchPresentationGroup {
                        id: group_id,
                        count: 0,
                        failed: 0,
                    });
                }
                let group = search_group.as_mut().expect("search group initialized");
                group.count += 1;
                let output = match crate::tools::execute_grouped_search(call, &cwd) {
                    Ok(output) => output,
                    Err(error) => {
                        group.failed += 1;
                        format!("Error: {error}")
                    }
                };
                history.push(provider::tool_message(call, &output));
                continue;
            }
            finish_active_search_group(&connection, &session_id, search_group.take(), false);
            let output = crate::tools::execute(
                call,
                &cwd,
                &mode,
                &permissions,
                &session_id,
                &connection,
                cancellation.clone(),
            )
            .await;
            history.push(provider::tool_message(call, &output));
        }
        message_id = agent_client_protocol::schema::v1::MessageId::new(Uuid::new_v4().to_string());
    }
    finish_active_search_group(&connection, &session_id, search_group.take(), true);
    Err("maximum tool-call rounds exceeded".into())
}

struct SearchPresentationGroup {
    id: String,
    count: usize,
    failed: usize,
}

fn finish_active_search_group(
    connection: &ConnectionTo<Client>,
    session_id: &SessionId,
    group: Option<SearchPresentationGroup>,
    interrupted: bool,
) {
    if let Some(group) = group {
        crate::tools::finish_search_group(
            connection,
            session_id,
            &group.id,
            group.count,
            group.failed + usize::from(interrupted),
        );
    }
}

fn normalize_tool_ids(turn: &mut ModelTurn) {
    for call in &mut turn.tool_calls {
        if call.id.is_empty() {
            call.id = Uuid::new_v4().to_string();
        }
    }
}

fn replay_history(
    connection: &ConnectionTo<Client>,
    session_id: &SessionId,
    history: &[serde_json::Value],
) -> agent_client_protocol::Result<()> {
    for message in history {
        let Some(text) = message.get("content").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
            .message_id(MessageId::new(Uuid::new_v4().to_string()));
        let update = match message.get("role").and_then(serde_json::Value::as_str) {
            Some("user") => SessionUpdate::UserMessageChunk(chunk),
            Some("assistant") => SessionUpdate::AgentMessageChunk(chunk),
            _ => continue,
        };
        connection.send_notification(SessionNotification::new(session_id.clone(), update))?;
    }
    Ok(())
}

fn session_title(history: &[serde_json::Value]) -> Option<String> {
    let prompt = history.iter().find_map(|message| {
        (message.get("role").and_then(serde_json::Value::as_str) == Some("user"))
            .then(|| message.get("content").and_then(serde_json::Value::as_str))
            .flatten()
    })?;
    let mut title = prompt.chars().take(80).collect::<String>();
    if prompt.chars().count() > 80 {
        title.push('…');
    }
    Some(title)
}

fn extract_prompt_text(prompt: &[ContentBlock]) -> String {
    prompt
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}

fn provider_prompt_content(
    prompt: &[ContentBlock],
) -> agent_client_protocol::Result<serde_json::Value> {
    let has_image = prompt
        .iter()
        .any(|block| matches!(block, ContentBlock::Image(_)));
    if !has_image {
        let text = extract_prompt_text(prompt);
        if text.is_empty() {
            return Err(Error::invalid_params().data("prompt must contain text or an image"));
        }
        return Ok(serde_json::Value::String(text));
    }
    let mut content = Vec::new();
    for block in prompt {
        match block {
            ContentBlock::Text(text) if !text.text.is_empty() => {
                content.push(serde_json::json!({ "type": "text", "text": text.text }));
            }
            ContentBlock::Image(image) => {
                if !matches!(
                    image.mime_type.as_str(),
                    "image/png" | "image/jpeg" | "image/webp" | "image/gif"
                ) {
                    return Err(Error::invalid_params()
                        .data(format!("unsupported image MIME type: {}", image.mime_type)));
                }
                if image.data.trim().is_empty() {
                    return Err(Error::invalid_params().data("image data must not be empty"));
                }
                content.push(serde_json::json!({
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:{};base64,{}", image.mime_type, image.data),
                    },
                }));
            }
            _ => {}
        }
    }
    if content.is_empty() {
        return Err(Error::invalid_params().data("prompt must contain text or an image"));
    }
    Ok(serde_json::Value::Array(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(persistence: crate::config::PersistenceConfig, source_path: PathBuf) -> Config {
        Config {
            name: "test-agent".into(),
            provider: crate::config::ProviderConfig {
                base_url: "https://example.test/v1".into(),
                api_key: "secret".into(),
                model: "test-model".into(),
                reasoning: None,
            },
            persistence,
            source_path,
        }
    }

    fn runtime() -> Runtime {
        Runtime::new(config(
            crate::config::PersistenceConfig {
                enabled: false,
                ..Default::default()
            },
            PathBuf::from("test-config.json"),
        ))
    }

    #[tokio::test]
    async fn sessions_have_unique_ids_and_independent_modes() {
        let runtime = runtime();
        let first = runtime
            .new_session(NewSessionRequest::new("/workspace"))
            .await;
        let second = runtime
            .new_session(NewSessionRequest::new("/workspace"))
            .await;
        assert_ne!(first.session_id, second.session_id);

        runtime
            .set_mode(&first.session_id, "code")
            .await
            .expect("set first mode");
        let sessions = runtime.sessions.lock().await;
        assert_eq!(sessions[&first.session_id].mode, "code");
        assert_eq!(sessions[&second.session_id].mode, "ask");
        assert_eq!(sessions[&first.session_id].cwd, PathBuf::from("/workspace"));
    }

    #[tokio::test]
    async fn changing_mode_discards_stale_reasoning_constraints() {
        let runtime = runtime();
        let created = runtime
            .new_session(NewSessionRequest::new("/workspace"))
            .await;
        {
            let mut sessions = runtime.sessions.lock().await;
            sessions
                .get_mut(&created.session_id)
                .expect("session")
                .history
                .push(serde_json::json!({
                    "role": "assistant",
                    "content": "I cannot edit in ask mode.",
                    "reasoning_details": [{ "type": "reasoning.text", "text": "read-only" }],
                }));
        }

        runtime
            .set_mode(&created.session_id, "code")
            .await
            .expect("switch to code mode");

        let sessions = runtime.sessions.lock().await;
        let session = sessions.get(&created.session_id).expect("session");
        assert_eq!(session.mode, "code");
        assert!(session.history[0].get("reasoning_details").is_none());
    }

    #[tokio::test]
    async fn cancel_targets_only_the_requested_session() {
        let runtime = runtime();
        let first = runtime.new_session(NewSessionRequest::new("/one")).await;
        let second = runtime.new_session(NewSessionRequest::new("/two")).await;
        let (_, _, _, _, _, first_cancel) = runtime
            .begin_turn(&PromptRequest::new(
                first.session_id.clone(),
                vec!["one".into()],
            ))
            .await
            .expect("begin first turn");
        let (_, _, _, _, _, second_cancel) = runtime
            .begin_turn(&PromptRequest::new(
                second.session_id.clone(),
                vec!["two".into()],
            ))
            .await
            .expect("begin second turn");

        runtime.cancel(&first.session_id).await;
        assert!(*first_cancel.borrow());
        assert!(!*second_cancel.borrow());
    }

    #[tokio::test]
    async fn capabilities_only_advertise_close() {
        let runtime = runtime();
        let response = runtime
            .initialize(InitializeRequest::new(ProtocolVersion::V1))
            .await;
        assert!(!response.agent_capabilities.load_session);
        assert!(response.agent_capabilities.prompt_capabilities.image);
        let capabilities = response.agent_capabilities.session_capabilities;
        assert!(capabilities.close.is_some());
        assert!(capabilities.list.is_none());
        assert!(capabilities.resume.is_none());
        assert!(capabilities.delete.is_none());
        assert!(response.auth_methods.is_empty());
    }

    #[tokio::test]
    async fn sessions_survive_runtime_restart() {
        let path = std::env::temp_dir().join(format!(
            "amarcode-acp-runtime-persistence-{}.json",
            Uuid::new_v4()
        ));
        let persistence = crate::config::PersistenceConfig {
            enabled: true,
            ttl_seconds: 24 * 60 * 60,
            path: Some(path.clone()),
        };
        let config = config(persistence, PathBuf::from("test-config.json"));
        let runtime = Runtime::new(config.clone());
        let created = runtime
            .new_session(NewSessionRequest::new("/workspace"))
            .await;
        runtime
            .set_mode(&created.session_id, "code")
            .await
            .expect("persist mode");
        {
            let mut sessions = runtime.sessions.lock().await;
            let session = sessions.get_mut(&created.session_id).expect("live session");
            session
                .history
                .push(serde_json::json!({ "role": "user", "content": "remember me" }));
            runtime.persist(&created.session_id, session);
        }
        drop(runtime);

        let restored = Runtime::new(config);
        let sessions = restored.sessions.lock().await;
        let session = sessions.get(&created.session_id).expect("restored session");
        assert_eq!(session.mode, "code");
        assert_eq!(session.history[0]["content"], "remember me");
        assert!(session.active_turn.is_none());
        drop(sessions);
        let capabilities = restored
            .initialize(InitializeRequest::new(ProtocolVersion::V1))
            .await
            .agent_capabilities;
        assert!(capabilities.load_session);
        assert!(capabilities.session_capabilities.list.is_some());
        assert!(capabilities.session_capabilities.resume.is_some());
        assert!(capabilities.session_capabilities.delete.is_some());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn extracts_text_from_typed_content_blocks() {
        let prompt = vec!["hello ".into(), "world".into()];
        assert_eq!(extract_prompt_text(&prompt), "hello world");
    }

    #[test]
    fn converts_acp_images_to_openai_image_urls() {
        let prompt = vec![
            ContentBlock::Text(TextContent::new("describe this")),
            ContentBlock::Image(agent_client_protocol::schema::v1::ImageContent::new(
                "AQID",
                "image/png",
            )),
        ];
        let content = provider_prompt_content(&prompt).expect("multimodal prompt");
        assert_eq!(
            content[0],
            serde_json::json!({
                "type": "text",
                "text": "describe this",
            })
        );
        assert_eq!(
            content[1],
            serde_json::json!({
                "type": "image_url",
                "image_url": { "url": "data:image/png;base64,AQID" },
            })
        );
    }

    #[test]
    fn rejects_unsupported_image_mime_types() {
        let prompt = vec![ContentBlock::Image(
            agent_client_protocol::schema::v1::ImageContent::new("AQID", "image/svg+xml"),
        )];
        assert!(provider_prompt_content(&prompt).is_err());
    }
}
