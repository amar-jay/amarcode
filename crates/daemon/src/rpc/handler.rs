//! RPC method dispatch.
//!
//! Thin switchboard: parse params, call `service` managers, serialize results.
//! No SQL and no ACP framing here.
//!
//! Reads → `agents` / `chats`. Agent turns → `sessions` (store-first).

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::{
    protocol::rpc::{
        methods, AuthenticateAgentParams, AuthenticateAgentResult, CancelParams, CancelResult,
        CreateChatParams, DeleteChatParams, DeleteChatResult, GetAttachmentParams,
        GetAttachmentResult, GetChatParams, HealthResult, InstallAgentParams, ListAgentsResult,
        ListChatsParams, ListChatsResult, PromptParams, PromptResultDto, RespondAgentParams,
        RespondAgentResult, SetSessionConfigOptionParams, SetSessionConfigOptionResult,
        SubscribeEventsParams, VersionResult,
    },
    service::{ChatDetail, MessageDetail, PromptResult},
    App, Error, Result,
};

/// Outcome of dispatching a single request.
///
/// Most methods return a normal JSON result. `subscribe_events` is special:
/// the connection layer acks, then leaves the request loop and streams events.
#[derive(Debug)]
pub enum DispatchOutcome {
    /// Write `{ "result": ... }` and keep accepting requests.
    Result(Value),
    /// Write the subscribe ack, then switch this socket to event streaming.
    Subscribe(SubscribeEventsParams),
}

/// Dispatch one RPC method against shared app state.
pub async fn dispatch(app: &App, method: &str, params: Value) -> Result<DispatchOutcome> {
    match method {
        // meta
        methods::HEALTH => Ok(DispatchOutcome::Result(health(app)?)),
        methods::VERSION => Ok(DispatchOutcome::Result(version()?)),
        methods::SUBSCRIBE_EVENTS => Ok(DispatchOutcome::Subscribe(parse_params(params)?)),

        // agents (read / install)
        methods::LIST_AGENTS => Ok(DispatchOutcome::Result(list_agents(app)?)),
        methods::INSTALL_AGENT => Ok(DispatchOutcome::Result(install_agent(app, params).await?)),
        methods::AUTHENTICATE_AGENT => Ok(DispatchOutcome::Result(
            authenticate_agent(app, params).await?,
        )),

        // chats (read / CRUD)
        methods::CREATE_CHAT => Ok(DispatchOutcome::Result(create_chat(app, params)?)),
        methods::LIST_CHATS => Ok(DispatchOutcome::Result(list_chats(app, params)?)),
        methods::GET_CHAT => Ok(DispatchOutcome::Result(get_chat(app, params)?)),
        methods::GET_ATTACHMENT => Ok(DispatchOutcome::Result(get_attachment(app, params)?)),
        methods::DELETE_CHAT => Ok(DispatchOutcome::Result(delete_chat(app, params).await?)),

        // sessions (ACP — may block; run off the async worker)
        methods::PROMPT => Ok(DispatchOutcome::Result(prompt(app, params).await?)),
        methods::SET_SESSION_CONFIG_OPTION => Ok(DispatchOutcome::Result(
            set_session_config_option(app, params).await?,
        )),
        methods::CANCEL => Ok(DispatchOutcome::Result(cancel(app, params).await?)),
        methods::RESPOND_PERMISSION | methods::RESPOND_INPUT => {
            Ok(DispatchOutcome::Result(respond_agent(app, params).await?))
        }

        _ => Err(Error::msg(format!("unknown method: {method}"))),
    }
}

// --- meta ------------------------------------------------------------------

fn health(app: &App) -> Result<Value> {
    let body = HealthResult {
        status: "ok".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        protocol_version: amarcode_protocol::PROTOCOL_VERSION,
        addr: app.config.daemon_addr.clone(),
    };
    to_value(body)
}

fn version() -> Result<Value> {
    to_value(VersionResult {
        version: env!("CARGO_PKG_VERSION").into(),
        protocol_version: amarcode_protocol::PROTOCOL_VERSION,
    })
}

// --- agents ----------------------------------------------------------------

fn list_agents(app: &App) -> Result<Value> {
    let agents = app.agents.list()?;
    to_value(ListAgentsResult { agents })
}

async fn install_agent(app: &App, params: Value) -> Result<Value> {
    let p: InstallAgentParams = parse_params(params)?;
    if p.agent_id.trim().is_empty() {
        return Err(Error::msg("agent_id must not be empty"));
    }
    let agent_id = p.agent_id;
    let result = tokio::task::block_in_place(|| app.agents.install(&agent_id))?;
    to_value(result)
}

async fn authenticate_agent(app: &App, params: Value) -> Result<Value> {
    let p: AuthenticateAgentParams = parse_params(params)?;
    if p.agent_id.trim().is_empty() {
        return Err(Error::msg("agent_id must not be empty"));
    }
    let agent_id = p.agent_id;
    let method_id = p.method_id;
    tokio::task::block_in_place(|| {
        app.sessions
            .authenticate_agent(&agent_id, method_id.as_deref())
    })?;
    to_value(AuthenticateAgentResult { ok: true })
}

// --- chats -----------------------------------------------------------------

fn create_chat(app: &App, params: Value) -> Result<Value> {
    let p: CreateChatParams = parse_params(params)?;
    if p.workspace_path.trim().is_empty() {
        return Err(Error::msg("workspace_path must not be empty"));
    }
    let chat = app.chats.create(p.workspace_path, p.title)?;
    to_value(chat)
}

fn list_chats(app: &App, params: Value) -> Result<Value> {
    let p: ListChatsParams = parse_params_or_default(params)?;
    let chats = app.chats.list(p.workspace_path.as_deref())?;
    to_value(ListChatsResult { chats })
}

fn get_chat(app: &App, params: Value) -> Result<Value> {
    let p: GetChatParams = parse_params(params)?;
    if p.include_messages {
        let detail = app.chats.get_with_messages(&p.chat_id)?;
        to_value(chat_detail_json(&detail)?)
    } else {
        let chat = app.chats.get_required(&p.chat_id)?;
        to_value(chat)
    }
}

fn get_attachment(app: &App, params: Value) -> Result<Value> {
    let p: GetAttachmentParams = parse_params(params)?;
    let (media_type, data) = app.sessions.get_attachment(&p.chat_id, &p.attachment_id)?;
    to_value(GetAttachmentResult { media_type, data })
}

async fn delete_chat(app: &App, params: Value) -> Result<Value> {
    let p: DeleteChatParams = parse_params(params)?;
    let chat_id = p.chat_id;
    tokio::task::block_in_place(|| app.sessions.delete_chat(&chat_id))?;
    to_value(DeleteChatResult { deleted: true })
}

fn chat_detail_json(detail: &ChatDetail) -> Result<Value> {
    // Explicit shape so clients get stable field names.
    let messages: Vec<Value> = detail
        .messages
        .iter()
        .map(message_detail_json)
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "chat": detail.chat,
        "messages": messages,
        "session_config": detail.session_config,
    }))
}

fn message_detail_json(detail: &MessageDetail) -> Result<Value> {
    Ok(json!({
        "message": detail.message,
        "parts": detail.parts,
        "agent_id": detail.agent_id,
    }))
}

// --- sessions --------------------------------------------------------------

async fn prompt(app: &App, params: Value) -> Result<Value> {
    let p: PromptParams = parse_params(params)?;
    if p.chat_id.trim().is_empty() {
        return Err(Error::msg("chat_id must not be empty"));
    }
    if p.agent_id.trim().is_empty() {
        return Err(Error::msg("agent_id must not be empty"));
    }

    let chat_id = p.chat_id;
    let agent_id = p.agent_id;
    let text = p.text;
    let attachments = p.attachments;
    let config_values = p.config_values;

    // ACP spawn/request is blocking; keep the async runtime free.
    let result = tokio::task::block_in_place(|| {
        app.sessions
            .prompt(&chat_id, &agent_id, text, attachments, config_values)
    });
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            if let Err(cleanup_error) =
                tokio::task::block_in_place(|| app.sessions.delete_chat_if_empty(&chat_id))
            {
                tracing::warn!(%chat_id, %cleanup_error, "failed cleaning up empty chat after prompt error");
            }
            return Err(error);
        }
    };
    to_value(prompt_dto(result))
}

async fn set_session_config_option(app: &App, params: Value) -> Result<Value> {
    let p: SetSessionConfigOptionParams = parse_params(params)?;
    let chat_id = p.chat_id.clone();
    let options = tokio::task::block_in_place(|| {
        app.sessions.set_session_config_option(
            &p.chat_id,
            crate::protocol::SessionConfigAssignment {
                config_id: p.config_id,
                value: p.value,
            },
        )
    })?;
    to_value(SetSessionConfigOptionResult { chat_id, options })
}

async fn cancel(app: &App, params: Value) -> Result<Value> {
    let p: CancelParams = parse_params(params)?;
    if p.chat_id.trim().is_empty() {
        return Err(Error::msg("chat_id must not be empty"));
    }
    let chat_id = p.chat_id.clone();
    tokio::task::block_in_place(|| app.sessions.cancel(&chat_id))?;
    to_value(CancelResult {
        cancelled: true,
        chat_id: p.chat_id,
    })
}

async fn respond_agent(app: &App, params: Value) -> Result<Value> {
    let p: RespondAgentParams = parse_params(params)?;
    if p.request_id.trim().is_empty() {
        return Err(Error::msg("request_id must not be empty"));
    }

    let request_id = p.request_id.clone();

    if let Some(err) = p.error {
        let message = err.message;
        let code = err.code;
        let data = err.data;
        let rid = request_id.clone();
        tokio::task::block_in_place(|| {
            app.sessions
                .respond_error_to_agent(&rid, code, &message, data)
        })?;
    } else {
        let result = p.result.unwrap_or(Value::Null);
        let rid = request_id.clone();
        tokio::task::block_in_place(|| app.sessions.respond_to_agent(&rid, result))?;
    }

    to_value(RespondAgentResult {
        ok: true,
        request_id,
    })
}

fn prompt_dto(result: PromptResult) -> PromptResultDto {
    PromptResultDto {
        run_id: result.run_id,
        chat_id: result.chat_id,
        agent_id: result.agent_id,
        user_message_id: result.user_message_id,
        acp_session_id: result.acp_session_id,
    }
}

// --- helpers ---------------------------------------------------------------

fn parse_params<T: DeserializeOwned>(params: Value) -> Result<T> {
    if params.is_null() {
        return Err(Error::msg("missing params object"));
    }
    serde_json::from_value(params).map_err(|err| Error::msg(format!("invalid params: {err}")))
}

fn parse_params_or_default<T: DeserializeOwned + Default>(params: Value) -> Result<T> {
    if params.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(params).map_err(|err| Error::msg(format!("invalid params: {err}")))
}

fn to_value<T: serde::Serialize>(value: T) -> Result<Value> {
    serde_json::to_value(value).map_err(Error::from)
}
