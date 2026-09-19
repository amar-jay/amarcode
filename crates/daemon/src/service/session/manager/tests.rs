use std::{
    collections::{HashMap, HashSet},
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

fn hydration_manager_with_messages(messages: &[(&str, &str, MessageRole, &str)]) -> SessionManager {
    let store = Arc::new(Store::open(Path::new(":memory:")).expect("store"));
    store
        .create_chat(&crate::store::Chat {
            id: "chat-1".to_owned(),
            workspace_path: "/tmp/workspace".to_owned(),
            title: "continuation".to_owned(),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
            archived_at: None,
        })
        .expect("create chat");
    for run_id in messages
        .iter()
        .map(|(_, run_id, _, _)| *run_id)
        .collect::<HashSet<_>>()
    {
        store
            .save_agent(&crate::protocol::AgentDefinition {
                id: run_id.to_owned(),
                name: run_id.to_owned(),
                command: "test-agent".to_owned(),
                arguments: vec![],
                environment: vec![],
                available: false,
                created_at: "2026-01-01T00:00:00Z".to_owned(),
                updated_at: "2026-01-01T00:00:00Z".to_owned(),
            })
            .expect("create agent");
        store
            .create_run(&AgentRun {
                id: run_id.to_owned(),
                chat_id: "chat-1".to_owned(),
                agent_id: run_id.to_owned(),
                acp_session_id: Some(format!("session-{run_id}")),
                status: RunStatus::Stopped,
                started_at: "2026-01-01T00:00:00Z".to_owned(),
                finished_at: Some("2026-01-01T00:00:01Z".to_owned()),
                error_message: None,
                context_usage: None,
            })
            .expect("create run");
    }
    for (index, (id, run_id, role, content)) in messages.iter().enumerate() {
        store
            .create_message(&Message {
                id: (*id).to_owned(),
                chat_id: "chat-1".to_owned(),
                agent_run_id: Some((*run_id).to_owned()),
                role: role.clone(),
                content: (*content).to_owned(),
                status: MessageStatus::Complete,
                created_at: format!("2026-01-01T00:00:{index:02}Z"),
                updated_at: format!("2026-01-01T00:00:{index:02}Z"),
            })
            .expect("create message");
    }
    let (events, _) = broadcast::channel(4);
    SessionManager::new(
        Arc::clone(&store),
        AgentManager::new(store, PathBuf::from("/tmp/amarcode-test")),
        events,
        PathBuf::from("/tmp/amarcode-test-attachments"),
    )
}

#[test]
fn resumed_agent_hydrates_only_messages_after_its_watermark() {
    let manager = hydration_manager_with_messages(&[
        ("a-user", "run-a", MessageRole::User, "question for A"),
        ("a-answer", "run-a", MessageRole::Assistant, "answer from A"),
        ("b-user", "run-b", MessageRole::User, "question for B"),
        ("b-answer", "run-b", MessageRole::Assistant, "answer from B"),
    ]);

    let hydrated = manager
        .hydrated_prompt(
            "chat-1",
            "current",
            "back to A",
            &HistoryHydration::AfterMessage("a-answer".to_owned()),
        )
        .expect("hydrate delta");

    assert!(!hydrated.contains("question for A"));
    assert!(!hydrated.contains("answer from A"));
    assert!(hydrated.contains("question for B"));
    assert!(hydrated.contains("answer from B"));
    assert!(hydrated.ends_with("User: back to A"));
}

#[test]
fn resumed_agent_without_intervening_messages_gets_plain_prompt() {
    let manager = hydration_manager_with_messages(&[(
        "a-answer",
        "run-a",
        MessageRole::Assistant,
        "answer from A",
    )]);

    let hydrated = manager
        .hydrated_prompt(
            "chat-1",
            "current",
            "continue A",
            &HistoryHydration::AfterMessage("a-answer".to_owned()),
        )
        .expect("hydrate empty delta");

    assert_eq!(hydrated, "continue A");
}

#[test]
fn missing_watermark_safely_falls_back_to_full_history() {
    let manager = hydration_manager_with_messages(&[(
        "existing",
        "run-b",
        MessageRole::User,
        "existing context",
    )]);

    let hydrated = manager
        .hydrated_prompt(
            "chat-1",
            "current",
            "new prompt",
            &HistoryHydration::AfterMessage("missing".to_owned()),
        )
        .expect("hydrate full fallback");

    assert!(hydrated.contains("existing context"));
    assert!(hydrated.ends_with("User: new prompt"));
}

#[test]
fn stale_pending_request_cannot_target_replacement_client() {
    let store = Arc::new(Store::open(Path::new(":memory:")).expect("store"));
    let (events, _) = broadcast::channel(4);
    let manager = SessionManager::new(
        Arc::clone(&store),
        AgentManager::new(store, PathBuf::from("/tmp/amarcode-test")),
        events,
        PathBuf::from("/tmp/amarcode-test-attachments"),
    );

    manager.inner.by_chat.lock().expect("live runs").insert(
        "chat-1".to_owned(),
        LiveRun {
            run_id: "new-run".to_owned(),
            agent_id: "agent".to_owned(),
            client: sleeping_client(),
            acp_session_id: Some("new-session".to_owned()),
            supports_images: false,
            session_configuration: SessionConfiguration::default(),
            history_hydration: HistoryHydration::None,
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
            acp_id: crate::acp::RpcId::Number(1),
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
    store
        .save_agent(&crate::protocol::AgentDefinition {
            id: "codex-acp".into(),
            name: "Codex".into(),
            command: "test-agent".into(),
            arguments: vec![],
            environment: vec![],
            available: false,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        })
        .expect("create agent");
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
            context_usage: None,
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
        AgentManager::new(Arc::clone(&store), PathBuf::from("/tmp/amarcode-test")),
        events,
        PathBuf::from("/tmp/amarcode-test-attachments"),
    );
    manager.inner.by_chat.lock().expect("live runs").insert(
        "chat-1".to_owned(),
        LiveRun {
            run_id: "run-1".to_owned(),
            agent_id: "codex-acp".to_owned(),
            client: sleeping_client(),
            acp_session_id: Some("session-1".to_owned()),
            supports_images: false,
            session_configuration: SessionConfiguration::default(),
            history_hydration: HistoryHydration::None,
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
            acp_id: crate::acp::RpcId::Number(1),
            method: "session/request_permission".to_owned(),
            params: Value::Null,
        },
    );

    manager
        .terminate_failed_prompt(
            "chat-1",
            "run-1",
            "user-message",
            &crate::service::session::classify_message("ACP request timed out"),
        )
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

#[test]
fn cancel_interrupts_partial_messages() {
    let store = Arc::new(Store::open(Path::new(":memory:")).expect("store"));
    store
        .save_agent(&crate::protocol::AgentDefinition {
            id: "grok-acp".into(),
            name: "Grok".into(),
            command: "test-agent".into(),
            arguments: vec![],
            environment: vec![],
            available: false,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        })
        .expect("create agent");
    store
        .create_chat(&crate::store::Chat {
            id: "chat-1".to_owned(),
            workspace_path: "/tmp/workspace".to_owned(),
            title: "cancel".to_owned(),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
            archived_at: None,
        })
        .expect("create chat");
    store
        .create_run(&AgentRun {
            id: "run-1".to_owned(),
            chat_id: "chat-1".to_owned(),
            agent_id: "grok-acp".to_owned(),
            acp_session_id: Some("session-1".to_owned()),
            status: RunStatus::Running,
            started_at: "2026-01-01T00:00:00Z".to_owned(),
            finished_at: None,
            error_message: None,
            context_usage: None,
        })
        .expect("create run");
    store
        .create_message(&Message {
            id: "partial-message".to_owned(),
            chat_id: "chat-1".to_owned(),
            agent_run_id: Some("run-1".to_owned()),
            role: MessageRole::Assistant,
            content: "I'll start by mapping".to_owned(),
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
                content_json: json!({ "text": "I'll start by mapping" }).to_string(),
            }],
        )
        .expect("create message part");

    let (events, _) = broadcast::channel(8);
    let manager = SessionManager::new(
        Arc::clone(&store),
        AgentManager::new(Arc::clone(&store), PathBuf::from("/tmp/amarcode-test")),
        events,
        PathBuf::from("/tmp/amarcode-test-attachments"),
    );
    manager.inner.by_chat.lock().expect("live runs").insert(
        "chat-1".to_owned(),
        LiveRun {
            run_id: "run-1".to_owned(),
            agent_id: "grok-acp".to_owned(),
            client: sleeping_client(),
            acp_session_id: Some("session-1".to_owned()),
            supports_images: false,
            session_configuration: SessionConfiguration::default(),
            history_hydration: HistoryHydration::None,
            streaming_message_ids: HashMap::from([(
                "upstream".to_owned(),
                "partial-message".to_owned(),
            )]),
            last_streaming_message_id: Some("partial-message".to_owned()),
            active_user_message_id: Some("user-message".to_owned()),
        },
    );

    manager.cancel("chat-1").expect("cancel");

    assert_eq!(
        store
            .get_message("partial-message")
            .expect("message")
            .expect("message row")
            .status,
        MessageStatus::Interrupted
    );
    assert_eq!(
        store
            .get_run("run-1")
            .expect("run")
            .expect("run row")
            .status,
        RunStatus::Stopped
    );
}
