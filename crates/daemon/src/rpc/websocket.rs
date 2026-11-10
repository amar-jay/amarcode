//! WS/WSS framing around the same RPC engine used by TCP.

use std::{fs::File, io::BufReader as StdBufReader, path::Path, sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::{timeout, Instant},
};
use tokio_rustls::{rustls, TlsAcceptor};
use tokio_tungstenite::{
    accept_hdr_async_with_config,
    tungstenite::{
        handshake::server::{ErrorResponse, Request, Response},
        http::StatusCode,
        protocol::WebSocketConfig,
        Message,
    },
    WebSocketStream,
};
use tracing::info;

use crate::{protocol::rpc::RpcResponse, App, Config, Error, Result};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(15);
pub(super) const MAX_CONNECTIONS: usize = 64;

pub struct Listener {
    pub(super) socket: TcpListener,
    pub(super) state: Arc<State>,
}

pub(super) struct State {
    tls: Option<TlsAcceptor>,
    origins: Vec<String>,
    browser: super::web_auth::BrowserAuth,
    bearer_token: String,
    address: String,
}

impl Listener {
    #[cfg(test)]
    pub(crate) fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.socket.local_addr()
    }
    pub async fn bind(config: &Config) -> Result<Self> {
        config.validate()?;
        let state = Arc::new(State::prepare(config)?);
        let socket = TcpListener::bind(&config.daemon_addr).await?;
        info!(address = %socket.local_addr()?, transport = if state.tls.is_some() { "wss" } else { "ws" }, path = "/rpc", client_certificate_required = config.ws_client_ca.is_some(), "WebSocket RPC listening");
        Ok(Self { socket, state })
    }
}

impl State {
    fn prepare(config: &Config) -> Result<Self> {
        let token_path = config.ws_token_file.as_deref().ok_or_else(|| {
            Error::msg("WebSocket RPC requires AMARCODE_WS_TOKEN_FILE pointing to a private access token file")
        })?;
        let bearer_token = load_token(token_path)?;
        let tls = match (&config.ws_tls_cert, &config.ws_tls_key) {
            (Some(cert_path), Some(key_path)) => {
                check_private_file(key_path)?;
                let certs = rustls_pemfile::certs(&mut StdBufReader::new(File::open(cert_path)?))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let key =
                    rustls_pemfile::private_key(&mut StdBufReader::new(File::open(key_path)?))?
                        .ok_or_else(|| {
                            Error::msg("TLS key file contains no supported PEM private key")
                        })?;
                let provider = Arc::new(rustls::crypto::ring::default_provider());
                let verifier = if let Some(path) = &config.ws_client_ca {
                    let mut roots = rustls::RootCertStore::empty();
                    for cert in amarcode_transport::certificates(path).map_err(Error::msg)? {
                        roots.add(cert).map_err(|e| Error::msg(e.to_string()))?;
                    }
                    rustls::server::WebPkiClientVerifier::builder_with_provider(
                        Arc::new(roots),
                        Arc::clone(&provider),
                    )
                    .build()
                    .map_err(|e| Error::msg(e.to_string()))?
                } else {
                    rustls::server::WebPkiClientVerifier::no_client_auth()
                };
                let config = rustls::ServerConfig::builder_with_provider(provider)
                    .with_safe_default_protocol_versions()
                    .map_err(|error| Error::msg(error.to_string()))?
                    .with_client_cert_verifier(verifier)
                    .with_single_cert(certs, key)
                    .map_err(|error| {
                        Error::msg(format!("Invalid WebSocket TLS certificate/key: {error}"))
                    })?;
                Some(TlsAcceptor::from(Arc::new(config)))
            }
            (None, None) => None,
            _ => {
                return Err(Error::msg(
                    "Both TLS certificate and private key are required",
                ))
            }
        };
        Ok(Self {
            tls,
            bearer_token,
            address: config.daemon_addr.clone(),
            origins: config.ws_allowed_origins.clone(),
            browser: super::web_auth::BrowserAuth::default(),
        })
    }

    pub(super) async fn handle(&self, stream: TcpStream, app: Arc<App>, peer: &str) -> Result<()> {
        stream.set_nodelay(true)?;
        if let Some(tls) = &self.tls {
            let stream = timeout(HANDSHAKE_TIMEOUT, tls.accept(stream))
                .await
                .map_err(|_| Error::msg("TLS handshake timed out"))??;
            self.handle_stream(stream, app, peer).await
        } else {
            self.handle_stream(stream, app, peer).await
        }
    }

    async fn handle_stream<S>(&self, stream: S, app: Arc<App>, peer: &str) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let Some(stream) =
            super::web_auth::route(stream, &self.bearer_token, &self.origins, &self.browser)
                .await?
        else {
            return Ok(());
        };
        let config = WebSocketConfig::default()
            .max_message_size(Some(amarcode_protocol::MAX_RPC_MESSAGE_BYTES))
            .max_frame_size(Some(amarcode_protocol::MAX_RPC_MESSAGE_BYTES));
        let mut terminal = false;
        #[allow(clippy::result_large_err)]
        // The handshake callback's HTTP response type is fixed by Tungstenite.
        let handshake = accept_hdr_async_with_config(
            stream,
            |request: &Request, response: Response| {
                let response = if request.headers().contains_key("sec-websocket-protocol") {
                    self.browser.handshake(request, response, &self.origins)?
                } else {
                    validate_handshake(request, response, &self.origins, &self.bearer_token)?
                };
                terminal = request.uri().path() == "/terminal";
                Ok(response)
            },
            Some(config),
        );
        let mut socket = timeout(HANDSHAKE_TIMEOUT, handshake)
            .await
            .map_err(|_| Error::msg("WebSocket handshake timed out"))?
            .map_err(|error| Error::msg(error.to_string()))?;
        if terminal {
            let result = super::terminal::serve(&mut socket).await;
            if let Err(error) = &result {
                let _ = send(
                    &mut socket,
                    Message::Text(
                        serde_json::json!({
                            "type": "error", "message": error.to_string()
                        })
                        .to_string()
                        .into(),
                    ),
                )
                .await;
            }
            let _ = timeout(WRITE_TIMEOUT, socket.close(None)).await;
            return result;
        }
        // Bounded duplex keeps framing separate from RPC/subscription semantics.
        // Both futures are polled together, including during large uploads and long prompts.
        let (bridge, rpc) = tokio::io::duplex(64 * 1024);
        let (rpc_read, rpc_write) = tokio::io::split(rpc);
        let handler = super::connection::handle_io_context(
            rpc_read,
            rpc_write,
            app,
            peer,
            Some(&self.address),
        );
        let transport = bridge_frames(socket, bridge);
        tokio::select! {
            result = handler => result,
            result = transport => result,
        }
    }
}

pub(crate) fn load_token(path: &Path) -> Result<String> {
    check_private_file(path)?;
    let token = std::fs::read_to_string(path)?.trim().to_owned();
    if token.len() < 32
        || token.len() > 512
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
    {
        return Err(Error::msg(
            "WebSocket token must contain 32–512 URL-safe ASCII characters",
        ));
    }
    Ok(token)
}

pub(super) fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

fn check_private_file(path: &Path) -> Result<()> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(Error::msg("WebSocket credentials must be regular files"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(Error::msg(format!(
                "Credential file {} must be private (chmod 600)",
                path.display()
            )));
        }
    }
    Ok(())
}

#[allow(clippy::result_large_err)] // HTTP handshake callback requires this response type.
fn validate_handshake(
    request: &Request,
    response: Response,
    origins: &[String],
    bearer_token: &str,
) -> std::result::Result<Response, ErrorResponse> {
    if !matches!(request.uri().path(), "/rpc" | "/terminal") || request.uri().query().is_some() {
        return Err(reject(StatusCode::NOT_FOUND));
    }
    let expected = format!("Bearer {bearer_token}");
    let mut headers = request.headers().get_all("authorization").iter();
    let supplied = headers
        .next()
        .map(|value| value.as_bytes())
        .unwrap_or_default();
    if headers.next().is_some() || !constant_time_equal(supplied, expected.as_bytes()) {
        return Err(reject(StatusCode::UNAUTHORIZED));
    }
    let mut supplied = request.headers().get_all("origin").iter();
    if let Some(origin) = supplied.next() {
        if supplied.next().is_some()
            || !origin
                .to_str()
                .is_ok_and(|value| origins.iter().any(|allowed| allowed == value))
        {
            return Err(reject(StatusCode::FORBIDDEN));
        }
    }
    Ok(response)
}

fn reject(status: StatusCode) -> ErrorResponse {
    let mut response = ErrorResponse::new(Some("WebSocket request rejected".into()));
    *response.status_mut() = status;
    response
}

async fn send<S>(socket: &mut WebSocketStream<S>, message: Message) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    timeout(WRITE_TIMEOUT, socket.send(message))
        .await
        .map_err(|_| Error::msg("WebSocket write timed out"))?
        .map_err(|error| Error::msg(error.to_string()))
}

async fn bridge_frames<S>(
    mut socket: WebSocketStream<S>,
    bridge: tokio::io::DuplexStream,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (reader, mut writer) = tokio::io::split(bridge);
    let mut outgoing = BufReader::new(reader).lines();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    let mut last_seen = Instant::now();
    loop {
        tokio::select! {
            incoming = socket.next() => {
                let Some(incoming) = incoming else { return Ok(()); };
                last_seen = Instant::now();
                match incoming.map_err(|error| Error::msg(error.to_string()))? {
                    Message::Text(text) => {
                        // Canonicalize exactly one JSON value: embedded newlines cannot inject additional RPCs.
                        match serde_json::from_str::<serde_json::Value>(&text) {
                            Ok(value) => {
                                let mut line = serde_json::to_vec(&value)?;
                                line.push(b'\n');
                                timeout(WRITE_TIMEOUT, writer.write_all(&line)).await
                                    .map_err(|_| Error::msg("WebSocket RPC input stalled"))??;
                            }
                            Err(_) => send(&mut socket, Message::Text(serde_json::to_string(&RpcResponse::err("Expected one JSON request per text message"))?.into())).await?,
                        }
                    }
                    Message::Ping(data) => send(&mut socket, Message::Pong(data)).await?,
                    Message::Pong(_) => {},
                    Message::Close(_) => { let _ = timeout(WRITE_TIMEOUT, socket.flush()).await; return Ok(()); },
                    _ => return Err(Error::msg("WebSocket RPC accepts text messages only")),
                }
            }
            line = outgoing.next_line() => {
                let Some(line) = line? else { return Ok(()); };
                send(&mut socket, Message::Text(line.into())).await?;
            }
            _ = heartbeat.tick() => {
                if last_seen.elapsed() >= Duration::from_secs(90) {
                    return Err(Error::msg("WebSocket heartbeat timed out"));
                }
                send(&mut socket, Message::Ping(Vec::new().into())).await?;
            }
        }
    }
}

#[cfg(test)]
#[path = "websocket_tests.rs"]
mod tests;
