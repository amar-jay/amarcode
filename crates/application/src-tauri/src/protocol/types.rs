//! Shared domain types used on the client wire protocol and across store/service.
//!
//! One vocabulary for status, roles, ACP method names, and raw envelopes.
//! Store rows use these enums (serialized to SQL TEXT at the store edge).

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Starting,
    Running,
    Completed,
    Stopped,
    Failed,
}

impl RunStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Stopped | Self::Failed)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "starting" => Ok(Self::Starting),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "stopped" => Ok(Self::Stopped),
            "failed" => Ok(Self::Failed),
            other => Err(format!("unknown run status: {other}")),
        }
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Lifecycle of a single user prompt turn (not the multi-turn ACP run/session).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Started,
    Completed,
    Cancelled,
    Failed,
}

impl TurnStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for TurnStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

impl MessageRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "system" => Ok(Self::System),
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            "tool" => Ok(Self::Tool),
            other => Err(format!("unknown message role: {other}")),
        }
    }
}

impl fmt::Display for MessageRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Streaming,
    Complete,
    Interrupted,
    Failed,
}

impl MessageStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Streaming => "streaming",
            Self::Complete => "complete",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "streaming" => Ok(Self::Streaming),
            "complete" => Ok(Self::Complete),
            "interrupted" => Ok(Self::Interrupted),
            "failed" => Ok(Self::Failed),
            other => Err(format!("unknown message status: {other}")),
        }
    }
}

impl fmt::Display for MessageStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessagePartKind {
    Text,
    ToolCall,
    ToolResult,
    Thinking,
    File,
    Image,
}

impl MessagePartKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::Thinking => "thinking",
            Self::File => "file",
            Self::Image => "image",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "text" => Ok(Self::Text),
            "tool_call" => Ok(Self::ToolCall),
            "tool_result" => Ok(Self::ToolResult),
            "thinking" => Ok(Self::Thinking),
            "file" => Ok(Self::File),
            "image" => Ok(Self::Image),
            other => Err(format!("unknown message part kind: {other}")),
        }
    }
}

impl fmt::Display for MessagePartKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcDirection {
    Sent,
    Received,
}

impl RpcDirection {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::Received => "received",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "sent" => Ok(Self::Sent),
            "received" => Ok(Self::Received),
            other => Err(format!("unknown rpc direction: {other}")),
        }
    }
}

impl fmt::Display for RpcDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Requests the daemon sends to an ACP agent (JSON-RPC method names).
///
/// These follow the [Agent Client Protocol](https://agentclientprotocol.com)
/// (`initialize`, `session/new`, `session/prompt`, …), not a proprietary
/// `agent.*` namespace. `Other` keeps extensions open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRpcMethod {
    Initialize,
    /// `session/new`
    CreateSession,
    /// `session/load`
    LoadSession,
    /// `session/resume`
    ResumeSession,
    /// `session/prompt`
    Prompt,
    /// `session/cancel`
    Cancel,
    /// `session/close`
    CloseSession,
    /// `session/list` (optional capability)
    ListSessions,
    Authenticate,
    Logout,
    Other(String),
}

impl AgentRpcMethod {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Initialize => "initialize",
            Self::CreateSession => "session/new",
            Self::LoadSession => "session/load",
            Self::ResumeSession => "session/resume",
            Self::Prompt => "session/prompt",
            Self::Cancel => "session/cancel",
            Self::CloseSession => "session/close",
            Self::ListSessions => "session/list",
            Self::Authenticate => "authenticate",
            Self::Logout => "logout",
            Self::Other(method) => method,
        }
    }
}

impl From<&str> for AgentRpcMethod {
    fn from(method: &str) -> Self {
        match method {
            "initialize" => Self::Initialize,
            "session/new" => Self::CreateSession,
            "session/load" => Self::LoadSession,
            "session/resume" => Self::ResumeSession,
            "session/prompt" => Self::Prompt,
            "session/cancel" => Self::Cancel,
            "session/close" => Self::CloseSession,
            "session/list" => Self::ListSessions,
            "authenticate" => Self::Authenticate,
            "logout" => Self::Logout,
            other => Self::Other(other.to_owned()),
        }
    }
}

impl fmt::Display for AgentRpcMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Inbound methods/notifications from an agent (or legacy mock names).
///
/// Real ACP streams most turn progress as `session/update` notifications.
/// Payload JSON stays flexible; `session_manager` interprets `sessionUpdate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEventMethod {
    /// `session/update` — primary streaming path (chunks, tools, commands, …).
    SessionUpdate,
    /// `session/request_permission` (agent → client request).
    PermissionRequested,
    /// Elicitation / free-form input request (agent → client).
    InputRequested,
    // --- legacy mock / informal names (still accepted) ---
    SessionCreated,
    SessionStatusChanged,
    MessageStarted,
    MessageDelta,
    MessageCompleted,
    MessageFailed,
    ThinkingDelta,
    ToolCallStarted,
    ToolCallOutput,
    ToolCallCompleted,
    ToolCallFailed,
    FileChangeProposed,
    FileChanged,
    CommandStarted,
    CommandOutput,
    CommandCompleted,
    PlanUpdated,
    ContextUsage,
    SessionEnded,
    Other(String),
}

impl AgentEventMethod {
    pub fn as_str(&self) -> &str {
        match self {
            Self::SessionUpdate => "session/update",
            Self::PermissionRequested => "session/request_permission",
            Self::InputRequested => "session/request_input",
            Self::SessionCreated => "session.created",
            Self::SessionStatusChanged => "session.statusChanged",
            Self::MessageStarted => "message.started",
            Self::MessageDelta => "message.delta",
            Self::MessageCompleted => "message.completed",
            Self::MessageFailed => "message.failed",
            Self::ThinkingDelta => "thinking.delta",
            Self::ToolCallStarted => "toolCall.started",
            Self::ToolCallOutput => "toolCall.output",
            Self::ToolCallCompleted => "toolCall.completed",
            Self::ToolCallFailed => "toolCall.failed",
            Self::FileChangeProposed => "file.changeProposed",
            Self::FileChanged => "file.changed",
            Self::CommandStarted => "command.started",
            Self::CommandOutput => "command.output",
            Self::CommandCompleted => "command.completed",
            Self::PlanUpdated => "plan.updated",
            Self::ContextUsage => "context.usage",
            Self::SessionEnded => "session.ended",
            Self::Other(method) => method,
        }
    }
}

impl From<&str> for AgentEventMethod {
    fn from(method: &str) -> Self {
        match method {
            "session/update" => Self::SessionUpdate,
            "session/request_permission" => Self::PermissionRequested,
            "session/request_input" | "elicitation/create" => Self::InputRequested,
            "session.created" => Self::SessionCreated,
            "session.statusChanged" => Self::SessionStatusChanged,
            "message.started" => Self::MessageStarted,
            "message.delta" => Self::MessageDelta,
            "message.completed" => Self::MessageCompleted,
            "message.failed" => Self::MessageFailed,
            "thinking.delta" => Self::ThinkingDelta,
            "toolCall.started" => Self::ToolCallStarted,
            "toolCall.output" => Self::ToolCallOutput,
            "toolCall.completed" => Self::ToolCallCompleted,
            "toolCall.failed" => Self::ToolCallFailed,
            "file.changeProposed" => Self::FileChangeProposed,
            "file.changed" => Self::FileChanged,
            "command.started" => Self::CommandStarted,
            "command.output" => Self::CommandOutput,
            "command.completed" => Self::CommandCompleted,
            "permission.requested" => Self::PermissionRequested,
            "input.requested" => Self::InputRequested,
            "plan.updated" => Self::PlanUpdated,
            "context.usage" => Self::ContextUsage,
            "session.ended" => Self::SessionEnded,
            other => Self::Other(other.to_owned()),
        }
    }
}

impl fmt::Display for AgentEventMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// In-memory envelope for one raw JSON-RPC notification or request.
///
/// Persist via [`crate::store::AcpEvent::from_envelope`] / `Store::save_acp_envelope`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcEnvelope {
    pub direction: RpcDirection,
    pub method: String,
    pub payload: Value,
}
