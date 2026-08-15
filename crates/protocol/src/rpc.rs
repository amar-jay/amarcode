use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::{AgentInfo, Chat};

pub mod methods {
    pub const HEALTH: &str = "health";
    pub const VERSION: &str = "version";
    pub const SUBSCRIBE_EVENTS: &str = "subscribe_events";
    pub const LIST_AGENTS: &str = "list_agents";
    pub const CREATE_CHAT: &str = "create_chat";
    pub const LIST_CHATS: &str = "list_chats";
    pub const GET_CHAT: &str = "get_chat";
    pub const GET_ATTACHMENT: &str = "get_attachment";
    pub const DELETE_CHAT: &str = "delete_chat";
    pub const PROMPT: &str = "prompt";
    pub const SET_SESSION_MODE: &str = "set_session_mode";
    pub const CANCEL: &str = "cancel";
    pub const RESPOND_PERMISSION: &str = "respond_permission";
    pub const RESPOND_INPUT: &str = "respond_input";
}

#[derive(Debug, Clone, Deserialize)]
pub struct RpcRequest {
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct HealthResult {
    pub status: String,
    pub version: String,
    pub protocol_version: u32,
    pub addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct VersionResult {
    pub version: String,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAgentsResult {
    pub agents: Vec<AgentInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CreateChatParams {
    pub workspace_path: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
pub struct ListChatsParams {
    #[serde(default)]
    pub workspace_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListChatsResult {
    pub chats: Vec<Chat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct GetChatParams {
    pub chat_id: String,
    #[serde(default = "default_true")]
    pub include_messages: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct DeleteChatParams {
    pub chat_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct DeleteChatResult {
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct GetAttachmentParams {
    pub chat_id: String,
    pub attachment_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct GetAttachmentResult {
    pub media_type: String,
    pub data: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct PromptAttachment {
    pub filename: Option<String>,
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct PromptParams {
    pub chat_id: String,
    pub agent_id: String,
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<PromptAttachment>,
    #[serde(default)]
    pub plan_mode: bool,
    #[serde(default)]
    pub session_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SetSessionModeParams {
    pub chat_id: String,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct PromptResultDto {
    pub run_id: String,
    pub chat_id: String,
    pub agent_id: String,
    pub user_message_id: String,
    pub acp_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelParams {
    pub chat_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CancelResult {
    pub cancelled: bool,
    pub chat_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RespondAgentParams {
    pub request_id: String,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<RespondAgentError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RespondAgentError {
    #[ts(type = "number")]
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RespondAgentResult {
    pub ok: bool,
    pub request_id: String,
}
