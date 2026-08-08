//! Client-facing RPC shapes for the TCP JSON-line protocol.
//!
//! Framing (one JSON object per line):
//! - request:  `{ "method": "...", "params": { ... } }`
//! - response: `{ "result": ... }` or `{ "error": "..." }`
//!
//! Handlers live in `rpc::handler`; this module is types only.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Well-known RPC method names.
pub mod methods {
    pub const HEALTH: &str = "health";
    pub const VERSION: &str = "version";
    pub const SUBSCRIBE_EVENTS: &str = "subscribe_events";

    pub const LIST_AGENTS: &str = "list_agents";

    pub const CREATE_CHAT: &str = "create_chat";
    pub const LIST_CHATS: &str = "list_chats";
    pub const GET_CHAT: &str = "get_chat";

    pub const PROMPT: &str = "prompt";
    pub const CANCEL: &str = "cancel";

    pub const RESPOND_PERMISSION: &str = "respond_permission";
    pub const RESPOND_INPUT: &str = "respond_input";
}

/// One client request line.
#[derive(Debug, Clone, Deserialize)]
pub struct RpcRequest {
    pub method: String,
    /// Optional JSON object (or any value). Missing params default to `null`.
    #[serde(default)]
    pub params: Value,
}

/// One server response line.
///
/// Exactly one of `result` or `error` is set when serializing successful
/// handler outcomes vs failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RpcResponse {
    pub fn ok(result: impl Into<Value>) -> Self {
        Self {
            result: Some(result.into()),
            error: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            result: None,
            error: Some(message.into()),
        }
    }
}

// --- health / version / subscribe ------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResult {
    pub status: String,
    pub version: String,
    pub addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionResult {
    pub version: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubscribeEventsParams {
    #[serde(default)]
    pub chat_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeEventsResult {
    pub subscribed: bool,
}

// --- agents ----------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAgentsResult {
    pub agents: Vec<super::AgentDefinition>,
}

// --- chats -----------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct CreateChatParams {
    pub workspace_path: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListChatsParams {
    #[serde(default)]
    pub workspace_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListChatsResult {
    pub chats: Vec<super::Chat>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetChatParams {
    pub chat_id: String,
    /// When true (default), include messages and parts for UI restore.
    #[serde(default = "default_true")]
    pub include_messages: bool,
}

fn default_true() -> bool {
    true
}

// --- session / agent runs --------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct PromptParams {
    pub chat_id: String,
    pub agent_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptResultDto {
    pub run_id: String,
    pub chat_id: String,
    pub agent_id: String,
    pub user_message_id: String,
    pub acp_session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CancelParams {
    pub chat_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelResult {
    pub cancelled: bool,
    pub chat_id: String,
}

/// Answer an agent-initiated permission or input request.
#[derive(Debug, Clone, Deserialize)]
pub struct RespondAgentParams {
    pub request_id: String,
    /// Success payload forwarded as JSON-RPC `result` to the agent.
    #[serde(default)]
    pub result: Option<Value>,
    /// If set, the daemon answers with a JSON-RPC error instead of `result`.
    #[serde(default)]
    pub error: Option<RespondAgentError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RespondAgentError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RespondAgentResult {
    pub ok: bool,
    pub request_id: String,
}
