//! Built-in Amarcode ACP preset, signed release installation, and private
//! provider configuration.

use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{protocol::rpc::AmarcodeAcpConfigResult, Error, Result};

use super::AgentManager;

pub const AGENT_ID: &str = "amarcode-acp";
const DEFAULT_MODEL: &str = "openrouter/auto";
const DEFAULT_RELEASE_URL: &str = "https://updates.amarcode.amarjay.com/v1/acp/latest.json";
const RELEASE_PUBLIC_KEY_HEX: &str =
    "5ef56cd7772e8c601ca9c5a15378b7088fc558e7edcde73770cbb116d9e255d2";
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseManifest {
    version: String,
    artifacts: HashMap<String, ReleaseArtifact>,
}

#[derive(Debug, Deserialize)]
struct ReleaseArtifact {
    target: String,
    url: String,
    sha256: String,
    size: u64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct ProviderFile {
    name: String,
    provider: ProviderConfig,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct ProviderConfig {
    base_url: String,
    api_key: String,
    model: String,
}

impl AgentManager {
    pub fn amarcode_acp_config(&self) -> Result<AmarcodeAcpConfigResult> {
        let mut provider = read_provider_file(&self.amarcode_acp_config_path())
            .unwrap_or_default()
            .provider;
        if provider.model.trim().is_empty() {
            provider.model = DEFAULT_MODEL.into();
        }
        Ok(config_result(provider))
    }

    pub fn set_amarcode_acp_config(
        &self,
        base_url: String,
        model: String,
        api_key: String,
        clear_api_key: bool,
    ) -> Result<AmarcodeAcpConfigResult> {
        let base_url = base_url.trim().trim_end_matches('/').to_owned();
        let model = model.trim().to_owned();
        if base_url.is_empty() || model.is_empty() {
            return Err(Error::msg("base_url and model must not be empty"));
        }
        let url = reqwest::Url::parse(&base_url)
            .map_err(|error| Error::msg(format!("invalid provider base URL: {error}")))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(Error::msg("provider base URL must be an HTTP(S) URL"));
        }

        let path = self.amarcode_acp_config_path();
        let existing_key = read_provider_file(&path)
            .ok()
            .map(|config| config.provider.api_key)
            .unwrap_or_default();
        let api_key = if clear_api_key {
            String::new()
        } else if api_key.trim().is_empty() {
            existing_key
        } else {
            api_key.trim().to_owned()
        };
        let file = ProviderFile {
            name: AGENT_ID.into(),
            provider: ProviderConfig {
                base_url,
                api_key,
                model,
            },
        };
        write_private_json(&path, &file)?;
        self.refresh_builtin_definition()?;
        Ok(config_result(file.provider))
    }

    pub(crate) fn install_amarcode_acp(&self) -> Result<()> {
        let _install_guard = self.amarcode_acp_install_guard()?;
        let (manifest, source_url) = fetch_verified_manifest()?;
        let target = release_target()?;
        let artifact = manifest.artifacts.get(target).ok_or_else(|| {
            Error::msg(format!("amarcode-acp release has no artifact for {target}"))
        })?;
        validate_artifact(target, artifact)?;
        let executable = self
            .managed_agents_dir()
            .join(AGENT_ID)
            .join(&manifest.version)
            .join(if cfg!(windows) {
                "amarcode-acp.exe"
            } else {
                "amarcode-acp"
            });
        if !verify_file(&executable, artifact)? {
            download_artifact(&source_url, artifact, &executable)?;
        }
        self.save_builtin_definition(&executable)?;
        Ok(())
    }

    pub fn update_amarcode_acp_if_installed(&self) {
        let installed = self
            .get(AGENT_ID)
            .ok()
            .flatten()
            .is_some_and(|agent| Path::new(&agent.command).is_file());
        if installed {
            if let Err(error) = self.install_amarcode_acp() {
                tracing::warn!(%error, "failed updating built-in amarcode-acp; retaining installed release");
            }
        }
    }

    pub(crate) fn amarcode_acp_configured(&self) -> bool {
        self.amarcode_acp_config()
            .map(|config| config.configured)
            .unwrap_or(false)
    }

    pub(crate) fn amarcode_acp_config_path(&self) -> PathBuf {
        self.credentials_dir().join(format!("{AGENT_ID}.json"))
    }
}

fn config_result(provider: ProviderConfig) -> AmarcodeAcpConfigResult {
    let has_api_key = !provider.api_key.trim().is_empty();
    let configured =
        has_api_key && !provider.base_url.trim().is_empty() && !provider.model.trim().is_empty();
    AmarcodeAcpConfigResult {
        base_url: provider.base_url,
        model: provider.model,
        has_api_key,
        configured,
    }
}

fn read_provider_file(path: &Path) -> Result<ProviderFile> {
    let bytes = fs::read(path).map_err(|error| Error::msg(error.to_string()))?;
    serde_json::from_slice(&bytes).map_err(Error::from)
}

pub(crate) fn config_file_is_ready(path: &Path) -> bool {
    read_provider_file(path)
        .map(|file| config_result(file.provider).configured)
        .unwrap_or(false)
}

fn write_private_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::msg("invalid config path"))?;
    fs::create_dir_all(parent).map_err(|error| Error::msg(error.to_string()))?;
    let temporary = path.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(value).map_err(Error::from)?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| Error::msg(format!("failed to write provider configuration: {error}")))?;
    file.write_all(&bytes)
        .map_err(|error| Error::msg(error.to_string()))?;
    file.sync_all()
        .map_err(|error| Error::msg(error.to_string()))?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path).map_err(|error| Error::msg(error.to_string()))?;
    }
    fs::rename(&temporary, path).map_err(|error| Error::msg(error.to_string()))
}

fn fetch_verified_manifest() -> Result<(ReleaseManifest, reqwest::Url)> {
    let url = std::env::var("AMARCODE_ACP_RELEASE_URL")
        .unwrap_or_else(|_| DEFAULT_RELEASE_URL.to_owned());
    let manifest_url = reqwest::Url::parse(&url).map_err(|error| Error::msg(error.to_string()))?;
    let signature_url = reqwest::Url::parse(&format!("{url}.sig"))
        .map_err(|error| Error::msg(error.to_string()))?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10 * 60))
        .build()
        .map_err(|error| Error::msg(error.to_string()))?;
    let bytes = client
        .get(manifest_url.clone())
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| Error::msg(format!("failed to download amarcode-acp manifest: {error}")))?
        .bytes()
        .map_err(|error| Error::msg(error.to_string()))?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(Error::msg("amarcode-acp manifest exceeds the safety limit"));
    }
    let signature = client
        .get(signature_url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| {
            Error::msg(format!(
                "failed to download amarcode-acp signature: {error}"
            ))
        })?
        .text()
        .map_err(|error| Error::msg(error.to_string()))?;
    verify_signature(&bytes, signature.trim())?;
    let manifest = serde_json::from_slice(&bytes)
        .map_err(|error| Error::msg(format!("invalid amarcode-acp manifest: {error}")))?;
    Ok((manifest, manifest_url))
}

fn verify_signature(bytes: &[u8], encoded: &str) -> Result<()> {
    let key: [u8; 32] = hex::decode(RELEASE_PUBLIC_KEY_HEX)
        .map_err(|error| Error::msg(error.to_string()))?
        .try_into()
        .map_err(|_| Error::msg("invalid release public key length"))?;
    let key = VerifyingKey::from_bytes(&key).map_err(|error| Error::msg(error.to_string()))?;
    let signature = BASE64
        .decode(encoded)
        .map_err(|error| Error::msg(format!("invalid release signature encoding: {error}")))?;
    let signature = Signature::from_slice(&signature)
        .map_err(|error| Error::msg(format!("invalid release signature: {error}")))?;
    key.verify(bytes, &signature)
        .map_err(|_| Error::msg("amarcode-acp manifest signature verification failed"))
}

fn validate_artifact(target: &str, artifact: &ReleaseArtifact) -> Result<()> {
    if artifact.target != target
        || artifact.sha256.len() != 64
        || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Error::msg("invalid amarcode-acp release artifact"));
    }
    Ok(())
}

fn verify_file(path: &Path, artifact: &ReleaseArtifact) -> Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    let bytes = fs::read(path).map_err(|error| Error::msg(error.to_string()))?;
    Ok(bytes.len() as u64 == artifact.size
        && sha256(&bytes) == artifact.sha256.to_ascii_lowercase())
}

fn download_artifact(base: &reqwest::Url, artifact: &ReleaseArtifact, path: &Path) -> Result<()> {
    let url = base
        .join(&artifact.url)
        .map_err(|error| Error::msg(error.to_string()))?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10 * 60))
        .build()
        .map_err(|error| Error::msg(error.to_string()))?;
    let bytes = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| Error::msg(format!("failed to download amarcode-acp: {error}")))?
        .bytes()
        .map_err(|error| Error::msg(error.to_string()))?;
    if bytes.len() as u64 != artifact.size || sha256(&bytes) != artifact.sha256.to_ascii_lowercase()
    {
        return Err(Error::msg("amarcode-acp artifact verification failed"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::msg("invalid install path"))?;
    fs::create_dir_all(parent).map_err(|error| Error::msg(error.to_string()))?;
    let temporary = path.with_extension(format!("{}.part", std::process::id()));
    fs::write(&temporary, &bytes).map_err(|error| Error::msg(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o755))
            .map_err(|error| Error::msg(error.to_string()))?;
    }
    if path.exists() {
        fs::remove_file(path).map_err(|error| Error::msg(error.to_string()))?;
    }
    fs::rename(&temporary, path).map_err(|error| Error::msg(error.to_string()))
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn release_target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        _ => Err(Error::msg(
            "amarcode-acp is not published for this platform",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::sync::Arc;

    #[test]
    fn provider_key_is_write_only_and_config_file_is_private() {
        let root =
            std::env::temp_dir().join(format!("amarcode-acp-config-{}", uuid::Uuid::new_v4()));
        let store = Arc::new(Store::open(&root.join("store.sqlite3")).expect("store"));
        let manager = AgentManager::new(store, &root);
        manager.sync_builtin_preset().expect("preset");
        assert_eq!(
            manager.amarcode_acp_config().expect("default config").model,
            DEFAULT_MODEL
        );

        let result = manager
            .set_amarcode_acp_config(
                "https://example.com/v1/".into(),
                "example/model".into(),
                "secret-key".into(),
                false,
            )
            .expect("save config");
        assert_eq!(result.base_url, "https://example.com/v1");
        assert!(result.has_api_key);
        assert!(result.configured);
        assert!(config_file_is_ready(&manager.amarcode_acp_config_path()));
        let serialized = serde_json::to_string(&result).expect("serialize response");
        assert!(!serialized.contains("secret-key"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = manager
                .amarcode_acp_config_path()
                .metadata()
                .expect("config metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }

        let cleared = manager
            .set_amarcode_acp_config(
                "https://example.com/v1".into(),
                "example/model".into(),
                String::new(),
                true,
            )
            .expect("clear key");
        assert!(!cleared.configured);
        assert!(!config_file_is_ready(&manager.amarcode_acp_config_path()));
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
