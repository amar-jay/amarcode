use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

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

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "sent" => Ok(Self::Sent),
            "received" => Ok(Self::Received),
            other => Err(format!("unknown rpc direction: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRpcMethod {
    Initialize,
    CreateSession,
    LoadSession,
    ResumeSession,
    Prompt,
    Cancel,
    CloseSession,
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

impl fmt::Display for AgentRpcMethod {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEventMethod {
    SessionUpdate,
    PermissionRequested,
    InputRequested,
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
            "session/request_permission" | "permission.requested" => Self::PermissionRequested,
            "session/request_input" | "elicitation/create" | "input.requested" => {
                Self::InputRequested
            }
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
            "plan.updated" => Self::PlanUpdated,
            "context.usage" => Self::ContextUsage,
            "session.ended" => Self::SessionEnded,
            other => Self::Other(other.to_owned()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcEnvelope {
    pub direction: RpcDirection,
    pub method: String,
    pub payload: Value,
}
