//! JSON-line TCP locally, authenticated WebSocket remotely; one socket per call.
use crate::config::Config;
use amarcode_protocol::rpc::{methods, SubscribeEventsParams};
use futures_util::{SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
};
use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};

pub(super) enum Connection {
    Tcp {
        reader: Lines<BufReader<OwnedReadHalf>>,
        writer: OwnedWriteHalf,
    },
    WebSocket(Box<WebSocketStream<MaybeTlsStream<TcpStream>>>),
}
impl Connection {
    pub(super) async fn remote(url: &str, token: &str) -> Result<Self, String> {
        super::remote::validate(url, token)?;
        let mut request = url
            .into_client_request()
            .map_err(|_| "Invalid remote daemon URL")?;
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {token}")
                .parse()
                .map_err(|_| "Invalid access token")?,
        );
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(36 * 1024 * 1024))
            .max_frame_size(Some(36 * 1024 * 1024));
        // The workspace enables multiple rustls crypto backends; choose one
        // explicitly rather than relying on process-global provider inference.
        let roots = tokio_rustls::rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let tls = tokio_rustls::rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
            tokio_rustls::rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let connector = tokio_tungstenite::Connector::Rustls(std::sync::Arc::new(tls));
        let (socket, _) = tokio::time::timeout(
            Duration::from_secs(15),
            tokio_tungstenite::connect_async_tls_with_config(
                request,
                Some(config),
                false,
                Some(connector),
            ),
        )
        .await
        .map_err(|_| "Remote daemon connection timed out")?
        .map_err(|error| format!("Remote daemon connection failed: {error}"))?;
        Ok(Self::WebSocket(Box::new(socket)))
    }
    async fn connect() -> Result<Self, String> {
        let settings = super::remote::load()?;
        if !settings.url.is_empty() {
            return Self::remote(&settings.url, &settings.token).await;
        }
        let mut delay = Duration::from_millis(25);
        let mut last_error = String::new();
        for attempt in 0..8 {
            match TcpStream::connect(&Config::get().daemon_addr).await {
                Ok(stream) => {
                    let (reader, writer) = stream.into_split();
                    return Ok(Self::Tcp {
                        reader: BufReader::new(reader).lines(),
                        writer,
                    });
                }
                Err(error) => last_error = error.to_string(),
            }
            if attempt < 7 {
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_millis(250));
            }
        }
        Err(format!("Daemon is unavailable: {last_error}"))
    }
    async fn send(&mut self, value: Value) -> Result<(), String> {
        let text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        match self {
            Self::Tcp { writer, .. } => writer
                .write_all(format!("{text}\n").as_bytes())
                .await
                .map_err(|e| e.to_string()),
            Self::WebSocket(socket) => tokio::time::timeout(
                Duration::from_secs(15),
                socket.send(Message::Text(text.into())),
            )
            .await
            .map_err(|_| "Remote daemon write timed out")?
            .map_err(|e| e.to_string()),
        }
    }
    async fn receive(&mut self) -> Result<Value, String> {
        match self {
            Self::Tcp { reader, .. } => {
                let line = reader
                    .next_line()
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or("Daemon closed the connection")?;
                serde_json::from_str(&line).map_err(|e| e.to_string())
            }
            Self::WebSocket(socket) => loop {
                let frame = tokio::time::timeout(Duration::from_secs(90), socket.next())
                    .await
                    .map_err(|_| "Remote daemon heartbeat timed out")?
                    .ok_or("Remote daemon closed the connection")?
                    .map_err(|e| e.to_string())?;
                match frame {
                    Message::Text(text) => {
                        return serde_json::from_str(&text).map_err(|e| e.to_string())
                    }
                    Message::Ping(data) => {
                        tokio::time::timeout(
                            Duration::from_secs(15),
                            socket.send(Message::Pong(data)),
                        )
                        .await
                        .map_err(|_| "Remote heartbeat write timed out")?
                        .map_err(|e| e.to_string())?;
                    }
                    Message::Pong(_) => {}
                    Message::Close(_) => return Err("Remote daemon closed the connection".into()),
                    _ => return Err("Remote daemon sent a non-text RPC message".into()),
                }
            },
        }
    }
    pub(super) async fn call<T: DeserializeOwned>(
        &mut self,
        method: &str,
        params: impl Serialize,
    ) -> Result<T, String> {
        self.send(serde_json::json!({ "method": method, "params": params }))
            .await?;
        let response = self.receive().await?;
        if let Some(error) = response.get("error").and_then(Value::as_str) {
            return Err(error.into());
        }
        serde_json::from_value(response.get("result").cloned().unwrap_or(Value::Null))
            .map_err(|e| e.to_string())
    }
}

pub struct DaemonBridge;
pub struct EventSubscription {
    connection: Connection,
}
impl DaemonBridge {
    pub fn new() -> Self {
        Self
    }
    pub async fn call<T: DeserializeOwned>(
        &mut self,
        method: &str,
        params: impl Serialize,
    ) -> Result<T, String> {
        Connection::connect().await?.call(method, params).await
    }
    pub async fn subscribe(params: SubscribeEventsParams) -> Result<EventSubscription, String> {
        let mut connection = Connection::connect().await?;
        let acknowledgement: Value = connection.call(methods::SUBSCRIBE_EVENTS, params).await?;
        if acknowledgement.get("subscribed").and_then(Value::as_bool) != Some(true) {
            return Err("Invalid subscription acknowledgement".into());
        }
        Ok(EventSubscription { connection })
    }
}
impl EventSubscription {
    pub async fn next<T: DeserializeOwned>(&mut self) -> Result<T, String> {
        let envelope = self.connection.receive().await?;
        serde_json::from_value(
            envelope
                .get("event")
                .cloned()
                .ok_or("Invalid daemon event envelope")?,
        )
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn remote_wss_rejects_untrusted_certificates_without_crypto_provider_panics() {
        use tokio_rustls::rustls;
        let cert = rustls::pki_types::CertificateDer::from(
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../daemon/tests/fixtures/websocket-cert.pem"
            ))
            .as_slice(),
        );
        // Decode the PEM fixtures through the already available OpenSSL-free
        // base64 parser; the fixture contains exactly one certificate/key.
        fn pem(bytes: &[u8]) -> Vec<u8> {
            use base64::Engine;
            let encoded = std::str::from_utf8(bytes)
                .unwrap()
                .lines()
                .filter(|line| !line.starts_with("---"))
                .collect::<String>();
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap()
        }
        let certificate = rustls::pki_types::CertificateDer::from(pem(cert.as_ref()));
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(pem(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../daemon/tests/fixtures/websocket-key.pem"
        ))));
        let tls = rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key.into())
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(tls));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("wss://{}/rpc", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            assert!(acceptor.accept(stream).await.is_err());
        });
        let result = Connection::remote(&url, &"a".repeat(64)).await;
        assert!(result.is_err());
        server.await.unwrap();
    }

    #[tokio::test]
    #[allow(clippy::result_large_err)] // Tungstenite fixes the handshake callback error type.
    async fn remote_rpc_and_subscription_handle_authentication_and_heartbeats() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/rpc", listener.local_addr().unwrap());
        let token = "a".repeat(64);
        let expected = format!("Bearer {token}");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_hdr_async(
                stream,
                |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                    assert_eq!(request.headers()["authorization"], expected);
                    assert_eq!(request.uri().path(), "/rpc");
                    Ok(response)
                },
            )
            .await
            .unwrap();
            let request: Value =
                serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            assert_eq!(request["method"], "health");
            socket.send(Message::Ping(vec![1, 2].into())).await.unwrap();
            assert!(matches!(
                socket.next().await.unwrap().unwrap(),
                Message::Pong(_)
            ));
            socket
                .send(Message::Text(r#"{"result":{"status":"ok"}}"#.into()))
                .await
                .unwrap();
            let request: Value =
                serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            assert_eq!(request["method"], "subscribe_events");
            socket
                .send(Message::Text(r#"{"result":{"subscribed":true}}"#.into()))
                .await
                .unwrap();
            socket
                .send(Message::Text(
                    r#"{"event":{"type":"chatUpdated","payload":{"chat_id":"remote-chat"}}}"#
                        .into(),
                ))
                .await
                .unwrap();
            socket.close(None).await.unwrap();
        });
        let mut connection = Connection::remote(&url, &token).await.unwrap();
        let health: Value = connection.call("health", ()).await.unwrap();
        assert_eq!(health["status"], "ok");
        let ack: Value = connection
            .call("subscribe_events", SubscribeEventsParams::default())
            .await
            .unwrap();
        assert_eq!(ack["subscribed"], true);
        let mut subscription = EventSubscription { connection };
        let event = subscription
            .next::<amarcode_protocol::EditorEvent>()
            .await
            .unwrap();
        assert!(
            matches!(event, amarcode_protocol::EditorEvent::ChatUpdated { chat_id } if chat_id == "remote-chat")
        );
        assert!(subscription.next::<Value>().await.is_err());
        server.await.unwrap();
    }
}
