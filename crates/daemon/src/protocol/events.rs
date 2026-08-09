//! Live event payloads sent to subscribed TCP clients.
//!
//! After `subscribe_events` succeeds, the connection receives lines like:
//! `{ "event": { "type": "runUpdated", "payload": { ... } } }`.
//!
//! These are the stable editor-facing events. ACP agent notifications are
//! translated into this shape by `service::session`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::{MessagePartKind, MessageStatus, RunStatus};

/// Wire envelope written after a successful `subscribe_events`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLine {
    pub event: EditorEvent,
}

/// Stable events emitted by the daemon to the editor frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum EditorEvent {
    ChatUpdated {
        chat_id: String,
    },
    RunUpdated {
        run_id: String,
        status: RunStatus,
        error_message: Option<String>,
    },
    MessageUpdated {
        message_id: String,
        status: MessageStatus,
    },
    MessagePartAdded {
        message_id: String,
        ordinal: i64,
        kind: MessagePartKind,
    },
    ApprovalRequired {
        run_id: String,
        request_id: String,
        details: Value,
    },
    QuestionRequired {
        run_id: String,
        request_id: String,
        details: Value,
    },
    WorkspaceFilesChanged {
        workspace_path: String,
        paths: Vec<String>,
    },
    AgentConnectionChanged {
        agent_id: String,
        connected: bool,
        error_message: Option<String>,
    },
}
