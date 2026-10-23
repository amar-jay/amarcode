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
        daemon_addr: "127.0.0.1:0".into(),
        acp_registry_source: None,
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
    start_with_token(tls, None).await
}

async fn start_with_token(
    tls: bool,
    token: Option<&str>,
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
    if tls {
        let cert = directory.path().join("cert.pem");
        let key = directory.path().join("key.pem");
        private_file(&cert, CERT);
        private_file(&key, KEY);
        config.ws_tls_cert = Some(cert);
        config.ws_tls_key = Some(key);
    }
    let mut listener = Listener::bind(&config).await.unwrap();
    Arc::get_mut(&mut listener.state).unwrap().bearer_token = token.map(str::to_owned);
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
    if let Some(token) = token {
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
    }
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
        validate_handshake(&request, Response::new(()), allowed, None).is_ok()
    }
    let origins = vec!["https://app.example.com".to_owned()];
    assert!(check("ws://localhost/rpc", None, &[]));
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
    let check =
        |request: &Request| validate_handshake(request, Response::new(()), &[], Some(&token));
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
        let (mut client, app, server, directory) = start_with_token(tls, Some(&token)).await;
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
