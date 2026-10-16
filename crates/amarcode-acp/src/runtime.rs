use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use agent_client_protocol::{
    schema::{
        v1::{
            AgentCapabilities, CancelNotification, CloseSessionRequest, CloseSessionResponse,
            ContentBlock, ContentChunk, Cost, DeleteSessionRequest, DeleteSessionResponse,
            Implementation, InitializeRequest, InitializeResponse, ListSessionsRequest,
            ListSessionsResponse, LoadSessionRequest, LoadSessionResponse, MessageId,
            NewSessionRequest, NewSessionResponse, PromptCapabilities, PromptRequest,
            PromptResponse, ResumeSessionRequest, ResumeSessionResponse, SessionCapabilities,
            SessionCloseCapabilities, SessionConfigOption, SessionConfigOptionCategory,
            SessionConfigOptionValue, SessionDeleteCapabilities, SessionId, SessionInfo,
            SessionListCapabilities, SessionNotification, SessionResumeCapabilities, SessionUpdate,
            SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, StopReason, TextContent,
            UsageUpdate,
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
    provider::{self, Completion, ModelLimits, ModelTurn},
};

#[derive(Clone)]
struct Runtime {
    config: Arc<Config>,
    http: HttpClient,
    sessions: Arc<Mutex<HashMap<SessionId, Session>>>,
    store: Option<Arc<SessionStore>>,
    models: Arc<Mutex<ModelCache>>,
}

#[derive(Default)]
struct ModelCache {
    fetched_at: Option<Instant>,
    ids: Vec<String>,
    context_lengths: HashMap<String, u64>,
    max_completion_tokens: HashMap<String, u64>,
}

struct Session {
    cwd: PathBuf,
    history: Vec<serde_json::Value>,
    model: String,
    active_turn: Option<ActiveTurn>,
    permissions: Arc<crate::tools::PermissionState>,
    cost_usd: f64,
}

impl Session {
    fn from_persisted(saved: PersistedSession, default_model: &str) -> Self {
        let model = if saved.model.trim().is_empty() {
            default_model.to_owned()
        } else {
            saved.model
        };
        Self {
            cwd: saved.cwd,
            history: saved.history,
            model,
            active_turn: None,
            permissions: Arc::new(crate::tools::PermissionState::default()),
            cost_usd: saved.cost_usd,
        }
    }

    fn persisted(&self, session_id: &SessionId) -> PersistedSession {
        PersistedSession {
            session_id: session_id.to_string(),
            cwd: self.cwd.clone(),
            history: self.history.clone(),
            model: self.model.clone(),
            updated_at: crate::persistence::now_seconds(),
            cost_usd: self.cost_usd,
        }
    }
}

struct ActiveTurn {
    id: Uuid,
    cancel: watch::Sender<bool>,
    history_len: usize,
}

pub async fn serve(config: Config) -> agent_client_protocol::Result<()> {
    build_agent(Runtime::new(config).map_err(|error| Error::internal_error().data(error))?)
        .connect_to(Stdio::new())
        .await
}

impl Runtime {
    fn new(config: Config) -> Result<Self, String> {
        let http = HttpClient::builder()
            .connect_timeout(Duration::from_secs(
                config.reliability.connect_timeout_seconds,
            ))
            .timeout(Duration::from_secs(
                config.reliability.request_timeout_seconds,
            ))
            .retry(reqwest::retry::never())
            .build()
            .map_err(|error| format!("failed to build provider HTTP client: {error}"))?;
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
            .map(|(id, saved)| {
                let session = Session::from_persisted(saved, &config.provider.model);
                (id, session)
            })
            .collect();
        Ok(Self {
            config: Arc::new(config),
            http,
            sessions: Arc::new(Mutex::new(sessions)),
            store,
            models: Arc::new(Mutex::new(ModelCache::default())),
        })
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
            model: self.config.provider.model.clone(),
            active_turn: None,
            permissions: Arc::new(crate::tools::PermissionState::default()),
            cost_usd: 0.0,
        };
        self.persist(&session_id, &session);
        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), session);
        let models = self.available_models().await;
        NewSessionResponse::new(session_id)
            .config_options(self.config_options(&self.config.provider.model, &models))
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
        let model = if saved.model.trim().is_empty() {
            self.config.provider.model.clone()
        } else {
            saved.model.clone()
        };
        self.sessions.lock().await.insert(
            session_id.clone(),
            Session::from_persisted(saved, &self.config.provider.model),
        );
        Ok(model)
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

    fn config_options(&self, model: &str, models: &[String]) -> Vec<SessionConfigOption> {
        vec![SessionConfigOption::select(
            "model",
            "Model",
            model.to_owned(),
            models
                .iter()
                .map(|id| {
                    agent_client_protocol::schema::v1::SessionConfigSelectOption::new(
                        id.clone(),
                        id.clone(),
                    )
                })
                .collect::<Vec<_>>(),
        )
        .category(SessionConfigOptionCategory::Model)]
    }

    async fn available_models(&self) -> Vec<String> {
        self.refresh_models().await;
        self.models.lock().await.ids.clone()
    }

    async fn model_limits(&self, model: &str) -> ModelLimits {
        self.refresh_models().await;
        let cache = self.models.lock().await;
        ModelLimits {
            context_length: self
                .config
                .context
                .context_length
                .or_else(|| cache.context_lengths.get(model).copied())
                .unwrap_or(provider::DEFAULT_CONTEXT_LENGTH),
            max_completion_tokens: cache.max_completion_tokens.get(model).copied(),
        }
    }

    async fn refresh_models(&self) {
        const MODEL_CACHE_TTL: Duration = Duration::from_secs(60 * 60);
        let cache = self.models.lock().await;
        if cache
            .fetched_at
            .is_some_and(|at| at.elapsed() < MODEL_CACHE_TTL)
            && !cache.ids.is_empty()
        {
            return;
        }
        let configured = self.config.provider.model.clone();
        // Network I/O must not hold the shared cache lock across sessions.
        drop(cache);
        let mut ids = vec![configured.clone()];
        let mut context_lengths = HashMap::new();
        let mut max_completion_tokens = HashMap::new();
        match provider::fetch_models(&self.http, &self.config).await {
            Ok(models) => {
                ids = models.iter().map(|model| model.id.clone()).collect();
                for model in models {
                    if let Some(context_length) = model.context_length {
                        context_lengths.insert(model.id.clone(), context_length);
                    }
                    if let Some(max_completion) = model.max_completion_tokens {
                        max_completion_tokens.insert(model.id.clone(), max_completion);
                    }
                }
            }
            Err(error) => {
                eprintln!("amarcode-acp: {error}; using configured model");
            }
        }
        if !ids.iter().any(|id| id == &configured) {
            ids.insert(0, configured);
        }
        let mut cache = self.models.lock().await;
        cache.fetched_at = Some(Instant::now());
        cache.ids = ids;
        cache.context_lengths = context_lengths;
        cache.max_completion_tokens = max_completion_tokens;
    }

    async fn set_model(
        &self,
        session_id: &SessionId,
        model: &str,
    ) -> agent_client_protocol::Result<()> {
        if model.trim().is_empty() {
            return Err(Error::invalid_params().data("model must not be empty"));
        }
        let models = self.available_models().await;
        if !models.iter().any(|available| available == model) {
            return Err(Error::invalid_params().data("unknown model"));
        }
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| Error::invalid_params().data("unknown session"))?;
        session.model = model.to_owned();
        self.persist(session_id, session);
        Ok(())
    }

    async fn session_config_options(
        &self,
        session_id: &SessionId,
    ) -> agent_client_protocol::Result<Vec<SessionConfigOption>> {
        let model = {
            let sessions = self.sessions.lock().await;
            let session = sessions
                .get(session_id)
                .ok_or_else(|| Error::invalid_params().data("unknown session"))?;
            session.model.clone()
        };
        let models = self.available_models().await;
        Ok(self.config_options(&model, &models))
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
            session.model.clone(),
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
            self.persist(session_id, session);
        }
    }

    async fn publish_usage(
        &self,
        session_id: &SessionId,
        connection: &ConnectionTo<Client>,
        usage: &provider::ProviderUsage,
        limits: ModelLimits,
        turn_tokens: u64,
    ) {
        let cost = {
            let mut sessions = self.sessions.lock().await;
            let Some(session) = sessions.get_mut(session_id) else {
                return;
            };
            if let Some(cost) = usage.cost_usd {
                session.cost_usd += cost;
            }
            session.cost_usd
        };
        let mut update = UsageUpdate::new(usage.input_tokens, limits.context_length).meta(serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(serde_json::json!({
            "amarcode": { "input_tokens": usage.input_tokens, "output_tokens": usage.output_tokens,
                "turn_tokens": turn_tokens, "estimated": !usage.reported }
        })).expect("usage metadata is an object"));
        if cost > 0.0 || usage.cost_usd.is_some() {
            update = update.cost(Cost::new(cost, "USD"));
        }
        let _ = connection.send_notification(SessionNotification::new(
            session_id.clone(),
            SessionUpdate::UsageUpdate(update),
        ));
    }
}

fn build_agent(runtime: Runtime) -> impl agent_client_protocol::ConnectTo<Client> {
    let initialize_runtime = runtime.clone();
    let new_session_runtime = runtime.clone();
    let load_session_runtime = runtime.clone();
    let resume_session_runtime = runtime.clone();
    let list_sessions_runtime = runtime.clone();
    let delete_session_runtime = runtime.clone();
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
                    load_session_runtime
                        .restore(&request.session_id, &request.cwd)
                        .await?;
                    let history = {
                        let sessions = load_session_runtime.sessions.lock().await;
                        sessions.get(&request.session_id).ok_or_else(|| {
                            Error::internal_error().data("restored session disappeared")
                        })?.history.clone()
                    };
                    replay_history(&connection, &request.session_id, &history)?;
                    let options = load_session_runtime
                        .session_config_options(&request.session_id)
                        .await?;
                    Ok(LoadSessionResponse::new()
                        .config_options(options))
                }
                .await;
                responder.respond_with_result(result)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ResumeSessionRequest, responder, _connection| {
                let result = async {
                    resume_session_runtime.restore(&request.session_id, &request.cwd).await?;
                    let options = resume_session_runtime
                        .session_config_options(&request.session_id)
                        .await?;
                    Ok(ResumeSessionResponse::new().config_options(options))
                }.await;
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
            async move |request: SetSessionConfigOptionRequest, responder, _connection| {
                let result = match (request.config_id.to_string().as_str(), request.value) {
                    ("model", SessionConfigOptionValue::ValueId { value }) => {
                        let model = value.to_string();
                        async {
                            set_config_runtime.set_model(&request.session_id, &model).await?;
                            let options = set_config_runtime
                                .session_config_options(&request.session_id)
                                .await?;
                            Ok(SetSessionConfigOptionResponse::new(options))
                        }.await
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
                let (turn_id, cwd, model, history, permissions, cancellation) =
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
                            &runtime, history, cwd, model, permissions, session_id.clone(), message_id,
                            connection, cancellation,
                        ) => outcome,
                        _ = request_cancellation.cancelled() => {
                            runtime.cancel(&session_id).await;
                            runtime.finish_turn(&session_id, turn_id, None).await;
                            return responder.respond_with_error(Error::request_cancelled());
                        }
                    };
                    match outcome {
                        Ok(Completion::Completed(outcome)) => {
                            runtime
                                .finish_turn(&session_id, turn_id, Some(outcome.history))
                                .await;
                            responder.respond(PromptResponse::new(outcome.stop_reason))
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
    model: String,
    permissions: Arc<crate::tools::PermissionState>,
    session_id: SessionId,
    mut message_id: agent_client_protocol::schema::v1::MessageId,
    connection: ConnectionTo<Client>,
    cancellation: watch::Receiver<bool>,
) -> Result<Completion<TurnOutcome>, String> {
    let settings = &runtime.config.agent;
    let deadline = tokio::time::Instant::now()
        .checked_add(Duration::from_secs(settings.max_turn_seconds))
        .ok_or("agent.max_turn_seconds exceeds the supported clock range")?;
    let expired = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (cancel, mut execution_cancellation) = watch::channel(false);
    let relay_cancel = cancel.clone();
    let relay_expired = expired.clone();
    let cancellation_observer = cancellation.clone();
    let mut external_cancellation = cancellation;
    let _relay = CancellationRelay(tokio::spawn(async move {
        if !*external_cancellation.borrow() {
            tokio::select! {
                biased;
                _ = external_cancellation.changed() => {},
                _ = tokio::time::sleep_until(deadline) => { relay_expired.store(true, std::sync::atomic::Ordering::Relaxed); },
            }
        }
        let _ = relay_cancel.send(true);
    }));
    let limits = tokio::select! {
        limits = runtime.model_limits(&model) => limits,
        _ = execution_cancellation.changed() => {
            return cancelled_or_limited(history, &expired, &cancellation_observer, &connection, &session_id);
        }
    };
    let context_cwd = cwd.clone();
    let workspace_bytes = runtime
        .config
        .context
        .max_workspace_bytes
        .min((limits.history_token_budget() / 4).saturating_mul(4));
    let workspace = tokio::select! {
        context = tokio::task::spawn_blocking(move || crate::context::workspace_context(&context_cwd, workspace_bytes)) => context.map_err(|error| format!("workspace context failed: {error}"))?,
        _ = execution_cancellation.changed() => { return cancelled_or_limited(history, &expired, &cancellation_observer, &connection, &session_id); }
    };
    let history_budget = limits
        .history_token_budget()
        .saturating_sub(provider::estimate_tokens(&workspace));
    let mut used_tokens = 0_u64;
    let mut used_cost = 0_f64;
    let mut tool_calls = 0;
    let mut progress = ProgressTracker::default();
    let mut search_group: Option<SearchPresentationGroup> = None;
    for round in 0..=settings.max_tool_rounds {
        if *execution_cancellation.borrow() {
            finish_active_search_group(&connection, &session_id, search_group.take(), true);
            return cancelled_or_limited(
                history,
                &expired,
                &cancellation_observer,
                &connection,
                &session_id,
            );
        }
        if round == settings.max_tool_rounds {
            finish_active_search_group(&connection, &session_id, search_group.take(), true);
            return limited_turn(
                history,
                StopReason::MaxTurnRequests,
                "tool-round limit reached",
                &connection,
                &session_id,
            );
        }
        if settings
            .max_cost_usd
            .is_some_and(|limit| used_cost >= limit)
        {
            finish_active_search_group(&connection, &session_id, search_group.take(), true);
            return limited_turn(
                history,
                StopReason::MaxTurnRequests,
                "reported provider cost budget reached",
                &connection,
                &session_id,
            );
        }
        let pruned = crate::context::prune_tool_results(
            &history,
            runtime.config.context.max_tool_result_bytes,
        );
        if let Some(plan) = crate::context::compaction_plan(
            &pruned,
            history_budget,
            runtime.config.context.summary_tokens,
        )? {
            let mut summary = String::new();
            let summary_batches = provider::summary_batches(
                &plan.older,
                history_budget
                    .saturating_sub(plan.summary_budget)
                    .saturating_sub(512),
            );
            for batch in summary_batches {
                let summary_input = provider::estimate_tokens(
                    &provider::summary_messages(&summary, &batch).to_string(),
                ) as u64;
                if used_tokens
                    .saturating_add(summary_input)
                    .saturating_add(plan.summary_budget as u64)
                    > settings.max_turn_tokens
                {
                    finish_active_search_group(&connection, &session_id, search_group.take(), true);
                    return limited_turn(
                        history,
                        StopReason::MaxTokens,
                        "token budget cannot cover context compaction",
                        &connection,
                        &session_id,
                    );
                }
                let completion = match provider::summarize_batch(
                    &runtime.http,
                    &runtime.config,
                    &model,
                    &batch,
                    &summary,
                    plan.summary_budget as u64,
                    &mut execution_cancellation,
                )
                .await
                {
                    Ok(completion) => completion,
                    Err(error) => {
                        finish_active_search_group(
                            &connection,
                            &session_id,
                            search_group.take(),
                            true,
                        );
                        if *execution_cancellation.borrow() {
                            return cancelled_or_limited(
                                history,
                                &expired,
                                &cancellation_observer,
                                &connection,
                                &session_id,
                            );
                        }
                        return Err(error);
                    }
                };
                let Completion::Completed((next_summary, usage)) = completion else {
                    finish_active_search_group(&connection, &session_id, search_group.take(), true);
                    return cancelled_or_limited(
                        history,
                        &expired,
                        &cancellation_observer,
                        &connection,
                        &session_id,
                    );
                };
                summary = next_summary;
                used_tokens = used_tokens.saturating_add(usage.total_tokens());
                used_cost += usage.cost_usd.unwrap_or(0.0);
                runtime
                    .publish_usage(&session_id, &connection, &usage, limits, used_tokens)
                    .await;
                if settings
                    .max_cost_usd
                    .is_some_and(|limit| used_cost >= limit)
                {
                    finish_active_search_group(&connection, &session_id, search_group.take(), true);
                    return limited_turn(
                        history,
                        StopReason::MaxTurnRequests,
                        "reported provider cost budget reached",
                        &connection,
                        &session_id,
                    );
                }
            }
            history = plan.finish(&summary);
        }
        let request_history = crate::context::prune_tool_results(
            &history,
            runtime.config.context.max_tool_result_bytes,
        );
        let input_tokens = crate::context::history_tokens(&request_history)
            .saturating_add(provider::estimate_tokens(&workspace))
            as u64
            + (limits
                .context_length
                .saturating_sub(limits.history_token_budget() as u64)
                .saturating_sub(
                    provider::DEFAULT_OUTPUT_RESERVE
                        .min(limits.context_length / 4)
                        .max(1),
                ));
        let remaining = settings
            .max_turn_tokens
            .saturating_sub(used_tokens)
            .saturating_sub(input_tokens);
        if remaining == 0 {
            finish_active_search_group(&connection, &session_id, search_group.take(), true);
            return limited_turn(
                history,
                StopReason::MaxTokens,
                "turn token budget reached",
                &connection,
                &session_id,
            );
        }
        let max_output = provider::DEFAULT_OUTPUT_RESERVE
            .min(limits.context_length / 4)
            .max(1)
            .min(limits.max_completion_tokens.unwrap_or(u64::MAX))
            .min(remaining);
        let completion = match provider::stream_completion(
            &runtime.http,
            &runtime.config,
            &model,
            &request_history,
            &workspace,
            max_output,
            provider::StreamContext {
                session_id: session_id.clone(),
                message_id,
                connection: connection.clone(),
                cancellation: execution_cancellation.clone(),
            },
        )
        .await
        {
            Ok(completion) => completion,
            Err(error) => {
                finish_active_search_group(&connection, &session_id, search_group.take(), true);
                if *execution_cancellation.borrow() {
                    return cancelled_or_limited(
                        history,
                        &expired,
                        &cancellation_observer,
                        &connection,
                        &session_id,
                    );
                }
                return Err(error);
            }
        };
        let Completion::Completed(mut turn) = completion else {
            finish_active_search_group(&connection, &session_id, search_group.take(), true);
            return cancelled_or_limited(
                history,
                &expired,
                &cancellation_observer,
                &connection,
                &session_id,
            );
        };
        used_tokens = used_tokens.saturating_add(turn.usage.total_tokens());
        used_cost += turn.usage.cost_usd.unwrap_or(0.0);
        runtime
            .publish_usage(&session_id, &connection, &turn.usage, limits, used_tokens)
            .await;
        normalize_tool_ids(&mut turn);
        history.push(provider::assistant_message(&turn));
        let token_stop = turn.finish_reason.as_deref() == Some("length")
            || (!turn.tool_calls.is_empty() && used_tokens >= settings.max_turn_tokens);
        if token_stop {
            for call in &turn.tool_calls {
                history.push(provider::tool_message(
                    call,
                    "Error: tool not executed because the model token limit was reached",
                ));
            }
            finish_active_search_group(&connection, &session_id, search_group.take(), true);
            return limited_turn(
                history,
                StopReason::MaxTokens,
                "model or turn token limit reached",
                &connection,
                &session_id,
            );
        }
        if turn.tool_calls.is_empty() {
            finish_active_search_group(&connection, &session_id, search_group.take(), false);
            return Ok(Completion::Completed(TurnOutcome {
                history,
                stop_reason: StopReason::EndTurn,
            }));
        }
        let mut round_results = Vec::new();
        let mut call_limit_reached = false;
        for call in &turn.tool_calls {
            // Fill every result, even on a budget stop, to avoid leaving orphan
            // tool calls in the history saved for the next user turn.
            if tool_calls >= settings.max_tool_calls || *execution_cancellation.borrow() {
                call_limit_reached |= tool_calls >= settings.max_tool_calls;
                history.push(provider::tool_message(
                    call,
                    "Error: tool not executed because the turn was stopped",
                ));
                continue;
            }
            tool_calls += 1;
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
                round_results.push(tool_fingerprint(call, &output));
                continue;
            }
            finish_active_search_group(&connection, &session_id, search_group.take(), false);
            let output = tokio::time::timeout_at(
                deadline + Duration::from_secs(5),
                crate::tools::execute(
                    call,
                    &cwd,
                    &permissions,
                    &session_id,
                    &connection,
                    execution_cancellation.clone(),
                ),
            )
            .await
            .unwrap_or_else(|_| "Error: turn deadline reached; terminal cleanup timed out".into());
            history.push(provider::tool_message(call, &output));
            round_results.push(tool_fingerprint(call, &output));
        }
        if *execution_cancellation.borrow() {
            finish_active_search_group(&connection, &session_id, search_group.take(), true);
            return cancelled_or_limited(
                history,
                &expired,
                &cancellation_observer,
                &connection,
                &session_id,
            );
        }
        if call_limit_reached {
            finish_active_search_group(&connection, &session_id, search_group.take(), true);
            return limited_turn(
                history,
                StopReason::MaxTurnRequests,
                "tool-call limit reached",
                &connection,
                &session_id,
            );
        }
        if progress.record(round_results, settings.repeated_tool_limit) {
            finish_active_search_group(&connection, &session_id, search_group.take(), true);
            return limited_turn(
                history,
                StopReason::MaxTurnRequests,
                "repeated tool calls produced no new results",
                &connection,
                &session_id,
            );
        }
        message_id = agent_client_protocol::schema::v1::MessageId::new(Uuid::new_v4().to_string());
    }
    finish_active_search_group(&connection, &session_id, search_group.take(), true);
    unreachable!("bounded agent loop always stops")
}

struct TurnOutcome {
    history: Vec<serde_json::Value>,
    stop_reason: StopReason,
}

struct CancellationRelay(tokio::task::JoinHandle<()>);
impl Drop for CancellationRelay {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn cancelled_or_limited(
    history: Vec<serde_json::Value>,
    expired: &std::sync::atomic::AtomicBool,
    external_cancellation: &watch::Receiver<bool>,
    connection: &ConnectionTo<Client>,
    session_id: &SessionId,
) -> Result<Completion<TurnOutcome>, String> {
    if *external_cancellation.borrow() {
        return Ok(Completion::Cancelled);
    }
    if expired.load(std::sync::atomic::Ordering::Relaxed) {
        limited_turn(
            history,
            StopReason::MaxTurnRequests,
            "turn time budget reached",
            connection,
            session_id,
        )
    } else {
        Ok(Completion::Cancelled)
    }
}

fn limited_turn(
    mut history: Vec<serde_json::Value>,
    reason: StopReason,
    message: &str,
    connection: &ConnectionTo<Client>,
    session_id: &SessionId,
) -> Result<Completion<TurnOutcome>, String> {
    let text = format!("Stopped: {message}. Completed tool results are retained; review any changes before continuing.");
    connection
        .send_notification(SessionNotification::new(
            session_id.clone(),
            SessionUpdate::AgentMessageChunk(
                ContentChunk::new(ContentBlock::Text(TextContent::new(&text)))
                    .message_id(MessageId::new(Uuid::new_v4().to_string())),
            ),
        ))
        .map_err(|error| format!("ACP write failed: {error}"))?;
    history.push(serde_json::json!({"role":"assistant", "content":text}));
    Ok(Completion::Completed(TurnOutcome {
        history,
        stop_reason: reason,
    }))
}

fn tool_fingerprint(call: &provider::ModelToolCall, output: &str) -> String {
    let arguments = serde_json::from_str::<serde_json::Value>(&call.arguments)
        .map(|mut value| {
            value.sort_all_objects();
            value.to_string()
        })
        .unwrap_or_else(|_| call.arguments.clone());
    // Hash results to keep loop-detection memory independent of tool output size.
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (call.name.as_str(), arguments.as_str(), output).hash(&mut hash);
    format!("{:x}", hash.finish())
}

#[derive(Default)]
struct ProgressTracker {
    recent: std::collections::VecDeque<Vec<String>>,
}
impl ProgressTracker {
    fn record(&mut self, round: Vec<String>, limit: usize) -> bool {
        self.recent.push_back(round);
        while self.recent.len() > limit.saturating_mul(2) {
            self.recent.pop_front();
        }
        // Detect both identical rounds and alternating A/B rounds, ignoring
        // provider-generated call IDs and JSON argument formatting.
        (1..=2).any(|period| {
            self.recent.len() >= limit.saturating_mul(period)
                && (1..limit.saturating_mul(period)).all(|offset| {
                    self.recent[self.recent.len() - 1 - offset]
                        == self.recent[self.recent.len() - 1 - offset % period]
                })
        })
    }
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

    #[test]
    fn detects_identical_and_alternating_tool_loops_but_allows_progress() {
        let mut tracker = ProgressTracker::default();
        assert!(!tracker.record(vec!["a".into()], 3));
        assert!(!tracker.record(vec!["a".into()], 3));
        assert!(tracker.record(vec!["a".into()], 3));
        let mut tracker = ProgressTracker::default();
        for signature in ["a", "b", "a", "b", "a"] {
            assert!(!tracker.record(vec![signature.into()], 3));
        }
        assert!(tracker.record(vec!["b".into()], 3));
        let mut tracker = ProgressTracker::default();
        for index in 0..20 {
            assert!(!tracker.record(vec![index.to_string()], 3));
        }
    }

    #[test]
    fn progress_detection_ignores_call_ids_and_argument_key_order() {
        let a = provider::ModelToolCall {
            id: "a".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"x","limit":10}"#.into(),
        };
        let b = provider::ModelToolCall {
            id: "b".into(),
            arguments: r#"{ "limit": 10, "path": "x" }"#.into(),
            ..a.clone()
        };
        assert_eq!(tool_fingerprint(&a, "same"), tool_fingerprint(&b, "same"));
        assert_ne!(
            tool_fingerprint(&a, "same"),
            tool_fingerprint(&b, "new result")
        );
    }

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
            agent: Default::default(),
            reliability: Default::default(),
            context: Default::default(),
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
        .expect("runtime")
    }

    #[tokio::test]
    async fn sessions_have_unique_ids_and_workspaces() {
        let runtime = runtime();
        let first = runtime
            .new_session(NewSessionRequest::new("/workspace"))
            .await;
        let second = runtime
            .new_session(NewSessionRequest::new("/workspace"))
            .await;
        assert_ne!(first.session_id, second.session_id);

        let sessions = runtime.sessions.lock().await;
        assert_eq!(sessions[&first.session_id].cwd, PathBuf::from("/workspace"));
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
    async fn model_selection_is_kept_per_session() {
        let runtime = runtime();
        let first = runtime.new_session(NewSessionRequest::new("/one")).await;
        let second = runtime.new_session(NewSessionRequest::new("/two")).await;
        {
            let mut cache = runtime.models.lock().await;
            cache.fetched_at = Some(Instant::now());
            cache.ids = vec!["test-model".into(), "other-model".into()];
        }

        runtime
            .set_model(&first.session_id, "other-model")
            .await
            .expect("select model");

        let sessions = runtime.sessions.lock().await;
        assert_eq!(sessions[&first.session_id].model, "other-model");
        assert_eq!(sessions[&second.session_id].model, "test-model");
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
        let runtime = Runtime::new(config.clone()).expect("runtime");
        let created = runtime
            .new_session(NewSessionRequest::new("/workspace"))
            .await;
        {
            let mut sessions = runtime.sessions.lock().await;
            let session = sessions.get_mut(&created.session_id).expect("live session");
            session
                .history
                .push(serde_json::json!({ "role": "user", "content": "remember me" }));
            runtime.persist(&created.session_id, session);
        }
        drop(runtime);

        let restored = Runtime::new(config).expect("runtime");
        let sessions = restored.sessions.lock().await;
        let session = sessions.get(&created.session_id).expect("restored session");
        assert_eq!(session.model, "test-model");
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
