use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::{MessagePartKind, MessageStatus, RunStatus, TurnStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLine {
    pub event: EditorEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
#[ts(tag = "type", content = "payload", rename_all = "camelCase")]
pub enum EditorEvent {
    ChatUpdated {
        chat_id: String,
    },
    RunUpdated {
        run_id: String,
        status: RunStatus,
        error_message: Option<String>,
    },
    TurnUpdated {
        chat_id: String,
        run_id: String,
        user_message_id: String,
        status: TurnStatus,
        #[serde(default)]
        stop_reason: Option<String>,
        #[serde(default)]
        error_message: Option<String>,
    },
    ContextRestoration {
        chat_id: String,
        run_id: String,
        source: String,
    },
    MessageUpdated {
        message_id: String,
        status: MessageStatus,
    },
    MessagePartAdded {
        message_id: String,
        #[ts(type = "number")]
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
