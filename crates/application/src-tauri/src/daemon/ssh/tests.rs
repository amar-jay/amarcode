use super::*;

fn config() -> SshTunnelConfig {
    SshTunnelConfig {
        host: "dev-box".into(),
        user: None,
        port: None,
        identity_file: None,
        daemon_port: 43721,
    }
}

#[test]
fn arguments_keep_host_verification_and_forwarding_local() {
    let mut config = config();
    config.user = Some("developer".into());
    config.port = Some(2222);
    let args: Vec<_> = config
        .arguments(45678)
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    for required in [
        "-N",
        "-T",
        "-n",
        "BatchMode=yes",
        "StrictHostKeyChecking=yes",
        "ExitOnForwardFailure=yes",
        "ForwardAgent=no",
        "ControlPath=none",
        "ForkAfterAuthentication=no",
        "127.0.0.1:45678:127.0.0.1:43721",
    ] {
        assert!(args.iter().any(|arg| arg == required), "missing {required}");
    }
    assert!(args.windows(2).any(|pair| pair == ["-l", "developer"]));
    assert!(args.windows(2).any(|pair| pair == ["-p", "2222"]));
    assert_eq!(args.last().unwrap(), "dev-box");
}

#[test]
fn aliases_and_ipv6_work_but_options_and_shell_syntax_are_rejected() {
    for host in ["dev-box", "example.com", "127.0.0.1", "::1"] {
        let mut config = config();
        config.host = host.into();
        assert!(config.validate().is_ok());
    }
    for host in [
        "",
        "-oProxyCommand=evil",
        "host;echo",
        "user@host",
        "host name",
        "host\nother",
        "ssh://host",
    ] {
        let mut config = config();
        config.host = host.into();
        assert!(config.validate().is_err(), "{host}");
    }
    let mut config = config();
    config.port = Some(0);
    assert!(config.validate().is_err());
    config.port = None;
    config.daemon_port = 0;
    assert!(config.validate().is_err());
    config.daemon_port = 43721;
    config.user = Some("-option".into());
    assert!(config.validate().is_err());
    config.user = None;
    config.identity_file = Some("relative-key".into());
    assert!(config.validate().is_err());
}

#[cfg(unix)]
mod processes {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::{
        handshake::server::{Request, Response},
        Message,
    };

    const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn manager(timeout: Duration) -> (tempfile::TempDir, SshTunnelManager) {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("fake-ssh");
        std::fs::write(&executable, include_str!("test-ssh.py")).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let manager = SshTunnelManager {
            executable: "python3".into(),
            test_script: Some(executable),
            startup_timeout: timeout,
            ..Default::default()
        };
        (directory, manager)
    }

    struct Server {
        port: u16,
        task: JoinHandle<()>,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn server(auth_required: bool, protocol: u32) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    #[allow(clippy::result_large_err)]
                    let handshake = |request: &Request, response: Response| {
                        if request
                            .headers()
                            .get("authorization")
                            .is_none_or(|header| header != &format!("Bearer {TOKEN}"))
                        {
                            let mut error = tokio_tungstenite::tungstenite::handshake::server::ErrorResponse::new(Some("Unauthorized".into()));
                            *error.status_mut() =
                                tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED;
                            return Err(error);
                        }
                        Ok(response)
                    };
                    let Ok(mut socket) =
                        tokio_tungstenite::accept_hdr_async(stream, handshake).await
                    else {
                        return;
                    };
                    if let Some(Ok(Message::Text(request))) = socket.next().await {
                        assert_eq!(
                            serde_json::from_str::<serde_json::Value>(&request).unwrap()["method"],
                            "health"
                        );
                        let _ = socket.send(Message::Text(json!({ "result": {
                            "remote_auth_required": auth_required, "status": "ok", "version": "test",
                            "protocol_version": protocol, "addr": "127.0.0.1:43721"
                        }}).to_string().into())).await;
                    }
                });
            }
        });
        Server { port, task }
    }

    fn alive(pid: u32) -> bool {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    }

    async fn stopped(pid: u32) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while alive(pid) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("SSH process must be killed and reaped");
    }

    #[tokio::test]
    async fn concurrent_callers_share_one_verified_tunnel_and_disconnect_reaps_it() {
        let server = server(true, amarcode_protocol::PROTOCOL_VERSION).await;
        let (_directory, manager) = manager(Duration::from_secs(3));
        let mut config = config();
        config.daemon_port = server.port;
        let (first, second) = tokio::join!(
            manager.connect(&config, TOKEN),
            manager.connect(&config, TOKEN)
        );
        assert_eq!(first.unwrap(), second.unwrap());
        let pid = manager
            .current
            .lock()
            .await
            .as_ref()
            .unwrap()
            .child
            .id()
            .unwrap();
        manager.disconnect().await;
        assert!(manager.current.lock().await.is_none());
        stopped(pid).await;
    }

    #[tokio::test]
    async fn failed_replacement_preserves_the_working_tunnel() {
        let server = server(true, amarcode_protocol::PROTOCOL_VERSION).await;
        let (_directory, manager) = manager(Duration::from_secs(3));
        let mut config = config();
        config.daemon_port = server.port;
        let endpoint = manager.connect(&config, TOKEN).await.unwrap();
        config.host = "reject".into();
        let error = manager.connect(&config, TOKEN).await.unwrap_err();
        assert!(error.contains("Host key verification failed"), "{error}");
        config.host = "dev-box".into();
        assert_eq!(manager.connect(&config, TOKEN).await.unwrap(), endpoint);
        manager.disconnect().await;
    }

    #[tokio::test]
    async fn an_exited_ssh_process_is_replaced_on_the_next_connection() {
        let server = server(true, amarcode_protocol::PROTOCOL_VERSION).await;
        let (_directory, manager) = manager(Duration::from_secs(3));
        let mut config = config();
        config.daemon_port = server.port;
        manager.connect(&config, TOKEN).await.unwrap();
        let old_pid = {
            let mut current = manager.current.lock().await;
            let child = &mut current.as_mut().unwrap().child;
            let pid = child.id().unwrap();
            child.kill().await.unwrap();
            pid
        };
        manager.connect(&config, TOKEN).await.unwrap();
        assert_ne!(
            manager
                .current
                .lock()
                .await
                .as_ref()
                .unwrap()
                .child
                .id()
                .unwrap(),
            old_pid
        );
        manager.disconnect().await;
    }

    #[tokio::test]
    async fn wrong_daemon_token_never_makes_a_tunnel_ready() {
        let server = server(true, amarcode_protocol::PROTOCOL_VERSION).await;
        let (directory, manager) = manager(Duration::from_millis(400));
        let mut config = config();
        config.daemon_port = server.port;
        assert!(manager.connect(&config, &"b".repeat(64)).await.is_err());
        assert!(manager.current.lock().await.is_none());
        let pid = std::fs::read_to_string(directory.path().join("pid"))
            .unwrap()
            .parse()
            .unwrap();
        stopped(pid).await;
    }

    #[tokio::test]
    async fn long_unicode_diagnostics_are_bounded_and_do_not_panic() {
        let (_directory, manager) = manager(Duration::from_secs(3));
        let mut config = config();
        config.host = "noisy".into();
        let error = manager.connect(&config, TOKEN).await.unwrap_err();
        assert!(error.contains("Host key verification failed"), "{error}");
        assert!(error.len() < STDERR_LIMIT + 100);
    }

    #[tokio::test]
    async fn successful_replacement_reaps_the_previous_tunnel() {
        let server = server(true, amarcode_protocol::PROTOCOL_VERSION).await;
        let (_directory, manager) = manager(Duration::from_secs(3));
        let mut config = config();
        config.daemon_port = server.port;
        manager.connect(&config, TOKEN).await.unwrap();
        let old_pid = manager
            .current
            .lock()
            .await
            .as_ref()
            .unwrap()
            .child
            .id()
            .unwrap();
        config.user = Some("another-user".into());
        manager.connect(&config, TOKEN).await.unwrap();
        stopped(old_pid).await;
        manager.disconnect().await;
    }

    #[tokio::test]
    async fn daemon_authentication_and_protocol_are_required() {
        for (auth, protocol, expected) in [
            (false, amarcode_protocol::PROTOCOL_VERSION, "authentication"),
            (true, 0, "protocol"),
        ] {
            let server = server(auth, protocol).await;
            let (_directory, manager) = manager(Duration::from_secs(3));
            let mut config = config();
            config.daemon_port = server.port;
            let error = manager.connect(&config, TOKEN).await.unwrap_err();
            assert!(error.contains(expected), "{error}");
            assert!(manager.current.lock().await.is_none());
        }
    }

    #[tokio::test]
    async fn timeout_and_disconnect_cancel_and_reap_startup() {
        for cancel in [false, true] {
            let (directory, manager) = manager(Duration::from_millis(400));
            let manager = Arc::new(manager);
            let pid_file = directory.path().join("pid");
            // The test helper writes its PID beside the executable for verification.
            let mut config = config();
            config.host = "stall".into();
            let task_manager = Arc::clone(&manager);
            let attempt = tokio::spawn(async move { task_manager.connect(&config, TOKEN).await });
            tokio::time::timeout(Duration::from_secs(2), async {
                while !pid_file.exists() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            let pid = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
            if cancel {
                manager.disconnect().await;
            }
            let error = attempt.await.unwrap().unwrap_err();
            assert!(
                error.contains(if cancel { "cancelled" } else { "timed out" }),
                "{error}"
            );
            stopped(pid).await;
        }
    }

    #[tokio::test]
    async fn cancelled_caller_and_dropped_manager_do_not_leave_processes_running() {
        let server = server(true, amarcode_protocol::PROTOCOL_VERSION).await;
        let (_directory, manager) = manager(Duration::from_secs(3));
        let mut config = config();
        config.daemon_port = server.port;
        manager.connect(&config, TOKEN).await.unwrap();
        let pid = manager
            .current
            .lock()
            .await
            .as_ref()
            .unwrap()
            .child
            .id()
            .unwrap();
        drop(manager);
        stopped(pid).await;

        let (directory, manager) = self::manager(Duration::from_secs(3));
        let manager = Arc::new(manager);
        let task_manager = Arc::clone(&manager);
        let attempt = tokio::spawn(async move {
            let mut config = super::config();
            config.host = "stall".into();
            task_manager.connect(&config, TOKEN).await
        });
        let pid_file = directory.path().join("pid");
        tokio::time::timeout(Duration::from_secs(2), async {
            while !pid_file.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let pid = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
        attempt.abort();
        let _ = attempt.await;
        stopped(pid).await;
    }

    fn settings(config: SshTunnelConfig, token: &str) -> crate::daemon::remote::ConnectionSettings {
        crate::daemon::remote::ConnectionSettings {
            url: String::new(),
            token: token.into(),
            ssh: Some(config),
        }
    }

    #[tokio::test]
    async fn ssh_save_verifies_then_persists_a_private_profile_and_reuses_its_token() {
        use crate::daemon::remote::save_at;
        let server = server(true, amarcode_protocol::PROTOCOL_VERSION).await;
        let (directory, manager) = manager(Duration::from_secs(3));
        let path = directory.path().join("connection.json");
        let (changes, receiver) = watch::channel(0);
        let mut config = config();
        config.daemon_port = server.port;
        let info = save_at(&path, &manager, &changes, settings(config.clone(), TOKEN))
            .await
            .unwrap();
        assert_eq!(*receiver.borrow(), 1);
        let public = serde_json::to_value(info).unwrap();
        assert_eq!(public["mode"], "ssh");
        assert_eq!(public["remote"], true);
        assert_eq!(public["hasToken"], true);
        assert_eq!(public["url"], "");
        assert!(!public.to_string().contains(TOKEN));
        assert!(public["scope"].as_str().unwrap().starts_with("ssh:"));
        let stored: crate::daemon::remote::ConnectionSettings =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored.ssh, Some(config.clone()));
        assert_eq!(stored.token, TOKEN);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o077,
            0
        );
        let pid = manager
            .current
            .lock()
            .await
            .as_ref()
            .unwrap()
            .child
            .id()
            .unwrap();
        save_at(&path, &manager, &changes, settings(config.clone(), ""))
            .await
            .unwrap();
        assert_eq!(*receiver.borrow(), 2);
        assert_eq!(
            manager.current.lock().await.as_ref().unwrap().child.id(),
            Some(pid)
        );
        config.user = Some("another-user".into());
        assert!(save_at(&path, &manager, &changes, settings(config, ""))
            .await
            .is_err());
        assert_eq!(*receiver.borrow(), 2);
        assert!(alive(pid));
        manager.disconnect().await;
    }

    #[tokio::test]
    async fn rejected_ssh_save_preserves_settings_tunnel_and_change_generation() {
        use crate::daemon::remote::save_at;
        let server = server(true, amarcode_protocol::PROTOCOL_VERSION).await;
        let (directory, manager) = manager(Duration::from_secs(3));
        let path = directory.path().join("connection.json");
        let (changes, receiver) = watch::channel(0);
        let mut config = config();
        config.daemon_port = server.port;
        save_at(&path, &manager, &changes, settings(config.clone(), TOKEN))
            .await
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        let pid = manager
            .current
            .lock()
            .await
            .as_ref()
            .unwrap()
            .child
            .id()
            .unwrap();
        config.host = "reject".into();
        assert!(save_at(&path, &manager, &changes, settings(config, TOKEN))
            .await
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(*receiver.borrow(), 1);
        assert_eq!(
            manager.current.lock().await.as_ref().unwrap().child.id(),
            Some(pid)
        );
        assert!(alive(pid));
        manager.disconnect().await;
    }

    #[tokio::test]
    async fn failed_commit_discards_candidate_and_keeps_previous_tunnel() {
        let server = server(true, amarcode_protocol::PROTOCOL_VERSION).await;
        let (directory, manager) = manager(Duration::from_secs(3));
        let mut config = config();
        config.daemon_port = server.port;
        let endpoint = manager.connect(&config, TOKEN).await.unwrap();
        let original_pid = manager
            .current
            .lock()
            .await
            .as_ref()
            .unwrap()
            .child
            .id()
            .unwrap();
        config.user = Some("new-user".into());
        let result = manager
            .connect_transaction(&config, TOKEN, true, || Err::<(), _>("disk full".into()))
            .await;
        assert_eq!(result.unwrap_err(), "disk full");
        assert_eq!(
            manager.current.lock().await.as_ref().unwrap().endpoint,
            endpoint
        );
        assert!(alive(original_pid));
        let candidate_pid = std::fs::read_to_string(directory.path().join("pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert_ne!(candidate_pid, original_pid);
        stopped(candidate_pid).await;
        // The same guard rejects an RPC holding an obsolete selected profile.
        let result = manager
            .connect_transaction(&config, TOKEN, false, || {
                Err::<(), _>("Daemon connection settings changed".into())
            })
            .await;
        assert!(result.unwrap_err().contains("settings changed"));
        assert!(alive(original_pid));
        manager.disconnect().await;
    }

    #[tokio::test]
    async fn switching_to_websocket_or_local_stops_ssh_only_after_success() {
        use crate::daemon::remote::{save_at, ConnectionSettings};
        let server = server(true, amarcode_protocol::PROTOCOL_VERSION).await;
        let bad_server = self::server(false, amarcode_protocol::PROTOCOL_VERSION).await;
        let (directory, manager) = manager(Duration::from_secs(3));
        let path = directory.path().join("connection.json");
        let (changes, receiver) = watch::channel(0);
        let mut config = config();
        config.daemon_port = server.port;
        save_at(&path, &manager, &changes, settings(config, TOKEN))
            .await
            .unwrap();
        let pid = manager
            .current
            .lock()
            .await
            .as_ref()
            .unwrap()
            .child
            .id()
            .unwrap();
        let direct = |port| ConnectionSettings {
            url: format!("ws://127.0.0.1:{port}/rpc"),
            token: TOKEN.into(),
            ssh: None,
        };
        assert!(save_at(&path, &manager, &changes, direct(bad_server.port))
            .await
            .is_err());
        assert!(alive(pid));
        assert_eq!(*receiver.borrow(), 1);
        save_at(&path, &manager, &changes, direct(server.port))
            .await
            .unwrap();
        stopped(pid).await;
        assert!(manager.current.lock().await.is_none());
        save_at(&path, &manager, &changes, ConnectionSettings::default())
            .await
            .unwrap();
        let stored: ConnectionSettings =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(stored.ssh.is_none() && stored.url.is_empty() && stored.token.is_empty());
        assert_eq!(*receiver.borrow(), 3);
    }

    #[tokio::test]
    async fn missing_ssh_and_invalid_tokens_fail_without_starting_a_tunnel() {
        let (directory, mut manager) = manager(Duration::from_secs(1));
        assert!(manager
            .connect(&config(), "short")
            .await
            .unwrap_err()
            .contains("token"));
        assert!(!directory.path().join("pid").exists());
        manager.executable = directory.path().join("missing-ssh");
        assert!(manager
            .connect(&config(), TOKEN)
            .await
            .unwrap_err()
            .contains("Cannot start OpenSSH"));
    }
}
