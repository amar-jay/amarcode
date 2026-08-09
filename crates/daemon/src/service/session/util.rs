//! Small pure helpers used across the session module.

use serde_json::Value;

use crate::protocol::EditorEvent;

use super::types::SessionInner;

pub(super) fn emit(inner: &SessionInner, event: EditorEvent) {
    let _ = inner.events.send(event);
}

pub(super) fn extract_session_id(value: &Value) -> Option<String> {
    value
        .get("sessionId")
        .or_else(|| value.get("session_id"))
        .or_else(|| value.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

pub(super) fn extract_text_delta(payload: &Value) -> Option<String> {
    // Prefer ACP content block: { "content": { "type": "text", "text": "..." } }
    if let Some(text) = payload
        .pointer("/content/text")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
    {
        return Some(text);
    }
    if let Some(text) = payload
        .get("update")
        .and_then(|u| u.pointer("/content/text"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
    {
        return Some(text);
    }

    payload
        .get("text")
        .or_else(|| payload.get("delta"))
        .or_else(|| payload.get("content"))
        .and_then(|v| {
            if let Some(s) = v.as_str() {
                Some(s.to_owned())
            } else if let Some(obj) = v.as_object() {
                obj.get("text").and_then(|t| t.as_str()).map(str::to_owned)
            } else if let Some(arr) = v.as_array() {
                let text = arr
                    .iter()
                    .filter_map(|item| {
                        item.as_str()
                            .map(str::to_owned)
                            .or_else(|| item.get("text")?.as_str().map(str::to_owned))
                    })
                    .collect::<String>();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            } else {
                None
            }
        })
}

pub(super) fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}
