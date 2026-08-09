//! Shared session types and live-run state.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::Value;
use tokio::sync::broadcast;

use crate::{
    acp::AcpClient,
    protocol::EditorEvent,
    store::Store,
};

pub(super) const ACP_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Result of starting a run / sending a prompt.
#[derive(Debug, Clone)]
pub struct PromptResult {
    pub run_id: String,
    pub chat_id: String,
    pub agent_id: String,
    pub user_message_id: String,
    pub acp_session_id: Option<String>,
}

/// Pending agent-initiated JSON-RPC request (permission / input).
#[derive(Debug, Clone)]
pub struct PendingAgentRequest {
    pub run_id: String,
    pub chat_id: String,
    pub acp_id: u64,
    pub method: String,
    pub params: Value,
}

pub(super) struct LiveRun {
    pub(super) run_id: String,
    pub(super) agent_id: String,
    pub(super) client: Arc<AcpClient>,
    pub(super) acp_session_id: Option<String>,
    /// Assistant messages currently being streamed, keyed by the upstream ACP
    /// message id. A turn may contain distinct commentary and final-answer
    /// messages, so they must not be collapsed into one row.
    pub(super) streaming_message_ids: HashMap<String, String>,
    /// Most recent visible assistant message. Thought chunks do not always use
    /// the same ACP message id, so attach them here when possible.
    pub(super) last_streaming_message_id: Option<String>,
    /// User message id for the in-flight prompt turn, if any.
    pub(super) active_user_message_id: Option<String>,
}

/// Shared pieces used by SessionManager methods and inbound worker threads.
pub(super) struct SessionInner {
    pub(super) store: Arc<Store>,
    pub(super) events: broadcast::Sender<EditorEvent>,
    pub(super) by_chat: Mutex<HashMap<String, LiveRun>>,
    pub(super) pending: Mutex<HashMap<String, PendingAgentRequest>>,
}
