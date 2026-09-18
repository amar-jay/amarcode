//! ACP context-usage normalization.
//!
//! Canonical ACP parsing is agent-independent. Private wire formats are kept
//! behind the adapter dispatch below and never leak into inbound routing.

use serde_json::Value;

use crate::{
    protocol::{AgentDefinition, ContextCost, ContextUsage, EditorEvent},
    Result,
};

use super::{types::SessionInner, util::emit};

#[derive(Debug, Default, PartialEq)]
struct Candidate {
    used: Option<u64>,
    size: Option<u64>,
    cost: Option<ContextCost>,
    extension: bool,
}

impl Candidate {
    /// Produce a complete product snapshot. Canonical partial updates may
    /// retain canonical state; private extensions must be complete themselves.
    fn complete(self, previous: Option<&ContextUsage>) -> Option<ContextUsage> {
        if self.extension && (self.used.is_none() || self.size.is_none()) {
            return None;
        }
        let used = self.used.or_else(|| previous.map(|usage| usage.used))?;
        let size = self.size.or_else(|| previous.map(|usage| usage.size))?;
        (size > 0).then(|| ContextUsage {
            used,
            size,
            cost: self
                .cost
                .or_else(|| previous.and_then(|usage| usage.cost.clone())),
        })
    }
}

fn canonical(payload: &Value) -> Option<Candidate> {
    let update = payload.get("update").unwrap_or(payload);
    let kind = update
        .get("sessionUpdate")
        .or_else(|| update.get("session_update"))
        .and_then(Value::as_str);
    if !matches!(kind, None | Some("usage_update")) {
        return None;
    }

    let candidate = Candidate {
        used: field(update, &["used", "tokens_used"]),
        size: field(update, &["size", "context_window"]),
        cost: update.get("cost").and_then(cost),
        extension: false,
    };
    has_values(&candidate).then_some(candidate)
}

/// Cheap guard that avoids database lookups for ordinary streaming updates.
fn may_have_extension(payload: &Value) -> bool {
    payload.pointer("/_meta/totalTokens").is_some()
        || payload.pointer("/update/_meta/totalTokens").is_some()
}

fn extension(agent: &AgentDefinition, payload: &Value) -> Option<Candidate> {
    grok::matches(agent)
        .then_some(payload)
        .and_then(grok::extract)
}

fn has_values(candidate: &Candidate) -> bool {
    candidate.used.is_some() || candidate.size.is_some() || candidate.cost.is_some()
}

fn field(object: &Value, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| object.get(*name).and_then(number))
}

fn number(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|value| value.try_into().ok()))
        .or_else(|| {
            value.as_f64().and_then(|value| {
                (value.is_finite()
                    && value >= 0.0
                    && value.fract() == 0.0
                    && value <= u64::MAX as f64)
                    .then_some(value as u64)
            })
        })
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn cost(value: &Value) -> Option<ContextCost> {
    let amount = value.get("amount")?.as_f64()?;
    let currency = value.get("currency")?.as_str()?.trim();
    (!currency.is_empty()).then(|| ContextCost {
        amount,
        currency: currency.to_owned(),
    })
}

/// Normalize, persist, and publish a context-usage update.
pub(super) fn apply_context_usage(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
    payload: &Value,
) -> Result<()> {
    let canonical = canonical(payload);
    if canonical.is_none() && !may_have_extension(payload) {
        return Ok(());
    }

    let Some(run) = inner.store.get_run(run_id)? else {
        return Ok(());
    };
    let candidate = match canonical {
        Some(candidate) => Some(candidate),
        None => inner
            .store
            .get_agent(&run.agent_id)?
            .and_then(|agent| extension(&agent, payload)),
    };
    let previous = run.context_usage;
    let Some(usage) = candidate.and_then(|value| value.complete(previous.as_ref())) else {
        return Ok(());
    };
    if previous.as_ref() == Some(&usage) {
        return Ok(());
    }

    inner.store.update_run_context_usage(run_id, &usage)?;
    emit(
        inner,
        EditorEvent::ContextUsageUpdated {
            chat_id: chat_id.to_owned(),
            run_id: run_id.to_owned(),
            usage,
        },
    );
    Ok(())
}

mod grok {
    use super::*;

    pub(super) fn matches(agent: &AgentDefinition) -> bool {
        std::iter::once(agent.id.as_str())
            .chain(std::iter::once(agent.command.as_str()))
            .chain(agent.arguments.iter().map(String::as_str))
            .any(|value| {
                let value = value.to_ascii_lowercase();
                value == "grok"
                    || value == "grok-acp"
                    || value.contains("@xai-official/grok")
                    || value.ends_with("/grok")
            })
    }

    pub(super) fn extract(payload: &Value) -> Option<Candidate> {
        let update = payload.get("update").unwrap_or(payload);
        let kind = update
            .get("sessionUpdate")
            .or_else(|| update.get("session_update"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if matches!(
            kind,
            "subagent_finished" | "turn_completed" | "response_completed"
        ) {
            return None;
        }
        let used = payload
            .get("_meta")
            .or_else(|| update.get("_meta"))
            .and_then(|meta| meta.get("totalTokens"))
            .and_then(number)?;
        Some(Candidate {
            used: Some(used),
            extension: true,
            ..Candidate::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn agent(id: &str) -> AgentDefinition {
        AgentDefinition {
            id: id.into(),
            name: id.into(),
            command: id.into(),
            arguments: vec![],
            environment: vec![],
            available: true,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn canonical_usage_is_complete() {
        let candidate = canonical(&json!({
            "update": { "sessionUpdate": "usage_update", "used": 53_000, "size": 200_000 }
        }))
        .expect("candidate");
        assert_eq!(candidate.complete(None).expect("usage").used, 53_000);
    }

    #[test]
    fn grok_extension_is_scoped_and_does_not_invent_size() {
        let payload = json!({
            "update": { "sessionUpdate": "agent_message_chunk" },
            "_meta": { "totalTokens": 23_153 }
        });
        assert!(extension(&agent("other"), &payload).is_none());
        assert!(extension(&agent("grok-acp"), &payload)
            .expect("candidate")
            .complete(None)
            .is_none());
    }

    #[test]
    fn grok_billing_summary_is_rejected() {
        let payload = json!({
            "update": { "sessionUpdate": "turn_completed" },
            "_meta": { "totalTokens": 405_000 }
        });
        assert!(extension(&agent("grok-acp"), &payload).is_none());
    }
}
