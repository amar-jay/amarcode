use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

/// Process-level configuration loaded before the ACP transport starts.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub name: String,
    pub provider: ProviderConfig,
    #[serde(default)]
    pub persistence: PersistenceConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub reliability: ReliabilityConfig,
    #[serde(default)]
    pub context: ContextConfig,
    #[serde(skip)]
    pub(crate) source_path: PathBuf,
}

impl Config {
    pub fn title(&self) -> String {
        self.name
            .split(['.', '_', '-'])
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                chars.next().map_or_else(String::new, |first| {
                    let mut word = first.to_ascii_uppercase().to_string();
                    word.extend(chars);
                    word
                })
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn from_file(path: &Path) -> Result<Self, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let mut config: Self = serde_json::from_str(&contents)
            .map_err(|error| format!("invalid JSON in {}: {error}", path.display()))?;
        config.name = config.name.trim().to_owned();
        config.source_path = path.to_owned();
        config.provider.base_url = config.provider.base_url.trim_end_matches('/').to_owned();
        if !valid_agent_name(&config.name) {
            return Err("name must start with an ASCII lowercase letter or digit and contain only lowercase letters, digits, '.', '_', or '-'".into());
        }
        if config.provider.base_url.is_empty()
            || config.provider.api_key.trim().is_empty()
            || config.provider.model.trim().is_empty()
        {
            return Err(
                "provider.base_url, provider.api_key, and provider.model must not be empty".into(),
            );
        }
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if self.agent.max_tool_rounds == 0
            || self.agent.max_tool_calls == 0
            || self.agent.max_turn_tokens == 0
            || self.agent.max_turn_seconds == 0
            || self.agent.repeated_tool_limit < 2
        {
            return Err(
                "agent limits must be positive; repeated_tool_limit must be at least 2".into(),
            );
        }
        if self
            .agent
            .max_cost_usd
            .is_some_and(|cost| !cost.is_finite() || cost <= 0.0)
        {
            return Err("agent.max_cost_usd must be finite and positive".into());
        }
        if self.reliability.connect_timeout_seconds == 0
            || self.reliability.request_timeout_seconds == 0
            || self.reliability.stream_idle_timeout_seconds == 0
            || self.reliability.retry_base_ms == 0
            || self.reliability.retry_max_ms == 0
            || self.reliability.max_retries > 10
        {
            return Err(
                "reliability timeouts/backoff must be positive; max_retries must be at most 10"
                    .into(),
            );
        }
        if self.context.context_length.is_some_and(|size| size < 1024)
            || self.context.max_tool_result_bytes < 256
            || self.context.max_workspace_bytes < 256
            || self.context.summary_tokens < 128
        {
            return Err("context window must be at least 1024 tokens, byte limits at least 256, and summary_tokens at least 128".into());
        }
        Ok(())
    }

    pub fn session_store_path(&self) -> PathBuf {
        let configured = self.persistence.path.as_deref();
        match configured {
            Some(path) if path.is_absolute() => path.to_owned(),
            Some(path) => self
                .source_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(path),
            None => self.source_path.with_extension("sessions.json"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub max_tool_rounds: usize,
    pub max_tool_calls: usize,
    pub max_turn_tokens: u64,
    pub max_turn_seconds: u64,
    pub repeated_tool_limit: usize,
    /// Enforced between requests only when the provider reports USD cost.
    pub max_cost_usd: Option<f64>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_tool_rounds: 30,
            max_tool_calls: 120,
            max_turn_tokens: 200_000,
            max_turn_seconds: 900,
            repeated_tool_limit: 3,
            max_cost_usd: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ReliabilityConfig {
    pub connect_timeout_seconds: u64,
    pub request_timeout_seconds: u64,
    pub stream_idle_timeout_seconds: u64,
    pub max_retries: usize,
    pub retry_base_ms: u64,
    pub retry_max_ms: u64,
}

impl Default for ReliabilityConfig {
    fn default() -> Self {
        Self {
            connect_timeout_seconds: 10,
            request_timeout_seconds: 180,
            stream_idle_timeout_seconds: 45,
            max_retries: 3,
            retry_base_ms: 250,
            retry_max_ms: 10_000,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ContextConfig {
    /// Override discovery, including for providers without model discovery.
    pub context_length: Option<u64>,
    pub max_tool_result_bytes: usize,
    pub max_workspace_bytes: usize,
    pub summary_tokens: u64,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            context_length: None,
            max_tool_result_bytes: 16_384,
            max_workspace_bytes: 24_576,
            summary_tokens: 1024,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PersistenceConfig {
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default = "default_ttl_seconds")]
    pub ttl_seconds: u64,
    #[serde(default)]
    pub path: Option<PathBuf>,
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ttl_seconds: default_ttl_seconds(),
            path: None,
        }
    }
}

fn enabled_by_default() -> bool {
    true
}

fn default_ttl_seconds() -> u64 {
    24 * 60 * 60
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    #[serde(alias = "baseUrl")]
    pub base_url: String,
    #[serde(alias = "apiKey")]
    pub api_key: String,
    pub model: String,
    /// Optional OpenAI-compatible reasoning request configuration.
    #[serde(default)]
    pub reasoning: Option<Value>,
}

impl ProviderConfig {
    pub fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    pub fn models_endpoint(&self) -> String {
        format!("{}/models", self.base_url)
    }

    pub fn is_openrouter(&self) -> bool {
        reqwest::Url::parse(&self.base_url)
            .ok()
            .is_some_and(|url| url.host_str() == Some("openrouter.ai"))
    }
}

fn valid_agent_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(options: Value) -> Config {
        let mut value = json!({"name":"test", "provider":{"base_url":"https://example.test/v1","api_key":"test","model":"test"}});
        for (key, value_option) in options.as_object().unwrap() {
            value[key] = value_option.clone();
        }
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn old_configs_receive_defaults_and_partial_overrides_work() {
        let default = config(json!({}));
        assert!(default.validate().is_ok());
        assert_eq!(default.agent.max_tool_rounds, 30);
        let custom = config(
            json!({"agent":{"max_tool_rounds":8},"reliability":{"max_retries":0},"context":{"context_length":8192}}),
        );
        assert!(custom.validate().is_ok());
        assert_eq!(custom.agent.max_tool_rounds, 8);
        assert_eq!(custom.agent.max_tool_calls, 120);
        assert_eq!(custom.reliability.max_retries, 0);
    }

    #[test]
    fn invalid_limits_are_rejected_before_transport_starts() {
        for options in [
            json!({"agent":{"max_tool_rounds":0}}),
            json!({"agent":{"repeated_tool_limit":1}}),
            json!({"agent":{"max_cost_usd":-1}}),
            json!({"reliability":{"max_retries":11}}),
            json!({"reliability":{"request_timeout_seconds":0}}),
            json!({"context":{"context_length":1}}),
        ] {
            assert!(config(options).validate().is_err());
        }
    }
}
