//! Exercise the real CLI/config/cache/listener path, rather than only flag parsing.
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    process::{Command, Stdio},
    time::Duration,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

fn command(directory: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_amarcode-daemon"));
    command
        .arg("run")
        .env("AMARCODE_APPDIR", directory)
        .env("AMARCODE_ACP_REGISTRY_SOURCE", "")
        .env_remove("AMARCODE_DAEMON_ADDR")
        .env_remove("AMARCODE_WS_TLS_CERT")
        .env_remove("AMARCODE_WS_TLS_KEY")
        .env_remove("AMARCODE_WS_ALLOW_ORIGIN");
    command
}

#[test]
fn transports_validate_shared_address_before_startup() {
    for transport in ["tcp", "websocket"] {
        let directory = tempfile::tempdir().unwrap();
        let output = command(directory.path())
            .args(["--transport", transport])
            .env("AMARCODE_DAEMON_ADDR", "invalid-address")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("AMARCODE_DAEMON_ADDR"));
        assert!(!directory.path().join("workspace.sqlite3").exists());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_mode_uses_only_its_selected_listener() {
    let directory = tempfile::tempdir().unwrap();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let child = command(directory.path())
        .args(["--transport", "websocket"])
        .env("AMARCODE_DAEMON_ADDR", address.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    struct Cleanup(std::process::Child);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut cleanup = Cleanup(child);
    let mut socket = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match connect_async(format!("ws://{address}/rpc")).await {
                Ok((socket, _)) => break socket,
                Err(_) => {
                    assert!(
                        cleanup.0.try_wait().unwrap().is_none(),
                        "daemon exited before listening"
                    );
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }
        }
    })
    .await
    .expect("WebSocket listener did not start");
    socket
        .send(Message::Text(json!({"method":"health"}).to_string().into()))
        .await
        .unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let reply: Value = serde_json::from_str(reply.to_text().unwrap()).unwrap();
    assert_eq!(reply["result"]["status"], "ok");
    assert_eq!(reply["result"]["addr"], address.to_string());
    socket.close(None).await.unwrap();
}
