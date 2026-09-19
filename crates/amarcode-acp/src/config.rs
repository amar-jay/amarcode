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
        Ok(config)
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
}

fn valid_agent_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}
