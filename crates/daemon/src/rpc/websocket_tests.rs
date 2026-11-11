use super::*;
use serde_json::{json, Value};
use tokio_tungstenite::{client_async, tungstenite::client::IntoClientRequest};

const CERT: &[u8] = include_bytes!("../../tests/fixtures/websocket-cert.pem");
const KEY: &[u8] = include_bytes!("../../tests/fixtures/websocket-key.pem");

trait TestIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> TestIo for T {}
type Client = WebSocketStream<Box<dyn TestIo>>;

fn private_file(path: &Path, contents: &[u8]) {
    std::fs::write(path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

/// Run a repository script, reporting its output when it fails.
///
/// These tests shell out to Python and OpenSSL, which behave differently per
/// platform. A bare exit status leaves nothing to act on, so surface both
/// streams in the failure message.
fn run_script(command: &mut std::process::Command) -> std::process::Output {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "script failed ({})\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}

fn test_config() -> Config {
    Config {
        transport: crate::config::DaemonTransport::Tcp,
        app_dir: Default::default(),
        db_path: Default::default(),
        default_workspace_path: None,
        daemon_addr: "127.0.0.1:0".into(),
        acp_registry_source: None,
        ws_token_file: None,
        ws_tls_cert: None,
        ws_tls_key: None,
        ws_client_ca: None,
        ws_client_crl: None,
        e2ee: None,
        ws_allowed_origins: Vec::new(),
    }
}

async fn start(
    tls: bool,
) -> (
    Client,
    Arc<App>,
    tokio::task::JoinHandle<Result<()>>,
    tempfile::TempDir,
) {
    start_with_token(tls, &"a".repeat(64)).await
}

async fn start_with_token(
    tls: bool,
    token: &str,
) -> (
    Client,
    Arc<App>,
    tokio::task::JoinHandle<Result<()>>,
    tempfile::TempDir,
) {
    let directory = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.transport = crate::config::DaemonTransport::WebSocket;
    config.daemon_addr = "127.0.0.1:0".into();
    let token_path = directory.path().join("access-token");
    private_file(&token_path, token.as_bytes());
    config.ws_token_file = Some(token_path);
    if tls {
        let cert = directory.path().join("cert.pem");
        let key = directory.path().join("key.pem");
        private_file(&cert, CERT);
        private_file(&key, KEY);
        config.ws_tls_cert = Some(cert);
        config.ws_tls_key = Some(key);
    }
    let listener = Listener::bind(&config).await.unwrap();
    let address = listener.socket.local_addr().unwrap();
    config.daemon_addr = address.to_string();
    config.app_dir = directory.path().to_owned();
    config.db_path = directory.path().join("store.sqlite3");
    let app = Arc::new(App::new(config).await.unwrap());
    let server_app = Arc::clone(&app);
    let server = tokio::spawn(async move {
        let (stream, _) = listener.socket.accept().await?;
        listener
            .state
            .handle(stream, server_app, "test-client")
            .await
    });
    let tcp = TcpStream::connect(address).await.unwrap();
    let io: Box<dyn TestIo> = if tls {
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_pemfile::certs(&mut StdBufReader::new(CERT)) {
            roots.add(cert.unwrap()).unwrap();
        }
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        Box::new(
            connector
                .connect(
                    rustls::pki_types::ServerName::try_from("localhost").unwrap(),
                    tcp,
                )
                .await
                .unwrap(),
        )
    } else {
        Box::new(tcp)
    };
    let scheme = if tls { "wss" } else { "ws" };
    let mut request = format!("{scheme}://localhost:{}/rpc", address.port())
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    let (client, _) = client_async(request, io).await.unwrap();
    (client, app, server, directory)
}

async fn receive(client: &mut Client) -> Value {
    let frame = timeout(Duration::from_secs(5), client.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(frame.to_text().unwrap()).unwrap()
}

async fn request(client: &mut Client, value: Value) -> Value {
    client
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
    receive(client).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_and_wss_support_rpc_large_frames_and_filtered_events() {
    for tls in [false, true] {
        let (mut client, app, server, directory) = start(tls).await;
        let health = request(&mut client, json!({"method":"health"})).await;
        assert_eq!(health["result"]["status"], "ok");
        // Larger than the bounded duplex: protects against a framing/RPC deadlock.
        let response = request(
            &mut client,
            json!({"method":"not_a_method","params":{"payload":"x".repeat(256 * 1024)}}),
        )
        .await;
        assert!(response["error"]
            .as_str()
            .unwrap()
            .contains("unknown method"));
        client.send(Message::Text("{}\n{}".into())).await.unwrap();
        assert!(receive(&mut client).await["error"]
            .as_str()
            .unwrap()
            .contains("one JSON request"));
        let subscribed = request(
            &mut client,
            json!({"method":"subscribe_events","params":{"chat_id":"wanted"}}),
        )
        .await;
        assert_eq!(subscribed["result"]["subscribed"], true);
        app.events
            .send(crate::protocol::EditorEvent::ChatUpdated {
                chat_id: "other".into(),
            })
            .unwrap();
        app.events
            .send(crate::protocol::EditorEvent::ChatUpdated {
                chat_id: "wanted".into(),
            })
            .unwrap();
        let event = receive(&mut client).await;
        assert_eq!(event["event"]["payload"]["chat_id"], "wanted");
        client.close(None).await.unwrap();
        timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        drop(client);
        drop(app);
        directory.close().unwrap();
    }
}

#[test]
fn origin_allowlist_and_rpc_path_are_enforced() {
    fn check(url: &str, origin: Option<&str>, allowed: &[String]) -> bool {
        let mut request = url.into_client_request().unwrap();
        if let Some(origin) = origin {
            request
                .headers_mut()
                .insert("origin", origin.parse().unwrap());
        }
        request
            .headers_mut()
            .insert("authorization", "Bearer test-token".parse().unwrap());
        validate_handshake(&request, Response::new(()), allowed, "test-token").is_ok()
    }
    let origins = vec!["https://app.example.com".to_owned()];
    assert!(check("ws://localhost/rpc", None, &[]));
    assert!(check("ws://localhost/terminal", None, &[]));
    assert!(!check("ws://localhost/terminal?token=secret", None, &[]));
    assert!(!check(
        "ws://localhost/terminal",
        Some("https://evil.example"),
        &origins
    ));
    assert!(!check(
        "ws://localhost/rpc",
        Some("https://evil.example"),
        &[]
    ));
    assert!(!check("ws://localhost/rpc", Some("null"), &origins));
    assert!(check(
        "ws://localhost/rpc",
        Some("https://app.example.com"),
        &origins
    ));
    assert!(!check("ws://localhost/rpc?unexpected=query", None, &[]));
    assert!(!check("ws://localhost/other", None, &[]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_queue_accepts_four_maximum_size_images() {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let (mut client, app, server, directory) = start(true).await;
    let chat = app
        .chats
        .create(directory.path().to_string_lossy(), None)
        .unwrap();
    app.store
        .pause_prompt_queue(&chat.id, Some("Waiting for test"))
        .unwrap();
    let mut image = vec![0; 10 * 1024 * 1024];
    image[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    let attachment = json!({"mime_type":"image/png", "data":STANDARD.encode(image)});
    let value = json!({"method":"enqueue_prompt", "params":{
        "request_id":"maximum-images",
        "prompt":{"chat_id":chat.id,"agent_id":"agent","text":"x".repeat(1024 * 1024),
            "attachments":vec![attachment; 4]}
    }});
    let encoded = value.to_string();
    assert!(encoded.len() > 36 * 1024 * 1024);
    assert!(encoded.len() < amarcode_protocol::MAX_RPC_MESSAGE_BYTES);
    client.send(Message::Text(encoded.into())).await.unwrap();
    // Parsing and persisting this 54 MiB payload takes longer in debug builds.
    let frame = timeout(Duration::from_secs(30), client.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let response: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
    assert_eq!(
        response["result"]["items"][0]["attachment_count"], 4,
        "{response}"
    );
    assert_eq!(response["result"]["items"][0]["status"], "queued");
    client.close(None).await.unwrap();
    timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[test]
fn tls_configuration_is_required_only_for_remote_binds() {
    let mut config = test_config();
    assert!(config.validate().is_ok());
    config.transport = crate::config::DaemonTransport::WebSocket;
    assert!(config.validate().is_err());
    config.ws_token_file = Some("token".into());
    assert!(config.validate().is_ok());
    config.daemon_addr.clear();
    assert!(config.validate().is_err());
    config.daemon_addr = "invalid-address".into();
    assert!(config
        .validate()
        .unwrap_err()
        .to_string()
        .contains("AMARCODE_DAEMON_ADDR"));
    config.daemon_addr = "127.0.0.1:0".into();
    assert!(config.validate().is_ok());
    config.daemon_addr = "0.0.0.0:443".into();
    assert!(config.validate().is_err());
    config.ws_tls_cert = Some("cert".into());
    assert!(config.validate().is_err());
    config.ws_tls_key = Some("key".into());
    assert!(config.validate().is_ok());
    config.ws_allowed_origins = vec!["*".into()];
    assert!(config.validate().is_err());
    config.ws_allowed_origins = vec!["https://example.com/path".into()];
    assert!(config.validate().is_err());
}

#[test]
fn malformed_tls_credentials_are_rejected_before_binding() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.transport = crate::config::DaemonTransport::WebSocket;
    config.daemon_addr = "127.0.0.1:0".into();
    let token = directory.path().join("access-token");
    private_file(&token, "a".repeat(64).as_bytes());
    config.ws_token_file = Some(token);
    assert!(State::prepare(&config).is_ok());
    let cert = directory.path().join("bad-cert");
    let key = directory.path().join("bad-key");
    private_file(&cert, b"bad certificate");
    private_file(&key, b"bad key");
    config.ws_tls_cert = Some(cert);
    config.ws_tls_key = Some(key);
    assert!(State::prepare(&config).is_err());
}

#[test]
#[allow(clippy::result_large_err)] // The handshake validator returns Tungstenite HTTP errors.
fn authenticated_handshake_rejects_missing_wrong_and_duplicate_tokens() {
    let token = "a".repeat(64);
    let mut request = "ws://localhost/rpc".into_client_request().unwrap();
    let check = |request: &Request| validate_handshake(request, Response::new(()), &[], &token);
    assert_eq!(
        check(&request).unwrap_err().status(),
        StatusCode::UNAUTHORIZED
    );
    request
        .headers_mut()
        .insert("authorization", "Bearer wrong".parse().unwrap());
    assert!(check(&request).is_err());
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    assert!(check(&request).is_ok());
    request
        .headers_mut()
        .append("authorization", format!("Bearer {token}").parse().unwrap());
    assert!(check(&request).is_err());
}

#[test]
fn token_files_must_be_private_and_nonempty() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("token");
    private_file(&path, b"short");
    assert!(load_token(&path).is_err());
    private_file(&path, "a".repeat(64).as_bytes());
    assert_eq!(load_token(&path).unwrap().len(), 64);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_token(&path).is_err());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authenticated_ws_and_wss_deliver_rpc_and_events() {
    for tls in [false, true] {
        let token = "a".repeat(64);
        let (mut client, app, server, directory) = start_with_token(tls, &token).await;
        assert_eq!(
            request(&mut client, json!({"method":"health"})).await["result"]["status"],
            "ok"
        );
        assert_eq!(
            request(
                &mut client,
                json!({"method":"subscribe_events","params":{}})
            )
            .await["result"]["subscribed"],
            true
        );
        app.events
            .send(crate::protocol::EditorEvent::ChatUpdated {
                chat_id: "remote".into(),
            })
            .unwrap();
        assert_eq!(
            receive(&mut client).await["event"]["payload"]["chat_id"],
            "remote"
        );
        client.close(None).await.unwrap();
        timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        drop(client);
        drop(app);
        directory.close().unwrap();
    }
}

#[test]
fn tcp_rpc_rejects_non_loopback_addresses() {
    let mut config = test_config();
    for address in ["127.0.0.1:0", "127.0.0.2:43821", "[::1]:43821"] {
        config.daemon_addr = address.into();
        assert!(config.validate().is_ok(), "{address}");
    }
    for address in [
        "0.0.0.0:43821",
        "192.0.2.1:43821",
        "[::]:43821",
        "[2001:db8::1]:43821",
    ] {
        config.daemon_addr = address.into();
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("loopback"),
            "{address}"
        );
    }
}

#[tokio::test]
async fn websocket_requires_valid_token_before_binding() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = test_config();
    config.transport = crate::config::DaemonTransport::WebSocket;
    assert!(
        matches!(Listener::bind(&config).await, Err(error) if error.to_string().contains("AMARCODE_WS_TOKEN_FILE"))
    );
    // State preparation also fails closed if called without config validation.
    assert!(State::prepare(&config).is_err());
    let path = directory.path().join("access-token");
    config.ws_token_file = Some(path.clone());
    assert!(Listener::bind(&config).await.is_err());
    for invalid in ["", "short", &"a".repeat(513), &"!".repeat(64)] {
        private_file(&path, invalid.as_bytes());
        assert!(Listener::bind(&config).await.is_err());
    }
    config.ws_token_file = Some(directory.path().to_owned());
    assert!(Listener::bind(&config).await.is_err());
    config.ws_token_file = Some(path.clone());
    private_file(&path, "a".repeat(64).as_bytes());
    assert!(Listener::bind(&config).await.is_ok());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Listener::bind(&config).await.is_err());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browser_ticket_opens_real_rpc_and_rejects_replay() {
    let (_desktop, app, desktop_server, directory) = start(false).await;
    desktop_server.abort();
    let state = State::prepare(&app.config).unwrap();
    let state = Arc::new(state);
    let exchange = async {
        let (mut client, server) = tokio::io::duplex(32768);
        let service = state.handle_stream(server, Arc::clone(&app), "browser-ticket", None);
        let request = async {
            client.write_all(format!("POST /web/session HTTP/1.1\r\nHost: localhost\r\nOrigin: https://remote.amarcode.amarjay.com\r\nAuthorization: Bearer {}\r\n\r\n", "a".repeat(64)).as_bytes()).await.unwrap();
            let mut response = String::new();
            tokio::io::AsyncReadExt::read_to_string(&mut client, &mut response).await.unwrap();
            serde_json::from_str::<Value>(response.split("\r\n\r\n").nth(1).unwrap()).unwrap()
        };
        let (served, ticket) = tokio::join!(service, request);
        served.unwrap(); ticket
    }.await;
    let ticket = exchange["ticket"].as_str().unwrap();
    for accepted in [true, false] {
        let (client, server) = tokio::io::duplex(32768);
        let mut request = "ws://localhost/rpc".into_client_request().unwrap();
        request.headers_mut().insert(
            "origin",
            "https://remote.amarcode.amarjay.com".parse().unwrap(),
        );
        request.headers_mut().insert(
            "sec-websocket-protocol",
            format!("amarcode.rpc.v1, amarcode.ticket.{ticket}")
                .parse()
                .unwrap(),
        );
        let service_state = Arc::clone(&state);
        let service_app = Arc::clone(&app);
        let service = tokio::spawn(async move {
            service_state
                .handle_stream(server, service_app, "browser", None)
                .await
        });
        let result = client_async(request, client).await;
        if accepted {
            let (mut socket, response) = result.unwrap();
            assert_eq!(
                response.headers()["sec-websocket-protocol"],
                "amarcode.rpc.v1"
            );
            socket
                .send(Message::Text(
                    "{\"method\":\"health\",\"params\":{}}\n".into(),
                ))
                .await
                .unwrap();
            let reply = timeout(Duration::from_secs(5), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let health: Value = serde_json::from_str(reply.to_text().unwrap()).unwrap();
            assert_eq!(health["result"]["remote_auth_required"], true);
            socket.close(None).await.unwrap();
        } else {
            assert!(result.is_err());
        }
        service.abort();
    }
    drop(directory);
}

#[tokio::test]
async fn mutual_tls_requires_trusted_client_cert_and_valid_server_identity() {
    let root = tempfile::tempdir().unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/tls-certificates.py");
    for name in ["trusted", "untrusted"] {
        run_script(
            std::process::Command::new("python3")
                .arg(&script)
                .args(["dev-ca", "--hostname", "localhost", "--output"])
                .arg(root.path().join(name)),
        );
    }
    let trusted = root.path().join("trusted");
    for (identity, server_name, expected) in [
        (Some("trusted"), "localhost", true),
        (None, "localhost", false),
        (Some("untrusted"), "localhost", false),
        (Some("trusted"), "wrong.example", false),
    ] {
        let mut config = test_config();
        config.transport = crate::config::DaemonTransport::WebSocket;
        config.app_dir = root.path().join(uuid::Uuid::new_v4().to_string());
        config.db_path = config.app_dir.join("store.sqlite3");
        let token = root.path().join("token");
        private_file(&token, "a".repeat(64).as_bytes());
        config.ws_token_file = Some(token);
        config.ws_tls_cert = Some(trusted.join("server-cert.pem"));
        config.ws_tls_key = Some(trusted.join("server-key.pem"));
        config.ws_client_ca = Some(trusted.join("ca.pem"));
        let listener = Listener::bind(&config).await.unwrap();
        let address = listener.socket.local_addr().unwrap();
        let app = Arc::new(App::new(config).await.unwrap());
        let server = tokio::spawn(async move {
            let (stream, _) = listener.socket.accept().await.unwrap();
            listener.state.handle(stream, app, "mtls-test").await
        });
        let directory = identity.map(|name| root.path().join(name));
        let client = amarcode_transport::client_config_with(
            Some(&trusted.join("ca.pem")),
            directory
                .as_ref()
                .map(|p| p.join("client-cert.pem"))
                .as_deref(),
            directory
                .as_ref()
                .map(|p| p.join("client-key.pem"))
                .as_deref(),
        )
        .unwrap();
        let tcp = TcpStream::connect(address).await.unwrap();
        let result = tokio_rustls::TlsConnector::from(Arc::new(client))
            .connect(
                rustls::pki_types::ServerName::try_from(server_name).unwrap(),
                tcp,
            )
            .await;
        let success = if let Ok(tls) = result {
            let mut request = "wss://localhost/rpc".into_client_request().unwrap();
            request.headers_mut().insert(
                "authorization",
                format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
            );
            match client_async(request, tls).await {
                Ok((mut socket, _)) => {
                    socket
                        .send(Message::Text(r#"{"method":"health"}"#.into()))
                        .await
                        .unwrap();
                    let response = socket.next().await.unwrap().unwrap().into_text().unwrap();
                    assert!(response.contains("ok"));
                    true
                }
                Err(_) => false,
            }
        } else {
            false
        };
        assert_eq!(success, expected, "client={identity:?}, name={server_name}");
        let _ = tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap();
    }
}

async fn encrypted_fixture() -> (
    tempfile::TempDir,
    Arc<App>,
    tokio::task::JoinHandle<Result<()>>,
    String,
) {
    let root = tempfile::tempdir().unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/tls-certificates.py");
    run_script(
        std::process::Command::new("python3")
            .arg(script)
            .args(["dev-ca", "--hostname", "localhost", "--output"])
            .arg(root.path().join("tls")),
    );
    let mut config = test_config();
    config.transport = crate::config::DaemonTransport::WebSocket;
    config.app_dir = root.path().join("data");
    config.db_path = config.app_dir.join("store.sqlite3");
    let token = root.path().join("token");
    private_file(&token, "a".repeat(64).as_bytes());
    config.ws_token_file = Some(token);
    config.e2ee = Some(amarcode_transport::TlsIdentity {
        cert: root.path().join("tls/server-cert.pem"),
        key: root.path().join("tls/server-key.pem"),
        client_ca: Some(root.path().join("tls/ca.pem")),
        client_crl: Some(root.path().join("tls/clients.crl.pem")),
    });
    let listener = Listener::bind(&config).await.unwrap();
    let url = format!("ws://{}/rpc", listener.socket.local_addr().unwrap());
    let app = Arc::new(App::new(config).await.unwrap());
    let server_app = Arc::clone(&app);
    let server = tokio::spawn(async move {
        crate::rpc::server::run(
            server_app,
            crate::rpc::server::Listener::WebSocket(listener),
        )
        .await
    });
    (root, app, server, url)
}
fn paired_client(root: &Path) -> amarcode_transport::encrypted::ClientIdentity {
    amarcode_transport::encrypted::ClientIdentity {
        tls: Arc::new(
            amarcode_transport::client_config_with_crl(
                Some(&root.join("tls/ca.pem")),
                Some(&root.join("tls/client-cert.pem")),
                Some(&root.join("tls/client-key.pem")),
                None,
                true,
            )
            .unwrap(),
        ),
        server_name: "localhost".into(),
    }
}
fn outer_config() -> Arc<rustls::ClientConfig> {
    Arc::new(amarcode_transport::client_config_with(None, None, None).unwrap())
}

#[tokio::test]
async fn encrypted_rpc_rejects_unpaired_legacy_and_revoked_clients_without_restart() {
    let (root, app, server, url) = encrypted_fixture().await;
    let mut client = amarcode_transport::encrypted::connect_with(
        &url,
        &"a".repeat(64),
        amarcode_protocol::MAX_RPC_MESSAGE_BYTES,
        Some(paired_client(root.path())),
        outer_config(),
    )
    .await
    .unwrap();
    client
        .send(Message::Text(r#"{"method":"health"}"#.into()))
        .await
        .unwrap();
    assert!(client
        .next()
        .await
        .unwrap()
        .unwrap()
        .to_text()
        .unwrap()
        .contains("ok"));
    assert!(amarcode_transport::encrypted::connect_with(
        &url,
        &"a".repeat(64),
        4096,
        None,
        outer_config()
    )
    .await
    .is_err());
    let unpaired = amarcode_transport::encrypted::ClientIdentity {
        tls: Arc::new(
            amarcode_transport::client_config_with_crl(
                Some(&root.path().join("tls/ca.pem")),
                None,
                None,
                None,
                true,
            )
            .unwrap(),
        ),
        server_name: "localhost".into(),
    };
    assert!(amarcode_transport::encrypted::connect_with(
        &url,
        &"a".repeat(64),
        4096,
        Some(unpaired),
        outer_config()
    )
    .await
    .is_err());
    drop(client);
    let original = std::fs::read(root.path().join("tls/clients.crl.pem")).unwrap();
    let expired = std::process::Command::new("openssl")
        .current_dir(root.path().join("tls"))
        .args([
            "ca",
            "-gencrl",
            "-config",
            "ca.cnf",
            "-crl_lastupdate",
            "20200101000000Z",
            "-crl_nextupdate",
            "20200102000000Z",
            "-out",
            "expired.pem",
            "-batch",
        ])
        .output()
        .unwrap();
    assert!(
        expired.status.success(),
        "{}",
        String::from_utf8_lossy(&expired.stderr)
    );
    std::fs::copy(
        root.path().join("tls/expired.pem"),
        root.path().join("tls/clients.crl.pem"),
    )
    .unwrap();
    assert!(amarcode_transport::encrypted::connect_with(
        &url,
        &"a".repeat(64),
        4096,
        Some(paired_client(root.path())),
        outer_config()
    )
    .await
    .is_err());
    // A syntactically valid CRL with a forged signature must also fail closed.
    use base64::Engine as _;
    let pem = String::from_utf8(original.clone()).unwrap();
    let encoded: String = pem
        .lines()
        .filter(|line| !line.starts_with("---"))
        .collect();
    let mut der = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    let last = der.len() - 1;
    der[last] ^= 1;
    let forged = format!(
        "-----BEGIN X509 CRL-----\n{}\n-----END X509 CRL-----\n",
        base64::engine::general_purpose::STANDARD.encode(der)
    );
    std::fs::write(root.path().join("tls/clients.crl.pem"), forged).unwrap();
    assert!(amarcode_transport::encrypted::connect_with(
        &url,
        &"a".repeat(64),
        4096,
        Some(paired_client(root.path())),
        outer_config()
    )
    .await
    .is_err());
    std::fs::write(root.path().join("tls/clients.crl.pem"), original).unwrap();
    let mut client = amarcode_transport::encrypted::connect_with(
        &url,
        &"a".repeat(64),
        4096,
        Some(paired_client(root.path())),
        outer_config(),
    )
    .await
    .unwrap();
    // CRL changes are picked up by the existing server; no restart or cached
    // TLS session can bypass revocation.
    let revoke = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/tls-crl.py");
    run_script(
        std::process::Command::new("python3")
            .arg(revoke)
            .arg("--ca-directory")
            .arg(root.path().join("tls"))
            .args(["--revoke", "client-cert.pem"]),
    );
    assert!(amarcode_transport::encrypted::connect_with(
        &url,
        &"a".repeat(64),
        4096,
        Some(paired_client(root.path())),
        outer_config()
    )
    .await
    .is_err());
    let closed = tokio::time::timeout(Duration::from_secs(2), client.next())
        .await
        .unwrap();
    assert!(!matches!(closed, Some(Ok(Message::Text(_)))));
    drop(client);
    // Corrupt or missing configured CRLs are never replaced with an allow-all policy.
    std::fs::write(root.path().join("tls/clients.crl.pem"), "invalid").unwrap();
    assert!(amarcode_transport::encrypted::connect_with(
        &url,
        &"a".repeat(64),
        4096,
        Some(paired_client(root.path())),
        outer_config()
    )
    .await
    .is_err());
    std::fs::remove_file(root.path().join("tls/clients.crl.pem")).unwrap();
    assert!(amarcode_transport::encrypted::connect_with(
        &url,
        &"a".repeat(64),
        4096,
        Some(paired_client(root.path())),
        outer_config()
    )
    .await
    .is_err());
    app.request_shutdown();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn encrypted_probe_authenticates_origin_without_bearer_or_client_identity() {
    let (root, app, server, url) = encrypted_fixture().await;
    let identity = amarcode_transport::encrypted::ClientIdentity {
        tls: Arc::new(
            amarcode_transport::client_config_with_crl(
                Some(&root.path().join("tls/ca.pem")),
                None,
                None,
                None,
                true,
            )
            .unwrap(),
        ),
        server_name: "localhost".into(),
    };
    let mut socket = amarcode_transport::encrypted::connect_with(
        &url.replace("/rpc", "/e2ee-health"),
        "",
        4096,
        Some(identity),
        outer_config(),
    )
    .await
    .unwrap();
    let response = socket.next().await.unwrap().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(response.to_text().unwrap()).unwrap(),
        json!({"status":"ok","end_to_end":true})
    );
    // Probe never accepts application requests, even with an unpaired client.
    let _ = socket
        .send(Message::Text(r#"{"method":"health"}"#.into()))
        .await;
    assert!(!matches!(socket.next().await, Some(Ok(Message::Text(_)))));
    drop(socket);
    app.request_shutdown();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn relay_cannot_read_tokens_or_rpc_and_tampered_or_replayed_records_fail() {
    use std::sync::{
        atomic::{AtomicU8, Ordering},
        Mutex,
    };
    for corrupt in [1, 2] {
        let (root, app, server, url) = encrypted_fixture().await;
        let relay = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_url = format!("ws://{}/rpc", relay.local_addr().unwrap());
        let capture = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let traffic = Arc::clone(&capture);
        let mode = Arc::new(AtomicU8::new(0));
        let attack = Arc::clone(&mode);
        let relay_task = tokio::spawn(async move {
            let (tcp, _) = relay.accept().await.unwrap();
            let mut front = accept_hdr_async_with_config(
                tcp,
                |request: &Request, mut response: Response| {
                    assert!(!request.headers().contains_key("authorization"));
                    assert_eq!(
                        request.headers().get("sec-websocket-protocol").unwrap(),
                        amarcode_transport::encrypted::PROTOCOL
                    );
                    response.headers_mut().insert(
                        "sec-websocket-protocol",
                        amarcode_transport::encrypted::PROTOCOL.parse().unwrap(),
                    );
                    Ok(response)
                },
                None,
            )
            .await
            .unwrap();
            let mut request = url.into_client_request().unwrap();
            request.headers_mut().insert(
                "sec-websocket-protocol",
                amarcode_transport::encrypted::PROTOCOL.parse().unwrap(),
            );
            let (mut back, _) = tokio_tungstenite::connect_async(request).await.unwrap();
            let mut previous = None;
            loop {
                tokio::select! {
                    message = front.next() => {
                        let Some(Ok(mut message)) = message else { break; };
                        if let Message::Binary(bytes) = &message {
                            traffic.lock().unwrap().push(bytes.to_vec());
                            let original = message.clone();
                            match attack.swap(0, Ordering::SeqCst) {
                                1 => { let mut damaged = bytes.to_vec(); let last = damaged.len()-1; damaged[last] ^= 1; message = Message::Binary(damaged.into()); },
                                2 => { message = previous.clone().unwrap(); },
                                _ => {},
                            }
                            previous = Some(original);
                        }
                        if back.send(message).await.is_err() { break; }
                    },
                    message = back.next() => {
                        let Some(Ok(message)) = message else { break; };
                        if let Message::Binary(bytes) = &message { traffic.lock().unwrap().push(bytes.to_vec()); }
                        if front.send(message).await.is_err() { break; }
                    }
                }
            }
        });
        let mut socket = amarcode_transport::encrypted::connect_with(
            &relay_url,
            &"a".repeat(64),
            amarcode_protocol::MAX_RPC_MESSAGE_BYTES,
            Some(paired_client(root.path())),
            outer_config(),
        )
        .await
        .unwrap();
        let marker = format!("private-rpc-{}", uuid::Uuid::new_v4());
        socket
            .send(Message::Text(
                json!({"method":marker,"params":{"attachment":"s".repeat(256 * 1024)}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        assert!(socket
            .next()
            .await
            .unwrap()
            .unwrap()
            .to_text()
            .unwrap()
            .contains(&marker));
        for bytes in capture.lock().unwrap().iter() {
            assert!(!bytes
                .windows(marker.len())
                .any(|window| window == marker.as_bytes()));
            assert!(!bytes
                .windows(64)
                .any(|window| window == "a".repeat(64).as_bytes()));
        }
        mode.store(corrupt, Ordering::SeqCst);
        socket
            .send(Message::Text(r#"{"method":"health"}"#.into()))
            .await
            .unwrap();
        let response = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap();
        assert!(
            !matches!(response, Some(Ok(Message::Text(_)))),
            "altered or replayed records must never dispatch"
        );
        drop(socket);
        tokio::time::timeout(Duration::from_secs(2), relay_task)
            .await
            .unwrap()
            .unwrap();
        app.request_shutdown();
        server.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn direct_mutual_tls_reloads_crls_and_disconnects_revoked_clients() {
    let (root, original_app, original_server, _) = encrypted_fixture().await;
    original_app.request_shutdown();
    original_server.await.unwrap().unwrap();
    drop(original_app);
    let mut config = test_config();
    config.transport = crate::config::DaemonTransport::WebSocket;
    config.app_dir = root.path().join("direct-data");
    config.db_path = config.app_dir.join("store.sqlite3");
    config.ws_token_file = Some(root.path().join("token"));
    config.ws_tls_cert = Some(root.path().join("tls/server-cert.pem"));
    config.ws_tls_key = Some(root.path().join("tls/server-key.pem"));
    config.ws_client_ca = Some(root.path().join("tls/ca.pem"));
    config.ws_client_crl = Some(root.path().join("tls/clients.crl.pem"));
    let listener = Listener::bind(&config).await.unwrap();
    let url = format!(
        "wss://localhost:{}/rpc",
        listener.socket.local_addr().unwrap().port()
    );
    let app = Arc::new(App::new(config).await.unwrap());
    let serve = Arc::clone(&app);
    let server = tokio::spawn(async move {
        crate::rpc::server::run(serve, crate::rpc::server::Listener::WebSocket(listener)).await
    });
    let outer = Arc::new(
        amarcode_transport::client_config_with_crl(
            Some(&root.path().join("tls/ca.pem")),
            Some(&root.path().join("tls/client-cert.pem")),
            Some(&root.path().join("tls/client-key.pem")),
            None,
            false,
        )
        .unwrap(),
    );
    let mut client = amarcode_transport::encrypted::connect_with(
        &url,
        &"a".repeat(64),
        4096,
        None,
        Arc::clone(&outer),
    )
    .await
    .unwrap();
    client
        .send(Message::Text(r#"{"method":"health"}"#.into()))
        .await
        .unwrap();
    assert!(client
        .next()
        .await
        .unwrap()
        .unwrap()
        .to_text()
        .unwrap()
        .contains("ok"));
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/tls-crl.py");
    run_script(
        std::process::Command::new("python3")
            .arg(script)
            .arg("--ca-directory")
            .arg(root.path().join("tls"))
            .args(["--revoke", "client-cert.pem"]),
    );
    assert!(
        amarcode_transport::encrypted::connect_with(&url, &"a".repeat(64), 4096, None, outer)
            .await
            .is_err()
    );
    let response = tokio::time::timeout(Duration::from_secs(2), client.next())
        .await
        .unwrap();
    assert!(!matches!(response, Some(Ok(Message::Text(_)))));
    app.request_shutdown();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn encrypted_event_subscriptions_and_terminal_control_use_the_inner_channel() {
    let (root, app, server, url) = encrypted_fixture().await;
    let mut events = amarcode_transport::encrypted::connect_with(
        &url,
        &"a".repeat(64),
        4096,
        Some(paired_client(root.path())),
        outer_config(),
    )
    .await
    .unwrap();
    events
        .send(Message::Text(
            r#"{"method":"subscribe_events","params":{}}"#.into(),
        ))
        .await
        .unwrap();
    assert!(events
        .next()
        .await
        .unwrap()
        .unwrap()
        .to_text()
        .unwrap()
        .contains("subscribed"));
    app.events
        .send(crate::protocol::EditorEvent::ChatUpdated {
            chat_id: "encrypted-chat-marker".into(),
        })
        .unwrap();
    assert!(events
        .next()
        .await
        .unwrap()
        .unwrap()
        .to_text()
        .unwrap()
        .contains("encrypted-chat-marker"));
    drop(events);
    let mut terminal = amarcode_transport::encrypted::connect_with(
        &url.replace("/rpc", "/terminal"),
        &"a".repeat(64),
        4096,
        Some(paired_client(root.path())),
        outer_config(),
    )
    .await
    .unwrap();
    // Reach the real terminal handler without starting a PTY or invoking a shell.
    terminal
        .send(Message::Text(
            r#"{"type":"resize","cols":80,"rows":24}"#.into(),
        ))
        .await
        .unwrap();
    let response = terminal.next().await.unwrap().unwrap();
    assert!(response
        .to_text()
        .unwrap()
        .contains("Expected terminal open"));
    drop(terminal);
    app.request_shutdown();
    server.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn revocation_watchdog_closes_the_socket_while_handler_is_blocked() {
    use std::io::Read;
    let (root, app, server, _) = encrypted_fixture().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let (socket, _) = listener.accept().await.unwrap();
    let socket = socket.into_std().unwrap();
    let state = State::prepare(&app.config).unwrap();
    let identity = app.config.e2ee.clone().unwrap();
    let chain = amarcode_transport::certificates(&root.path().join("tls/client-cert.pem")).unwrap();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let blocked = tokio::spawn(async move {
        state
            .with_revocation(
                async move {
                    tokio::task::block_in_place(|| {
                        started.send(()).unwrap();
                        let _ = wait.recv();
                    });
                    Ok(())
                },
                Some(&identity),
                chain,
                Some(&socket),
            )
            .await
    });
    ready.await.unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/tls-crl.py");
    run_script(
        std::process::Command::new("python3")
            .arg(script)
            .arg("--ca-directory")
            .arg(root.path().join("tls"))
            .args(["--revoke", "client-cert.pem"]),
    );
    let read = tokio::task::block_in_place(|| client.read(&mut [0]));
    assert!(
        matches!(read, Ok(0))
            || read.as_ref().is_err_and(|e| matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
            )),
        "revocation must close the socket while the handler is blocked: {read:?}"
    );
    release.send(()).unwrap();
    let _ = blocked.await.unwrap();
    app.request_shutdown();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn paired_clients_check_server_revocation_on_each_new_connection() {
    let (root, app, server, url) = encrypted_fixture().await;
    let client = || amarcode_transport::encrypted::ClientIdentity {
        tls: Arc::new(
            amarcode_transport::client_config_with_crl(
                Some(&root.path().join("tls/ca.pem")),
                Some(&root.path().join("tls/client-cert.pem")),
                Some(&root.path().join("tls/client-key.pem")),
                Some(&root.path().join("tls/clients.crl.pem")),
                true,
            )
            .unwrap(),
        ),
        server_name: "localhost".into(),
    };
    let socket = amarcode_transport::encrypted::connect_with(
        &url,
        &"a".repeat(64),
        4096,
        Some(client()),
        outer_config(),
    )
    .await
    .unwrap();
    drop(socket);
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/tls-crl.py");
    run_script(
        std::process::Command::new("python3")
            .arg(script)
            .arg("--ca-directory")
            .arg(root.path().join("tls"))
            .args(["--revoke", "server-cert.pem"]),
    );
    assert!(amarcode_transport::encrypted::connect_with(
        &url,
        &"a".repeat(64),
        4096,
        Some(client()),
        outer_config()
    )
    .await
    .is_err());
    app.request_shutdown();
    server.await.unwrap().unwrap();
}
