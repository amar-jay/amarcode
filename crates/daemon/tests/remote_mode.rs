#![cfg(unix)]
//! Real CLI coverage with an isolated fake connector; no Cloudflare account needed.
use std::{
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn setup(root: &Path, connector_body: &str) -> (Command, std::net::SocketAddr) {
    let cloudflared = root.join("cloudflared");
    fs::write(
        &cloudflared,
        format!("#!/usr/bin/env python3\n{connector_body}\n"),
    )
    .unwrap();
    fs::set_permissions(&cloudflared, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        root.join("remote.json"),
        serde_json::json!({"cloudflared": cloudflared}).to_string(),
    )
    .unwrap();
    fs::write(root.join("access-token"), "a".repeat(64)).unwrap();
    fs::set_permissions(root.join("access-token"), fs::Permissions::from_mode(0o600)).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let mut command = Command::new(env!("CARGO_BIN_EXE_amarcode-daemon"));
    command
        .arg("--remote")
        .env("AMARCODE_REMOTE_ROOT", root)
        .env("AMARCODE_DAEMON_ADDR", address.to_string())
        .env("AMARCODE_ACP_REGISTRY_SOURCE", "")
        .env_remove("AMARCODE_APPDIR")
        .env_remove("AMARCODE_WS_TOKEN_FILE")
        .env_remove("AMARCODE_STORE_PATH")
        .env_remove("AMARCODE_WS_TLS_CERT")
        .env_remove("AMARCODE_WS_TLS_KEY")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    (command, address)
}

fn wait_for_file(path: &Path, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(
            child.try_wait().unwrap().is_none(),
            "remote daemon exited before connector startup"
        );
        assert!(Instant::now() < deadline, "connector did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_exit(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < deadline, "remote daemon did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn remote_starts_after_listener_prints_url_and_stops_connector_on_sigterm() {
    let root = tempfile::tempdir().unwrap();
    let body = r#"import os, pathlib, socket, sys, time
from urllib.parse import urlparse
import shutil
for tool in ['bun', 'bunx', 'uv', 'uvx']:
    assert pathlib.Path(shutil.which(tool)).parent == pathlib.Path(os.environ['AMARCODE_REMOTE_ROOT'], 'runtime-bin')
origin = urlparse(sys.argv[-1])
socket.create_connection((origin.hostname, origin.port), timeout=2).close()
print('INF connector booting; this log must be hidden', file=sys.stderr, flush=True)
print('| https://my-remote.trycloudflare.com |', file=sys.stderr, flush=True)
print('| https://my-remote.trycloudflare.com |', file=sys.stderr, flush=True)
print('INF connection registered', file=sys.stderr, flush=True)
pathlib.Path(os.environ['AMARCODE_REMOTE_ROOT'], 'connector.pid').write_text(str(os.getpid()))
time.sleep(120)"#;
    let (mut command, address) = setup(root.path(), body);
    let runtime_bin = root.path().join("runtime-bin");
    fs::create_dir(&runtime_bin).unwrap();
    for tool in ["bun", "bunx", "uv", "uvx"] {
        let executable = runtime_bin.join(tool);
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(executable, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(
        root.path().join("remote.json"),
        serde_json::json!({
            "cloudflared": root.path().join("cloudflared"), "tool_bin_dirs": [runtime_bin]
        })
        .to_string(),
    )
    .unwrap();
    command.args(["--transport", "websocket"]);
    let mut running = Running(command.spawn().unwrap());
    let pid_path = root.path().join("connector.pid");
    wait_for_file(&pid_path, &mut running.0);
    // Let the output worker format the URL before sending shutdown.
    std::thread::sleep(Duration::from_millis(100));
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(running.0.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    assert!(wait_for_exit(&mut running.0).success());
    let mut output = String::new();
    running
        .0
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    assert_eq!(
        output,
        format!(
            "Remote URL: wss://my-remote.trycloudflare.com/rpc\nAccess token: {}\n",
            "a".repeat(64)
        )
    );
    let mut stderr = String::new();
    running
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(stderr.is_empty(), "Unexpected terminal logs: {stderr}");
    let file_log = fs::read_to_string(root.path().join("data/daemon.log")).unwrap();
    assert!(file_log.contains("starting"));
    assert!(
        !file_log.contains(&"a".repeat(64)),
        "credentials must not enter file logs"
    );
    let connector_pid = fs::read_to_string(pid_path)
        .unwrap()
        .parse::<i32>()
        .unwrap();
    assert!(nix::sys::signal::kill(nix::unistd::Pid::from_raw(connector_pid), None).is_err());
    assert!(std::net::TcpListener::bind(address).is_ok());
}

#[test]
fn connector_failure_stops_daemon_and_explains_how_to_restart() {
    let root = tempfile::tempdir().unwrap();
    let (mut command, address) = setup(
        root.path(),
        "import sys\nprint('connector failed', file=sys.stderr)\nsys.exit(7)",
    );
    let mut running = Running(command.spawn().unwrap());
    assert!(!wait_for_exit(&mut running.0).success());
    let mut error = String::new();
    running
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut error)
        .unwrap();
    assert!(error.contains("Cloudflare tunnel exited"), "{error}");
    assert!(!error.contains("panicked"), "{error}");
    assert!(
        !error.contains("connector failed"),
        "raw connector logs must be hidden"
    );
    assert!(std::net::TcpListener::bind(address).is_ok());
}

#[test]
fn occupied_listener_never_starts_a_tunnel_to_the_other_process() {
    let root = tempfile::tempdir().unwrap();
    let body = "import os, pathlib\npathlib.Path(os.environ['AMARCODE_REMOTE_ROOT'], 'unexpected-tunnel').touch()";
    let (mut command, _) = setup(root.path(), body);
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    command.env(
        "AMARCODE_DAEMON_ADDR",
        reservation.local_addr().unwrap().to_string(),
    );
    let mut running = Running(command.spawn().unwrap());
    assert!(!wait_for_exit(&mut running.0).success());
    assert!(!root.path().join("unexpected-tunnel").exists());
}

#[test]
fn remote_rejects_tcp_and_lifecycle_commands_before_setup() {
    for arguments in [
        vec!["--transport", "tcp", "--remote"],
        vec!["install", "--remote"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_amarcode-daemon"))
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_agent_installation_is_returned_and_written_to_remote_log() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
    let root = tempfile::tempdir().unwrap();
    let body = "import os, pathlib, time\npathlib.Path(os.environ['AMARCODE_REMOTE_ROOT'], 'ready').touch()\ntime.sleep(120)";
    let (mut command, address) = setup(root.path(), body);
    let mut running = Running(command.spawn().unwrap());
    wait_for_file(&root.path().join("ready"), &mut running.0);
    let mut request = format!("ws://{address}/rpc").into_client_request().unwrap();
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    socket
        .send(Message::Text(
            serde_json::json!({"method":"install_agent", "params":{"agent_id":"missing-agent"}})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let response: serde_json::Value = serde_json::from_str(response.to_text().unwrap()).unwrap();
    assert!(response["error"]
        .as_str()
        .unwrap()
        .contains("agent not found"));
    let log = fs::read_to_string(root.path().join("data/daemon.log")).unwrap();
    assert!(log.contains("starting agent installation"));
    assert!(log.contains("agent installation failed"));
    assert!(log.contains("missing-agent"));
    assert!(!log.contains(&"a".repeat(64)));
    socket.close(None).await.unwrap();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(running.0.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    assert!(wait_for_exit(&mut running.0).success());
}
