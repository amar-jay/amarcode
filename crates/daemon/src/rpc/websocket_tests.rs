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
        let service = state.handle_stream(server, Arc::clone(&app), "browser-ticket");
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
                .handle_stream(server, service_app, "browser")
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
