#![cfg(unix)]
//! Loopback CLI lifecycle coverage. Named connector lifecycle is covered in remote::tests.
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

fn setup(root: &Path, connector_body: &str, no_tunnel: bool) -> (Command, std::net::SocketAddr) {
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
    if no_tunnel {
        command.arg("--no-tunnel");
    }
    (command, address)
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
fn remote_no_tunnel_announces_loopback_and_stops_cleanly() {
    let root = tempfile::tempdir().unwrap();
    // Fake connector must never run in loopback mode.
    let body = "import os, pathlib\npathlib.Path(os.environ['AMARCODE_REMOTE_ROOT'], 'unexpected-tunnel').touch()";
    let (mut command, address) = setup(root.path(), body, true);
    command.args(["--transport", "websocket"]);
    let mut running = Running(command.spawn().unwrap());
    // Wait for the daemon to become reachable on loopback, then shut it down.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if std::net::TcpStream::connect(address).is_ok() {
            break;
        }
        assert!(
            running.0.try_wait().unwrap().is_none(),
            "remote daemon exited early"
        );
        assert!(Instant::now() < deadline, "loopback listener did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
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
    assert!(
        output.contains("Authenticated daemon listening"),
        "{output}"
    );
    assert!(output.contains(&address.to_string()), "{output}");
    assert!(!root.path().join("unexpected-tunnel").exists());
    let mut stderr = String::new();
    running
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(stderr.is_empty(), "Unexpected terminal logs: {stderr}");
    assert!(
        !output.contains(&"a".repeat(64)),
        "credentials must not enter stdout"
    );
}

#[test]
fn occupied_listener_exits_without_starting_a_connector() {
    let root = tempfile::tempdir().unwrap();
    let body = "import os, pathlib\npathlib.Path(os.environ['AMARCODE_REMOTE_ROOT'], 'unexpected-tunnel').touch()";
    let (mut command, _) = setup(root.path(), body, true);
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
fn isolated_remote_rejects_tcp_and_lifecycle_commands_before_setup() {
    let directory = tempfile::tempdir().unwrap();
    for arguments in [
        vec!["--transport", "tcp", "--remote"],
        vec!["install", "--remote"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_amarcode-daemon"))
            .args(arguments)
            .env("AMARCODE_REMOTE_ROOT", directory.path())
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
    let (mut command, address) = setup(root.path(), body, true);
    let mut running = Running(command.spawn().unwrap());
    let mut request = format!("ws://{address}/rpc").into_client_request().unwrap();
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
    );
    let (mut socket, _) = {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match tokio_tungstenite::connect_async(request.clone()).await {
                Ok(pair) => break pair,
                Err(_) => {
                    assert!(
                        running.0.try_wait().unwrap().is_none(),
                        "remote daemon exited early"
                    );
                    assert!(Instant::now() < deadline, "loopback daemon did not start");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
    };
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

#[test]
fn removed_quick_mode_cannot_launch_a_connector_or_enroll() {
    let root = tempfile::tempdir().unwrap();
    let body = "import os, pathlib\npathlib.Path(os.environ['AMARCODE_REMOTE_ROOT'], 'unexpected-tunnel').touch()";
    let (mut command, address) = setup(root.path(), body, false);
    command.env("AMARCODE_TUNNEL_MODE", "quick");
    let output = command.output().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("Only named Cloudflare tunnels are supported"),
        "{error}"
    );
    assert!(!root.path().join("unexpected-tunnel").exists());
    assert!(!root.path().join("named-installation.json").exists());
    assert!(std::net::TcpListener::bind(address).is_ok());
}
