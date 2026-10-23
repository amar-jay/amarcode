//! Foreground remote mode: one command owns the daemon and its Cloudflare tunnel.
use crate::{App, Config, Error, Result};
use serde::Deserialize;
use std::{env, fs, path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

#[derive(Deserialize)]
pub struct Setup {
    pub cloudflared: PathBuf,
}

/// Called before Tokio starts: the installed remote defaults never affect local runs.
pub fn prepare() -> Result<Setup> {
    let root = env::var_os("AMARCODE_REMOTE_ROOT")
        .map(PathBuf::from)
        .or_else(|| dirs::data_local_dir().map(|path| path.join("amarcode-remote")))
        .ok_or_else(|| Error::msg("Cannot locate the remote installation directory"))?;
    let setup: Setup =
        serde_json::from_slice(&fs::read(root.join("remote.json")).map_err(|_| {
            Error::msg(
                "Remote setup is missing; run the Amarcode installer once before using --remote",
            )
        })?)?;
    if !setup.cloudflared.is_absolute() || !setup.cloudflared.is_file() {
        return Err(Error::msg(
            "Installed cloudflared is missing; rerun the Amarcode installer",
        ));
    }
    for (key, value) in [
        ("AMARCODE_APPDIR", root.join("data").into_os_string()),
        (
            "AMARCODE_WS_TOKEN_FILE",
            root.join("access-token").into_os_string(),
        ),
    ] {
        if env::var_os(key).is_none() {
            env::set_var(key, value);
        }
    }
    Ok(setup)
}

pub fn validate(config: &Config) -> Result<()> {
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
        return Err(Error::msg("--remote requires a loopback WebSocket listener with a fixed port and no origin TLS; Cloudflare provides public TLS"));
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

pub async fn run(config: Config, setup: Setup) -> Result<()> {
    validate(&config)?;
    let address = config.daemon_addr.clone();
    let app = App::new(config).await?;
    let (ready, listening) = tokio::sync::oneshot::channel();
    let mut server = tokio::spawn(app.run_with_ready(ready));
    if listening.await.is_err() {
        return server.await.map_err(|e| Error::msg(e.to_string()))?;
    }
    let mut tunnel = match Command::new(&setup.cloudflared)
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
    println!("Remote daemon listening on {address}");
    println!(
        "Access token file: {}",
        env::var("AMARCODE_WS_TOKEN_FILE").unwrap_or_default()
    );
    println!("Waiting for the Cloudflare URL. Press Ctrl+C to stop the daemon and tunnel.");
    let stderr = tunnel.stderr.take().expect("piped cloudflared stderr");
    let output = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut announced = false;
        while let Ok(Some(line)) = lines.next_line().await {
            if !announced {
                if let Some(url) = tunnel_url(&line) {
                    println!("\nRemote URL: {url}\nIn Amarcode: Settings → Connection → Remote. Use this URL and your saved access token.\n");
                    announced = true;
                }
            }
            eprintln!("cloudflared: {line}");
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
}
