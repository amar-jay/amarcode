//! Classify ACP transport/remote failures for UI and cleanup paths.

use serde_json::Value;

use crate::{
    acp::AcpError,
    protocol::AgentFailureKind,
};

#[derive(Debug, Clone)]
pub struct ClassifiedFailure {
    pub kind: AgentFailureKind,
    pub message: String,
    pub auth_methods: Option<Value>,
}

impl std::fmt::Display for ClassifiedFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

pub fn classify_acp_failure(error: &AcpError) -> ClassifiedFailure {
    match error {
        AcpError::Remote { code, message, data } => classify_remote(*code, message, data.as_ref()),
        AcpError::ConnectionClosed => ClassifiedFailure {
            kind: AgentFailureKind::AdapterExited,
            message: "Agent adapter exited unexpectedly".into(),
            auth_methods: None,
        },
        AcpError::Timeout { .. }
        | AcpError::IdleTimeout { .. }
        | AcpError::TotalTimeout { .. } => ClassifiedFailure {
            kind: AgentFailureKind::Timeout,
            message: error.to_string(),
            auth_methods: None,
        },
        AcpError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => ClassifiedFailure {
            kind: AgentFailureKind::Unavailable,
            message: format!("Underlying agent executable was not found: {io}"),
            auth_methods: None,
        },
        other => ClassifiedFailure {
            kind: AgentFailureKind::Error,
            message: other.to_string(),
            auth_methods: None,
        },
    }
}

pub fn classify_message(message: &str) -> ClassifiedFailure {
    let lower = message.to_ascii_lowercase();
    if looks_like_auth(&lower, None) {
        return ClassifiedFailure {
            kind: AgentFailureKind::AuthRequired,
            message: humanize_auth(message),
            auth_methods: None,
        };
    }
    if looks_like_unavailable(&lower) {
        return ClassifiedFailure {
            kind: AgentFailureKind::Unavailable,
            message: humanize_unavailable(message),
            auth_methods: None,
        };
    }
    if lower.contains("timed out") || lower.contains("timeout") {
        return ClassifiedFailure {
            kind: AgentFailureKind::Timeout,
            message: message.to_owned(),
            auth_methods: None,
        };
    }
    if lower.contains("disconnected") || lower.contains("connection closed") {
        return ClassifiedFailure {
            kind: AgentFailureKind::AdapterExited,
            message: if message.trim().is_empty() {
                "Agent adapter exited unexpectedly".into()
            } else {
                message.to_owned()
            },
            auth_methods: None,
        };
    }
    ClassifiedFailure {
        kind: AgentFailureKind::Error,
        message: message.to_owned(),
        auth_methods: None,
    }
}

pub fn with_stderr_detail(failure: ClassifiedFailure, stderr_tail: &str) -> ClassifiedFailure {
    let detail = stderr_tail.trim();
    if detail.is_empty() {
        return failure;
    }
    let line = detail
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(detail);
    if failure.message.contains(line) {
        return failure;
    }
    ClassifiedFailure {
        message: format!("{}. Last adapter output: {line}", failure.message.trim_end_matches('.')),
        ..failure
    }
}

fn classify_remote(code: Option<i64>, message: &str, data: Option<&Value>) -> ClassifiedFailure {
    let lower = message.to_ascii_lowercase();
    if looks_like_auth(&lower, data) || code == Some(-32000) && lower.contains("auth") {
        return ClassifiedFailure {
            kind: AgentFailureKind::AuthRequired,
            message: humanize_auth(message),
            auth_methods: data.cloned(),
        };
    }
    if looks_like_unavailable(&lower) {
        return ClassifiedFailure {
            kind: AgentFailureKind::Unavailable,
            message: humanize_unavailable(message),
            auth_methods: None,
        };
    }
    ClassifiedFailure {
        kind: AgentFailureKind::Error,
        message: format!("ACP remote error {code:?}: {message}"),
        auth_methods: None,
    }
}

fn looks_like_auth(lower: &str, data: Option<&Value>) -> bool {
    if lower.contains("auth_required")
        || lower.contains("authentication required")
        || lower.contains("not authenticated")
        || lower.contains("sign in")
        || lower.contains("login required")
    {
        return true;
    }
    match data {
        Some(Value::Object(map)) => map
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|value| {
                let value = value.to_ascii_lowercase();
                value == "agent" || value == "terminal" || value.contains("auth")
            })
            || map.contains_key("authMethods")
            || map.contains_key("auth_methods"),
        Some(Value::Array(items)) => items.iter().any(|item| {
            item.get("type")
                .and_then(Value::as_str)
                .is_some_and(|value| {
                    let value = value.to_ascii_lowercase();
                    value == "agent" || value == "terminal"
                })
        }),
        _ => false,
    }
}

fn looks_like_unavailable(lower: &str) -> bool {
    lower.contains("enoent")
        || lower.contains("no such file")
        || lower.contains("not found")
        || lower.contains("command not found")
        || lower.contains("executable not found")
        || lower.contains("is not installed")
        || lower.contains("no such binary")
        || lower.contains("unable to locate")
        || lower.contains("cannot find")
}

fn humanize_auth(message: &str) -> String {
    if message.to_ascii_lowercase().contains("sign in")
        || message.to_ascii_lowercase().contains("auth")
    {
        if message.chars().count() < 180 {
            return message.to_owned();
        }
    }
    "Sign in required before this agent can run".into()
}

fn humanize_unavailable(message: &str) -> String {
    if message.chars().count() < 180 {
        message.to_owned()
    } else {
        "The underlying agent runtime is not available on this machine".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_auth_required_remote_errors() {
        let failure = classify_acp_failure(&AcpError::Remote {
            code: Some(-32000),
            message: "AUTH_REQUIRED".into(),
            data: Some(json!([{ "id": "login", "type": "agent" }])),
        });
        assert_eq!(failure.kind, AgentFailureKind::AuthRequired);
        assert!(failure.auth_methods.is_some());
    }

    #[test]
    fn classifies_missing_binary_remote_errors() {
        let failure = classify_acp_failure(&AcpError::Remote {
            code: None,
            message: "claude: command not found".into(),
            data: None,
        });
        assert_eq!(failure.kind, AgentFailureKind::Unavailable);
    }

    #[test]
    fn classifies_connection_closed_as_adapter_exit() {
        let failure = classify_acp_failure(&AcpError::ConnectionClosed);
        assert_eq!(failure.kind, AgentFailureKind::AdapterExited);
    }

    #[test]
    fn appends_stderr_detail_once() {
        let failure = with_stderr_detail(
            ClassifiedFailure {
                kind: AgentFailureKind::AdapterExited,
                message: "Agent adapter exited unexpectedly".into(),
                auth_methods: None,
            },
            "fatal: codex binary missing\n",
        );
        assert!(failure.message.contains("codex binary missing"));
    }
}
