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
        take_streaming_messages,
    },
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
    inner: &SessionInner,
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

            let request_id = new_pending_request_id();
            let pending = PendingAgentRequest {
                request_id: request_id.clone(),
                run_id: run_id.to_owned(),
                chat_id: chat_id.to_owned(),
                acp_id: id,
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
                debug!(%run_id, %chat_id, acp_id = id, "ignoring request from replaced ACP run");
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
            if let Some(live) = disconnected {
                let agent_id = live.agent_id;
                let active_user_message_id = live.active_user_message_id;
                if let Some(user_message_id) = active_user_message_id {
                    emit(
                        inner,
                        EditorEvent::TurnUpdated {
                            chat_id: chat_id.to_owned(),
                            run_id: run_id.to_owned(),
                            user_message_id,
                            status: TurnStatus::Failed,
                            stop_reason: None,
                            error_message: Some("agent disconnected".into()),
                        },
                    );
                }
                inner.store.update_run(
                    run_id,
                    RunStatus::Stopped,
                    None,
                    Some("agent disconnected"),
                )?;
                emit(
                    inner,
                    EditorEvent::RunUpdated {
                        run_id: run_id.to_owned(),
                        status: RunStatus::Stopped,
                        error_message: Some("agent disconnected".into()),
                    },
                );
                if !agent_id.is_empty() {
                    emit(
                        inner,
                        EditorEvent::AgentConnectionChanged {
                            agent_id,
                            connected: false,
                            error_message: Some("agent disconnected".into()),
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
                emit(
                    inner,
                    EditorEvent::RunUpdated {
                        run_id: run_id.to_owned(),
                        status,
                        error_message: error,
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
        | AgentEventMethod::ContextUsage
        | AgentEventMethod::Other(_) => {}
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
    let kind = update
        .get("sessionUpdate")
        .and_then(|v| v.as_str())
        .unwrap_or("");

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
        "tool_call" | "tool_call_update" => {
            append_tool_part(inner, run_id, chat_id, update)?;
        }
        "available_commands_update" => {
            // Informational; already logged in acp_events.
        }
        _ => {
            debug!(%kind, "unhandled sessionUpdate kind");
        }
    }
    Ok(())
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

    use super::{is_reasoning_message_chunk, new_pending_request_id};

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
}
