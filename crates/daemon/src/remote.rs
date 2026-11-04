//! Foreground remote mode, with optional Cloudflare or private loopback access.
use crate::{App, Config, Error, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use std::{env, fs, io::Write, path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

/// Public short-link origin served by the updates Cloudflare Worker.
pub const REMOTE_LINK_ORIGIN: &str = "https://updates.amarcode.amarjay.com";
const REMOTE_LINK_PREFIX: &str = "/r/";
const REMOTE_LINK_MAX_PAYLOAD: usize = 512;

#[derive(Default, Deserialize)]
pub struct Setup {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub listen_addr: Option<String>,
    #[serde(default)]
    pub cloudflared: Option<PathBuf>,
    #[serde(default)]
    pub tool_bin_dirs: Vec<PathBuf>,
}

pub fn instance_root() -> Result<PathBuf> {
    crate::config::resolve()?
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| Error::msg("Cannot locate the daemon instance directory"))
}

/// Load optional access settings for the standard instance before Tokio starts.
/// Legacy explicit roots remain available for deliberately isolated instances.
pub fn configured() -> Result<Option<Setup>> {
    let root = instance_root()?;
    match fs::read(root.join("remote.json")) {
        Ok(bytes) => {
            let setup: Setup = serde_json::from_slice(&bytes)?;
            apply_environment(&root, &setup, false)?;
            if setup.enabled {
                Ok(Some(setup))
            } else {
                Ok(None)
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Persist access settings without starting a second daemon or a tunnel.
pub fn configure_access(enabled: bool) -> Result<()> {
    configure_access_with_notice(enabled, true)
}

fn configure_access_with_notice(enabled: bool, announce: bool) -> Result<()> {
    let root = instance_root()?;
    fs::create_dir_all(&root)?;
    let path = root.join("remote.json");
    let mut settings: serde_json::Value = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(e.into()),
    };
    let object = settings
        .as_object_mut()
        .ok_or_else(|| Error::msg("remote.json must be an object"))?;
    object.insert("enabled".into(), enabled.into());
    if enabled {
        let token = root.join("access-token");
        if !token.exists() {
            use std::io::Write;
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&token)?;
            writeln!(
                file,
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            )?;
            file.sync_all()?;
        }
        crate::rpc::websocket::load_token(&token)?;
    }
    crate::update::releases::atomic_write(&path, &serde_json::to_vec_pretty(&settings)?)
        .map_err(Error::msg)?;
    if announce {
        println!(
            "Remote access {}. Restart the existing daemon to apply.\nAccess token file: {}",
            if enabled { "enabled" } else { "disabled" },
            root.join("access-token").display()
        );
    }
    Ok(())
}

/// Called before Tokio starts. Remote access shares the standard database.
pub fn prepare(require_tunnel: bool) -> Result<Setup> {
    let root = env::var_os("AMARCODE_REMOTE_ROOT")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(instance_root)?;
    let setup: Setup = match fs::read(root.join("remote.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if !require_tunnel && error.kind() == std::io::ErrorKind::NotFound => Setup::default(),
        Err(_) => return Err(Error::msg("Remote setup is missing or unreadable; use an existing remote installation, or --remote --no-tunnel with a configured token file")),
    };
    if require_tunnel
        && setup
            .cloudflared
            .as_ref()
            .is_none_or(|path| !path.is_absolute() || !path.is_file())
    {
        return Err(Error::msg(
            "Installed cloudflared is missing; rerun the Amarcode installer",
        ));
    }
    apply_environment(&root, &setup, env::var_os("AMARCODE_REMOTE_ROOT").is_some())?;
    Ok(setup)
}

fn apply_environment(root: &std::path::Path, setup: &Setup, isolated: bool) -> Result<()> {
    if !setup.tool_bin_dirs.is_empty() {
        let path = env::var_os("PATH").unwrap_or_default();
        let mut directories = Vec::new();
        for directory in &setup.tool_bin_dirs {
            if !directory.is_absolute() || !directory.is_dir() {
                return Err(Error::msg(
                    "Installed agent runtimes are missing; rerun the Amarcode installer",
                ));
            }
            if !directories.contains(directory) {
                directories.push(directory.clone());
            }
        }
        for directory in env::split_paths(&path) {
            if !directories.contains(&directory) {
                directories.push(directory);
            }
        }
        env::set_var(
            "PATH",
            env::join_paths(directories)
                .map_err(|error| Error::msg(format!("Invalid installed runtime PATH: {error}")))?,
        );
    }
    if isolated && env::var_os("AMARCODE_APPDIR").is_none() {
        env::set_var("AMARCODE_APPDIR", root.join("data"));
    }
    if env::var_os("AMARCODE_WS_TOKEN_FILE").is_none() {
        env::set_var("AMARCODE_WS_TOKEN_FILE", root.join("access-token"));
    }
    Ok(())
}

/// Access configuration cloned from the single application's configuration.
pub fn access_config(local: &Config, setup: &Setup) -> Result<Config> {
    let mut remote = local.clone();
    remote.transport = crate::config::DaemonTransport::WebSocket;
    remote.daemon_addr = env::var("AMARCODE_WS_ADDR")
        .ok()
        .or_else(|| setup.listen_addr.clone())
        .unwrap_or_else(|| crate::config::DEFAULT_WS_DAEMON_ADDR.into());
    remote.ws_token_file = env::var_os("AMARCODE_WS_TOKEN_FILE")
        .map(PathBuf::from)
        .or(Some(instance_root()?.join("access-token")));
    remote.ws_tls_cert = env::var_os("AMARCODE_WS_TLS_CERT").map(PathBuf::from);
    remote.ws_tls_key = env::var_os("AMARCODE_WS_TLS_KEY").map(PathBuf::from);
    remote.ws_allowed_origins = env::var("AMARCODE_WS_ALLOW_ORIGIN")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    remote.validate()?;
    Ok(remote)
}

pub async fn run_shared(local: Config, setup: Setup, no_tunnel: bool) -> Result<()> {
    let remote = access_config(&local, &setup)?;
    validate(&remote, no_tunnel)?;
    let listener = crate::rpc::websocket::Listener::bind(&remote).await?;
    let app = App::new(local).await?;
    let (ready, listening) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(app.run_with_access(listener, ready));
    if listening.await.is_err() {
        return server.await.map_err(|e| Error::msg(e.to_string()))?;
    }
    finish(remote, setup, no_tunnel, server).await
}

/// Attach Cloudflare to a live authenticated listener; never opens a database.
pub async fn tunnel(local: Config, setup: Setup) -> Result<()> {
    let remote = access_config(&local, &setup)?;
    validate(&remote, false)?;
    check_health(&remote).await?;
    finish(remote, setup, false, tokio::spawn(std::future::pending())).await
}

async fn check_health(remote: &Config) -> Result<()> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
    let token = crate::rpc::websocket::load_token(
        remote
            .ws_token_file
            .as_deref()
            .ok_or_else(|| Error::msg("Remote token file missing"))?,
    )?;
    let mut request = format!("ws://{}/rpc", remote.daemon_addr)
        .into_client_request()
        .map_err(|e| Error::msg(e.to_string()))?;
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|_| Error::msg("Invalid remote token"))?,
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let (mut socket, _) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|e| {
                Error::msg(format!(
                    "Start or restart the existing daemon before opening a tunnel: {e}"
                ))
            })?;
        socket
            .send(Message::Text(
                serde_json::json!({"method":"health"}).to_string().into(),
            ))
            .await
            .map_err(|e| Error::msg(e.to_string()))?;
        let response = socket
            .next()
            .await
            .ok_or_else(|| Error::msg("Daemon closed the health connection"))?
            .map_err(|e| Error::msg(e.to_string()))?;
        let reply: serde_json::Value =
            serde_json::from_str(response.to_text().map_err(|e| Error::msg(e.to_string()))?)?;
        if reply["result"]["remote_auth_required"] != true || reply["result"]["status"] != "ok" {
            return Err(Error::msg(
                "Listener is not an authenticated Amarcode daemon",
            ));
        }
        if reply["result"]["protocol_version"] != amarcode_protocol::PROTOCOL_VERSION {
            return Err(Error::msg(
                "The running daemon has an incompatible protocol version",
            ));
        }
        socket
            .close(None)
            .await
            .map_err(|e| Error::msg(e.to_string()))?;
        Ok(())
    })
    .await
    .map_err(|_| Error::msg("Daemon health check timed out"))??;
    Ok(())
}

fn announce_loopback(config: &Config) -> Result<()> {
    let token_path = config
        .ws_token_file
        .as_deref()
        .ok_or_else(|| Error::msg("Remote access token is not configured"))?;
    println!(
        "Authenticated daemon listening on {}\nAccess token file: {}",
        config.daemon_addr,
        token_path.display()
    );
    Ok(())
}

pub fn validate(config: &Config, no_tunnel: bool) -> Result<()> {
    let address: std::net::SocketAddr = config
        .daemon_addr
        .parse()
        .map_err(|_| Error::msg("Invalid remote daemon address"))?;
    if config.transport != crate::config::DaemonTransport::WebSocket
        || !address.ip().is_loopback()
        || address.port() == 0
        || config.ws_tls_cert.is_some()
        || config.ws_tls_key.is_some()
    {
        return Err(Error::msg("Remote mode requires a loopback WebSocket listener with a fixed port and no origin TLS; SSH or Cloudflare provides transport encryption"));
    }
    if no_tunnel && address.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) {
        return Err(Error::msg(
            "--remote --no-tunnel requires 127.0.0.1 for private loopback access",
        ));
    }
    Ok(())
}

fn tunnel_url(line: &str) -> Option<String> {
    line.split_whitespace().find_map(|word| {
        let word = word.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | '|' | '(' | ')'));
        let url = reqwest::Url::parse(word).ok()?;
        (url.scheme() == "https"
            && url.host_str()?.ends_with(".trycloudflare.com")
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none())
        .then(|| format!("wss://{}/rpc", url.host_str().unwrap()))
    })
}

fn tunnel_target(url: &str) -> Option<&str> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let label = parsed.host_str()?.strip_suffix(".trycloudflare.com")?;
    (parsed.as_str() == url
        && url.len() <= 256
        && parsed.scheme() == "wss"
        && !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && parsed.port().is_none()
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.path() == "/rpc"
        && parsed.query().is_none()
        && parsed.fragment().is_none())
    .then_some(url)
}

/// Brotli-compress then base64url-encode a Cloudflare quick-tunnel WebSocket URL.
pub fn encode_remote_link(wss_url: &str) -> Option<String> {
    tunnel_target(wss_url)?;
    let mut compressed = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 5, 22);
        writer.write_all(wss_url.as_bytes()).ok()?;
        writer.flush().ok()?;
    }
    Some(URL_SAFE_NO_PAD.encode(compressed))
}

/// Decode a short-link payload back to a validated `wss://*.trycloudflare.com/rpc` URL.
pub fn decode_remote_link(payload: &str) -> Option<String> {
    if payload.is_empty()
        || payload.len() > REMOTE_LINK_MAX_PAYLOAD
        || !payload
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let compressed = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let mut plain = Vec::new();
    let mut reader =
        std::io::Read::take(brotli::Decompressor::new(compressed.as_slice(), 4096), 257);
    std::io::copy(&mut reader, &mut plain).ok()?;
    if plain.len() > 256 {
        return None;
    }
    let url = String::from_utf8(plain).ok()?;
    tunnel_target(&url).map(str::to_owned)
}

/// Resolve branded links locally; no credentials or network requests are needed.
pub fn resolve_remote_url(value: &str) -> Result<String> {
    let value = value.trim();
    let prefix = format!("{REMOTE_LINK_ORIGIN}{REMOTE_LINK_PREFIX}");
    match value.strip_prefix(&prefix) {
        Some(payload) => {
            decode_remote_link(payload).ok_or_else(|| Error::msg("Invalid remote link"))
        }
        None => Ok(value.to_owned()),
    }
}

/// Brand the tunnel URL as `https://updates…/r/<payload>` for sharing and paste-to-connect.
pub fn short_remote_url(wss_url: &str) -> Option<String> {
    Some(format!(
        "{REMOTE_LINK_ORIGIN}{REMOTE_LINK_PREFIX}{}",
        encode_remote_link(wss_url)?
    ))
}

pub async fn run(config: Config, setup: Setup, no_tunnel: bool) -> Result<()> {
    validate(&config, no_tunnel)?;
    crate::rpc::websocket::load_token(
        config
            .ws_token_file
            .as_deref()
            .ok_or_else(|| Error::msg("Remote access token file is not configured"))?,
    )?;
    let app = App::new(config.clone()).await?;
    let (ready, listening) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(app.run_with_ready(ready));
    if listening.await.is_err() {
        return server.await.map_err(|e| Error::msg(e.to_string()))?;
    }
    finish(config, setup, no_tunnel, server).await
}

async fn finish(
    config: Config,
    setup: Setup,
    no_tunnel: bool,
    mut server: tokio::task::JoinHandle<Result<()>>,
) -> Result<()> {
    let cloudflared = if no_tunnel {
        None
    } else {
        Some(
            setup
                .cloudflared
                .as_deref()
                .ok_or_else(|| Error::msg("Installed cloudflared is missing"))?,
        )
    };
    let token_path = config
        .ws_token_file
        .as_deref()
        .ok_or_else(|| Error::msg("Remote access token file is not configured"))?;
    let token = crate::rpc::websocket::load_token(token_path)?;
    let address = config.daemon_addr.clone();
    let Some(cloudflared) = cloudflared else {
        announce_loopback(&config)?;
        return server
            .await
            .map_err(|error| Error::msg(error.to_string()))?;
    };
    let mut tunnel = match Command::new(cloudflared)
        .args(["tunnel", "--url", &format!("http://{address}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(tunnel) => tunnel,
        Err(error) => {
            server.abort();
            let _ = server.await;
            return Err(Error::msg(format!("Cannot start cloudflared: {error}")));
        }
    };
    let stderr = tunnel.stderr.take().expect("piped cloudflared stderr");
    let output = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut announced = false;
        while let Ok(Some(line)) = lines.next_line().await {
            if !announced {
                if let Some(url) = tunnel_url(&line) {
                    let share = short_remote_url(&url).unwrap_or(url);
                    println!("Remote URL: {share}\nAccess token: {token}");
                    announced = true;
                }
            }
        }
    });
    let outcome = tokio::select! {
        biased;
        _ = crate::rpc::server::shutdown_signal() => Ok(()),
        result = &mut server => result.unwrap_or_else(|e| Err(Error::msg(e.to_string()))),
        status = tunnel.wait() => Err(Error::msg(match status {
            Ok(status) => format!("Cloudflare tunnel exited ({status}); remote daemon stopped. Run amarcode-daemon --remote to reconnect"),
            Err(error) => format!("Cloudflare tunnel failed: {error}"),
        })),
    };
    // Never leave a public tunnel pointing to another process after this daemon exits.
    let _ = tunnel.kill().await;
    if !server.is_finished() {
        server.abort();
        let _ = server.await;
    }
    output.abort();
    let _ = output.await;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_only_cloudflare_quick_tunnel_origins() {
        assert_eq!(
            tunnel_url("| https://test-project.trycloudflare.com |"),
            Some("wss://test-project.trycloudflare.com/rpc".into())
        );
        for line in [
            "https://evil.example",
            "https://trycloudflare.com.evil.example",
            "http://x.trycloudflare.com",
            "https://x.trycloudflare.com/path",
            "https://user@x.trycloudflare.com",
        ] {
            assert_eq!(tunnel_url(line), None);
        }
    }

    #[test]
    fn short_links_round_trip_only_cloudflare_tunnel_targets() {
        let target = "wss://test-project.trycloudflare.com/rpc";
        let short = short_remote_url(target).unwrap();
        assert!(short.starts_with(&format!("{REMOTE_LINK_ORIGIN}{REMOTE_LINK_PREFIX}")));
        let payload = short
            .strip_prefix(&format!("{REMOTE_LINK_ORIGIN}{REMOTE_LINK_PREFIX}"))
            .unwrap();
        assert_eq!(decode_remote_link(payload).as_deref(), Some(target));
        assert!(encode_remote_link("wss://evil.example/rpc").is_none());
        assert!(decode_remote_link("!!!!").is_none());
        assert!(decode_remote_link("").is_none());
        // Fixture produced by Node's Brotli encoder, not this Rust implementation.
        assert_eq!(
            decode_remote_link("GyQA-EVveF74fZ_kozRKWpNRSuFFluJLSL8ZFamnLl78dZiGMGgH").as_deref(),
            Some("wss://my-remote.trycloudflare.com/rpc")
        );
        for target in [
            "a".repeat(100_000),
            "wss://nested.host.trycloudflare.com/rpc".into(),
            "wss://host.trycloudflare.com:8443/rpc".into(),
            "wss://user:secret@host.trycloudflare.com/rpc".into(),
            "wss://host.trycloudflare.com/rpc?secret=x".into(),
        ] {
            let mut compressed = Vec::new();
            {
                let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 5, 22);
                writer.write_all(target.as_bytes()).unwrap();
            }
            assert!(decode_remote_link(&URL_SAFE_NO_PAD.encode(compressed)).is_none());
        }
    }
}
