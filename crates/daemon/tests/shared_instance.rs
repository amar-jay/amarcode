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
        .env("SHELL", "/bin/sh")
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

async fn terminal_daemon(base: &Path, local: &str, remote: &str) -> (Process, String) {
    assert!(command(base, local, remote)
        .args(["access", "enable"])
        .status()
        .unwrap()
        .success());
    let token = std::fs::read_to_string(base.join("com.amarcode.desktop/access-token"))
        .unwrap()
        .trim()
        .to_owned();
    let mut daemon = Process(command(base, local, remote).spawn().unwrap());
    ready(&mut daemon, local).await;
    (daemon, token)
}
async fn terminal_socket(
    addr: &str,
    token: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>> {
    let mut request = format!("ws://{addr}/terminal")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    tokio_tungstenite::connect_async(request).await.unwrap().0
}
#[tokio::test]
async fn remote_shell_authenticates_streams_resizes_and_reports_exit() {
    let base = tempfile::tempdir().unwrap();
    let local = port();
    let remote = port();
    let (_daemon, token) = terminal_daemon(base.path(), &local, &remote).await;
    assert!(
        tokio_tungstenite::connect_async(format!("ws://{remote}/terminal"))
            .await
            .is_err()
    );
    let mut socket = terminal_socket(&remote, &token).await;
    socket
        .send(Message::Text(
            json!({"type":"open", "cols":80, "rows":24})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    socket
        .send(Message::Text(
            json!({"type":"resize", "cols":93, "rows":37})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    // Avoid relying on echoed command text to verify execution.
    socket
        .send(Message::Binary(
            b"printf 'SHELL_%s\\n' WORKED; stty size; exit 7\n"
                .to_vec()
                .into(),
        ))
        .await
        .unwrap();
    let (output, exit) = tokio::time::timeout(Duration::from_secs(5), async {
        let mut output = String::new();
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Binary(bytes) => output.push_str(&String::from_utf8_lossy(&bytes)),
                Message::Text(text) => {
                    let event: Value = serde_json::from_str(&text).unwrap();
                    if event["type"] == "exit" {
                        break (output, event["code"].as_u64().unwrap());
                    }
                }
                _ => (),
            }
        }
    })
    .await
    .unwrap();
    assert!(output.contains("SHELL_WORKED"), "{output}");
    assert!(output.contains("37 93"), "{output}");
    assert_eq!(exit, 7);
    assert_eq!(
        tcp(&local, json!({"method":"health"})).await["result"]["status"],
        "ok"
    );
}
#[tokio::test]
async fn remote_shell_disconnect_kills_its_shell_and_invalid_open_spawns_nothing() {
    let base = tempfile::tempdir().unwrap();
    let local = port();
    let remote = port();
    let (_daemon, token) = terminal_daemon(base.path(), &local, &remote).await;
    let mut invalid = terminal_socket(&remote, &token).await;
    invalid
        .send(Message::Text(
            json!({"type":"open","cols":0,"rows":24}).to_string().into(),
        ))
        .await
        .unwrap();
    let error = tokio::time::timeout(Duration::from_secs(3), invalid.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let error: Value = serde_json::from_str(error.to_text().unwrap()).unwrap();
    assert_eq!(error["type"], "error");
    assert!(error["message"].as_str().unwrap().contains("dimensions"));
    let pidfile = base.path().join("shell.pid");
    let mut socket = terminal_socket(&remote, &token).await;
    socket
        .send(Message::Text(
            json!({"type":"open","cols":80,"rows":24})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    socket
        .send(Message::Binary(
            format!("echo $$ > '{}'\n", pidfile.display())
                .into_bytes()
                .into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !pidfile.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let pid = std::fs::read_to_string(pidfile)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    socket.close(None).await.unwrap();
    drop(socket);
    tokio::time::timeout(Duration::from_secs(3), async {
        while Path::new(&format!("/proc/{pid}")).exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        tcp(&local, json!({"method":"health"})).await["result"]["status"],
        "ok"
    );
}
#[test]
fn ssh_client_does_not_create_or_start_a_daemon_without_a_terminal() {
    let base = tempfile::tempdir().unwrap();
    let output = command(base.path(), &port(), &port())
        .args(["ssh", "wss://example.invalid/rpc"])
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal"));
    assert!(!base.path().join("com.amarcode.desktop").exists());
}

#[tokio::test]
async fn ssh_cli_prompts_for_hidden_token_and_runs_a_real_remote_shell() {
    use std::io::{Read, Write};
    let base = tempfile::tempdir().unwrap();
    let local = port();
    let remote = port();
    let (_daemon, token) = terminal_daemon(base.path(), &local, &remote).await;
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = portable_pty::CommandBuilder::new(env!("CARGO_BIN_EXE_amarcode-daemon"));
    command.arg("ssh");
    command.env_remove("AMARCODE_WS_TOKEN_FILE");
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut writer = pair.master.take_writer().unwrap();
    let mut reader = pair.master.try_clone_reader().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let mut bytes = [0; 4096];
        loop {
            match reader.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if tx.send(bytes[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let mut output = String::new();
        let mut phase = 0;
        while let Some(bytes) = rx.recv().await {
            output.push_str(&String::from_utf8_lossy(&bytes));
            if phase == 0 && output.contains("Remote daemon URL:") {
                writer
                    .write_all(format!("ws://{remote}/rpc\n").as_bytes())
                    .unwrap();
                phase = 1;
            }
            if phase == 1 && output.contains("Daemon access token:") {
                writer.write_all(format!("{token}\n").as_bytes()).unwrap();
                phase = 2;
            }
            // The shell prompt appears after authentication and raw mode starts.
            if phase == 2 && (output.ends_with("$ ") || output.ends_with("# ")) {
                writer
                    .write_all(b"printf 'CLI_%s\\n' WORKED; exit 9\n")
                    .unwrap();
                phase = 3;
            }
        }
        output
    })
    .await;
    if result.is_err() {
        let _ = child.kill();
    }
    let output = result.expect("CLI terminal interaction timed out");
    assert!(!output.contains(&token), "token must not be echoed");
    assert!(output.contains("CLI_WORKED"), "{output}");
    assert_eq!(child.wait().unwrap().exit_code(), 9);
}

#[tokio::test]
async fn remote_shell_drains_large_output_before_exit() {
    let base = tempfile::tempdir().unwrap();
    let local = port();
    let remote = port();
    let (_daemon, token) = terminal_daemon(base.path(), &local, &remote).await;
    let mut socket = terminal_socket(&remote, &token).await;
    socket
        .send(Message::Text(
            json!({"type":"open","cols":80,"rows":24})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    socket.send(Message::Binary(b"python3 -c 'import sys; sys.stdout.write(\"Z\" * 2000000); sys.stdout.flush()'; exit 0\n".to_vec().into())).await.unwrap();
    let count = tokio::time::timeout(Duration::from_secs(10), async {
        let mut count = 0;
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Binary(bytes) => count += bytes.iter().filter(|&&b| b == b'Z').count(),
                Message::Text(text)
                    if serde_json::from_str::<Value>(&text).unwrap()["type"] == "exit" =>
                {
                    break count
                }
                _ => (),
            }
            // Exercise backpressure instead of only the fast loopback path.
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    // The terminal also echoes the Z in the command itself.
    assert_eq!(count, 2_000_001);
}
#[tokio::test]
async fn remote_shell_ctrl_c_interrupts_a_command_and_preserves_the_shell() {
    let base = tempfile::tempdir().unwrap();
    let local = port();
    let remote = port();
    let (_daemon, token) = terminal_daemon(base.path(), &local, &remote).await;
    let mut socket = terminal_socket(&remote, &token).await;
    socket
        .send(Message::Text(
            json!({"type":"open","cols":80,"rows":24})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    socket
        .send(Message::Binary(
            b"printf 'WAIT_%s\\n' READY; sleep 30\n".to_vec().into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut output = String::new();
        while !output.contains("WAIT_READY") {
            if let Message::Binary(bytes) = socket.next().await.unwrap().unwrap() {
                output.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    socket.send(Message::Binary(vec![3].into())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    socket
        .send(Message::Binary(
            b"printf 'AFTER_%s\\n' INTERRUPT; exit 4\n".to_vec().into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut output = String::new();
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Binary(bytes) => output.push_str(&String::from_utf8_lossy(&bytes)),
                Message::Text(text) => {
                    let event: Value = serde_json::from_str(&text).unwrap();
                    if event["type"] == "exit" {
                        assert_eq!(event["code"], 4);
                        assert!(output.contains("AFTER_INTERRUPT"));
                        break;
                    }
                }
                _ => (),
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn ssh_cli_token_file_and_sigterm_restore_the_local_terminal() {
    use std::io::Read;
    let base = tempfile::tempdir().unwrap();
    let local = port();
    let remote = port();
    let (_daemon, _token) = terminal_daemon(base.path(), &local, &remote).await;
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let before = pair.master.get_termios().unwrap().local_flags;
    let mut command = portable_pty::CommandBuilder::new(env!("CARGO_BIN_EXE_amarcode-daemon"));
    command.args(["ssh", &format!("ws://{remote}/rpc"), "--token-file"]);
    command.arg(base.path().join("com.amarcode.desktop/access-token"));
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let mut bytes = [0; 4096];
        loop {
            match reader.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if tx.send(bytes[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let started = tokio::time::timeout(Duration::from_secs(5), async {
        let mut output = String::new();
        while let Some(bytes) = rx.recv().await {
            output.push_str(&String::from_utf8_lossy(&bytes));
            if output.ends_with("$ ") || output.ends_with("# ") {
                return output;
            }
        }
        panic!("CLI exited before its shell opened: {output}");
    })
    .await;
    if started.is_err() {
        let _ = child.kill();
    }
    let output = started.unwrap();
    assert!(!output.contains("Daemon access token:"));
    assert_ne!(pair.master.get_termios().unwrap().local_flags, before);
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.process_id().unwrap() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    let status = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    if status.is_err() {
        let _ = child.kill();
    }
    assert_eq!(status.unwrap().exit_code(), 143);
    assert_eq!(pair.master.get_termios().unwrap().local_flags, before);
}

#[tokio::test]
async fn terminal_workers_exit_when_a_detached_descendant_keeps_the_pty_open() {
    let base = tempfile::tempdir().unwrap();
    let local = port();
    let remote = port();
    let (daemon, token) = terminal_daemon(base.path(), &local, &remote).await;
    let task_dir = format!("/proc/{}/task", daemon.0.id());
    let before = std::fs::read_dir(&task_dir).unwrap().count();
    let mut socket = terminal_socket(&remote, &token).await;
    socket
        .send(Message::Text(
            json!({"type":"open","cols":80,"rows":24})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    // The detached child deliberately retains the slave for a few seconds.
    socket.send(Message::Binary(b"python3 -c 'import os,time; p=os.fork(); p and os._exit(0); os.setsid(); time.sleep(4)'; exit 0\n".to_vec().into())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Message::Text(text) = socket.next().await.unwrap().unwrap() {
                if serde_json::from_str::<Value>(&text).unwrap()["type"] == "exit" {
                    break;
                }
            }
        }
    })
    .await
    .unwrap();
    drop(socket);
    tokio::time::timeout(Duration::from_millis(500), async {
        while std::fs::read_dir(&task_dir).unwrap().count() > before {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("PTY I/O workers must stop without waiting for a detached descendant");
}

#[tokio::test]
async fn standalone_ssh_client_supports_tls_and_rejects_untrusted_certificates() {
    use std::{
        io::{Read, Write},
        sync::Arc,
    };
    let base = tempfile::tempdir().unwrap();
    let token_path = base.path().join("token");
    let mut options = std::fs::OpenOptions::new();
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = options
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&token_path)
        .unwrap();
    writeln!(file, "{}", "test-token-".repeat(8)).unwrap();
    let certs = rustls_pemfile::certs(&mut std::io::BufReader::new(
        include_bytes!("fixtures/websocket-cert.pem").as_slice(),
    ))
    .collect::<std::io::Result<Vec<_>>>()
    .unwrap();
    let key = rustls_pemfile::private_key(&mut std::io::BufReader::new(
        include_bytes!("fixtures/websocket-key.pem").as_slice(),
    ))
    .unwrap()
    .unwrap();
    let config = tokio_rustls::rustls::ServerConfig::builder_with_provider(Arc::new(
        tokio_rustls::rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(certs, key)
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        // The client must reject this self-signed certificate.
        assert!(tokio_rustls::TlsAcceptor::from(Arc::new(config))
            .accept(stream)
            .await
            .is_err());
    });
    let pair = portable_pty::native_pty_system()
        .openpty(portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = portable_pty::CommandBuilder::new(env!("CARGO_BIN_EXE_amarcode-daemon"));
    command.args([
        "ssh",
        &format!("wss://localhost:{}/rpc", addr.port()),
        "--token-file",
    ]);
    command.arg(token_path);
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let mut bytes = [0; 4096];
        loop {
            match reader.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if tx.send(bytes[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let mut output = String::new();
        while let Some(bytes) = rx.recv().await {
            output.push_str(&String::from_utf8_lossy(&bytes));
        }
        output
    })
    .await;
    if result.is_err() {
        let _ = child.kill();
        server.abort();
    }
    let output = result.unwrap();
    assert_eq!(child.wait().unwrap().exit_code(), 1, "{output}");
    assert!(
        output.to_lowercase().contains("certificate"),
        "must reach TLS certificate verification: {output}"
    );
    assert!(!output.contains("panicked"), "{output}");
    server.await.unwrap();
}

#[tokio::test]
async fn remote_shell_does_not_inherit_the_daemons_tmux_session() {
    let base = tempfile::tempdir().unwrap();
    let local = port();
    let remote = port();
    assert!(command(base.path(), &local, &remote)
        .args(["access", "enable"])
        .status()
        .unwrap()
        .success());
    let token =
        std::fs::read_to_string(base.path().join("com.amarcode.desktop/access-token")).unwrap();
    let mut daemon = Process(
        command(base.path(), &local, &remote)
            .env("TMUX", "/tmp/daemon-tmux-session,123,0")
            .env("TMUX_PANE", "%1")
            .env("STY", "daemon-screen-session")
            .spawn()
            .unwrap(),
    );
    ready(&mut daemon, &local).await;
    let mut socket = terminal_socket(&remote, token.trim()).await;
    socket
        .send(Message::Text(
            json!({"type":"open","cols":80,"rows":24})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    socket
        .send(Message::Binary(
            b"printf 'MUX_%s:%s:%s_END\\n' \"$TMUX\" \"$TMUX_PANE\" \"$STY\"; exit 0\n"
                .to_vec()
                .into(),
        ))
        .await
        .unwrap();
    let output = tokio::time::timeout(Duration::from_secs(3), async {
        let mut output = String::new();
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Binary(bytes) => output.push_str(&String::from_utf8_lossy(&bytes)),
                Message::Text(text)
                    if serde_json::from_str::<Value>(&text).unwrap()["type"] == "exit" =>
                {
                    break output
                }
                _ => (),
            }
        }
    })
    .await
    .unwrap();
    assert!(output.contains("MUX_::_END"), "{output}");
    assert!(!output.contains("daemon-tmux-session"));
    assert!(!output.contains("daemon-screen-session"));
}

#[tokio::test]
async fn remote_workspace_is_shared_by_clients_without_changing_the_data_directory() {
    let base = tempfile::tempdir().unwrap();
    let local = port();
    let remote = port();
    let root = base.path().join("com.amarcode.desktop");
    let project = base.path().join("project with spaces");
    let other = base.path().join("another project");
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(&other).unwrap();
    assert!(command(base.path(), &local, &remote)
        .args(["access", "enable"])
        .status()
        .unwrap()
        .success());
    let token = std::fs::read_to_string(root.join("access-token"))
        .unwrap()
        .trim()
        .to_owned();
    let mut daemon = Process(
        command(base.path(), &local, &remote)
            .current_dir(base.path())
            .args(["--remote", "project with spaces", "--no-tunnel"])
            .spawn()
            .unwrap(),
    );
    ready(&mut daemon, &local).await;
    let expected = project
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    for health in [
        tcp(&local, json!({"method":"health"})).await,
        ws(&remote, &token, json!({"method":"health"})).await,
    ] {
        assert_eq!(health["result"]["default_workspace_path"], expected);
    }
    let created = ws(
        &remote,
        &token,
        json!({"method":"create_chat",
        "params":{"workspace_path":"","title":"Default workspace"}}),
    )
    .await;
    assert_eq!(created["result"]["workspace_path"], expected);
    let explicit = tcp(
        &local,
        json!({"method":"create_chat",
        "params":{"workspace_path":other,"title":"Explicit workspace"}}),
    )
    .await;
    assert_eq!(
        explicit["result"]["workspace_path"],
        other.to_string_lossy().as_ref()
    );
    assert!(root.join("data/workspace.sqlite3").exists());
    assert!(!project.join("workspace.sqlite3").exists());
    assert!(!project.join("data").exists());
    drop(daemon);
    // The launch-time suggestion does not persist into a different startup.
    let mut daemon = Process(command(base.path(), &local, &remote).spawn().unwrap());
    ready(&mut daemon, &local).await;
    assert!(tcp(&local, json!({"method":"health"})).await["result"]
        .get("default_workspace_path")
        .is_none());
}

#[test]
fn invalid_remote_workspace_fails_before_remote_setup_or_database_creation() {
    let base = tempfile::tempdir().unwrap();
    let file = base.path().join("a-file");
    std::fs::write(&file, "not a folder").unwrap();
    for path in [file, base.path().join("missing")] {
        let result = command(base.path(), &port(), &port())
            .arg("--remote")
            .arg(path)
            .arg("--no-tunnel")
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("workspace"));
        assert!(!base.path().join("com.amarcode.desktop").exists());
    }
}
