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

use crate::{
    protocol::rpc::{
        AcpProfileAction, AmarcodeAcpConfigResult, AmarcodeAcpProfile, SetAmarcodeAcpConfigParams,
    },
    Error, Result,
};

use super::AgentManager;

pub const AGENT_ID: &str = "amarcode-acp";
const DEFAULT_MODEL: &str = "default";
const FREE_URL: &str = "https://free.amarcode.amarjay.com";
const FREE_PROFILE: &str = "amarcode-free";
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
    #[serde(default)]
    active_profile_id: String,
    #[serde(default)]
    profiles: Vec<SavedProfile>,
    // Retain adapter options (persistence, context, reliability, etc.) when editing providers.
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SavedProfile {
    id: String,
    name: String,
    provider: ProviderConfig,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct ProviderConfig {
    base_url: String,
    #[serde(default)]
    api_key: String,
    model: String,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

fn free_provider() -> ProviderConfig {
    ProviderConfig {
        base_url: FREE_URL.into(),
        model: DEFAULT_MODEL.into(),
        ..Default::default()
    }
}

fn is_free(provider: &ProviderConfig) -> bool {
    matches!(
        provider.base_url.trim_end_matches('/'),
        FREE_URL | "https://free.amarcode.amarjay.com/v1"
    )
}

impl AgentManager {
    pub fn amarcode_acp_config(&self) -> Result<AmarcodeAcpConfigResult> {
        let _guard = self
            .acp_provider_lock
            .lock()
            .map_err(|_| Error::msg("provider configuration lock poisoned"))?;
        let file = self.load_provider_profiles()?;
        Ok(profiles_result(&file))
    }

    // Migrate the previous single-provider file without losing its credentials.
    fn load_provider_profiles(&self) -> Result<ProviderFile> {
        let path = self.amarcode_acp_config_path();
        let mut file = if path.exists() {
            read_provider_file(&path)?
        } else {
            ProviderFile {
                name: AGENT_ID.into(),
                provider: free_provider(),
                ..Default::default()
            }
        };
        if file.active_profile_id.is_empty() {
            if !file.provider.base_url.trim().is_empty() && !is_free(&file.provider) {
                file.active_profile_id = uuid::Uuid::new_v4().to_string();
                file.profiles.push(SavedProfile {
                    id: file.active_profile_id.clone(),
                    name: "My provider".into(),
                    provider: file.provider.clone(),
                });
            } else {
                file.active_profile_id = FREE_PROFILE.into();
                file.provider = free_provider();
            }
            write_private_json(&path, &file)?;
        }
        Ok(file)
    }

    pub fn set_amarcode_acp_config(
        &self,
        base_url: String,
        model: String,
        api_key: String,
        clear_api_key: bool,
    ) -> Result<AmarcodeAcpConfigResult> {
        self.update_amarcode_acp_config(SetAmarcodeAcpConfigParams {
            base_url,
            model,
            api_key,
            clear_api_key,
            ..Default::default()
        })
    }

    pub fn update_amarcode_acp_config(
        &self,
        params: SetAmarcodeAcpConfigParams,
    ) -> Result<AmarcodeAcpConfigResult> {
        let _guard = self
            .acp_provider_lock
            .lock()
            .map_err(|_| Error::msg("provider configuration lock poisoned"))?;
        let mut file = self.load_provider_profiles()?;
        let id = params
            .profile_id
            .as_deref()
            .unwrap_or(&file.active_profile_id)
            .to_owned();
        match params.action {
            AcpProfileAction::Reset => file.active_profile_id = FREE_PROFILE.into(),
            AcpProfileAction::Activate => {
                if id != FREE_PROFILE && !file.profiles.iter().any(|p| p.id == id) {
                    return Err(Error::msg("provider profile not found"));
                }
                file.active_profile_id = id;
            }
            AcpProfileAction::Delete => {
                let index = file
                    .profiles
                    .iter()
                    .position(|p| p.id == id)
                    .ok_or_else(|| Error::msg("provider profile not found"))?;
                file.profiles.remove(index);
                if file.active_profile_id == id {
                    file.active_profile_id = FREE_PROFILE.into();
                }
            }
            AcpProfileAction::Save | AcpProfileAction::Create => {
                let base_url = params.base_url.trim().trim_end_matches('/').to_owned();
                let model = params.model.trim().to_owned();
                let url = reqwest::Url::parse(&base_url)
                    .map_err(|error| Error::msg(format!("invalid provider base URL: {error}")))?;
                if !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
                    || model.is_empty()
                {
                    return Err(Error::msg("use an HTTP(S) base URL without credentials, query or fragment, and a model ID"));
                }
                if matches!(
                    base_url.as_str(),
                    FREE_URL | "https://free.amarcode.amarjay.com/v1"
                ) {
                    return Err(Error::msg("Choose Use default for Amarcode Free"));
                }
                let create =
                    matches!(params.action, AcpProfileAction::Create) || id == FREE_PROFILE;
                let index = if create {
                    None
                } else {
                    Some(
                        file.profiles
                            .iter()
                            .position(|p| p.id == id)
                            .ok_or_else(|| Error::msg("provider profile not found"))?,
                    )
                };
                let previous = index.map(|i| &file.profiles[i]);
                // A blank field only retains the key for this profile at the same URL.
                let api_key = if params.clear_api_key {
                    String::new()
                } else if !params.api_key.trim().is_empty() {
                    params.api_key.trim().to_owned()
                } else {
                    previous
                        .filter(|p| p.provider.base_url == base_url)
                        .map(|p| p.provider.api_key.clone())
                        .unwrap_or_default()
                };
                let name = params
                    .name
                    .as_deref()
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .or_else(|| previous.map(|p| p.name.as_str()))
                    .unwrap_or("My provider")
                    .to_owned();
                let profile = SavedProfile {
                    id: if create {
                        uuid::Uuid::new_v4().to_string()
                    } else {
                        id
                    },
                    name,
                    provider: ProviderConfig {
                        base_url,
                        model,
                        api_key,
                        extra: previous
                            .map(|p| p.provider.extra.clone())
                            .unwrap_or_default(),
                    },
                };
                file.active_profile_id = profile.id.clone();
                if let Some(index) = index {
                    file.profiles[index] = profile;
                } else {
                    file.profiles.push(profile);
                }
            }
        }
        file.provider = if file.active_profile_id == FREE_PROFILE {
            free_provider()
        } else {
            file.profiles
                .iter()
                .find(|p| p.id == file.active_profile_id)
                .ok_or_else(|| Error::msg("active provider profile not found"))?
                .provider
                .clone()
        };
        write_private_json(&self.amarcode_acp_config_path(), &file)?;
        self.refresh_builtin_definition()?;
        Ok(profiles_result(&file))
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

    pub(crate) fn amarcode_acp_config_path(&self) -> PathBuf {
        self.credentials_dir().join(format!("{AGENT_ID}.json"))
    }
}

fn config_result(provider: ProviderConfig) -> AmarcodeAcpConfigResult {
    let has_api_key = !provider.api_key.trim().is_empty();
    let free = is_free(&provider);
    let configured = (free || has_api_key)
        && !provider.base_url.trim().is_empty()
        && !provider.model.trim().is_empty();
    AmarcodeAcpConfigResult {
        base_url: provider.base_url,
        model: provider.model,
        has_api_key,
        configured,
        active_profile_id: String::new(),
        profiles: vec![],
    }
}

fn profile_result(id: String, name: String, provider: &ProviderConfig) -> AmarcodeAcpProfile {
    AmarcodeAcpProfile {
        id,
        name,
        base_url: provider.base_url.clone(),
        model: provider.model.clone(),
        has_api_key: !provider.api_key.trim().is_empty(),
    }
}

fn profiles_result(file: &ProviderFile) -> AmarcodeAcpConfigResult {
    let mut result = config_result(file.provider.clone());
    result.active_profile_id = file.active_profile_id.clone();
    result.profiles.push(profile_result(
        FREE_PROFILE.into(),
        "Amarcode Free (default)".into(),
        &free_provider(),
    ));
    result.profiles.extend(
        file.profiles
            .iter()
            .map(|p| profile_result(p.id.clone(), p.name.clone(), &p.provider)),
    );
    result
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

    fn active_key(manager: &AgentManager) -> String {
        read_provider_file(&manager.amarcode_acp_config_path())
            .unwrap()
            .provider
            .api_key
    }

    fn manager(root: &Path) -> AgentManager {
        AgentManager::new(
            Arc::new(Store::open(&root.join("store.sqlite3")).unwrap()),
            root,
        )
    }

    #[test]
    fn fresh_default_is_keyless() {
        let root = tempfile::tempdir().unwrap();
        let manager = manager(root.path());
        manager.sync_builtin_preset().unwrap();
        let config = manager.amarcode_acp_config().unwrap();
        assert_eq!(config.base_url, FREE_URL);
        assert_eq!(config.active_profile_id, FREE_PROFILE);
        assert!(config.configured);
        assert!(!config.has_api_key);
        assert!(config_file_is_ready(&manager.amarcode_acp_config_path()));
    }

    #[test]
    fn profiles_preserve_separate_keys_and_reset_does_not_delete_them() {
        let root = tempfile::tempdir().unwrap();
        let manager = manager(root.path());
        let first = manager
            .set_amarcode_acp_config(
                "https://first.example/v1".into(),
                "one".into(),
                "key-one".into(),
                false,
            )
            .unwrap();
        let second = manager
            .update_amarcode_acp_config(SetAmarcodeAcpConfigParams {
                action: AcpProfileAction::Create,
                name: Some("Second".into()),
                base_url: "https://second.example/v1".into(),
                model: "two".into(),
                api_key: "key-two".into(),
                ..Default::default()
            })
            .unwrap();
        assert_ne!(first.active_profile_id, second.active_profile_id);
        assert_eq!(second.profiles.len(), 3);
        assert!(!serde_json::to_string(&second).unwrap().contains("key-two"));
        let reset = manager
            .update_amarcode_acp_config(SetAmarcodeAcpConfigParams {
                action: AcpProfileAction::Reset,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(reset.base_url, FREE_URL);
        assert!(!reset.has_api_key);
        assert_eq!(reset.profiles.len(), 3);
        assert!(read_provider_file(&manager.amarcode_acp_config_path())
            .unwrap()
            .provider
            .api_key
            .is_empty());
        manager
            .update_amarcode_acp_config(SetAmarcodeAcpConfigParams {
                action: AcpProfileAction::Activate,
                profile_id: Some(first.active_profile_id.clone()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(active_key(&manager), "key-one");
        // Blank key preserves only the same profile's same-endpoint key.
        manager
            .set_amarcode_acp_config(
                "https://first.example/v1".into(),
                "one-new".into(),
                String::new(),
                false,
            )
            .unwrap();
        assert_eq!(active_key(&manager), "key-one");
        let changed = manager
            .set_amarcode_acp_config(
                "https://different.example/v1".into(),
                "one".into(),
                String::new(),
                false,
            )
            .unwrap();
        assert!(!changed.has_api_key);
        assert!(!changed.configured);
        let deleted = manager
            .update_amarcode_acp_config(SetAmarcodeAcpConfigParams {
                action: AcpProfileAction::Delete,
                profile_id: Some(first.active_profile_id),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(deleted.active_profile_id, FREE_PROFILE);
        assert_eq!(deleted.profiles.len(), 2);
        manager
            .update_amarcode_acp_config(SetAmarcodeAcpConfigParams {
                action: AcpProfileAction::Activate,
                profile_id: Some(second.active_profile_id),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(active_key(&manager), "key-two");
        // Survives daemon restart, never exposed in the response.
        let reopened = super::tests::manager(root.path());
        assert_eq!(active_key(&reopened), "key-two");
    }

    #[test]
    fn legacy_provider_migration_keeps_settings_and_malformed_files_are_not_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let manager = manager(root.path());
        let path = manager.amarcode_acp_config_path();
        write_private_json(
            &path,
            &serde_json::json!({
                "name":AGENT_ID,
                "provider":{"base_url":"https://legacy.example/v1","model":"legacy",
                    "api_key":"legacy-key","reasoning":{"enabled":false}},
                "agent":{"max_tool_rounds":8}
            }),
        )
        .unwrap();
        let result = manager.amarcode_acp_config().unwrap();
        assert_eq!(result.base_url, "https://legacy.example/v1");
        assert_eq!(result.profiles.len(), 2);
        assert_eq!(active_key(&manager), "legacy-key");
        manager
            .update_amarcode_acp_config(SetAmarcodeAcpConfigParams {
                action: AcpProfileAction::Reset,
                ..Default::default()
            })
            .unwrap();
        let file = read_provider_file(&path).unwrap();
        assert_eq!(file.extra["agent"]["max_tool_rounds"], 8);
        assert_eq!(
            file.profiles[0].provider.extra["reasoning"]["enabled"],
            false
        );
        fs::write(&path, b"invalid-json").unwrap();
        assert!(manager.amarcode_acp_config().is_err());
        assert!(manager.sync_builtin_preset().is_ok());
        assert_eq!(fs::read(&path).unwrap(), b"invalid-json");
    }

    #[test]
    fn unknown_profile_operations_leave_configuration_unchanged() {
        let root = tempfile::tempdir().unwrap();
        let manager = manager(root.path());
        manager.sync_builtin_preset().unwrap();
        let path = manager.amarcode_acp_config_path();
        let before = fs::read(&path).unwrap();
        for action in [
            AcpProfileAction::Activate,
            AcpProfileAction::Delete,
            AcpProfileAction::Save,
        ] {
            assert!(manager
                .update_amarcode_acp_config(SetAmarcodeAcpConfigParams {
                    action,
                    profile_id: Some("missing".into()),
                    base_url: "https://example.com/v1".into(),
                    model: "model".into(),
                    ..Default::default()
                })
                .is_err());
            assert_eq!(fs::read(&path).unwrap(), before);
        }
    }

    #[test]
    fn provider_key_is_write_only_and_config_file_is_private() {
        let root =
            std::env::temp_dir().join(format!("amarcode-acp-config-{}", uuid::Uuid::new_v4()));
        {
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
        }
        // Drop Store before deleting: Windows keeps the SQLite file locked while open.
        let _ = std::fs::remove_dir_all(root);
    }
}
