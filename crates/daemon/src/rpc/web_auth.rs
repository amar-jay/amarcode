//! Bounded HTTP ticket exchange, sharing the existing authenticated WS listener.
use crate::{Error, Result};
use std::{
    collections::HashMap,
    io::Cursor,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_tungstenite::tungstenite::{
    handshake::server::{ErrorResponse, Request, Response},
    http::StatusCode,
};

pub const WEB_ORIGIN: &str = "https://remote.amarcode.amarjay.com";
const PROTOCOL: &str = "amarcode.rpc.v1";
const PREFIX: &str = "amarcode.ticket.";
const TTL: Duration = Duration::from_secs(30);
#[derive(Default)]
pub(super) struct BrowserAuth {
    tickets: Mutex<HashMap<String, (String, Instant)>>,
}
fn allowed(origin: &str, origins: &[String]) -> bool {
    origin == WEB_ORIGIN || origins.iter().any(|o| o == origin)
}
fn reject() -> ErrorResponse {
    let mut response = ErrorResponse::new(Some("Browser authentication rejected".into()));
    *response.status_mut() = StatusCode::UNAUTHORIZED;
    response
}
impl BrowserAuth {
    fn issue(&self, origin: &str) -> Result<String> {
        let mut tickets = self
            .tickets
            .lock()
            .map_err(|_| Error::msg("Ticket lock poisoned"))?;
        tickets.retain(|_, (_, expiry)| *expiry > Instant::now());
        if tickets.len() >= 128 {
            return Err(Error::msg("Too many pending browser connections"));
        }
        let ticket = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        tickets.insert(ticket.clone(), (origin.into(), Instant::now() + TTL));
        Ok(ticket)
    }
    #[allow(clippy::result_large_err)]
    pub fn handshake(
        &self,
        request: &Request,
        mut response: Response,
        origins: &[String],
    ) -> std::result::Result<Response, ErrorResponse> {
        if request.uri().path() != "/rpc"
            || request.uri().query().is_some()
            || request.headers().contains_key("authorization")
        {
            return Err(reject());
        }
        let origin = request
            .headers()
            .get("origin")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(reject)?;
        if request.headers().get_all("origin").iter().count() != 1 || !allowed(origin, origins) {
            return Err(reject());
        }
        let protocols = request
            .headers()
            .get_all("sec-websocket-protocol")
            .iter()
            .map(|v| v.to_str().unwrap_or_default())
            .collect::<Vec<_>>()
            .join(",");
        let parts = protocols.split(',').map(str::trim).collect::<Vec<_>>();
        if parts.len() != 2 || !parts.contains(&PROTOCOL) {
            return Err(reject());
        }
        let ticket = parts
            .iter()
            .find_map(|p| p.strip_prefix(PREFIX))
            .ok_or_else(reject)?;
        let mut tickets = self.tickets.lock().map_err(|_| reject())?;
        // Even an invalid-origin or expired attempt can never replay a used ticket.
        let (issued_origin, expiry) = tickets.remove(ticket).ok_or_else(reject)?;
        if issued_origin != origin || expiry <= Instant::now() {
            return Err(reject());
        }
        response
            .headers_mut()
            .insert("sec-websocket-protocol", PROTOCOL.parse().unwrap());
        Ok(response)
    }
}

/// Read a bounded header once, then replay it unchanged to Tungstenite unless it
/// is the one HTTP endpoint owned here. HTTP bodies never enter the RPC engine.
pub(super) async fn route<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    token: &str,
    origins: &[String],
    auth: &BrowserAuth,
) -> Result<Option<Replay<S>>> {
    let header = tokio::time::timeout(Duration::from_secs(10), async {
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") {
            if bytes.len() >= 16 * 1024 {
                return Err(Error::msg("HTTP header too large"));
            }
            bytes.push(stream.read_u8().await?);
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| Error::msg("HTTP header timed out"))??;
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut request = httparse::Request::new(&mut headers);
    request
        .parse(&header)
        .map_err(|_| Error::msg("Invalid HTTP header"))?;
    if request.path != Some("/web/session") {
        return Ok(Some(Replay {
            header: Cursor::new(header),
            stream,
        }));
    }
    let get = |name: &str| -> Option<&[u8]> {
        let mut values = request
            .headers
            .iter()
            .filter(|h| h.name.eq_ignore_ascii_case(name));
        let value = values.next()?.value;
        if values.next().is_some() {
            None
        } else {
            Some(value)
        }
    };
    let origin = get("origin")
        .and_then(|v| std::str::from_utf8(v).ok())
        .filter(|o| allowed(o, origins));
    let (status, body) = if origin.is_none() {
        ("403 Forbidden", "{\"error\":\"Origin rejected\"}".into())
    } else if request.method == Some("OPTIONS")
        && get("access-control-request-method") == Some(b"POST".as_slice())
    {
        ("204 No Content", String::new())
    } else if request.method != Some("POST") {
        (
            "405 Method Not Allowed",
            "{\"error\":\"POST required\"}".into(),
        )
    } else if !super::websocket::constant_time_equal(
        get("authorization").unwrap_or_default(),
        format!("Bearer {token}").as_bytes(),
    ) {
        (
            "401 Unauthorized",
            "{\"error\":\"Invalid access token\"}".into(),
        )
    } else {
        match auth.issue(origin.unwrap_or_default()) {
            Ok(ticket) => (
                "200 OK",
                serde_json::json!({"ticket":ticket,"web_api_version":1,"expires_in":30})
                    .to_string(),
            ),
            Err(_) => (
                "429 Too Many Requests",
                "{\"error\":\"Try connecting again shortly\"}".into(),
            ),
        }
    };
    let cors = origin.map(|o| format!("Access-Control-Allow-Origin: {o}\r\nAccess-Control-Allow-Methods: POST\r\nAccess-Control-Allow-Headers: Authorization, Content-Type\r\n")).unwrap_or_default();
    let response = format!("HTTP/1.1 {status}\r\n{cors}Vary: Origin\r\nCache-Control: no-store\r\nContent-Type: application/json\r\nX-Content-Type-Options: nosniff\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    tokio::time::timeout(
        Duration::from_secs(10),
        stream.write_all(response.as_bytes()),
    )
    .await
    .map_err(|_| Error::msg("HTTP response timed out"))??;
    stream.shutdown().await?;
    Ok(None)
}

pub(super) struct Replay<S> {
    header: Cursor<Vec<u8>>,
    stream: S,
}
impl<S: AsyncRead + Unpin> AsyncRead for Replay<S> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if this.header.position() < this.header.get_ref().len() as u64 {
            std::pin::Pin::new(&mut this.header).poll_read(cx, buf)
        } else {
            std::pin::Pin::new(&mut this.stream).poll_read(cx, buf)
        }
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for Replay<S> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.get_mut().stream).poll_write(cx, buf)
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    fn handshake(auth: &BrowserAuth, ticket: &str, origin: &str, path: &str) -> bool {
        let mut request = format!("wss://daemon.example{path}")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("origin", origin.parse().unwrap());
        request.headers_mut().insert(
            "sec-websocket-protocol",
            format!("{PROTOCOL}, {PREFIX}{ticket}").parse().unwrap(),
        );
        auth.handshake(&request, Response::new(()), &[]).is_ok()
    }
    #[test]
    fn tickets_are_one_use_bound_to_origin_and_expire() {
        let auth = BrowserAuth::default();
        let ticket = auth.issue(WEB_ORIGIN).unwrap();
        assert!(!handshake(&auth, &ticket, "https://evil.example", "/rpc"));
        assert!(!handshake(&auth, &ticket, WEB_ORIGIN, "/terminal"));
        assert!(!handshake(&auth, &ticket, WEB_ORIGIN, "/rpc?token=bad"));
        assert!(handshake(&auth, &ticket, WEB_ORIGIN, "/rpc"));
        assert!(!handshake(&auth, &ticket, WEB_ORIGIN, "/rpc"));
        let ticket = auth.issue(WEB_ORIGIN).unwrap();
        auth.tickets.lock().unwrap().get_mut(&ticket).unwrap().1 = Instant::now() - TTL;
        assert!(!handshake(&auth, &ticket, WEB_ORIGIN, "/rpc"));
        for _ in 0..128 {
            auth.issue(WEB_ORIGIN).unwrap();
        }
        assert!(auth.issue(WEB_ORIGIN).is_err());
    }
    async fn exchange(request: String, auth: &BrowserAuth) -> String {
        let (mut client, server) = tokio::io::duplex(32768);
        let client_io = async {
            client.write_all(request.as_bytes()).await.unwrap();
            let mut response = String::new();
            client.read_to_string(&mut response).await.unwrap();
            response
        };
        let (result, response) = tokio::join!(route(server, TOKEN, &[], auth), client_io);
        assert!(result.unwrap().is_none());
        response
    }
    #[tokio::test]
    async fn ticket_exchange_checks_token_origin_method_and_cors() {
        let auth = BrowserAuth::default();
        let request = |method, origin, token| {
            format!("{method} /web/session HTTP/1.1\r\nHost: daemon.example\r\nOrigin: {origin}\r\nAuthorization: Bearer {token}\r\nAccess-Control-Request-Method: POST\r\n\r\n")
        };
        let response = exchange(request("POST", WEB_ORIGIN, TOKEN), &auth).await;
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("Cache-Control: no-store"));
        assert!(response.contains(&format!("Access-Control-Allow-Origin: {WEB_ORIGIN}")));
        let value: serde_json::Value =
            serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert!(handshake(
            &auth,
            value["ticket"].as_str().unwrap(),
            WEB_ORIGIN,
            "/rpc"
        ));
        assert!(exchange(request("POST", WEB_ORIGIN, "bad"), &auth)
            .await
            .starts_with("HTTP/1.1 401"));
        let rejected = exchange(request("POST", "https://evil.example", TOKEN), &auth).await;
        assert!(rejected.starts_with("HTTP/1.1 403"));
        assert!(!rejected.contains("Access-Control-Allow-Origin"));
        assert!(exchange(request("GET", WEB_ORIGIN, TOKEN), &auth)
            .await
            .starts_with("HTTP/1.1 405"));
        assert!(exchange(request("OPTIONS", WEB_ORIGIN, ""), &auth)
            .await
            .starts_with("HTTP/1.1 204"));
        assert_eq!(auth.tickets.lock().unwrap().len(), 0);
    }
    #[tokio::test]
    async fn non_ticket_headers_are_replayed_without_consuming_following_data() {
        let (mut client, server) = tokio::io::duplex(32768);
        let request = b"GET /rpc HTTP/1.1\r\nHost: localhost\r\n\r\nFRAME";
        client.write_all(request).await.unwrap();
        client.shutdown().await.unwrap();
        let mut replay = route(server, TOKEN, &[], &BrowserAuth::default())
            .await
            .unwrap()
            .unwrap();
        let mut bytes = Vec::new();
        replay.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, request);
    }
}
