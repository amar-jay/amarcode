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
use tracing::{debug, info, warn};

use crate::{
    protocol::{
        rpc::{RpcRequest, RpcResponse, SubscribeEventsParams, SubscribeEventsResult},
        EditorEvent, EventLine,
    },
    rpc::handler::{self, DispatchOutcome},
    App, Result,
};

/// Handle one accepted client socket until disconnect or fatal I/O error.
pub async fn handle(stream: TcpStream, app: Arc<App>) -> Result<()> {
    let peer = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    info!(%peer, "client connected");

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
                write_response(
                    &mut writer,
                    &RpcResponse::err(format!("invalid request: {err}")),
                )
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
                // Register before awaiting socket I/O. Any event emitted while
                // the acknowledgement is being written is then queued for this
                // subscriber instead of falling into a registration gap.
                let receiver = acknowledge_subscription(&mut writer, &app.events).await?;
                debug!(%peer, ?filter, "entering event subscription mode");
                return stream_events(&mut writer, &mut lines, filter, receiver).await;
            }
            Err(err) => {
                write_response(&mut writer, &RpcResponse::err(err.to_string())).await?;
            }
        }
    }

    info!(%peer, "client disconnected");
    Ok(())
}

async fn acknowledge_subscription<W>(
    writer: &mut W,
    events: &broadcast::Sender<EditorEvent>,
) -> Result<broadcast::Receiver<EditorEvent>>
where
    W: AsyncWriteExt + Unpin,
{
    let receiver = events.subscribe();
    let ack = SubscribeEventsResult { subscribed: true };
    let value = serde_json::to_value(ack)?;
    write_response(writer, &RpcResponse::ok(value)).await?;
    Ok(receiver)
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

async fn write_event<W: AsyncWriteExt + Unpin>(writer: &mut W, event: &EditorEvent) -> Result<()> {
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
/// Event production is owned by `service::session` (broadcast). The receiver
/// is already registered before this function starts, so events emitted while
/// the subscription acknowledgement was in flight remain queued.
async fn stream_events<R, W>(
    writer: &mut W,
    lines: &mut tokio::io::Lines<R>,
    filter: SubscribeEventsParams,
    mut rx: broadcast::Receiver<EditorEvent>,
) -> Result<()>
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: AsyncWriteExt + Unpin,
{
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{duplex, AsyncBufReadExt, BufReader};

    use super::*;

    #[tokio::test]
    async fn event_during_subscription_ack_is_retained() {
        let (events, _) = broadcast::channel(4);
        let (client, mut server) = duplex(1);
        let task_events = events.clone();

        // The one-byte duplex capacity forces the acknowledgement write to
        // remain pending until the client starts reading it.
        let subscription =
            tokio::spawn(async move { acknowledge_subscription(&mut server, &task_events).await });

        tokio::time::timeout(Duration::from_secs(1), async {
            while events.receiver_count() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("subscription receiver registration");
        let event = EditorEvent::ChatUpdated {
            chat_id: "chat-during-ack".into(),
        };
        events.send(event).expect("send test event");

        let mut ack = String::new();
        BufReader::new(client)
            .read_line(&mut ack)
            .await
            .expect("read subscription ack");
        let mut receiver = subscription
            .await
            .expect("subscription task")
            .expect("subscription ack");

        assert!(ack.contains("\"subscribed\":true"), "unexpected ack: {ack}");
        match receiver.try_recv().expect("queued event") {
            EditorEvent::ChatUpdated { chat_id } => assert_eq!(chat_id, "chat-during-ack"),
            other => panic!("unexpected queued event: {other:?}"),
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
        EditorEvent::ContextRestoration { chat_id, .. } => Some(chat_id.as_str()),
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
