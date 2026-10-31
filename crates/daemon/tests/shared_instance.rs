#![cfg(target_os = "linux")]
//! Local and remote connections must operate on one live application and store.
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn command(base: &Path, tcp: &str, ws: &str) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_amarcode-daemon"));
    cmd.env("XDG_DATA_HOME", base)
        .env("HOME", base)
        .env("AMARCODE_DAEMON_ADDR", tcp)
        .env("AMARCODE_WS_ADDR", ws)
        .env("AMARCODE_ACP_REGISTRY_SOURCE", "")
        .env_remove("AMARCODE_REMOTE_ROOT")
        .env_remove("AMARCODE_APPDIR")
        .env_remove("AMARCODE_STORE_PATH")
        .env_remove("AMARCODE_WS_TOKEN_FILE")
        .env_remove("AMARCODE_WS_TLS_CERT")
        .env_remove("AMARCODE_WS_TLS_KEY")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd
}
fn port() -> String {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string()
}
async fn tcp(addr: &str, request: Value) -> Value {
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    tokio::time::timeout(
        Duration::from_secs(3),
        BufReader::new(socket).read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    serde_json::from_str(&line).unwrap()
}
async fn ws(addr: &str, token: &str, request: Value) -> Value {
    let mut req = format!("ws://{addr}/rpc").into_client_request().unwrap();
    req.headers_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    socket
        .send(Message::Text(request.to_string().into()))
        .await
        .unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let value = serde_json::from_str(reply.to_text().unwrap()).unwrap();
    socket.close(None).await.unwrap();
    value
}
async fn ready(process: &mut Process, addr: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if TcpStream::connect(addr).await.is_ok() {
                break;
            }
            assert!(
                process.0.try_wait().unwrap().is_none(),
                "daemon exited before listening"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn local_and_remote_share_state_and_access_survives_restarts() {
    let base = tempfile::tempdir().unwrap();
    let local = port();
    let remote = port();
    let root = base.path().join("com.amarcode.desktop");
    assert!(command(base.path(), &local, &remote)
        .args(["access", "enable"])
        .status()
        .unwrap()
        .success());
    let token = std::fs::read_to_string(root.join("access-token"))
        .unwrap()
        .trim()
        .to_owned();
    assert!(!root.join("data/workspace.sqlite3").exists());
    let mut daemon = Process(command(base.path(), &local, &remote).spawn().unwrap());
    ready(&mut daemon, &local).await;
    let health = tcp(&local, json!({"method":"health"})).await;
    assert_eq!(health["result"]["remote_auth_required"], false);
    assert_eq!(health["result"]["addr"], local);
    let health = ws(&remote, &token, json!({"method":"health"})).await;
    assert_eq!(health["result"]["remote_auth_required"], true);
    assert_eq!(health["result"]["addr"], remote);
    assert!(
        tokio_tungstenite::connect_async(format!("ws://{remote}/rpc"))
            .await
            .is_err()
    );
    let set = tcp(&local, json!({"method":"set_daemon_config", "params":{"store_acp_events":true,"acp_event_retention_days":9}})).await;
    assert_eq!(set["result"]["store_acp_events"], true);
    let get = ws(&remote, &token, json!({"method":"get_daemon_config"})).await;
    assert_eq!(get["result"]["acp_event_retention_days"], 9);
    let mut second = command(base.path(), &port(), &port());
    let output = second
        .args(["--remote", "--no-tunnel"])
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("already owns database"));
    // Python installer and Rust runtime must agree on database ownership locks.
    let lock_probe = Command::new("python3").args(["-c", "import fcntl,sys; f=open(sys.argv[1],'a+b');\ntry: fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)\nexcept BlockingIOError: sys.exit(0)\nsys.exit(1)"])
        .arg(root.join("data/workspace.sqlite3.amarcode.lock")).status().unwrap();
    assert!(
        lock_probe.success(),
        "installer must see the live Rust database lock"
    );
    assert!(!base.path().join("amarcode-remote").exists());
    drop(daemon);
    let mut daemon = Process(command(base.path(), &local, &remote).spawn().unwrap());
    ready(&mut daemon, &local).await;
    assert_eq!(
        ws(&remote, &token, json!({"method":"get_daemon_config"})).await["result"]
            ["acp_event_retention_days"],
        9
    );
    assert_eq!(
        std::fs::read_to_string(root.join("access-token"))
            .unwrap()
            .trim(),
        token
    );
    drop(daemon);
    assert!(command(base.path(), &local, &remote)
        .args(["access", "disable"])
        .status()
        .unwrap()
        .success());
    let mut daemon = Process(command(base.path(), &local, &remote).spawn().unwrap());
    ready(&mut daemon, &local).await;
    assert!(TcpStream::connect(&remote).await.is_err());
    assert_eq!(
        tcp(&local, json!({"method":"get_daemon_config"})).await["result"]
            ["acp_event_retention_days"],
        9
    );
}

#[tokio::test]
async fn cloudflare_attaches_without_owning_the_daemon_lifetime() {
    use std::os::unix::fs::PermissionsExt;
    let base = tempfile::tempdir().unwrap();
    let local = port();
    let remote = port();
    let root = base.path().join("com.amarcode.desktop");
    assert!(command(base.path(), &local, &remote)
        .args(["access", "enable"])
        .status()
        .unwrap()
        .success());
    let connector = root.join("cloudflared");
    let marker = root.join("connector-started");
    std::fs::write(&connector, format!("#!/usr/bin/env python3\nimport pathlib, time\npathlib.Path({:?}).touch()\ntime.sleep(120)\n", marker.to_str().unwrap())).unwrap();
    std::fs::set_permissions(&connector, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        root.join("remote.json"),
        json!({"enabled":true,"cloudflared":connector}).to_string(),
    )
    .unwrap();
    let mut daemon = Process(command(base.path(), &local, &remote).spawn().unwrap());
    ready(&mut daemon, &local).await;
    let mut tunnel = Process(
        command(base.path(), &local, &remote)
            .arg("tunnel")
            .spawn()
            .unwrap(),
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        while !marker.exists() {
            assert!(tunnel.0.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(tunnel.0.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    assert!(tunnel.0.wait().unwrap().success());
    assert!(daemon.0.try_wait().unwrap().is_none());
    assert_eq!(
        tcp(&local, json!({"method":"health"})).await["result"]["status"],
        "ok"
    );
}
