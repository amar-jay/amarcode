//! ACP session configuration snapshots.
//!
//! Agents advertise options on `session/new` and after `session/set_config_option`.
//! Amarcode stores the snapshot as-is and applies user changes without mapping
//! onto a hardcoded plan/build/ask vocabulary.

use serde_json::{json, Value};

use crate::{
    protocol::{
        SessionConfigAssignment, SessionConfigOption, SessionConfigSelectChoice, SessionConfigValue,
    },
    Error, Result,
};

pub(super) const SET_CONFIG_OPTION_METHOD: &str = "session/set_config_option";

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct SessionConfiguration {
    pub(super) options: Vec<SessionConfigOption>,
}

impl SessionConfiguration {
    pub(super) fn from_response(response: &Value) -> Self {
        Self::from_options_value(
            response
                .get("configOptions")
                .or_else(|| response.get("config_options")),
        )
    }

    pub(super) fn from_update(update: &Value) -> Self {
        Self::from_options_value(
            update
                .get("configOptions")
                .or_else(|| update.get("config_options")),
        )
    }

    pub(super) fn from_options_value(value: Option<&Value>) -> Self {
        let options = value
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(parse_option)
            .collect();
        Self { options }
    }

    pub(super) fn as_protocol(&self) -> Vec<SessionConfigOption> {
        self.options.clone()
    }

    pub(super) fn validate_assignment(
        &self,
        assignment: &SessionConfigAssignment,
    ) -> Result<SessionConfigValue> {
        let option = self
            .options
            .iter()
            .find(|option| option.id == assignment.config_id)
            .ok_or_else(|| {
                Error::msg(format!(
                    "unknown session config option: {}",
                    assignment.config_id
                ))
            })?;
        match (option.option_type.as_str(), &assignment.value) {
            ("select", SessionConfigValue::Id { value }) => {
                if option.options.iter().any(|choice| choice.value == *value) {
                    Ok(assignment.value.clone())
                } else {
                    Err(Error::msg(format!(
                        "invalid value {value} for {}",
                        option.id
                    )))
                }
            }
            ("boolean", SessionConfigValue::Boolean { .. }) => Ok(assignment.value.clone()),
            (option_type, _) => Err(Error::msg(format!(
                "cannot set {option_type} option {} with this value",
                option.id
            ))),
        }
    }
}

pub(super) fn set_config_params(
    session_id: &str,
    config_id: &str,
    value: &SessionConfigValue,
) -> Value {
    match value {
        SessionConfigValue::Id { value } => json!({
            "sessionId": session_id,
            "configId": config_id,
            "type": "id",
            "value": value,
        }),
        SessionConfigValue::Boolean { value } => json!({
            "sessionId": session_id,
            "configId": config_id,
            "type": "boolean",
            "value": value,
        }),
    }
}

fn parse_option(value: &Value) -> Option<SessionConfigOption> {
    let id = value
        .get("id")
        .or_else(|| value.get("configId"))
        .or_else(|| value.get("config_id"))?
        .as_str()?
        .to_owned();
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&id)
        .to_owned();
    let option_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let current_value = value
        .get("currentValue")
        .or_else(|| value.get("current_value"))
        .cloned()
        .unwrap_or(Value::Null);
    let options = value
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|choice| {
            Some(SessionConfigSelectChoice {
                value: choice.get("value")?.as_str()?.to_owned(),
                name: choice
                    .get("name")
                    .and_then(Value::as_str)
                    .or_else(|| choice.get("value").and_then(Value::as_str))?
                    .to_owned(),
                description: choice
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect();
    Some(SessionConfigOption {
        id,
        name,
        description: value
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        category: value
            .get("category")
            .and_then(Value::as_str)
            .map(str::to_owned),
        option_type,
        current_value,
        options,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_select_and_boolean_options() {
        let configuration = SessionConfiguration::from_response(&json!({
            "configOptions": [
                {
                    "id": "mode",
                    "name": "Session Mode",
                    "category": "mode",
                    "type": "select",
                    "currentValue": "ask",
                    "options": [
                        { "value": "ask", "name": "Ask" },
                        { "value": "code", "name": "Code" }
                    ]
                },
                {
                    "id": "brave_mode",
                    "name": "Brave Mode",
                    "type": "boolean",
                    "currentValue": false
                }
            ]
        }));
        assert_eq!(configuration.options.len(), 2);
        assert_eq!(configuration.options[0].option_type, "select");
        assert_eq!(configuration.options[1].current_value, json!(false));
    }

    #[test]
    fn ignores_unknown_types_for_assignment_but_keeps_them() {
        let configuration = SessionConfiguration::from_response(&json!({
            "configOptions": [{
                "id": "temperature",
                "name": "Temperature",
                "type": "number",
                "currentValue": 0.2
            }]
        }));
        assert_eq!(configuration.options[0].option_type, "number");
        let error = configuration
            .validate_assignment(&SessionConfigAssignment {
                config_id: "temperature".into(),
                value: SessionConfigValue::Id {
                    value: "0.2".into(),
                },
            })
            .expect_err("unknown types cannot be set");
        assert!(error.to_string().contains("cannot set"));
    }

    #[test]
    fn rejects_select_values_not_advertised() {
        let configuration = SessionConfiguration::from_response(&json!({
            "configOptions": [{
                "id": "mode",
                "name": "Mode",
                "type": "select",
                "currentValue": "ask",
                "options": [{ "value": "ask", "name": "Ask" }]
            }]
        }));
        assert!(configuration
            .validate_assignment(&SessionConfigAssignment {
                config_id: "mode".into(),
                value: SessionConfigValue::Id {
                    value: "code".into(),
                },
            })
            .is_err());
    }

    #[test]
    fn empty_response_is_a_complete_empty_snapshot() {
        let configuration = SessionConfiguration::from_response(&json!({
            "configOptions": []
        }));
        assert!(configuration.options.is_empty());
    }

    #[test]
    fn builds_acp_wire_values_for_select_and_boolean() {
        assert_eq!(
            set_config_params(
                "session-1",
                "mode",
                &SessionConfigValue::Id {
                    value: "code".into()
                },
            ),
            json!({
                "sessionId": "session-1",
                "configId": "mode",
                "type": "id",
                "value": "code"
            })
        );
        assert_eq!(
            set_config_params(
                "session-1",
                "brave_mode",
                &SessionConfigValue::Boolean { value: true },
            ),
            json!({
                "sessionId": "session-1",
                "configId": "brave_mode",
                "type": "boolean",
                "value": true
            })
        );
    }
}
