//! The desktop is deliberately absent: exercise workspace operations over both
//! daemon transports, including concurrent saves from independent clients.
use std::{
    fs,
    net::SocketAddr,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

use amarcode_protocol::{rpc::methods, WorkspaceChange, WorkspaceDiff, WorkspaceInfo};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

struct Daemon {
    child: Child,
    address: SocketAddr,
    websocket: bool,
    _directory: tempfile::TempDir,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Daemon {
    async fn start(websocket: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let child = Command::new(env!("CARGO_BIN_EXE_amarcode-daemon"))
            .args([
                "run",
                "--transport",
                if websocket { "websocket" } else { "tcp" },
            ])
            .env("AMARCODE_APPDIR", directory.path())
            .env("AMARCODE_ACP_REGISTRY_SOURCE", "")
            .env("AMARCODE_DAEMON_ADDR", address.to_string())
            .env_remove("AMARCODE_WS_TLS_CERT")
            .env_remove("AMARCODE_WS_TLS_KEY")
            .env_remove("AMARCODE_WS_ALLOW_ORIGIN")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut daemon = Self {
            child,
            address,
            websocket,
            _directory: directory,
        };
        timeout(Duration::from_secs(10), async {
            loop {
                if TcpStream::connect(address).await.is_ok() {
                    break;
                }
                assert!(
                    daemon.child.try_wait().unwrap().is_none(),
                    "daemon exited before listening"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("daemon did not start");
        daemon
    }

    async fn request(&self, method: &str, params: Value) -> Value {
        timeout(Duration::from_secs(10), async {
            let request = json!({ "method": method, "params": params }).to_string();
            let response = if self.websocket {
                let (mut socket, _) = connect_async(format!("ws://{}/rpc", self.address))
                    .await
                    .unwrap();
                socket.send(Message::Text(request.into())).await.unwrap();
                let response = socket
                    .next()
                    .await
                    .unwrap()
                    .unwrap()
                    .into_text()
                    .unwrap()
                    .to_string();
                socket.close(None).await.unwrap();
                response
            } else {
                let mut socket = TcpStream::connect(self.address).await.unwrap();
                socket
                    .write_all(format!("{request}\n").as_bytes())
                    .await
                    .unwrap();
                let mut line = String::new();
                BufReader::new(socket).read_line(&mut line).await.unwrap();
                line
            };
            serde_json::from_str(&response).unwrap()
        })
        .await
        .expect("workspace RPC timed out")
    }

    async fn call(&self, method: &str, params: Value) -> Value {
        let response = self.request(method, params).await;
        assert!(response.get("error").is_none(), "{method}: {response}");
        response.get("result").expect("RPC result").clone()
    }
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn exercise_workspace(websocket: bool) {
    let daemon = Daemon::start(websocket).await;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    git(root, &["init"]);
    git(root, &["config", "user.name", "Workspace test"]);
    git(root, &["config", "user.email", "workspace@example.com"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["config", "core.autocrlf", "false"]);
    fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(root.join("file.txt"), "committed\r\n").unwrap();
    fs::write(root.join("ignored.txt"), "ignored").unwrap();
    fs::create_dir(root.join("node_modules")).unwrap();
    fs::write(root.join("node_modules/dependency.js"), "ignored").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);

    let workspace = json!({ "workspace_path": root });
    let file = json!({ "workspace_path": root, "relative_path": "file.txt" });
    let health = daemon.call(methods::HEALTH, json!({})).await;
    assert_eq!(
        health["protocol_version"],
        amarcode_protocol::PROTOCOL_VERSION
    );
    let info: WorkspaceInfo = serde_json::from_value(
        daemon
            .call(methods::GET_WORKSPACE_INFO, workspace.clone())
            .await,
    )
    .unwrap();
    assert!(info.is_git_repository);
    assert!(info.branch_name.is_some());
    let files = daemon
        .call(methods::LIST_WORKSPACE_FILES, workspace.clone())
        .await;
    assert_eq!(files, json!([".gitignore", "file.txt"]));
    assert_eq!(
        daemon
            .call(
                methods::LIST_WORKSPACE_FILES,
                json!({ "workspace_path": root, "query": " FILE.TXT " })
            )
            .await,
        json!(["file.txt"])
    );
    assert_eq!(
        daemon
            .call(methods::READ_WORKSPACE_FILE, file.clone())
            .await,
        "committed\r\n"
    );

    assert!(daemon.call(methods::SAVE_WORKSPACE_FILE, json!({
        "workspace_path": root, "relative_path": "file.txt", "expected_content": "committed\r\n", "content": "staged\r\n"
    })).await.is_null());
    daemon
        .call(methods::STAGE_WORKSPACE_FILE, file.clone())
        .await;
    fs::write(root.join("file.txt"), "working\r\n").unwrap();
    let changes: Vec<WorkspaceChange> = serde_json::from_value(
        daemon
            .call(methods::LIST_WORKSPACE_CHANGES, workspace.clone())
            .await,
    )
    .unwrap();
    assert_eq!(changes.len(), 1);
    assert!(changes[0].staged && changes[0].unstaged);
    for (staged, before, after, comparison) in [
        (true, "committed\r\n", "staged\r\n", "Staged"),
        (false, "staged\r\n", "working\r\n", "Unstaged"),
    ] {
        let diff: WorkspaceDiff = serde_json::from_value(
            daemon
                .call(
                    methods::GET_WORKSPACE_FILE_DIFF,
                    json!({
                        "workspace_path": root, "relative_path": "file.txt", "staged": staged
                    }),
                )
                .await,
        )
        .unwrap();
        assert_eq!(
            (
                diff.before.as_str(),
                diff.after.as_str(),
                diff.comparison.as_str()
            ),
            (before, after, comparison)
        );
    }
    daemon
        .call(methods::UNSTAGE_WORKSPACE_FILE, file.clone())
        .await;
    let changes = daemon
        .call(methods::LIST_WORKSPACE_CHANGES, workspace.clone())
        .await;
    assert_eq!(changes[0]["staged"], false);
    daemon
        .call(methods::STAGE_ALL_WORKSPACE_FILES, workspace.clone())
        .await;
    daemon
        .call(
            methods::COMMIT_WORKSPACE_CHANGES,
            json!({ "workspace_path": root, "message": "workspace RPC commit" }),
        )
        .await;
    assert_eq!(
        daemon
            .call(methods::LIST_WORKSPACE_CHANGES, workspace.clone())
            .await,
        json!([])
    );
    let diff = daemon
        .call(methods::GET_WORKSPACE_FILE_DIFF, file.clone())
        .await;
    assert_eq!(diff["comparison"], "Current file");

    // Independent clients cannot both overwrite the same expected version.
    let save = |content: &str| json!({ "workspace_path": root, "relative_path": "file.txt", "expected_content": "working\r\n", "content": content });
    let (first, second) = tokio::join!(
        daemon.request(methods::SAVE_WORKSPACE_FILE, save("first\r\n")),
        daemon.request(methods::SAVE_WORKSPACE_FILE, save("second\r\n"))
    );
    assert_ne!(first.get("error").is_some(), second.get("error").is_some());
    let content = fs::read_to_string(root.join("file.txt")).unwrap();
    assert!(content == "first\r\n" || content == "second\r\n");
    for method in [
        methods::READ_WORKSPACE_FILE,
        methods::STAGE_WORKSPACE_FILE,
        methods::GET_WORKSPACE_FILE_DIFF,
    ] {
        let error = daemon
            .request(
                method,
                json!({ "workspace_path": root, "relative_path": "../outside.txt" }),
            )
            .await;
        assert!(error["error"]
            .as_str()
            .unwrap()
            .contains("inside the workspace"));
    }
    assert!(
        daemon
            .request(methods::SAVE_WORKSPACE_FILE, file.clone())
            .await
            .get("error")
            .is_some(),
        "missing save fields must fail"
    );
    assert!(daemon
        .request(
            methods::COMMIT_WORKSPACE_CHANGES,
            json!({ "workspace_path": root, "message": "  " })
        )
        .await
        .get("error")
        .is_some());

    // Deleted tracked paths remain usable for staging without reading a file.
    fs::remove_file(root.join("file.txt")).unwrap();
    daemon.call(methods::STAGE_WORKSPACE_FILE, file).await;
    let diff = daemon
        .call(
            methods::GET_WORKSPACE_FILE_DIFF,
            json!({ "workspace_path": root, "relative_path": "file.txt", "staged": true }),
        )
        .await;
    assert_eq!(diff["after"], "");
    assert_eq!(diff["status"], "D");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workspace_operations_over_tcp() {
    exercise_workspace(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workspace_operations_over_websocket() {
    exercise_workspace(true).await;
}
