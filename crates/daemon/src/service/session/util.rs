//! Small pure helpers used across the session module.

use serde_json::{json, Value};

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

pub(super) fn extract_stop_reason(value: &Value) -> Option<String> {
    value
        .get("stopReason")
        .or_else(|| value.get("stop_reason"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

/// Map UI / CLI convenience payloads into ACP `session/request_permission` results.
///
/// Agents expect:
/// ```json
/// { "outcome": { "outcome": "selected", "optionId": "allow-once" } }
/// ```
/// or `{ "outcome": { "outcome": "cancelled" } }`.
///
/// Clients historically sent `{ "allow": true }` which agents reject, aborting the turn.
pub(super) fn normalize_permission_result(pending_params: &Value, result: Value) -> Value {
    // Already ACP-shaped.
    if result.get("outcome").is_some() {
        return result;
    }

    let options = pending_params
        .get("options")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    if result
        .get("outcome")
        .and_then(|value| value.as_str())
        .is_some_and(|value| value == "cancelled")
        || result.get("cancelled").and_then(|value| value.as_bool()) == Some(true)
    {
        return json!({ "outcome": { "outcome": "cancelled" } });
    }

    if let Some(option_id) = result
        .get("optionId")
        .or_else(|| result.get("option_id"))
        .and_then(|value| value.as_str())
    {
        return json!({
            "outcome": {
                "outcome": "selected",
                "optionId": option_id,
            }
        });
    }

    let allow = result
        .get("allow")
        .and_then(|value| value.as_bool())
        .or_else(|| {
            result
                .get("approved")
                .and_then(|value| value.as_bool())
        });

    if let Some(allow) = allow {
        let preferred_kinds: &[&str] = if allow {
            &["allow_once", "allow_always"]
        } else {
            &["reject_once", "reject_always"]
        };
        if let Some(option_id) = pick_option_id(&options, preferred_kinds) {
            return json!({
                "outcome": {
                    "outcome": "selected",
                    "optionId": option_id,
                }
            });
        }
        // Fall back to common ids if the agent omitted options.
        let fallback = if allow { "allow-once" } else { "reject-once" };
        return json!({
            "outcome": {
                "outcome": "selected",
                "optionId": fallback,
            }
        });
    }

    // Unknown shape — pass through so we don't invent a decision.
    result
}

fn pick_option_id(options: &[Value], preferred_kinds: &[&str]) -> Option<String> {
    for kind in preferred_kinds {
        if let Some(id) = options.iter().find_map(|option| {
            let option_kind = option.get("kind").and_then(|value| value.as_str())?;
            if option_kind == *kind {
                option
                    .get("optionId")
                    .or_else(|| option.get("option_id"))
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            } else {
                None
            }
        }) {
            return Some(id);
        }
    }
    // Prefer any option whose id/name looks like an allow/reject match.
    for kind in preferred_kinds {
        let needle = kind.replace('_', "-");
        if let Some(id) = options.iter().find_map(|option| {
            let id = option
                .get("optionId")
                .or_else(|| option.get("option_id"))
                .and_then(|value| value.as_str())?;
            if id == *kind || id == needle || id.contains(kind.split('_').next().unwrap_or("")) {
                Some(id.to_owned())
            } else {
                None
            }
        }) {
            return Some(id);
        }
    }
    None
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
