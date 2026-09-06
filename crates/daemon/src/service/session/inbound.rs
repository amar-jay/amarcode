//! ACP inbound worker: notifications, agent-initiated requests, disconnects.

use std::{sync::Arc, thread};

use serde_json::{json, Value};
use tracing::{debug, warn};

use crate::{
    acp::AcpInbound,
    protocol::{
        AgentEventMethod, EditorEvent, MessageStatus, RpcDirection, RpcEnvelope, RunStatus,
        TurnStatus,
    },
    Error, Result,
};

use super::{
    messages::{
        append_text_delta, append_thinking_delta, append_tool_part, complete_run,
        ensure_streaming_message, finalize_message, remove_pending_requests_for_run,
        take_streaming_messages, take_streaming_messages_from_live,
    },
    terminal::is_terminal_method,
    types::{PendingAgentRequest, SessionInner},
    util::{emit, extract_text_delta},
};

pub(super) fn spawn_inbound_worker(
    inner: Arc<SessionInner>,
    run_id: String,
    chat_id: String,
    inbound: std::sync::mpsc::Receiver<AcpInbound>,
) {
    thread::Builder::new()
        .name(format!("acp-inbound-{run_id}"))
        .spawn(move || {
            while let Ok(msg) = inbound.recv() {
                if let Err(err) = handle_inbound(&inner, &run_id, &chat_id, msg) {
                    warn!(%run_id, error = %err, "failed handling ACP inbound");
                }
            }
        })
        .expect("spawn acp inbound worker");
}

fn handle_inbound(
    inner: &Arc<SessionInner>,
    run_id: &str,
    chat_id: &str,
    msg: AcpInbound,
) -> Result<()> {
    match msg {
        AcpInbound::Notification { event, envelope } => {
            // 1. STORE raw envelope
            inner.store.save_acp_envelope(run_id, &envelope)?;
            if !run_owns_chat(inner, run_id, chat_id)? {
                debug!(%run_id, %chat_id, "ignoring notification from replaced ACP run");
                return Ok(());
            }
            // 2. apply product state + 3. EVENTS
            apply_notification(inner, run_id, chat_id, event, &envelope)?;
        }
        AcpInbound::Request { id, method, params } => {
            let envelope = RpcEnvelope {
                direction: RpcDirection::Received,
                method: method.clone(),
                payload: params.clone(),
            };
            inner.store.save_acp_envelope(run_id, &envelope)?;

            if is_terminal_method(&method) {
                super::terminal::spawn_terminal_request(
                    Arc::clone(inner),
                    run_id.to_owned(),
                    chat_id.to_owned(),
                    id,
                    method,
                    params,
                );
                return Ok(());
            }

            let request_id = new_pending_request_id();
            let pending = PendingAgentRequest {
                request_id: request_id.clone(),
                run_id: run_id.to_owned(),
                chat_id: chat_id.to_owned(),
                acp_id: id.clone(),
                method: method.clone(),
                params: params.clone(),
            };
            // Hold the live-run lock through insertion. A replacement must
            // wait, then removes this run's pending requests after detaching.
            let live = inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            if live.get(chat_id).is_none_or(|live| live.run_id != run_id) {
                debug!(%run_id, %chat_id, %id, "ignoring request from replaced ACP run");
                return Ok(());
            }
            inner
                .pending
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?
                .insert(request_id.clone(), pending);
            drop(live);

            let agent_method = AgentEventMethod::from(method.as_str());
            match agent_method {
                AgentEventMethod::InputRequested => {
                    emit(
                        inner,
                        EditorEvent::QuestionRequired {
                            run_id: run_id.to_owned(),
                            request_id,
                            details: params,
                        },
                    );
                }
                _ => {
                    // Default: treat agent-initiated requests as approvals
                    // (permissions, etc.).
                    emit(
                        inner,
                        EditorEvent::ApprovalRequired {
                            run_id: run_id.to_owned(),
                            request_id,
                            details: params,
                        },
                    );
                }
            }
        }
        AcpInbound::InvalidMessage { error, raw } => {
            warn!(%run_id, %error, %raw, "invalid ACP message");
            let envelope = RpcEnvelope {
                direction: RpcDirection::Received,
                method: "invalid".into(),
                payload: json!({ "error": error, "raw": raw }),
            };
            let _ = inner.store.save_acp_envelope(run_id, &envelope);
        }
        AcpInbound::Disconnected => {
            debug!(%run_id, "ACP disconnected");
            inner.terminals.release_run(run_id);
            // Check and remove atomically so an old reader cannot remove a
            // replacement that became live between two lock acquisitions.
            let disconnected = {
                let mut guard = inner
                    .by_chat
                    .lock()
                    .map_err(|_| Error::msg("session lock poisoned"))?;
                if guard.get(chat_id).is_some_and(|live| live.run_id == run_id) {
                    guard.remove(chat_id)
                } else {
                    None
                }
            };
            if let Some(mut live) = disconnected {
                let agent_id = live.agent_id.clone();
                let active_user_message_id = live.active_user_message_id.clone();
                for message_id in take_streaming_messages_from_live(&mut live) {
                    finalize_message(inner, &message_id, MessageStatus::Interrupted)?;
                    emit(
                        inner,
                        EditorEvent::MessageUpdated {
                            message_id,
                            status: MessageStatus::Interrupted,
                        },
                    );
                }
                let failure = crate::service::session::classify_message("agent disconnected");
                if let Some(user_message_id) = active_user_message_id {
                    emit(
                        inner,
                        EditorEvent::TurnUpdated {
                            chat_id: chat_id.to_owned(),
                            run_id: run_id.to_owned(),
                            user_message_id,
                            status: TurnStatus::Failed,
                            stop_reason: None,
                            error_message: Some(failure.message.clone()),
                            error_kind: Some(failure.kind),
                        },
                    );
                }
                inner.store.update_run(
                    run_id,
                    RunStatus::Stopped,
                    None,
                    Some(failure.message.as_str()),
                )?;
                emit(
                    inner,
                    EditorEvent::RunUpdated {
                        run_id: run_id.to_owned(),
                        status: RunStatus::Stopped,
                        error_message: Some(failure.message.clone()),
                        error_kind: Some(failure.kind),
                    },
                );
                if !agent_id.is_empty() {
                    emit(
                        inner,
                        EditorEvent::AgentConnectionChanged {
                            agent_id,
                            connected: false,
                            error_message: Some(failure.message),
                            error_kind: Some(failure.kind),
                        },
                    );
                }
                remove_pending_requests_for_run(inner, run_id);
            }
        }
        AcpInbound::Barrier(acknowledge) => {
            let _ = acknowledge.send(());
        }
    }
    Ok(())
}

fn new_pending_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn apply_notification(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
    event: AgentEventMethod,
    envelope: &RpcEnvelope,
) -> Result<()> {
    match event {
        // Primary ACP streaming path used by Copilot and the official protocol.
        AgentEventMethod::SessionUpdate => {
            apply_session_update(inner, run_id, chat_id, &envelope.payload)?;
        }
        AgentEventMethod::MessageStarted => {
            let message_id = ensure_streaming_message(inner, run_id, chat_id, None)?;
            emit(
                inner,
                EditorEvent::MessageUpdated {
                    message_id,
                    status: MessageStatus::Streaming,
                },
            );
        }
        AgentEventMethod::MessageDelta => {
            let message_id = ensure_streaming_message(inner, run_id, chat_id, None)?;
            if let Some(delta) = extract_text_delta(&envelope.payload) {
                append_text_delta(inner, &message_id, &delta)?;
            }
            emit(
                inner,
                EditorEvent::MessageUpdated {
                    message_id,
                    status: MessageStatus::Streaming,
                },
            );
        }
        AgentEventMethod::ThinkingDelta => {
            let message_id = ensure_streaming_message(inner, run_id, chat_id, None)?;
            if let Some(delta) = extract_text_delta(&envelope.payload) {
                append_thinking_delta(inner, &message_id, &delta)?;
            }
            emit(
                inner,
                EditorEvent::MessageUpdated {
                    message_id,
                    status: MessageStatus::Streaming,
                },
            );
        }
        AgentEventMethod::MessageCompleted => {
            for message_id in take_streaming_messages(inner, run_id, chat_id) {
                finalize_message(inner, &message_id, MessageStatus::Complete)?;
                emit(
                    inner,
                    EditorEvent::MessageUpdated {
                        message_id,
                        status: MessageStatus::Complete,
                    },
                );
            }
        }
        AgentEventMethod::MessageFailed => {
            for message_id in take_streaming_messages(inner, run_id, chat_id) {
                finalize_message(inner, &message_id, MessageStatus::Failed)?;
                emit(
                    inner,
                    EditorEvent::MessageUpdated {
                        message_id,
                        status: MessageStatus::Failed,
                    },
                );
            }
        }
        AgentEventMethod::ToolCallStarted
        | AgentEventMethod::ToolCallOutput
        | AgentEventMethod::ToolCallCompleted
        | AgentEventMethod::ToolCallFailed => {
            append_tool_part(inner, run_id, chat_id, &envelope.payload)?;
        }
        AgentEventMethod::SessionEnded => {
            complete_run(inner, run_id, chat_id, RunStatus::Completed, None)?;
        }
        AgentEventMethod::SessionStatusChanged => {
            if let Some(status) = envelope
                .payload
                .get("status")
                .and_then(|v| v.as_str())
                .and_then(|s| RunStatus::parse(s).ok())
            {
                let error = envelope
                    .payload
                    .get("error")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                inner
                    .store
                    .update_run(run_id, status, None, error.as_deref())?;
                let failure = error
                    .as_deref()
                    .map(crate::service::session::classify_message);
                emit(
                    inner,
                    EditorEvent::RunUpdated {
                        run_id: run_id.to_owned(),
                        status,
                        error_message: failure.as_ref().map(|item| item.message.clone()),
                        error_kind: failure.as_ref().map(|item| item.kind),
                    },
                );
            }
        }
        AgentEventMethod::FileChanged | AgentEventMethod::FileChangeProposed => {
            if let Some(paths) = envelope.payload.get("paths").and_then(|v| v.as_array()) {
                let paths: Vec<String> = paths
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect();
                if let Ok(Some(chat)) = inner.store.get_chat(chat_id) {
                    emit(
                        inner,
                        EditorEvent::WorkspaceFilesChanged {
                            workspace_path: chat.workspace_path,
                            paths,
                        },
                    );
                }
            }
        }
        AgentEventMethod::PermissionRequested => {
            emit(
                inner,
                EditorEvent::ApprovalRequired {
                    run_id: run_id.to_owned(),
                    request_id: uuid::Uuid::new_v4().to_string(),
                    details: envelope.payload.clone(),
                },
            );
        }
        AgentEventMethod::InputRequested => {
            emit(
                inner,
                EditorEvent::QuestionRequired {
                    run_id: run_id.to_owned(),
                    request_id: uuid::Uuid::new_v4().to_string(),
                    details: envelope.payload.clone(),
                },
            );
        }
        AgentEventMethod::SessionCreated
        | AgentEventMethod::CommandStarted
        | AgentEventMethod::CommandOutput
        | AgentEventMethod::CommandCompleted
        | AgentEventMethod::PlanUpdated
        | AgentEventMethod::ContextUsage => {}
        AgentEventMethod::Other(_) => {
            // Grok streams tool/permission telemetry as `_x.ai/session_notification`
            // with the same `{ update: { sessionUpdate } }` envelope as ACP
            // `session/update`. Apply it so a hung first tool still lands in
            // message_parts even if the standard `tool_call` update never arrives.
            if looks_like_session_update(&envelope.payload) {
                apply_session_update(inner, run_id, chat_id, &envelope.payload)?;
            }
        }
    }
    Ok(())
}

fn run_owns_chat(inner: &SessionInner, run_id: &str, chat_id: &str) -> Result<bool> {
    let guard = inner
        .by_chat
        .lock()
        .map_err(|_| Error::msg("session lock poisoned"))?;
    Ok(guard.get(chat_id).is_some_and(|live| live.run_id == run_id))
}

/// Handle ACP `session/update` notification params.
fn apply_session_update(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
    payload: &Value,
) -> Result<()> {
    let update = payload.get("update").unwrap_or(payload);
    let kind = session_update_kind(payload).unwrap_or("");

    match kind {
        "agent_message_chunk" => {
            let message_id = ensure_streaming_message(
                inner,
                run_id,
                chat_id,
                update.get("messageId").and_then(|value| value.as_str()),
            )?;
            if let Some(delta) = extract_text_delta(update) {
                if is_reasoning_message_chunk(update) {
                    append_thinking_delta(inner, &message_id, &delta)?;
                } else {
                    append_text_delta(inner, &message_id, &delta)?;
                }
            }
            emit(
                inner,
                EditorEvent::MessageUpdated {
                    message_id,
                    status: MessageStatus::Streaming,
                },
            );
        }
        "user_message_chunk" => {
            // The submitted prompt has already been persisted as a user
            // message before `session/prompt` is sent. Some ACP agents echo
            // it back as a session update; never turn that echo into an
            // assistant message. The raw notification remains in acp_events.
        }
        "agent_thought_chunk" => {
            let message_id = ensure_streaming_message(
                inner,
                run_id,
                chat_id,
                update.get("messageId").and_then(Value::as_str),
            )?;
            if let Some(delta) = extract_text_delta(update) {
                append_thinking_delta(inner, &message_id, &delta)?;
            }
            emit(
                inner,
                EditorEvent::MessageUpdated {
                    message_id,
                    status: MessageStatus::Streaming,
                },
            );
        }
        "tool_call" | "tool_call_update" | "tool_call_delta_chunk" => {
            append_tool_part(inner, run_id, chat_id, update)?;
        }
        "available_commands_update"
        | "session_info_update"
        | "usage_update"
        | "config_option_update"
        | "session_summary_generated"
        | "response_completed"
        | "turn_completed"
        | "last_turn_summary"
        | "pending_interaction"
        | "interaction_resolved"
        | "model_changed" => {
            // Informational ACP / Grok telemetry; already logged in acp_events.
            // `pending_interaction` is resolved inside Grok (yolo), not via
            // `session/request_permission`, so it must not become ApprovalRequired.
        }
        _ => {
            debug!(%kind, "unhandled sessionUpdate kind");
        }
    }
    Ok(())
}

fn looks_like_session_update(payload: &Value) -> bool {
    session_update_kind(payload).is_some()
}

fn session_update_kind(payload: &Value) -> Option<&str> {
    let update = payload.get("update").unwrap_or(payload);
    update.get("sessionUpdate").and_then(Value::as_str)
}

fn is_reasoning_message_chunk(update: &Value) -> bool {
    matches!(
        update
            .get("_meta")
            .and_then(|meta| meta.get("codex"))
            .and_then(|codex| codex.get("phase"))
            .and_then(Value::as_str),
        Some("commentary" | "analysis" | "reasoning")
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde_json::json;

    use super::{
        is_reasoning_message_chunk, looks_like_session_update, new_pending_request_id,
        session_update_kind,
    };

    #[test]
    fn daemon_request_ids_do_not_share_the_acp_id_namespace() {
        let ids = (0..64)
            .map(|_| new_pending_request_id())
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), 64);
        assert!(ids.iter().all(|id| uuid::Uuid::parse_str(id).is_ok()));
    }

    #[test]
    fn codex_commentary_is_classified_as_reasoning() {
        assert!(is_reasoning_message_chunk(&json!({
            "_meta": { "codex": { "phase": "commentary" } }
        })));
        assert!(is_reasoning_message_chunk(&json!({
            "_meta": { "codex": { "phase": "analysis" } }
        })));
    }

    #[test]
    fn final_and_unphased_messages_remain_visible_answers() {
        assert!(!is_reasoning_message_chunk(&json!({
            "_meta": { "codex": { "phase": "final_answer" } }
        })));
        assert!(!is_reasoning_message_chunk(&json!({})));
    }

    #[test]
    fn grok_session_notification_payloads_look_like_session_updates() {
        let payload = json!({
            "sessionId": "s1",
            "update": {
                "name": "list_dir",
                "sessionUpdate": "tool_call_delta_chunk",
                "tool_call_id": "call-1",
                "tool_index": 0
            }
        });
        assert!(looks_like_session_update(&payload));
        assert_eq!(session_update_kind(&payload), Some("tool_call_delta_chunk"));
    }

    #[test]
    fn grok_prompt_complete_is_not_a_session_update() {
        let payload = json!({
            "sessionId": "s1",
            "promptId": "p1",
            "stopReason": "end_turn"
        });
        assert!(!looks_like_session_update(&payload));
        assert_eq!(session_update_kind(&payload), None);
    }
}

#[cfg(all(test, unix))]
mod grok_inbound_tests {
    use std::{collections::HashMap, sync::Arc};

    use serde_json::json;
    use tokio::sync::broadcast;

    use crate::{
        acp::AcpClient,
        protocol::{
            AgentEventMethod, MessagePartKind, MessageRole, MessageStatus, RpcDirection,
            RpcEnvelope, RunStatus,
        },
        store::{AgentRun, Message, Store},
    };

    use super::super::types::LiveRun;
    use super::{apply_notification, SessionInner};

    fn sleeping_client() -> Arc<AcpClient> {
        let arguments = vec!["-c".to_owned(), "sleep 30".to_owned()];
        let (client, _inbound) =
            AcpClient::spawn("/bin/sh", &arguments, &[], None).expect("spawn sleeping test agent");
        Arc::new(client)
    }

    #[test]
    fn grok_tool_call_delta_notification_is_stored_as_a_tool_part() {
        let store = Arc::new(Store::open(std::path::Path::new(":memory:")).expect("store"));
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
                title: "grok".to_owned(),
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
            })
            .expect("create run");
        store
            .create_message(&Message {
                id: "msg-1".to_owned(),
                chat_id: "chat-1".to_owned(),
                agent_run_id: Some("run-1".to_owned()),
                role: MessageRole::Assistant,
                content: "I'll start by mapping the repo.".to_owned(),
                status: MessageStatus::Streaming,
                created_at: "2026-01-01T00:00:01Z".to_owned(),
                updated_at: "2026-01-01T00:00:01Z".to_owned(),
            })
            .expect("create message");

        let (events, _) = broadcast::channel(8);
        let inner = SessionInner {
            store: Arc::clone(&store),
            events,
            prompt_locks: std::sync::Mutex::new(HashMap::new()),
            by_chat: std::sync::Mutex::new(HashMap::from([(
                "chat-1".to_owned(),
                LiveRun {
                    run_id: "run-1".to_owned(),
                    agent_id: "grok-acp".to_owned(),
                    client: sleeping_client(),
                    acp_session_id: Some("session-1".to_owned()),
                    supports_images: false,
                    session_configuration: Default::default(),
                    needs_history_hydration: false,
                    streaming_message_ids: HashMap::from([(
                        "__default__".to_owned(),
                        "msg-1".to_owned(),
                    )]),
                    last_streaming_message_id: Some("msg-1".to_owned()),
                    active_user_message_id: Some("user-1".to_owned()),
                },
            )])),
            pending: std::sync::Mutex::new(HashMap::new()),
            terminals: Default::default(),
        };

        let envelope = RpcEnvelope {
            direction: RpcDirection::Received,
            method: "_x.ai/session_notification".into(),
            payload: json!({
                "sessionId": "session-1",
                "update": {
                    "arguments_delta": "{\"target_directory\":\"/tmp/workspace\"}",
                    "name": "list_dir",
                    "sessionUpdate": "tool_call_delta_chunk",
                    "tool_call_id": "call-1",
                    "tool_index": 0
                }
            }),
        };
        apply_notification(
            &inner,
            "run-1",
            "chat-1",
            AgentEventMethod::Other("_x.ai/session_notification".into()),
            &envelope,
        )
        .expect("apply grok tool notification");

        let parts = store.message_parts("msg-1").expect("parts");
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].kind, MessagePartKind::ToolCall);
        assert!(parts[0].content_json.contains("list_dir"));
        assert!(parts[0].content_json.contains("tool_call_delta_chunk"));
    }
}
