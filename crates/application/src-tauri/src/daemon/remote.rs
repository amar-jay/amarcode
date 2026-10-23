//! Desktop connection preferences. Credentials never cross back into the webview.
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

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ConnectionSettings {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub remote: bool,
    pub url: String,
    pub has_token: bool,
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
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "Cannot read saved daemon connection settings".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(ConnectionSettings::default())
        }
        Err(_) => Err("Cannot read saved daemon connection settings".into()),
    }
}

pub fn info() -> Result<ConnectionInfo, String> {
    let settings = load()?;
    Ok(ConnectionInfo {
        remote: !settings.url.is_empty(),
        url: settings.url,
        has_token: !settings.token.is_empty(),
    })
}

pub fn is_remote() -> Result<bool, String> {
    Ok(!load()?.url.is_empty())
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
    let _guard = SAVE_LOCK.lock().await;
    let url = url.trim().to_owned();
    let mut token = token.trim().to_owned();
    if !url.is_empty() {
        let previous = load()?;
        if token.is_empty() && previous.url == url {
            token = previous.token;
        }
        validate(&url, &token)?;
        // Verify authentication and protocol before replacing a working connection.
        let mut connection = super::bridge::Connection::remote(&url, &token).await?;
        let health: amarcode_protocol::rpc::HealthResult = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            connection.call("health", ()),
        )
        .await
        .map_err(|_| "Remote daemon health check timed out")??;
        if !health.remote_auth_required {
            return Err("This daemon does not enforce remote authentication. Install a release with remote support and configure AMARCODE_WS_TOKEN_FILE on its host".into());
        }
        if health.status != "ok" || health.protocol_version != amarcode_protocol::PROTOCOL_VERSION {
            return Err(format!(
                "Remote daemon must use protocol {} (received {})",
                amarcode_protocol::PROTOCOL_VERSION,
                health.protocol_version
            ));
        }
    } else {
        token.clear();
    }
    let settings = ConnectionSettings { url, token };
    let path = SETTINGS_PATH
        .get()
        .ok_or("Connection settings are not initialized")?;
    let parent = path.parent().ok_or("Invalid settings path")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!(
        "{}.tmp",
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
    use std::io::Write;
    let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    file.write_all(&serde_json::to_vec(&settings).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(&temporary, path).map_err(|e| e.to_string())?;
    CHANGES
        .get_or_init(|| tokio::sync::watch::channel(0).0)
        .send_modify(|generation| *generation = generation.wrapping_add(1));
    info()
}

#[cfg(test)]
mod tests {
    use super::*;
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
            "wss://example.com/rpc?token=secret",
            "wss://user:pass@example.com/rpc",
        ] {
            assert!(validate(url, &token).is_err(), "{url}");
        }
        assert!(validate("wss://example.com/rpc", "short").is_err());
    }
}
