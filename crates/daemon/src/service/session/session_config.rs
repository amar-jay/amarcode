//! Agent-neutral ACP session configuration.
//!
//! Agents advertise their own option ids and values in `configOptions`.
//! This module translates Amarcode's canonical plan/build/ask modes only when
//! those advertised options contain a compatible value. Unsupported agents
//! retain their defaults; no executable-name checks belong here.

use serde_json::{json, Value};

use crate::{Error, Result};

const SET_CONFIG_OPTION_METHOD: &str = "session/set_config_option";

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct SessionConfiguration {
    options: Vec<SessionConfigOption>,
}

#[derive(Debug, Clone, PartialEq)]
struct SessionConfigOption {
    id: String,
    category: Option<String>,
    kind: String,
    current_value: Value,
    values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct ConfigChange {
    config_id: String,
    value_type: &'static str,
    value: Value,
}

impl SessionConfiguration {
    pub(super) fn from_response(response: &Value) -> Self {
        let options = response
            .get("configOptions")
            .or_else(|| response.get("config_options"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(SessionConfigOption::from_value)
            .collect();
        Self { options }
    }

    fn is_empty(&self) -> bool {
        self.options.is_empty()
    }

    fn mode_changes(&self, mode: &str) -> Result<(bool, Vec<ConfigChange>)> {
        if !matches!(mode, "plan" | "build" | "ask") {
            return Err(Error::msg("mode must be plan, build, or ask"));
        }

        let has_collaboration_mode = self
            .options
            .iter()
            .any(|option| option.id == "collaboration_mode");
        let mut changes = Vec::new();
        let mut supported = false;

        for option in &self.options {
            if !matches!(option.kind.as_str(), "select" | "id") {
                continue;
            }
            let candidates: &[&str] = match option.id.as_str() {
                // Codex and any compatible agent may expose planning as a
                // separate collaboration dimension.
                "collaboration_mode" => match mode {
                    "plan" => &["plan"],
                    "build" | "ask" => &["default"],
                    _ => unreachable!(),
                },
                // `mode` is standardized only as a category, not as a fixed
                // value vocabulary. Select the first value the agent actually
                // advertised, ordered by closest semantic match.
                "mode" => mode_candidates(mode, has_collaboration_mode),
                _ if option.category.as_deref() == Some("mode") => {
                    mode_candidates(mode, has_collaboration_mode)
                }
                _ => continue,
            };

            let Some(value) = candidates
                .iter()
                .find(|candidate| option.values.iter().any(|value| value == **candidate))
            else {
                continue;
            };
            supported = true;
            if option.current_value.as_str() == Some(value) {
                continue;
            }
            changes.push(ConfigChange {
                config_id: option.id.clone(),
                value_type: "id",
                value: json!(value),
            });
        }
        Ok((supported, changes))
    }
}

impl SessionConfigOption {
    fn from_value(value: &Value) -> Option<Self> {
        let id = value
            .get("id")
            .or_else(|| value.get("configId"))
            .or_else(|| value.get("config_id"))?
            .as_str()?
            .to_owned();
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("select")
            .to_owned();
        let values = value
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|option| option.get("value")?.as_str().map(str::to_owned))
            .collect();
        Some(Self {
            id,
            category: value
                .get("category")
                .and_then(Value::as_str)
                .map(str::to_owned),
            kind,
            current_value: value
                .get("currentValue")
                .or_else(|| value.get("current_value"))
                .cloned()
                .unwrap_or(Value::Null),
            values,
        })
    }
}

fn mode_candidates(mode: &str, has_collaboration_mode: bool) -> &'static [&'static str] {
    match mode {
        "plan" if has_collaboration_mode => &["agent", "code", "default"],
        "plan" => &["plan", "ask", "read-only", "read_only"],
        "build" => &["agent", "code", "build", "acceptEdits", "default"],
        "ask" => &["read-only", "read_only", "ask", "plan", "default"],
        _ => &[],
    }
}

/// Apply a canonical mode using only options advertised by the active agent.
///
/// Returns `Ok(false)` when the agent has no compatible session mode option.
/// Each successful response replaces the cached options because ACP responses
/// are complete snapshots and dependent values may have changed.
pub(super) fn configure_session<F>(
    session_id: &str,
    configuration: &mut SessionConfiguration,
    mode: &str,
    mut request: F,
) -> Result<bool>
where
    F: FnMut(&str, Value) -> Result<Value>,
{
    let (supported, changes) = configuration.mode_changes(mode)?;
    if !supported {
        return Ok(false);
    }

    for change in changes {
        let config_id = change.config_id.clone();
        let selected_value = change.value.clone();
        let response = request(
            SET_CONFIG_OPTION_METHOD,
            json!({
                "sessionId": session_id,
                "configId": change.config_id,
                "type": change.value_type,
                "value": change.value,
            }),
        )?;
        let updated = SessionConfiguration::from_response(&response);
        if !updated.is_empty() {
            *configuration = updated;
        } else if let Some(option) = configuration
            .options
            .iter_mut()
            .find(|option| option.id == config_id)
        {
            option.current_value = selected_value;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codex_options() -> Value {
        json!({
            "configOptions": [
                {
                    "id": "mode",
                    "category": "mode",
                    "type": "select",
                    "currentValue": "read-only",
                    "options": [
                        { "value": "read-only", "name": "Read only" },
                        { "value": "agent", "name": "Agent" }
                    ]
                },
                {
                    "id": "collaboration_mode",
                    "category": "mode",
                    "type": "select",
                    "currentValue": "default",
                    "options": [
                        { "value": "default", "name": "Default" },
                        { "value": "plan", "name": "Plan" }
                    ]
                }
            ]
        })
    }

    #[test]
    fn codex_plan_uses_both_advertised_dimensions() {
        let configuration = SessionConfiguration::from_response(&codex_options());
        let (_, changes) = configuration.mode_changes("plan").expect("plan changes");
        assert_eq!(changes.len(), 2);
        assert!(changes
            .iter()
            .any(|change| { change.config_id == "mode" && change.value == json!("agent") }));
        assert!(changes.iter().any(|change| {
            change.config_id == "collaboration_mode" && change.value == json!("plan")
        }));
    }

    #[test]
    fn generic_agent_mode_uses_only_advertised_values() {
        let response = json!({
            "configOptions": [{
                "configId": "mode",
                "category": "mode",
                "type": "select",
                "currentValue": "ask",
                "options": [
                    { "value": "ask", "name": "Ask" },
                    { "value": "code", "name": "Code" }
                ]
            }]
        });
        let configuration = SessionConfiguration::from_response(&response);
        let (_, changes) = configuration.mode_changes("build").expect("build changes");
        assert_eq!(changes[0].value, json!("code"));
    }

    #[test]
    fn agent_without_mode_configuration_is_unsupported() {
        let mut configuration = SessionConfiguration::from_response(&json!({
            "configOptions": [{
                "id": "model",
                "type": "select",
                "currentValue": "one",
                "options": [{ "value": "one", "name": "One" }]
            }]
        }));
        let applied = configure_session("session", &mut configuration, "build", |_, _| {
            panic!("unsupported configuration must not send a request")
        })
        .expect("configuration result");
        assert!(!applied);
    }

    #[test]
    fn already_selected_mode_is_supported_without_a_request() {
        let mut configuration = SessionConfiguration::from_response(&json!({
            "configOptions": [{
                "id": "mode",
                "category": "mode",
                "type": "select",
                "currentValue": "code",
                "options": [
                    { "value": "ask", "name": "Ask" },
                    { "value": "code", "name": "Code" }
                ]
            }]
        }));
        let applied = configure_session("session", &mut configuration, "build", |_, _| {
            panic!("an already selected mode must not send a request")
        })
        .expect("configuration result");
        assert!(applied);
    }
}
