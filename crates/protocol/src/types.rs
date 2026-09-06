use serde::{Deserialize, Serialize};
use ts_rs::TS;

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "snake_case")]
        #[ts(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $value),+ }
            }

            pub fn parse(value: &str) -> Result<Self, String> {
                match value {
                    $($value => Ok(Self::$variant),)+
                    other => Err(format!("unknown {}: {other}", stringify!($name))),
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

string_enum!(RunStatus {
    Starting => "starting",
    Running => "running",
    Completed => "completed",
    Stopped => "stopped",
    Failed => "failed",
});

impl RunStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Stopped | Self::Failed)
    }
}

string_enum!(TurnStatus {
    Started => "started",
    Completed => "completed",
    Cancelled => "cancelled",
    Failed => "failed",
});

impl TurnStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
}

string_enum!(MessageRole {
    System => "system",
    User => "user",
    Assistant => "assistant",
    Tool => "tool",
});

string_enum!(MessageStatus {
    Streaming => "streaming",
    Complete => "complete",
    Interrupted => "interrupted",
    Failed => "failed",
});

string_enum!(MessagePartKind {
    Text => "text",
    ToolCall => "tool_call",
    ToolResult => "tool_result",
    Thinking => "thinking",
    File => "file",
    Image => "image",
});

string_enum!(AgentFailureKind {
    AuthRequired => "auth_required",
    AdapterExited => "adapter_exited",
    Timeout => "timeout",
    Unavailable => "unavailable",
    Error => "error",
});

string_enum!(AgentRuntimeStatus {
    Ready => "ready",
    AuthRequired => "auth_required",
    Unavailable => "unavailable",
    Error => "error",
});

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AgentDefinition {
    pub id: String,
    pub name: String,
    pub command: String,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
    /// Whether the launch command currently resolves on this host.
    pub available: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// An agent definition annotated with live resolution details for the UI.
///
/// `available` mirrors the persisted agents.available column after the daemon
/// refreshes host resolution. `resolved_command` / `unavailable_reason` are
/// computed at list time for diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub command: String,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub created_at: String,
    pub updated_at: String,
    pub available: bool,
    pub resolved_command: Option<String>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Chat {
    pub id: String,
    pub workspace_path: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Message {
    pub id: String,
    pub chat_id: String,
    pub agent_run_id: Option<String>,
    pub role: MessageRole,
    pub content: String,
    pub status: MessageStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct MessagePart {
    pub message_id: String,
    #[ts(type = "number")]
    pub ordinal: i64,
    pub kind: MessagePartKind,
    pub content_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct MessageDetail {
    pub message: Message,
    pub parts: Vec<MessagePart>,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ChatDetail {
    pub chat: Chat,
    pub messages: Vec<MessageDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(untagged)]
pub enum GetChatResult {
    Detail(ChatDetail),
    Chat(Chat),
}
