//! TLS 1.3 inside a WebSocket relay. Only TLS ciphertext crosses the outer socket.
use crate::{client_config, client_config_with_crl, path, rustls};
use futures_util::{Sink, Stream};
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, protocol::WebSocketConfig, Message},
    WebSocketStream,
};

pub const PROTOCOL: &str = "amarcode.e2ee.tls13.v1";
pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
pub type BoxIo = Box<dyn Io>;
pub type Socket = WebSocketStream<BoxIo>;

pub struct WebSocketIo<S> {
    socket: WebSocketStream<S>,
    buffered: Vec<u8>,
    offset: usize,
}
impl<S> WebSocketIo<S> {
    pub fn new(socket: WebSocketStream<S>) -> Self {
        Self {
            socket,
            buffered: vec![],
            offset: 0,
        }
    }
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for WebSocketIo<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let mut frames = 0;
        loop {
            frames += 1;
            if frames > 32 {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            if this.offset < this.buffered.len() {
                let count = buf.remaining().min(this.buffered.len() - this.offset);
                buf.put_slice(&this.buffered[this.offset..this.offset + count]);
                this.offset += count;
                return Poll::Ready(Ok(()));
            }
            match Pin::new(&mut this.socket).poll_flush(cx) {
                Poll::Ready(Err(e)) => return Poll::Ready(Err(error(e))),
                Poll::Pending => return Poll::Pending,
                _ => {}
            }
            match Pin::new(&mut this.socket).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) | Poll::Ready(Some(Ok(Message::Close(_)))) => {
                    return Poll::Ready(Ok(()))
                }
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Err(error(e))),
                Poll::Ready(Some(Ok(Message::Binary(bytes)))) => {
                    if bytes.len() > 64 * 1024 {
                        return Poll::Ready(Err(error("Encrypted record frame too large")));
                    }
                    this.buffered = bytes.to_vec();
                    this.offset = 0;
                }
                Poll::Ready(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
                _ => return Poll::Ready(Err(error("Expected encrypted binary records"))),
            }
        }
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for WebSocketIo<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        match Pin::new(&mut this.socket).poll_ready(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Poll::Ready(Err(error(e))),
            Poll::Ready(Ok(())) => {
                let count = buf.len().min(16 * 1024);
                match Pin::new(&mut this.socket)
                    .start_send(Message::Binary(buf[..count].to_vec().into()))
                {
                    Ok(()) => Poll::Ready(Ok(count)),
                    Err(e) => Poll::Ready(Err(error(e))),
                }
            }
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().socket)
            .poll_flush(cx)
            .map_err(error)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().socket)
            .poll_close(cx)
            .map_err(error)
    }
}

pub struct ClientIdentity {
    pub tls: Arc<rustls::ClientConfig>,
    pub server_name: String,
}
impl ClientIdentity {
    pub fn from_env() -> Result<Option<Self>, String> {
        let ca = path("AMARCODE_E2EE_CA");
        let cert = path("AMARCODE_E2EE_CLIENT_CERT");
        let key = path("AMARCODE_E2EE_CLIENT_KEY");
        let crl = path("AMARCODE_E2EE_SERVER_CRL");
        if ca.is_none()
            && cert.is_none()
            && key.is_none()
            && crl.is_none()
            && std::env::var_os("AMARCODE_E2EE_SERVER_NAME").is_none()
        {
            return Ok(None);
        }
        if ca.is_none() || cert.is_none() || key.is_none() {
            return Err(
                "E2EE requires an out-of-band server CA and a paired client certificate/key".into(),
            );
        }
        Ok(Some(Self {
            tls: Arc::new(client_config_with_crl(
                ca.as_deref(),
                cert.as_deref(),
                key.as_deref(),
                crl.as_deref(),
                true,
            )?),
            server_name: std::env::var("AMARCODE_E2EE_SERVER_NAME").map_err(|_| {
                "Set AMARCODE_E2EE_SERVER_NAME to the out-of-band paired daemon identity"
            })?,
        }))
    }
}

pub async fn connect(url: &str, token: &str, max_message: usize) -> Result<Socket, String> {
    let identity = ClientIdentity::from_env()?;
    connect_with(
        url,
        token,
        max_message,
        identity,
        Arc::new(client_config()?),
    )
    .await
}
pub async fn connect_with(
    url: &str,
    token: &str,
    max_message: usize,
    identity: Option<ClientIdentity>,
    outer_tls: Arc<rustls::ClientConfig>,
) -> Result<Socket, String> {
    connect_with_address(url, token, max_message, identity, outer_tls, None).await
}
pub async fn connect_with_address(
    url: &str,
    token: &str,
    max_message: usize,
    identity: Option<ClientIdentity>,
    outer_tls: Arc<rustls::ClientConfig>,
    address: Option<&str>,
) -> Result<Socket, String> {
    tokio::time::timeout(Duration::from_secs(20), async {
        let target = url::Url::parse(url).map_err(|_| "Invalid daemon URL")?;
        if !matches!(target.scheme(), "ws" | "wss")
            || target.query().is_some()
            || target.fragment().is_some()
            || !target.username().is_empty()
            || target.password().is_some()
        {
            return Err("Invalid daemon WebSocket URL".into());
        }
        let host = target
            .host_str()
            .ok_or("Missing daemon host")?
            .trim_matches(['[', ']']);
        if target.scheme() == "ws" && !matches!(host, "localhost" | "127.0.0.1" | "::1") {
            return Err("Remote endpoints require WSS".into());
        }
        let port = target
            .port()
            .unwrap_or(if target.scheme() == "wss" { 443 } else { 80 });
        let tcp = if let Some(address) = address {
            tokio::net::TcpStream::connect(address).await
        } else {
            tokio::net::TcpStream::connect((host, port)).await
        }
        .map_err(|e| e.to_string())?;
        let io: BoxIo = if target.scheme() == "wss" {
            Box::new(
                tokio_rustls::TlsConnector::from(outer_tls)
                    .connect(
                        rustls::pki_types::ServerName::try_from(host.to_owned())
                            .map_err(|e| e.to_string())?,
                        tcp,
                    )
                    .await
                    .map_err(|e| e.to_string())?,
            )
        } else {
            Box::new(tcp)
        };
        let config = || {
            WebSocketConfig::default()
                .max_message_size(Some(max_message))
                .max_frame_size(Some(max_message))
        };
        let mut request = url.into_client_request().map_err(|e| e.to_string())?;
        if identity.is_some() {
            request
                .headers_mut()
                .insert("sec-websocket-protocol", PROTOCOL.parse().unwrap());
        } else {
            request.headers_mut().insert(
                "authorization",
                format!("Bearer {token}")
                    .parse()
                    .map_err(|_| "Invalid access token")?,
            );
        }
        let (outer, response) = tokio_tungstenite::client_async_with_config(
            request,
            io,
            Some(if identity.is_some() {
                WebSocketConfig::default()
                    .max_message_size(Some(64 * 1024))
                    .max_frame_size(Some(64 * 1024))
            } else {
                config()
            }),
        )
        .await
        .map_err(|e| e.to_string())?;
        let Some(identity) = identity else {
            return Ok(outer);
        };
        if response
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|v| v.to_str().ok())
            != Some(PROTOCOL)
        {
            return Err(
                "Server did not negotiate end-to-end encryption; refusing downgrade".into(),
            );
        }
        let tls = tokio_rustls::TlsConnector::from(identity.tls)
            .connect(
                rustls::pki_types::ServerName::try_from(identity.server_name)
                    .map_err(|e| e.to_string())?,
                WebSocketIo::new(outer),
            )
            .await
            .map_err(|e| e.to_string())?;
        // The bearer token and application handshake exist only inside authenticated inner TLS.
        let mut request = url.into_client_request().map_err(|e| e.to_string())?;
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {token}")
                .parse()
                .map_err(|_| "Invalid access token")?,
        );
        let (inner, _) = tokio_tungstenite::client_async_with_config(
            request,
            Box::new(tls) as BoxIo,
            Some(config()),
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(inner)
    })
    .await
    .map_err(|_| "Encrypted daemon connection timed out".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn failed_negotiation_never_sends_a_token_or_retries_without_encryption() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/rpc", listener.local_addr().unwrap());
        let relay = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let socket = tokio_tungstenite::accept_hdr_async(
                tcp,
                |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                    assert!(!request.headers().contains_key("authorization"));
                    assert_eq!(request.headers()["sec-websocket-protocol"], PROTOCOL);
                    Ok(response) // Deliberately omit protocol negotiation.
                },
            )
            .await
            .unwrap();
            drop(socket);
            // No second plaintext connection follows a failed upgrade.
            assert!(
                tokio::time::timeout(Duration::from_millis(100), listener.accept())
                    .await
                    .is_err()
            );
        });
        let tls = Arc::new(crate::client_config_with(None, None, None).unwrap());
        let error = connect_with(
            &url,
            "never-send-this-token",
            4096,
            Some(ClientIdentity {
                tls: Arc::clone(&tls),
                server_name: "localhost".into(),
            }),
            tls,
        )
        .await
        .err()
        .expect("missing protocol must fail");
        assert!(!error.contains("never-send-this-token"));
        relay.await.unwrap();
    }
}
