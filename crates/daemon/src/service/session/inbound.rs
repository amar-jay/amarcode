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
		usage::apply_context_usage,
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
            // Persist durable protocol milestones, but not high-frequency
            // streaming deltas whose product state is stored below.
            if should_persist_notification(&envelope) {
                inner.store.save_acp_envelope(run_id, &envelope)?;
            }
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
        AgentEventMethod::ContextUsage => {
            apply_context_usage(inner, run_id, chat_id, &envelope.payload)?;
        }
        AgentEventMethod::SessionCreated
        | AgentEventMethod::CommandStarted
        | AgentEventMethod::CommandOutput
        | AgentEventMethod::CommandCompleted
        | AgentEventMethod::PlanUpdated => {}
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
                // if is_reasoning_message_chunk(update) {
                //     append_thinking_delta(inner, &message_id, &delta)?;
                // } else {
                append_text_delta(inner, &message_id, &delta)?;
                // }
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
            // assistant message. This redundant echo is not retained as raw
            // activity either.
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
        "config_option_update" => {
            let configuration = super::session_config::SessionConfiguration::from_update(update);
            let mut guard = inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            let Some(live) = guard.get_mut(chat_id).filter(|live| live.run_id == run_id) else {
                return Ok(());
            };
            live.session_configuration = configuration.clone();
            drop(guard);

            let options = configuration.as_protocol();
            inner.store.set_session_config(chat_id, &options)?;
            emit(
                inner,
                EditorEvent::SessionConfigUpdated {
                    chat_id: chat_id.to_owned(),
                    options,
                },
            );
        }
        "session_info_update" => {
            apply_session_title(inner, chat_id, update)?;
        }
        "usage_update" => {}
        "available_commands_update"
        | "session_summary_generated"
        | "response_completed"
        | "turn_completed"
        | "last_turn_summary"
        | "pending_interaction"
        | "interaction_resolved"
        | "model_changed" => {
            // Informational ACP / Grok telemetry. Low-frequency milestones are
            // retained in acp_events by the inbound retention policy.
            // `pending_interaction` is resolved inside Grok (yolo), not via
            // `session/request_permission`, so it must not become ApprovalRequired.
        }
        _ => {
            debug!(%kind, "unhandled sessionUpdate kind");
        }
    }
    // Usage normalization owns both canonical parsing and scoped extensions.
    super::usage::apply_context_usage(inner, run_id, chat_id, payload)?;
    Ok(())
}

/// Adopt a human-readable title supplied in ACP session metadata.
/// Missing, null, and blank titles leave the existing fallback untouched.
pub(super) fn apply_session_title(
    inner: &SessionInner,
    chat_id: &str,
    session_info: &Value,
) -> Result<()> {
    let Some(title) = session_info
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
    else {
        return Ok(());
    };
    let Some(chat) = inner.store.get_chat(chat_id)? else {
        return Ok(());
    };
    if chat.title == title {
        return Ok(());
    }

    inner.store.update_title(chat_id, title)?;
    emit(
        inner,
        EditorEvent::ChatUpdated {
            chat_id: chat_id.to_owned(),
        },
    );
    Ok(())
}

fn looks_like_session_update(payload: &Value) -> bool {
    session_update_kind(payload).is_some()
}

fn session_update_kind(payload: &Value) -> Option<&str> {
    let update = payload.get("update").unwrap_or(payload);
    update.get("sessionUpdate").and_then(Value::as_str)
}

/// Raw ACP traffic is an activity/debugging aid, not the source used to
/// restore chats. Avoid duplicating token streams and repeated snapshots that
/// are already folded into messages, message parts, or live session state.
fn should_persist_notification(envelope: &RpcEnvelope) -> bool {
    let Some(kind) = session_update_kind(&envelope.payload) else {
        return true;
    };

    match kind {
        "agent_message_chunk" | "agent_thought_chunk" | "user_message_chunk" => false,
        // This is commonly a complete command catalog repeated during a run.
        // It is neither used for chat restore nor exposed as product state.
        "available_commands_update" => false,
        // `tool_call` retains the start. For updates, retain only terminal
        // milestones; intermediate output remains in derived message parts.
        "tool_call_update" | "tool_call_delta_chunk" => {
            let update = envelope.payload.get("update").unwrap_or(&envelope.payload);
            matches!(
                update.get("status").and_then(Value::as_str),
                Some("completed" | "failed" | "cancelled")
            )
        }
        _ => true,
    }
}

// too specific to codex, trying to be generic across all agents.
// fn is_reasoning_message_chunk(update: &Value) -> bool {
//     matches!(
//         update
//             .get("_meta")
//             .and_then(|meta| meta.get("codex"))
//             .and_then(|codex| codex.get("phase"))
//             .and_then(Value::as_str),
//         Some("commentary" | "analysis" | "reasoning")
//     )
// }

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde_json::json;

    use super::super::usage::apply_context_usage;
    use super::{
        apply_session_title, apply_session_update, new_pending_request_id,
        should_persist_notification, SessionInner,
    };

    #[test]
    fn context_usage_is_persisted_and_emitted() {
        let store = std::sync::Arc::new(
            crate::store::Store::open(std::path::Path::new(":memory:")).expect("store"),
        );
        store
            .save_agent(&crate::protocol::AgentDefinition {
                id: "agent-1".into(),
                name: "Agent".into(),
                command: "agent".into(),
                arguments: vec![],
                environment: vec![],
                available: true,
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
            })
            .expect("agent");
        store
            .create_chat(&crate::store::Chat {
                id: "chat-1".into(),
                workspace_path: "/tmp/workspace".into(),
                title: "Chat".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
                archived_at: None,
            })
            .expect("chat");
        store
            .create_run(&crate::store::AgentRun {
                id: "run-1".into(),
                chat_id: "chat-1".into(),
                agent_id: "agent-1".into(),
                acp_session_id: Some("session-1".into()),
                status: crate::protocol::RunStatus::Running,
                started_at: "2026-01-01T00:00:00Z".into(),
                finished_at: None,
                error_message: None,
                context_usage: None,
            })
            .expect("run");
        let (events, mut receiver) = tokio::sync::broadcast::channel(4);
        let inner = SessionInner {
            store: std::sync::Arc::clone(&store),
            events,
            prompt_locks: std::sync::Mutex::new(std::collections::HashMap::new()),
            by_chat: std::sync::Mutex::new(std::collections::HashMap::new()),
            pending: std::sync::Mutex::new(std::collections::HashMap::new()),
            terminals: Default::default(),
        };

        apply_context_usage(
            &inner,
            "run-1",
            "chat-1",
            &json!({
                "used": 53_000,
                "size": 200_000,
                "cost": { "amount": 0.42, "currency": "USD" }
            }),
        )
        .expect("apply usage");

        let usage = store
            .get_run("run-1")
            .expect("read run")
            .expect("run exists")
            .context_usage
            .expect("usage persisted");
        assert_eq!(usage.used, 53_000);
        assert_eq!(usage.size, 200_000);
        assert_eq!(usage.cost.expect("cost").amount, 0.42);
        assert!(matches!(
            receiver.try_recv(),
            Ok(crate::protocol::EditorEvent::ContextUsageUpdated { chat_id, run_id, usage })
                if chat_id == "chat-1" && run_id == "run-1" && usage.used == 53_000
        ));
    }

    #[test]
    fn usage_update_null_used_is_not_zero() {
        let store = std::sync::Arc::new(
            crate::store::Store::open(std::path::Path::new(":memory:")).expect("store"),
        );
        store
            .save_agent(&crate::protocol::AgentDefinition {
                id: "agent-1".into(),
                name: "Agent".into(),
                command: "agent".into(),
                arguments: vec![],
                environment: vec![],
                available: true,
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
            })
            .expect("agent");
        store
            .create_chat(&crate::store::Chat {
                id: "chat-1".into(),
                workspace_path: "/tmp/workspace".into(),
                title: "Chat".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
                archived_at: None,
            })
            .expect("chat");
        store
            .create_run(&crate::store::AgentRun {
                id: "run-1".into(),
                chat_id: "chat-1".into(),
                agent_id: "agent-1".into(),
                acp_session_id: Some("session-1".into()),
                status: crate::protocol::RunStatus::Running,
                started_at: "2026-01-01T00:00:00Z".into(),
                finished_at: None,
                error_message: None,
                context_usage: None,
            })
            .expect("run");
        let (events, mut receiver) = tokio::sync::broadcast::channel(4);
        let inner = SessionInner {
            store: std::sync::Arc::clone(&store),
            events,
            prompt_locks: std::sync::Mutex::new(std::collections::HashMap::new()),
            by_chat: std::sync::Mutex::new(std::collections::HashMap::new()),
            pending: std::sync::Mutex::new(std::collections::HashMap::new()),
            terminals: Default::default(),
        };

        apply_context_usage(
            &inner,
            "run-1",
            "chat-1",
            &json!({
                "sessionId": "s",
                "update": {
                    "sessionUpdate": "usage_update",
                    "used": null,
                    "size": 200_000
                }
            }),
        )
        .expect("apply");

        let usage = store
            .get_run("run-1")
            .expect("read run")
            .expect("run exists")
            .context_usage;
        assert!(usage.is_none());
        assert!(receiver.try_recv().is_err());
    }

    fn notification(kind: &str, fields: serde_json::Value) -> crate::protocol::RpcEnvelope {
        let mut update = serde_json::Map::new();
        update.insert("sessionUpdate".into(), json!(kind));
        if let Some(fields) = fields.as_object() {
            update.extend(fields.clone());
        }
        crate::protocol::RpcEnvelope {
            direction: crate::protocol::RpcDirection::Received,
            method: "session/update".into(),
            payload: json!({ "sessionId": "session-1", "update": update }),
        }
    }

    #[test]
    fn skips_redundant_session_update_streams() {
        for kind in [
            "agent_message_chunk",
            "agent_thought_chunk",
            "user_message_chunk",
            "available_commands_update",
        ] {
            assert!(!should_persist_notification(&notification(kind, json!({}))));
        }
        assert!(!should_persist_notification(&notification(
            "tool_call_update",
            json!({ "status": "in_progress" })
        )));
        assert!(!should_persist_notification(&notification(
            "tool_call_delta_chunk",
            json!({ "data": "partial output" })
        )));
    }

    #[test]
    fn retains_meaningful_and_terminal_session_updates() {
        for status in ["completed", "failed", "cancelled"] {
            assert!(should_persist_notification(&notification(
                "tool_call_update",
                json!({ "status": status })
            )));
        }
        assert!(should_persist_notification(&notification(
            "tool_call",
            json!({ "status": "in_progress" })
        )));
        assert!(should_persist_notification(&notification(
            "plan",
            json!({ "entries": [] })
        )));
    }

    #[test]
    fn session_info_title_replaces_fallback_and_emits_chat_update() {
        let store = std::sync::Arc::new(
            crate::store::Store::open(std::path::Path::new(":memory:")).expect("store"),
        );
        store
            .create_chat(&crate::store::Chat {
                id: "chat-1".into(),
                workspace_path: "/tmp/workspace".into(),
                title: "first prompt fallback".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
                archived_at: None,
            })
            .expect("create chat");
        let (events, mut receiver) = tokio::sync::broadcast::channel(4);
        let inner = SessionInner {
            store: std::sync::Arc::clone(&store),
            events,
            prompt_locks: std::sync::Mutex::new(std::collections::HashMap::new()),
            by_chat: std::sync::Mutex::new(std::collections::HashMap::new()),
            pending: std::sync::Mutex::new(std::collections::HashMap::new()),
            terminals: Default::default(),
        };

        apply_session_update(
            &inner,
            "run-1",
            "chat-1",
            &json!({
                "sessionId": "session-1",
                "update": {
                    "sessionUpdate": "session_info_update",
                    "title": "  Agent-generated title  "
                }
            }),
        )
        .expect("apply title");

        assert_eq!(
            store.get_chat("chat-1").expect("read chat").unwrap().title,
            "Agent-generated title"
        );
        assert!(matches!(
            receiver.try_recv(),
            Ok(crate::protocol::EditorEvent::ChatUpdated { chat_id }) if chat_id == "chat-1"
        ));

        apply_session_title(&inner, "chat-1", &json!({ "title": "  " }))
            .expect("ignore blank title");
        assert_eq!(
            store.get_chat("chat-1").expect("read chat").unwrap().title,
            "Agent-generated title"
        );
    }

    #[test]
    fn daemon_request_ids_do_not_share_the_acp_id_namespace() {
        let ids = (0..64)
            .map(|_| new_pending_request_id())
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), 64);
        assert!(ids.iter().all(|id| uuid::Uuid::parse_str(id).is_ok()));
    }
}
