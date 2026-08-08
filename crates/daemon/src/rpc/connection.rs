//! Single client TCP connection.
//!
//! Loop:
//! 1. read a line
//! 2. deserialize `protocol::rpc` request
//! 3. call `handler::dispatch`
//! 4. write result or error line
//!
//! Special case `subscribe_events`: after ack, leave the request loop and
//! forward matching `protocol::events` until the client disconnects.

use std::sync::Arc;

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::broadcast,
};
use tracing::{debug, warn};

use crate::{
    App, Result,
    protocol::{
        EditorEvent, EventLine,
        rpc::{RpcRequest, RpcResponse, SubscribeEventsParams, SubscribeEventsResult},
    },
    rpc::handler::{self, DispatchOutcome},
};

/// Handle one accepted client socket until disconnect or fatal I/O error.
pub async fn handle(stream: TcpStream, app: Arc<App>) -> Result<()> {
    let peer = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    debug!(%peer, "client connected");

    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let request = match serde_json::from_str::<RpcRequest>(line) {
            Ok(req) => req,
            Err(err) => {
                write_response(&mut writer, &RpcResponse::err(format!("invalid request: {err}")))
                    .await?;
                continue;
            }
        };

        debug!(%peer, method = %request.method, "rpc request");

        match handler::dispatch(&app, &request.method, request.params).await {
            Ok(DispatchOutcome::Result(value)) => {
                write_response(&mut writer, &RpcResponse::ok(value)).await?;
            }
            Ok(DispatchOutcome::Subscribe(filter)) => {
                let ack = SubscribeEventsResult { subscribed: true };
                let value = serde_json::to_value(ack)?;
                write_response(&mut writer, &RpcResponse::ok(value)).await?;
                debug!(%peer, ?filter, "entering event subscription mode");
                return stream_events(&app, &mut writer, &mut lines, filter).await;
            }
            Err(err) => {
                write_response(&mut writer, &RpcResponse::err(err.to_string())).await?;
            }
        }
    }

    debug!(%peer, "client disconnected");
    Ok(())
}

async fn write_response<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    response: &RpcResponse,
) -> Result<()> {
    let mut line = serde_json::to_string(response)?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

async fn write_event<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    event: &EditorEvent,
) -> Result<()> {
    let mut line = serde_json::to_string(&EventLine {
        event: event.clone(),
    })?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

/// After subscribe ack: forward matching events until the client goes away.
///
/// Event production is owned by `service::session_manager` (broadcast). Until
/// that lands, this waits on the shared bus (or client disconnect) so the
/// protocol shape is already correct.
async fn stream_events<R, W>(
    app: &App,
    writer: &mut W,
    lines: &mut tokio::io::Lines<R>,
    filter: SubscribeEventsParams,
) -> Result<()>
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let mut rx = app.events.subscribe();

    loop {
        tokio::select! {
            // Client closed the socket (or sent trailing noise we ignore).
            line = lines.next_line() => {
                match line? {
                    None => {
                        debug!("subscriber disconnected");
                        return Ok(());
                    }
                    Some(extra) => {
                        if !extra.trim().is_empty() {
                            warn!(
                                "ignoring request on subscription connection: {}",
                                extra.trim()
                            );
                        }
                    }
                }
            }
            msg = rx.recv() => {
                match msg {
                    Ok(event) => {
                        if event_matches(&event, &filter) {
                            write_event(writer, &event).await?;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "subscriber lagged; dropped events");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("event bus closed");
                        return Ok(());
                    }
                }
            }
        }
    }
}

fn event_matches(event: &EditorEvent, filter: &SubscribeEventsParams) -> bool {
    if filter.chat_id.is_none() && filter.run_id.is_none() && filter.session_id.is_none() {
        return true;
    }

    // session_id is not present on EditorEvent yet.
    if filter.session_id.is_some() {
        return false;
    }

    let chat_id = match event {
        EditorEvent::ChatUpdated { chat_id } => Some(chat_id.as_str()),
        _ => None,
    };

    let run_id = match event {
        EditorEvent::RunUpdated { run_id, .. }
        | EditorEvent::ApprovalRequired { run_id, .. }
        | EditorEvent::QuestionRequired { run_id, .. } => Some(run_id.as_str()),
        _ => None,
    };

    if let Some(want) = filter.chat_id.as_deref() {
        if chat_id != Some(want) {
            return false;
        }
    }
    if let Some(want) = filter.run_id.as_deref() {
        if run_id != Some(want) {
            return false;
        }
    }

    true
}
