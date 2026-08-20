//! Streaming assistant messages, parts, and run completion helpers.

use serde_json::{json, Value};

use crate::{
    protocol::{EditorEvent, MessagePartKind, MessageRole, MessageStatus, RunStatus, TurnStatus},
    store::{Message, MessagePart},
    Error, Result,
};

use super::{
    types::{LiveRun, SessionInner},
    util::{emit, timestamp},
};

pub(super) fn append_tool_part(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
    payload: &Value,
) -> Result<()> {
    let message_id = ensure_context_message(inner, run_id, chat_id)?;
    let ordinal = next_part_ordinal(inner, &message_id)?;
    let mut parts = inner.store.message_parts(&message_id)?;
    parts.push(MessagePart {
        message_id: message_id.clone(),
        ordinal,
        kind: MessagePartKind::ToolCall,
        content_json: payload.to_string(),
    });
    inner.store.replace_message_parts(&message_id, &parts)?;
    emit(
        inner,
        EditorEvent::MessagePartAdded {
            message_id,
            ordinal,
            kind: MessagePartKind::ToolCall,
        },
    );
    Ok(())
}

pub(super) fn complete_run(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
    status: RunStatus,
    error: Option<&str>,
) -> Result<()> {
    let live = {
        let mut guard = inner
            .by_chat
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?;
        if guard.get(chat_id).is_none_or(|live| live.run_id != run_id) {
            return Ok(());
        }
        guard.remove(chat_id).expect("live run checked above")
    };

    for message_id in live.streaming_message_ids.into_values() {
        finalize_message(inner, &message_id, MessageStatus::Complete)?;
        emit(
            inner,
            EditorEvent::MessageUpdated {
                message_id,
                status: MessageStatus::Complete,
            },
        );
    }
    if let Some(user_message_id) = live.active_user_message_id {
        let turn_status = if status == RunStatus::Failed {
            TurnStatus::Failed
        } else if status == RunStatus::Stopped {
            TurnStatus::Cancelled
        } else {
            TurnStatus::Completed
        };
        emit(
            inner,
            EditorEvent::TurnUpdated {
                chat_id: chat_id.to_owned(),
                run_id: run_id.to_owned(),
                user_message_id,
                status: turn_status,
                stop_reason: None,
                error_message: error.map(str::to_owned),
            },
        );
    }
    inner.store.update_run(run_id, status, None, error)?;
    emit(
        inner,
        EditorEvent::RunUpdated {
            run_id: run_id.to_owned(),
            status,
            error_message: error.map(str::to_owned),
        },
    );
    remove_pending_requests_for_run(inner, run_id);
    emit(
        inner,
        EditorEvent::AgentConnectionChanged {
            agent_id: live.agent_id,
            connected: false,
            error_message: error.map(str::to_owned),
        },
    );
    Ok(())
}

pub(super) fn ensure_streaming_message(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
    upstream_message_id: Option<&str>,
) -> Result<String> {
    let mut guard = inner
        .by_chat
        .lock()
        .map_err(|_| Error::msg("session lock poisoned"))?;
    let live = guard
        .get_mut(chat_id)
        .filter(|live| live.run_id == run_id)
        .ok_or_else(|| Error::msg("no live run for streaming message"))?;

    // Older ACP agents omit `messageId`; retain the old single-message
    // behavior for them. Agents such as Copilot provide it, allowing one turn
    // to carry separate commentary and final-answer messages.
    let stream_key = upstream_message_id.unwrap_or("__default__");
    if let Some(id) = live.streaming_message_ids.get(stream_key) {
        let id = id.clone();
        live.last_streaming_message_id = Some(id.clone());
        return Ok(id);
    }

    let now = timestamp();
    let message = Message {
        id: uuid::Uuid::new_v4().to_string(),
        chat_id: chat_id.to_owned(),
        agent_run_id: Some(run_id.to_owned()),
        role: MessageRole::Assistant,
        content: String::new(),
        status: MessageStatus::Streaming,
        created_at: now.clone(),
        updated_at: now,
    };
    inner.store.create_message(&message)?;
    live.streaming_message_ids
        .insert(stream_key.to_owned(), message.id.clone());
    live.last_streaming_message_id = Some(message.id.clone());
    Ok(message.id)
}

/// Associate non-message events such as thinking and tool calls with the
/// latest visible assistant message. ACP does not give these events the same
/// `messageId` as their surrounding commentary/final message.
pub(super) fn ensure_context_message(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
) -> Result<String> {
    let last_message_id = {
        let guard = inner
            .by_chat
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?;
        guard
            .get(chat_id)
            .filter(|live| live.run_id == run_id)
            .and_then(|live| live.last_streaming_message_id.clone())
    };
    last_message_id.map_or_else(
        || ensure_streaming_message(inner, run_id, chat_id, None),
        Ok,
    )
}

pub(super) fn take_streaming_messages(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
) -> Vec<String> {
    let Ok(mut guard) = inner.by_chat.lock() else {
        return Vec::new();
    };
    let Some(live) = guard.get_mut(chat_id).filter(|live| live.run_id == run_id) else {
        return Vec::new();
    };
    take_streaming_messages_from_live(live)
}

pub(super) fn remove_pending_requests_for_run(inner: &SessionInner, run_id: &str) {
    if let Ok(mut pending) = inner.pending.lock() {
        pending.retain(|_, request| request.run_id != run_id);
    }
}

pub(super) fn take_streaming_messages_from_live(live: &mut LiveRun) -> Vec<String> {
    live.last_streaming_message_id = None;
    live.streaming_message_ids
        .drain()
        .map(|(_, id)| id)
        .collect()
}

pub(super) fn append_text_delta(inner: &SessionInner, message_id: &str, delta: &str) -> Result<()> {
    // Load current content from messages list is heavy; update by reading via store messages filter.
    // Simpler: get all messages for chat is not available by id — we only have messages(chat_id).
    // Use replace on content via update_message with accumulated: need current content.
    // Store doesn't have get_message — add lightweight approach: only append via parts + update content from parts rebuild is heavy.
    // Minimal: update_message replaces full content — fetch via listing is wrong.
    // For now append by reading message_parts text parts or use a SQL get.
    // Quick fix: store message content update using parts + content field:
    // We'll get content by scanning is not ideal. Add get via update that concatenates:
    // Actually create_message stores empty; we can keep content in parts only and set content to previous+delta by...
    // Use replace: load parts, find text part, append, also set content field.

    let mut parts = inner.store.message_parts(message_id)?;
    if let Some(text_part) = parts.iter_mut().find(|p| p.kind == MessagePartKind::Text) {
        let mut obj: Value = serde_json::from_str(&text_part.content_json).unwrap_or(json!({}));
        let existing = obj
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let combined = format!("{existing}{delta}");
        obj = json!({ "text": combined });
        text_part.content_json = obj.to_string();
        inner
            .store
            .update_message(message_id, &combined, MessageStatus::Streaming)?;
        inner.store.replace_message_parts(message_id, &parts)?;
    } else {
        // Thought/tool parts may already occupy ordinal 0 (Grok streams
        // `agent_thought_chunk` before the visible answer).
        let ordinal = parts.iter().map(|part| part.ordinal).max().unwrap_or(-1) + 1;
        parts.push(MessagePart {
            message_id: message_id.to_owned(),
            ordinal,
            kind: MessagePartKind::Text,
            content_json: json!({ "text": delta }).to_string(),
        });
        inner
            .store
            .update_message(message_id, delta, MessageStatus::Streaming)?;
        inner.store.replace_message_parts(message_id, &parts)?;
        emit(
            inner,
            EditorEvent::MessagePartAdded {
                message_id: message_id.to_owned(),
                ordinal,
                kind: MessagePartKind::Text,
            },
        );
    }
    Ok(())
}

pub(super) fn append_thinking_delta(
    inner: &SessionInner,
    message_id: &str,
    delta: &str,
) -> Result<()> {
    let mut parts = inner.store.message_parts(message_id)?;
    if let Some(thinking_part) = parts
        .last_mut()
        .filter(|part| part.kind == MessagePartKind::Thinking)
    {
        let content: Value = serde_json::from_str(&thinking_part.content_json).unwrap_or(json!({}));
        let existing = content
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        thinking_part.content_json = json!({ "text": format!("{existing}{delta}") }).to_string();
        inner.store.replace_message_parts(message_id, &parts)?;
    } else {
        let ordinal = parts.iter().map(|part| part.ordinal).max().unwrap_or(-1) + 1;
        parts.push(MessagePart {
            message_id: message_id.to_owned(),
            ordinal,
            kind: MessagePartKind::Thinking,
            content_json: json!({ "text": delta }).to_string(),
        });
        inner.store.replace_message_parts(message_id, &parts)?;
        emit(
            inner,
            EditorEvent::MessagePartAdded {
                message_id: message_id.to_owned(),
                ordinal,
                kind: MessagePartKind::Thinking,
            },
        );
    }
    Ok(())
}

pub(super) fn finalize_message(
    inner: &SessionInner,
    message_id: &str,
    status: MessageStatus,
) -> Result<()> {
    // Keep existing content; re-read is hard without get_message. Status-only update with empty content would wipe.
    // load parts text:
    let parts = inner.store.message_parts(message_id)?;
    let content = parts
        .iter()
        .filter(|p| p.kind == MessagePartKind::Text)
        .filter_map(|p| {
            serde_json::from_str::<Value>(&p.content_json)
                .ok()
                .and_then(|v| v.get("text")?.as_str().map(str::to_owned))
        })
        .collect::<Vec<_>>()
        .join("");
    inner.store.update_message(message_id, &content, status)?;
    Ok(())
}

pub(super) fn next_part_ordinal(inner: &SessionInner, message_id: &str) -> Result<i64> {
    let parts = inner.store.message_parts(message_id)?;
    Ok(parts.iter().map(|p| p.ordinal).max().unwrap_or(-1) + 1)
}

#[cfg(all(test, unix))]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use tokio::sync::broadcast;

    use crate::{acp::AcpClient, store::Store};

    use super::*;

    fn sleeping_client() -> Arc<AcpClient> {
        let arguments = vec!["-c".to_owned(), "sleep 30".to_owned()];
        let (client, _inbound) =
            AcpClient::spawn("/bin/sh", &arguments, &[], None).expect("spawn sleeping test agent");
        Arc::new(client)
    }

    #[test]
    fn stale_run_cannot_drain_current_streaming_messages() {
        let store = Arc::new(Store::open(std::path::Path::new(":memory:")).expect("store"));
        let (events, _) = broadcast::channel(4);
        let inner = SessionInner {
            store,
            events,
            prompt_locks: std::sync::Mutex::new(HashMap::new()),
            by_chat: std::sync::Mutex::new(HashMap::from([(
                "chat-1".to_owned(),
                LiveRun {
                    run_id: "new-run".to_owned(),
                    agent_id: "agent".to_owned(),
                    client: sleeping_client(),
                    acp_session_id: Some("session".to_owned()),
                    supports_images: false,
                    session_configuration: Default::default(),
                    needs_history_hydration: false,
                    streaming_message_ids: HashMap::from([(
                        "upstream".to_owned(),
                        "new-message".to_owned(),
                    )]),
                    last_streaming_message_id: Some("new-message".to_owned()),
                    active_user_message_id: None,
                },
            )])),
            pending: std::sync::Mutex::new(HashMap::new()),
        };

        assert!(take_streaming_messages(&inner, "old-run", "chat-1").is_empty());
        assert_eq!(
            take_streaming_messages(&inner, "new-run", "chat-1"),
            vec!["new-message"]
        );
    }
}

#[cfg(test)]
mod append_tests {
    use std::{collections::HashMap, sync::Arc};

    use tokio::sync::broadcast;

    use crate::{
        protocol::{MessageRole, MessageStatus, RunStatus},
        store::{AgentRun, Store},
    };

    use super::*;

    fn inner_with_assistant_message() -> (SessionInner, String) {
        let store = Arc::new(Store::open(std::path::Path::new(":memory:")).expect("store"));
        store.seed_presets().expect("seed agents");
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
                id: "msg-1".to_owned(),
                chat_id: "chat-1".to_owned(),
                agent_run_id: Some("run-1".to_owned()),
                role: MessageRole::Assistant,
                content: String::new(),
                status: MessageStatus::Streaming,
                created_at: "2026-01-01T00:00:01Z".to_owned(),
                updated_at: "2026-01-01T00:00:01Z".to_owned(),
            })
            .expect("create message");
        let (events, _) = broadcast::channel(8);
        let inner = SessionInner {
            store,
            events,
            prompt_locks: std::sync::Mutex::new(HashMap::new()),
            by_chat: std::sync::Mutex::new(HashMap::new()),
            pending: std::sync::Mutex::new(HashMap::new()),
        };
        (inner, "msg-1".to_owned())
    }

    #[test]
    fn text_after_thinking_is_kept_as_visible_answer() {
        let (inner, message_id) = inner_with_assistant_message();
        append_thinking_delta(&inner, &message_id, "planning the reply").expect("thinking");
        append_text_delta(&inner, &message_id, "Hello ").expect("first text");
        append_text_delta(&inner, &message_id, "world.").expect("second text");
        finalize_message(&inner, &message_id, MessageStatus::Complete).expect("finalize");

        let message = inner
            .store
            .get_message(&message_id)
            .expect("load")
            .expect("message");
        assert_eq!(message.content, "Hello world.");

        let parts = inner.store.message_parts(&message_id).expect("parts");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].kind, MessagePartKind::Thinking);
        assert_eq!(parts[1].kind, MessagePartKind::Text);
        assert_ne!(
            parts[0].ordinal, parts[1].ordinal,
            "thinking and text must not share an ordinal"
        );
    }
}
