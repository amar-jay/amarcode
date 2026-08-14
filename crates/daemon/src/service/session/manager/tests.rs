use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::sync::broadcast;

use crate::{acp::AcpClient, service::AgentManager, store::Store};

use super::*;

fn sleeping_client() -> Arc<AcpClient> {
    let arguments = vec!["-c".to_owned(), "sleep 30".to_owned()];
    let (client, _inbound) =
        AcpClient::spawn("/bin/sh", &arguments, &[], None).expect("spawn test agent");
    Arc::new(client)
}

#[test]
fn stale_pending_request_cannot_target_replacement_client() {
    let store = Arc::new(Store::open(Path::new(":memory:")).expect("store"));
    let (events, _) = broadcast::channel(4);
    let manager = SessionManager::new(
        Arc::clone(&store),
        AgentManager::new(store, PathBuf::from("/tmp/tools")),
        events,
    );

    manager.inner.by_chat.lock().expect("live runs").insert(
        "chat-1".to_owned(),
        LiveRun {
            run_id: "new-run".to_owned(),
            agent_id: "agent".to_owned(),
            client: sleeping_client(),
            acp_session_id: Some("new-session".to_owned()),
            session_configuration: SessionConfiguration::default(),
            needs_history_hydration: false,
            streaming_message_ids: HashMap::new(),
            last_streaming_message_id: None,
            active_user_message_id: None,
        },
    );
    manager.inner.pending.lock().expect("pending").insert(
        "daemon-request".to_owned(),
        PendingAgentRequest {
            request_id: "daemon-request".to_owned(),
            run_id: "old-run".to_owned(),
            chat_id: "chat-1".to_owned(),
            acp_id: 1,
            method: "session/request_permission".to_owned(),
            params: Value::Null,
        },
    );

    let error = manager
        .respond_to_agent("daemon-request", json!({ "allow": true }))
        .expect_err("stale request must be rejected");
    assert!(error.to_string().contains("replaced run"));
    assert!(manager
        .pending_requests()
        .expect("pending requests")
        .is_empty());
    assert_eq!(
        manager
            .live_run_for_chat("chat-1")
            .expect("live run")
            .expect("current run")
            .0,
        "new-run"
    );
}

#[test]
fn failed_prompt_interrupts_partial_messages() {
    let store = Arc::new(Store::open(Path::new(":memory:")).expect("store"));
    store.seed_presets().expect("seed agents");
    store
        .create_chat(&crate::store::Chat {
            id: "chat-1".to_owned(),
            workspace_path: "/tmp/workspace".to_owned(),
            title: "timeout".to_owned(),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
            archived_at: None,
        })
        .expect("create chat");
    store
        .create_run(&AgentRun {
            id: "run-1".to_owned(),
            chat_id: "chat-1".to_owned(),
            agent_id: "codex-acp".to_owned(),
            acp_session_id: Some("session-1".to_owned()),
            status: RunStatus::Running,
            started_at: "2026-01-01T00:00:00Z".to_owned(),
            finished_at: None,
            error_message: None,
        })
        .expect("create run");
    store
        .create_message(&Message {
            id: "partial-message".to_owned(),
            chat_id: "chat-1".to_owned(),
            agent_run_id: Some("run-1".to_owned()),
            role: MessageRole::Assistant,
            content: "partial".to_owned(),
            status: MessageStatus::Streaming,
            created_at: "2026-01-01T00:00:01Z".to_owned(),
            updated_at: "2026-01-01T00:00:01Z".to_owned(),
        })
        .expect("create partial message");
    store
        .replace_message_parts(
            "partial-message",
            &[MessagePart {
                message_id: "partial-message".to_owned(),
                ordinal: 0,
                kind: MessagePartKind::Text,
                content_json: json!({ "text": "partial" }).to_string(),
            }],
        )
        .expect("create message part");

    let (events, _) = broadcast::channel(8);
    let manager = SessionManager::new(
        Arc::clone(&store),
        AgentManager::new(Arc::clone(&store), PathBuf::from("/tmp/tools")),
        events,
    );
    manager.inner.by_chat.lock().expect("live runs").insert(
        "chat-1".to_owned(),
        LiveRun {
            run_id: "run-1".to_owned(),
            agent_id: "codex-acp".to_owned(),
            client: sleeping_client(),
            acp_session_id: Some("session-1".to_owned()),
            session_configuration: SessionConfiguration::default(),
            needs_history_hydration: false,
            streaming_message_ids: HashMap::from([(
                "upstream".to_owned(),
                "partial-message".to_owned(),
            )]),
            last_streaming_message_id: Some("partial-message".to_owned()),
            active_user_message_id: Some("user-message".to_owned()),
        },
    );
    manager.inner.pending.lock().expect("pending").insert(
        "pending".to_owned(),
        PendingAgentRequest {
            request_id: "pending".to_owned(),
            run_id: "run-1".to_owned(),
            chat_id: "chat-1".to_owned(),
            acp_id: 1,
            method: "session/request_permission".to_owned(),
            params: Value::Null,
        },
    );

    manager
        .terminate_failed_prompt("chat-1", "run-1", "user-message", "ACP request timed out")
        .expect("terminate failed prompt");

    assert!(manager
        .live_run_for_chat("chat-1")
        .expect("live run")
        .is_none());
    assert!(manager.pending_requests().expect("pending").is_empty());
    assert_eq!(
        store
            .get_run("run-1")
            .expect("run")
            .expect("run row")
            .status,
        RunStatus::Failed
    );
    assert_eq!(
        store
            .get_message("partial-message")
            .expect("message")
            .expect("message row")
            .status,
        MessageStatus::Interrupted
    );
}
