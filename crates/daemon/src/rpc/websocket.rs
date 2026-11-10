//! WS/WSS framing around the same RPC engine used by TCP.

#[cfg(test)]
use std::io::BufReader as StdBufReader;
use std::{path::Path, sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::{timeout, Instant},
};
#[cfg(test)]
use tokio_rustls::rustls;
use tokio_rustls::TlsAcceptor;
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
    tls_identity: Option<amarcode_transport::TlsIdentity>,
    e2ee: Option<amarcode_transport::TlsIdentity>,
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
        let tls_identity = match (&config.ws_tls_cert, &config.ws_tls_key) {
            (Some(cert), Some(key)) => Some(amarcode_transport::TlsIdentity {
                cert: cert.clone(),
                key: key.clone(),
                client_ca: config.ws_client_ca.clone(),
                client_crl: config.ws_client_crl.clone(),
            }),
            (None, None) => None,
            _ => {
                return Err(Error::msg(
                    "Both TLS certificate and private key are required",
                ))
            }
        };
        let tls = tls_identity
            .as_ref()
            .map(|identity| {
                identity
                    .server_config(false)
                    .map(|config| TlsAcceptor::from(Arc::new(config)))
            })
            .transpose()
            .map_err(Error::msg)?;
        if let Some(inner) = &config.e2ee {
            inner.server_config(true).map_err(Error::msg)?;
        }
        Ok(Self {
            tls,
            tls_identity,
            e2ee: config.e2ee.clone(),
            bearer_token,
            address: config.daemon_addr.clone(),
            origins: config.ws_allowed_origins.clone(),
            browser: super::web_auth::BrowserAuth::default(),
        })
    }

    pub(super) async fn handle(&self, stream: TcpStream, app: Arc<App>, peer: &str) -> Result<()> {
        stream.set_nodelay(true)?;
        let standard = stream.into_std()?;
        let shutdown_socket = standard.try_clone()?;
        let stream = TcpStream::from_std(standard)?;
        if let Some(tls) = &self.tls {
            // Re-read signed CRLs and trust anchors for every full handshake.
            let refreshed;
            let tls = if self
                .tls_identity
                .as_ref()
                .is_some_and(|identity| identity.client_crl.is_some())
            {
                refreshed = TlsAcceptor::from(Arc::new(
                    self.tls_identity
                        .as_ref()
                        .unwrap()
                        .server_config(false)
                        .map_err(Error::msg)?,
                ));
                &refreshed
            } else {
                tls
            };
            let stream = timeout(HANDSHAKE_TIMEOUT, tls.accept(stream))
                .await
                .map_err(|_| Error::msg("TLS handshake timed out"))??;
            let chain = stream
                .get_ref()
                .1
                .peer_certificates()
                .map(|chain| chain.to_vec())
                .unwrap_or_default();
            self.with_revocation(
                self.handle_stream(stream, app, peer, Some(&shutdown_socket)),
                self.tls_identity.as_ref(),
                chain,
                Some(&shutdown_socket),
            )
            .await
        } else {
            self.handle_stream(stream, app, peer, Some(&shutdown_socket))
                .await
        }
    }

    async fn handle_stream<S>(
        &self,
        stream: S,
        app: Arc<App>,
        peer: &str,
        shutdown_socket: Option<&std::net::TcpStream>,
    ) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let Some(stream) = super::web_auth::route(
            stream,
            &self.bearer_token,
            &self.origins,
            &self.browser,
            self.e2ee.is_some(),
        )
        .await?
        else {
            return Ok(());
        };
        let limit = if self.e2ee.is_some() {
            64 * 1024
        } else {
            amarcode_protocol::MAX_RPC_MESSAGE_BYTES
        };
        let config = WebSocketConfig::default()
            .max_message_size(Some(limit))
            .max_frame_size(Some(limit));
        let mut terminal = false;
        let mut encrypted = false;
        let mut probe = false;
        #[allow(clippy::result_large_err)]
        // The handshake callback's HTTP response type is fixed by Tungstenite.
        let handshake = accept_hdr_async_with_config(
            stream,
            |request: &Request, response: Response| {
                if request
                    .headers()
                    .get("sec-websocket-protocol")
                    .and_then(|v| v.to_str().ok())
                    == Some(amarcode_transport::encrypted::PROTOCOL)
                {
                    if self.e2ee.is_none()
                        || request.headers().contains_key("authorization")
                        || request.headers().contains_key("origin")
                        || request.uri().query().is_some()
                        || !matches!(request.uri().path(), "/rpc" | "/terminal" | "/e2ee-health")
                        || request
                            .headers()
                            .get_all("sec-websocket-protocol")
                            .iter()
                            .count()
                            != 1
                    {
                        return Err(reject(StatusCode::UNAUTHORIZED));
                    }
                    encrypted = true;
                    probe = request.uri().path() == "/e2ee-health";
                    let mut response = response;
                    response.headers_mut().insert(
                        "sec-websocket-protocol",
                        amarcode_transport::encrypted::PROTOCOL.parse().unwrap(),
                    );
                    return Ok(response);
                }
                if self.e2ee.is_some() {
                    return Err(reject(StatusCode::UPGRADE_REQUIRED));
                }
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
        let socket = timeout(HANDSHAKE_TIMEOUT, handshake)
            .await
            .map_err(|_| Error::msg("WebSocket handshake timed out"))?
            .map_err(|error| Error::msg(error.to_string()))?;
        if encrypted {
            let mut identity = self.e2ee.as_ref().unwrap().clone();
            // The probe authenticates only the server and exposes no application
            // data. It can never enter the authenticated RPC or terminal routes.
            if probe {
                identity.client_ca = None;
                identity.client_crl = None;
            }
            let acceptor =
                TlsAcceptor::from(Arc::new(identity.server_config(true).map_err(Error::msg)?));
            let io = amarcode_transport::encrypted::WebSocketIo::new(socket);
            let tls = timeout(HANDSHAKE_TIMEOUT, acceptor.accept(io))
                .await
                .map_err(|_| Error::msg("Inner TLS handshake timed out"))??;
            let chain = tls
                .get_ref()
                .1
                .peer_certificates()
                .map(|chain| chain.to_vec())
                .unwrap_or_default();
            let mut terminal = false;
            let handshake = accept_hdr_async_with_config(
                tls,
                |request: &Request, response: Response| {
                    if request.headers().contains_key("sec-websocket-protocol")
                        || request.headers().contains_key("origin")
                    {
                        return Err(reject(StatusCode::UNAUTHORIZED));
                    }
                    if probe {
                        if request.uri().path() != "/e2ee-health" || request.uri().query().is_some()
                        {
                            return Err(reject(StatusCode::UNAUTHORIZED));
                        }
                        return Ok(response);
                    }
                    let response = validate_handshake(request, response, &[], &self.bearer_token)?;
                    terminal = request.uri().path() == "/terminal";
                    Ok(response)
                },
                Some(
                    WebSocketConfig::default()
                        .max_message_size(Some(amarcode_protocol::MAX_RPC_MESSAGE_BYTES))
                        .max_frame_size(Some(amarcode_protocol::MAX_RPC_MESSAGE_BYTES)),
                ),
            );
            let mut socket = timeout(HANDSHAKE_TIMEOUT, handshake)
                .await
                .map_err(|_| Error::msg("Encrypted authentication timed out"))?
                .map_err(|e| Error::msg(e.to_string()))?;
            if probe {
                send(
                    &mut socket,
                    Message::Text(r#"{"status":"ok","end_to_end":true}"#.into()),
                )
                .await?;
                let _ = timeout(WRITE_TIMEOUT, socket.close(None)).await;
                return Ok(());
            }
            return self
                .with_revocation(
                    self.serve_socket(socket, terminal, app, peer),
                    self.e2ee.as_ref(),
                    chain,
                    shutdown_socket,
                )
                .await;
        }
        self.serve_socket(socket, terminal, app, peer).await
    }

    async fn with_revocation(
        &self,
        future: impl std::future::Future<Output = Result<()>>,
        identity: Option<&amarcode_transport::TlsIdentity>,
        chain: Vec<tokio_rustls::rustls::pki_types::CertificateDer<'static>>,
        shutdown_socket: Option<&std::net::TcpStream>,
    ) -> Result<()> {
        let Some(identity) = identity.filter(|identity| identity.client_crl.is_some()) else {
            return future.await;
        };
        #[cfg(not(test))]
        let period = Duration::from_secs(30);
        #[cfg(test)]
        let period = Duration::from_millis(25);
        let identity = identity.clone();
        let shutdown_socket = shutdown_socket
            .ok_or_else(|| Error::msg("Revocation checking requires a network socket"))?
            .try_clone()?;
        // A separate task can shut down the OS socket even while an RPC is in
        // block_in_place. Merely racing a timer in that RPC task is insufficient.
        struct Watch(tokio::task::JoinHandle<Result<()>>);
        impl Drop for Watch {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let mut watch = Watch(tokio::spawn(async move {
            loop {
                tokio::time::sleep(period).await;
                if let Err(error) = identity.verify_client(&chain) {
                    let _ = shutdown_socket.shutdown(std::net::Shutdown::Both);
                    return Err(Error::msg(error));
                }
            }
        }));
        tokio::pin!(future);
        tokio::select! {
            result = &mut future => result,
            result = &mut watch.0 => result.map_err(|e| Error::msg(e.to_string()))?,
        }
    }

    async fn serve_socket<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        mut socket: WebSocketStream<S>,
        terminal: bool,
        app: Arc<App>,
        peer: &str,
    ) -> Result<()> {
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
