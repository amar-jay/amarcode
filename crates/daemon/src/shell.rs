//! Terminal client for an already-running authenticated remote daemon.
use crate::{Error, Result};
use futures_util::{SinkExt, StreamExt};
use std::{
    io::{IsTerminal, Read, Write},
    path::PathBuf,
};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

fn terminal_url(value: &str) -> Result<String> {
    let value = crate::remote::resolve_remote_url(value)?;
    let mut url =
        reqwest::Url::parse(&value).map_err(|_| Error::msg("Enter a valid wss:// daemon URL"))?;
    let loopback = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if !(url.scheme() == "wss" || url.scheme() == "ws" && loopback)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/" | "/rpc" | "/terminal")
    {
        return Err(Error::msg("Use wss:// for remote access (ws:// is allowed only on loopback), without credentials or query parameters"));
    }
    url.set_path("/terminal");
    Ok(url.into())
}
struct RawMode;
impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// `x11`: None detects the local display; Some forces forwarding on or off.
pub async fn connect(
    url: Option<String>,
    token_file: Option<PathBuf>,
    x11: Option<bool>,
) -> Result<i32> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(Error::msg("amarcoded ssh requires an interactive terminal"));
    }
    let local_display = match x11 {
        Some(false) => None,
        Some(true) => Some(crate::x11::LocalDisplay::from_environment().await?),
        None if std::env::var_os("DISPLAY").is_none_or(|display| display.is_empty()) => None,
        None => match crate::x11::LocalDisplay::from_environment().await {
            Ok(display) => Some(display),
            Err(error) => {
                eprintln!("X11 forwarding unavailable: {error}. Continuing without it.");
                None
            }
        },
    };
    let mut forwarding = crate::x11::Mux::new();
    let url = match url {
        Some(url) => url,
        None => {
            eprint!("Remote daemon URL: ");
            std::io::stderr().flush()?;
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            line
        }
    };
    let url = terminal_url(&url)?;
    let token_file =
        token_file.or_else(|| std::env::var_os("AMARCODE_WS_TOKEN_FILE").map(PathBuf::from));
    let token = match token_file {
        Some(path) => crate::rpc::websocket::load_token(&path)?,
        None => rpassword::prompt_password("Daemon access token: ")?
            .trim()
            .to_owned(),
    };
    if !(32..=512).contains(&token.len())
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
    {
        return Err(Error::msg("Invalid daemon access token"));
    }
    let mut request = url
        .into_client_request()
        .map_err(|e| Error::msg(e.to_string()))?;
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|_| Error::msg("Invalid token"))?,
    );
    // Reqwest and the listener enable different Rustls providers. Select one
    // explicitly for this CLI process before Tungstenite builds its TLS client.
    let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    let (mut socket, _) = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .map_err(|_| Error::msg("Remote connection timed out"))?
    .map_err(|e| Error::msg(format!("Cannot open remote terminal: {e}")))?;
    let mut dimensions = crossterm::terminal::size()?;
    let mut open = serde_json::json!({"type":"open","cols":dimensions.0,"rows":dimensions.1});
    if let Some(local) = &local_display {
        open["x11"] = serde_json::to_value(local.request())?;
    }
    send(&mut socket, Message::Text(open.to_string().into())).await?;
    if local_display.is_some() {
        negotiate_x11(&mut socket).await?;
    }
    let shutdown = shutdown_signal()?;
    tokio::pin!(shutdown);
    crossterm::terminal::enable_raw_mode()?;
    let _raw = RawMode;
    let (tx, mut input) = tokio::sync::mpsc::channel(16);
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut bytes = [0; 4096];
        loop {
            match stdin.read(&mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if tx.blocking_send(bytes[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let mut terminal_credits = crate::x11::TERMINAL_WINDOW;
    let mut resize = tokio::time::interval(std::time::Duration::from_millis(200));
    loop {
        tokio::select! {
            frame = forwarding.next(), if local_display.is_some() => {
                if let Some(frame) = frame {
                    send(&mut socket, Message::Text(serde_json::to_string(&frame)?.into())).await?;
                }
            }
            code = &mut shutdown => {
                let _ = send(&mut socket, Message::Close(None)).await;
                return Ok(code);
            }
            bytes = input.recv(), if local_display.is_none() || terminal_credits > 0 => match bytes {
                Some(bytes) => {
                    if local_display.is_some() { terminal_credits -= 1; }
                    send(&mut socket, Message::Binary(bytes.into())).await?;
                },
                None => { let _ = send(&mut socket, Message::Close(None)).await; return Ok(0); }
            },
            frame = socket.next() => match frame {
                Some(Ok(Message::Binary(bytes))) => {
                    let mut stdout = std::io::stdout().lock();
                    stdout.write_all(&bytes)?;
                    stdout.flush()?;
                }
                Some(Ok(Message::Text(text))) => {
                    let value: serde_json::Value = serde_json::from_str(&text)?;
                    if value["type"] == "terminal_ack" {
                        if local_display.is_none() || terminal_credits >= crate::x11::TERMINAL_WINDOW {
                            return Err(Error::msg("Unexpected terminal acknowledgement"));
                        }
                        terminal_credits += 1;
                        continue;
                    }
                    if value["type"].as_str().is_some_and(|kind| kind.starts_with("x11_")) {
                        let frame: crate::x11::Frame = serde_json::from_value(value)?;
                        let local = local_display.as_ref()
                            .ok_or_else(|| Error::msg("Remote requested X11 forwarding without permission"))?;
                        match frame {
                            crate::x11::Frame::Open { id } => forwarding.open_local(id, local.clone())?,
                            frame => forwarding.receive(frame)?,
                        }
                        continue;
                    }
                    if value["type"] == "error" {
                        return Err(Error::msg(value["message"].as_str().unwrap_or("Remote terminal failed")));
                    }
                    if value["type"] == "exit" {
                        let code = value["code"].as_i64().unwrap_or(1).clamp(0, 255) as i32;
                        let _ = send(&mut socket, Message::Close(None)).await;
                        return Ok(code);
                    }
                }
                Some(Ok(Message::Ping(bytes))) => send(&mut socket, Message::Pong(bytes)).await?,
                Some(Ok(Message::Pong(_))) => (),
                None | Some(Ok(Message::Close(_))) => return Err(Error::msg("Remote terminal disconnected")),
                Some(Err(e)) => return Err(Error::msg(e.to_string())),
                _ => (),
            },
            _ = resize.tick() => {
                let next = crossterm::terminal::size()?;
                if next != dimensions {
                    dimensions = next;
                    send(&mut socket, Message::Text(serde_json::json!({"type":"resize","cols":next.0,"rows":next.1}).to_string().into())).await?;
                }
            }
        }
    }
}

async fn negotiate_x11<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
) -> Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    let value: serde_json::Value = serde_json::from_str(&text)?;
                    if value["type"] == "x11_ready" && value["version"] == 1 {
                        return Ok(());
                    }
                    return Err(Error::msg(value["message"].as_str().unwrap_or(
                        "Remote daemon does not support X11 forwarding; update it or reconnect with --no-x11",
                    )));
                }
                Some(Ok(Message::Ping(bytes))) => send(socket, Message::Pong(bytes)).await?,
                Some(Ok(Message::Pong(_))) => (),
                _ => {
                    return Err(Error::msg(
                        "Remote daemon does not support X11 forwarding; update it or reconnect with --no-x11",
                    ))
                }
            }
        }
    })
    .await
    .map_err(|_| Error::msg("X11 forwarding negotiation timed out; update the remote daemon"))??;
    Ok(())
}

fn shutdown_signal() -> Result<impl std::future::Future<Output = i32>> {
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    #[cfg(unix)]
    let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    Ok(async move {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = terminate.recv() => 143,
                _ = hangup.recv() => 129,
                _ = tokio::signal::ctrl_c() => 130,
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
            130
        }
    })
}

async fn send<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
    message: Message,
) -> Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(15), socket.send(message))
        .await
        .map_err(|_| Error::msg("Remote terminal write timed out"))?
        .map_err(|e| Error::msg(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::terminal_url;
    #[tokio::test]
    async fn x11_requires_explicit_support_instead_of_silently_opening_an_old_shell() {
        use futures_util::SinkExt;
        use tokio_tungstenite::{
            tungstenite::{protocol::Role, Message},
            WebSocketStream,
        };
        for supported in [false, true] {
            let (a, b) = tokio::io::duplex(4096);
            let mut client = WebSocketStream::from_raw_socket(a, Role::Client, None).await;
            let mut server = WebSocketStream::from_raw_socket(b, Role::Server, None).await;
            server
                .send(if supported {
                    Message::Text(
                        serde_json::json!({"type":"x11_ready","version":1})
                            .to_string()
                            .into(),
                    )
                } else {
                    Message::Binary(b"old shell prompt".to_vec().into())
                })
                .await
                .unwrap();
            let result = super::negotiate_x11(&mut client).await;
            assert_eq!(result.is_ok(), supported);
            if !supported {
                assert!(result.unwrap_err().to_string().contains("update"));
            }
        }
    }

    #[test]
    fn remote_terminal_requires_tls_and_never_accepts_url_credentials() {
        assert_eq!(
            terminal_url("wss://host.example/rpc").unwrap(),
            "wss://host.example/terminal"
        );
        assert!(terminal_url("ws://127.0.0.1:43721").is_ok());
        let branded =
            crate::remote::short_remote_url("wss://my-remote.trycloudflare.com/rpc").unwrap();
        assert_eq!(
            terminal_url(&branded).unwrap(),
            "wss://my-remote.trycloudflare.com/terminal"
        );
        assert!(terminal_url("https://updates.amarcode.amarjay.com/r/invalid").is_err());
        for url in [
            "ws://host.example",
            "https://host.example",
            "wss://user:secret@host.example",
            "wss://host.example?token=secret",
            "wss://host.example/other",
        ] {
            assert!(terminal_url(url).is_err());
        }
    }
}
