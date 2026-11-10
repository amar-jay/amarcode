//! Desktop connection preferences. Credentials never cross back into the webview.
use super::ssh::{self, SshTunnelConfig, SshTunnelManager};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::OnceLock};
use tauri::Manager;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

static SAVE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static CHANGES: OnceLock<tokio::sync::watch::Sender<u64>> = OnceLock::new();

pub fn changes() -> tokio::sync::watch::Receiver<u64> {
    CHANGES
        .get_or_init(|| tokio::sync::watch::channel(0).0)
        .subscribe()
}

static SETTINGS_PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionSettings {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshTunnelConfig>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub remote: bool,
    pub url: String,
    pub has_token: bool,
    pub end_to_end_required: bool,
    pub mode: ConnectionMode,
    pub ssh: Option<SshTunnelConfig>,
    /// Stable host preference scope, independent of the temporary SSH port.
    pub scope: String,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionMode {
    Local,
    Websocket,
    Ssh,
}

impl ConnectionSettings {
    fn info(&self) -> ConnectionInfo {
        let mode = if self.ssh.is_some() {
            ConnectionMode::Ssh
        } else if self.url.is_empty() {
            ConnectionMode::Local
        } else {
            ConnectionMode::Websocket
        };
        let scope = match &self.ssh {
            Some(ssh) => format!(
                "ssh:{}",
                serde_json::to_string(&(&ssh.host, &ssh.user, ssh.port, ssh.daemon_port))
                    .expect("SSH profile identity")
            ),
            None => self.url.clone(),
        };
        ConnectionInfo {
            remote: self.ssh.is_some() || !self.url.is_empty(),
            url: self.url.clone(),
            has_token: !self.token.is_empty(),
            end_to_end_required: std::env::var_os("AMARCODE_E2EE_CA")
                .is_some_and(|value| !value.is_empty()),
            mode,
            ssh: self.ssh.clone(),
            scope,
        }
    }
}

pub fn initialize(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let path = app.path().app_local_data_dir()?.join("connection.json");
    SETTINGS_PATH
        .set(path)
        .map_err(|_| "connection settings already initialized")?;
    Ok(())
}

pub fn load() -> Result<ConnectionSettings, String> {
    let Some(path) = SETTINGS_PATH.get() else {
        return Ok(ConnectionSettings::default());
    };
    read_settings(path)
}

fn read_settings(path: &std::path::Path) -> Result<ConnectionSettings, String> {
    match fs::read(path) {
        Ok(bytes) => {
            let settings: ConnectionSettings = serde_json::from_slice(&bytes)
                .map_err(|_| "Cannot read saved daemon connection settings")?;
            if settings.ssh.is_some() && !settings.url.is_empty() {
                return Err("Saved settings contain conflicting SSH and WebSocket targets".into());
            }
            Ok(settings)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(ConnectionSettings::default())
        }
        Err(_) => Err("Cannot read saved daemon connection settings".into()),
    }
}

pub fn info() -> Result<ConnectionInfo, String> {
    Ok(load()?.info())
}

pub fn is_remote() -> Result<bool, String> {
    Ok(load()?.info().remote)
}

pub fn validate(url: &str, token: &str) -> Result<(), String> {
    let request = url
        .into_client_request()
        .map_err(|_| "Enter a WebSocket URL such as wss://daemon.example.com/rpc")?;
    let uri = request.uri();
    let host = uri.host().unwrap_or_default();
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1");
    if !(uri.scheme_str() == Some("wss") || (uri.scheme_str() == Some("ws") && loopback))
        || uri.path() != "/rpc"
        || uri.query().is_some()
        || uri
            .authority()
            .is_none_or(|authority| authority.as_str().contains('@'))
    {
        return Err("Use wss://host/rpc without credentials or query parameters (ws:// is allowed only on loopback)".into());
    }
    if token.len() < 32
        || token.len() > 512
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
    {
        return Err("Enter the daemon's 32–512 character access token".into());
    }
    Ok(())
}

pub async fn save(url: String, token: String) -> Result<ConnectionInfo, String> {
    save_target(url, None, token).await
}

async fn save_target(
    url: String,
    ssh: Option<SshTunnelConfig>,
    token: String,
) -> Result<ConnectionInfo, String> {
    let _guard = SAVE_LOCK.lock().await;
    let path = SETTINGS_PATH
        .get()
        .ok_or("Connection settings are not initialized")?;
    let changes = CHANGES.get_or_init(|| tokio::sync::watch::channel(0).0);
    save_at(
        path,
        ssh::shared(),
        changes,
        ConnectionSettings { url, token, ssh },
    )
    .await
}

/// Caller serializes saves. Candidate verification never changes persisted state.
pub(super) async fn save_at(
    path: &std::path::Path,
    manager: &SshTunnelManager,
    changes: &tokio::sync::watch::Sender<u64>,
    mut settings: ConnectionSettings,
) -> Result<ConnectionInfo, String> {
    settings.url = settings.url.trim().to_owned();
    settings.token = settings.token.trim().to_owned();
    if settings.ssh.is_some() && !settings.url.is_empty() {
        return Err("Choose either an SSH or WebSocket target".into());
    }
    if settings.ssh.is_some() || !settings.url.is_empty() {
        let previous = read_settings(path)?;
        if settings.token.is_empty() && previous.url == settings.url && previous.ssh == settings.ssh
        {
            settings.token = previous.token;
        }
    } else {
        settings.token.clear();
    }
    let commit = || {
        persist_settings(path, &settings)?;
        changes.send_modify(|generation| *generation = generation.wrapping_add(1));
        Ok(settings.info())
    };
    if let Some(config) = &settings.ssh {
        manager
            .connect_transaction(config, &settings.token, true, commit)
            .await
            .map(|(_, info)| info)
    } else {
        if !settings.url.is_empty() {
            validate(&settings.url, &settings.token)?;
            tokio::time::timeout(
                std::time::Duration::from_secs(30),
                ssh::verify_daemon(&settings.url, &settings.token),
            )
            .await
            .map_err(|_| "Remote daemon health check timed out")?
            .map_err(ssh::ProbeError::message)?;
        }
        manager.disconnect_transaction(commit).await
    }
}

fn persist_settings(path: &std::path::Path, settings: &ConnectionSettings) -> Result<(), String> {
    use std::io::Write;
    let bytes = serde_json::to_vec(settings).map_err(|e| e.to_string())?;
    let parent = path.parent().ok_or("Invalid settings path")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
    let result = (|| {
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&temporary, path).map_err(|e| e.to_string())
    })();
    let _ = fs::remove_file(&temporary);
    result
}

/// Reject an old RPC that loaded its profile before a connection switch.
pub(super) fn ensure_selected(settings: &ConnectionSettings) -> Result<(), String> {
    if load()? == *settings {
        Ok(())
    } else {
        Err("Daemon connection settings changed".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_connection_files_keep_their_original_mode_and_scope() {
        for (json, expected_mode, expected_scope) in [
            (r#"{}"#, "local", ""),
            (
                r#"{"url":"wss://example.com/rpc","token":"secret"}"#,
                "websocket",
                "wss://example.com/rpc",
            ),
        ] {
            let settings: ConnectionSettings = serde_json::from_str(json).unwrap();
            let info = serde_json::to_value(settings.info()).unwrap();
            assert_eq!(info["mode"], expected_mode);
            assert_eq!(info["scope"], expected_scope);
            assert!(info.get("token").is_none());
        }
    }

    #[test]
    fn failed_atomic_replacement_leaves_no_temporary_credential_file() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("connection.json");
        fs::create_dir(&target).unwrap();
        assert!(persist_settings(&target, &ConnectionSettings::default()).is_err());
        assert!(target.is_dir());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn local_mode_can_repair_a_corrupt_settings_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("connection.json");
        fs::write(&path, "invalid settings").unwrap();
        let (changes, receiver) = tokio::sync::watch::channel(0);
        save_at(
            &path,
            &SshTunnelManager::new(),
            &changes,
            ConnectionSettings::default(),
        )
        .await
        .unwrap();
        assert!(read_settings(&path).unwrap().token.is_empty());
        assert_eq!(*receiver.borrow(), 1);
    }

    #[test]
    fn ssh_scope_is_stable_across_key_rotation_and_excludes_credentials() {
        let mut settings = ConnectionSettings {
            ssh: Some(SshTunnelConfig {
                host: "dev-box".into(),
                user: Some("dev".into()),
                port: None,
                identity_file: None,
                daemon_port: 43721,
            }),
            ..Default::default()
        };
        let scope = settings.info().scope;
        settings.token = "private token".into();
        settings.ssh.as_mut().unwrap().identity_file = Some("/new/key".into());
        assert_eq!(settings.info().scope, scope);
        settings.ssh.as_mut().unwrap().host = "different-box".into();
        assert_ne!(settings.info().scope, scope);
    }

    #[test]
    fn remote_urls_require_tls_and_credentials_are_not_in_urls() {
        let token = "a".repeat(64);
        for url in [
            "wss://example.com/rpc",
            "ws://127.0.0.1:43721/rpc",
            "ws://[::1]:43721/rpc",
        ] {
            assert!(validate(url, &token).is_ok(), "{url}");
        }
        for url in [
            "ws://example.com/rpc",
            "https://example.com/rpc",
            "wss://example.com/",
            "https://updates.amarcode.amarjay.com/r/retired",
            "wss://example.com/rpc?token=secret",
            "wss://user:pass@example.com/rpc",
        ] {
            assert!(validate(url, &token).is_err(), "{url}");
        }
        assert!(validate("wss://example.com/rpc", "short").is_err());
    }
}
