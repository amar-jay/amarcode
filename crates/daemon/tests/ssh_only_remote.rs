//! Exercise SSH-only startup through the installed CLI without a Cloudflare binary.
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};

const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_amarcode-daemon"));
    command
        .env("AMARCODE_REMOTE_ROOT", root)
        .env("AMARCODE_ACP_REGISTRY_SOURCE", "")
        .env_remove("AMARCODE_APPDIR")
        .env_remove("AMARCODE_STORE_PATH")
        .env_remove("AMARCODE_WS_TOKEN_FILE")
        .env_remove("AMARCODE_WS_TLS_CERT")
        .env_remove("AMARCODE_WS_TLS_KEY");
    command
}

fn token(root: &Path) {
    let path = root.join("access-token");
    std::fs::write(&path, TOKEN).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

struct Cleanup(Child);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn ssh_only_uses_remote_data_and_authentication_without_cloudflare_or_metadata() {
    for (metadata, custom_paths) in [(false, false), (true, false), (false, true)] {
        let directory = tempfile::tempdir().unwrap();
        token(directory.path());
        if metadata {
            let bin = directory.path().join("managed-bin");
            std::fs::create_dir(&bin).unwrap();
            std::fs::write(
                directory.path().join("remote.json"),
                json!({
                    "cloudflared": directory.path().join("missing-cloudflared"),
                    "tool_bin_dirs": [bin],
                })
                .to_string(),
            )
            .unwrap();
        }
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let mut command = command(directory.path());
        let auth_token = if custom_paths {
            "b".repeat(64)
        } else {
            TOKEN.to_owned()
        };
        let data = directory
            .path()
            .join(if custom_paths { "custom-data" } else { "data" });
        if custom_paths {
            let custom_token = directory.path().join("custom-token");
            std::fs::rename(directory.path().join("access-token"), &custom_token).unwrap();
            std::fs::write(&custom_token, &auth_token).unwrap();
            command
                .env("AMARCODE_APPDIR", &data)
                .env("AMARCODE_WS_TOKEN_FILE", custom_token);
        }
        let child = command
            .args(["--remote", "--no-tunnel"])
            .env("AMARCODE_DAEMON_ADDR", address.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut child = Cleanup(child);
        let mut stdout = tokio::io::BufReader::new(
            tokio::process::ChildStdout::from_std(child.0.stdout.take().unwrap()).unwrap(),
        );
        let mut socket = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let mut request = format!("ws://{address}/rpc").into_client_request().unwrap();
                request.headers_mut().insert(
                    "authorization",
                    format!("Bearer {auth_token}").parse().unwrap(),
                );
                match connect_async(request).await {
                    Ok((socket, _)) => break socket,
                    Err(_) => {
                        assert!(
                            child.0.try_wait().unwrap().is_none(),
                            "SSH-only daemon exited before listening"
                        );
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                }
            }
        })
        .await
        .expect("SSH-only listener");
        socket
            .send(Message::Text(json!({"method":"health"}).to_string().into()))
            .await
            .unwrap();
        let reply = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let health: Value = serde_json::from_str(reply.to_text().unwrap()).unwrap();
        assert_eq!(health["result"]["status"], "ok");
        assert_eq!(health["result"]["remote_auth_required"], true);
        assert_eq!(health["result"]["addr"], address.to_string());
        assert!(data.join("workspace.sqlite3").is_file());
        assert!(!directory.path().join("workspace.sqlite3").exists());
        let error = connect_async(format!("ws://{address}/rpc"))
            .await
            .unwrap_err();
        assert!(
            matches!(error, tokio_tungstenite::tungstenite::Error::Http(response) if response.status() == 401)
        );
        use tokio::io::AsyncBufReadExt;
        let mut announcement = String::new();
        tokio::time::timeout(Duration::from_secs(2), stdout.read_line(&mut announcement))
            .await
            .unwrap()
            .unwrap();
        assert!(announcement.contains(&format!("Authenticated daemon listening on {address}")));
        announcement.clear();
        stdout.read_line(&mut announcement).await.unwrap();
        assert!(announcement.contains("Access token file:"));
        assert!(!announcement.contains(&auth_token));
        socket.close(None).await.unwrap();
    }
}

#[test]
fn no_tunnel_requires_remote_run_and_ipv4_loopback() {
    let directory = tempfile::tempdir().unwrap();
    token(directory.path());
    for arguments in [
        vec!["--no-tunnel"],
        vec!["--remote", "--no-tunnel", "--transport", "tcp"],
        vec!["--remote", "--no-tunnel", "status"],
    ] {
        let output = command(directory.path()).args(arguments).output().unwrap();
        assert!(!output.status.success());
        assert!(!directory.path().join("data/workspace.sqlite3").exists());
    }
    for address in [
        "0.0.0.0:43721",
        "[::1]:43721",
        "127.0.0.2:43721",
        "127.0.0.1:0",
    ] {
        let output = command(directory.path())
            .args(["--remote", "--no-tunnel"])
            .env("AMARCODE_DAEMON_ADDR", address)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{address}");
        assert!(!directory.path().join("data/workspace.sqlite3").exists());
    }
}

#[test]
fn invalid_or_missing_token_fails_before_opening_remote_data() {
    for contents in [None, Some("short")] {
        let directory = tempfile::tempdir().unwrap();
        if let Some(contents) = contents {
            token(directory.path());
            std::fs::write(directory.path().join("access-token"), contents).unwrap();
        }
        let output = command(directory.path())
            .args(["--remote", "--no-tunnel"])
            .env("AMARCODE_DAEMON_ADDR", "127.0.0.1:43721")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!directory.path().join("data/workspace.sqlite3").exists());
    }
}

#[test]
fn cloudflare_mode_still_requires_its_installed_connector() {
    let directory = tempfile::tempdir().unwrap();
    token(directory.path());
    std::fs::write(directory.path().join("remote.json"), "{}").unwrap();
    let output = command(directory.path()).arg("--remote").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cloudflared"));
    assert!(!directory.path().join("data/workspace.sqlite3").exists());
}
