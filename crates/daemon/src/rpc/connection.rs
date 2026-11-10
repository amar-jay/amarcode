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
    store::Store,
    App, Result,
};

/// Handle one accepted client socket until disconnect or fatal I/O error.
pub async fn handle(stream: TcpStream, app: Arc<App>) -> Result<()> {
    let peer = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let (reader, writer) = stream.into_split();
    handle_io(reader, writer, app, &peer).await
}

/// Shared JSON-line engine, also used by the WebSocket frame adapter.
pub(super) async fn handle_io<R, W>(reader: R, writer: W, app: Arc<App>, peer: &str) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    handle_io_context(reader, writer, app, peer, None).await
}

pub(super) async fn handle_io_context<R, W>(
    reader: R,
    mut writer: W,
    app: Arc<App>,
    peer: &str,
    authenticated_address: Option<&str>,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
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
                // Invalid framing is fatal. In particular, never skip HTTP
                // headers and then dispatch a JSON RPC hidden in the body.
                return Ok(());
            }
        };

        debug!(%peer, method = %request.method, "rpc request");

        match handler::dispatch(&app, &request.method, request.params).await {
            Ok(DispatchOutcome::Result(mut value)) => {
                if request.method == crate::protocol::rpc::methods::HEALTH {
                    value["remote_auth_required"] = authenticated_address.is_some().into();
                    if let Some(address) = authenticated_address {
                        value["addr"] = address.into();
                    }
                }
                write_response(&mut writer, &RpcResponse::ok(value)).await?;
            }
            Ok(DispatchOutcome::Subscribe(filter)) => {
                // Register before awaiting socket I/O. Any event emitted while
                // the acknowledgement is being written is then queued for this
                // subscriber instead of falling into a registration gap.
                let receiver = acknowledge_subscription(&mut writer, &app.events).await?;
                // Replay work already in progress. Closing the app drops the
                // event stream; the daemon keeps the prompts running.
                for snapshot in app.sessions.retained_session_snapshots()? {
                    let event = EditorEvent::SessionStateUpdated { snapshot };
                    if event_matches(&event, &filter, &app.store)? {
                        write_event(&mut writer, &event).await?;
                    }
                }
                info!(%peer, ?filter, "entering event subscription mode");
                return stream_events(
                    &mut writer,
                    &mut lines,
                    filter,
                    receiver,
                    Arc::clone(&app.store),
                )
                .await;
            }
            Err(err) => {
                write_response(&mut writer, &RpcResponse::err(err.to_string())).await?;
            }
        }
    }

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
    store: Arc<Store>,
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
                        if event_matches(&event, &filter, &store)? {
                            write_event(writer, &event).await?;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "subscriber lagged; reconnect required");
                        return Err(crate::Error::msg("event subscriber lagged; reconnect and recover session state"));
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

#[derive(Default)]
struct EventScope {
    chat_id: Option<String>,
    run_id: Option<String>,
    session_id: Option<String>,
}

fn event_matches(
    event: &EditorEvent,
    filter: &SubscribeEventsParams,
    store: &Store,
) -> Result<bool> {
    if filter.chat_id.is_none() && filter.run_id.is_none() && filter.session_id.is_none() {
        return Ok(true);
    }

    let mut scope = match event {
        EditorEvent::SessionStateUpdated { snapshot } => EventScope {
            chat_id: Some(snapshot.chat_id.clone()),
            run_id: snapshot.run_id.clone(),
            session_id: None,
        },
        EditorEvent::ChatUpdated { chat_id } => EventScope {
            chat_id: Some(chat_id.clone()),
            ..EventScope::default()
        },
        EditorEvent::TurnUpdated {
            chat_id, run_id, ..
        }
        | EditorEvent::ContextRestoration {
            chat_id, run_id, ..
        }
        | EditorEvent::SessionCommandsUpdated {
            chat_id,
            catalog: crate::protocol::SessionCommands { run_id, .. },
        }
        | EditorEvent::ContextUsageUpdated {
            chat_id, run_id, ..
        } => EventScope {
            chat_id: Some(chat_id.clone()),
            run_id: Some(run_id.clone()),
            session_id: None,
        },
        EditorEvent::RunUpdated { run_id, .. }
        | EditorEvent::ApprovalRequired { run_id, .. }
        | EditorEvent::QuestionRequired { run_id, .. } => EventScope {
            run_id: Some(run_id.clone()),
            ..EventScope::default()
        },
        EditorEvent::MessageUpdated { message_id, .. }
        | EditorEvent::MessagePartAdded { message_id, .. } => {
            let Some(message) = store.get_message(message_id)? else {
                return Ok(false);
            };
            EventScope {
                chat_id: Some(message.chat_id),
                run_id: message.agent_run_id,
                session_id: None,
            }
        }
        EditorEvent::WorkspaceFilesChanged { .. }
        | EditorEvent::AgentConnectionChanged { .. }
        | EditorEvent::AgentAuthRequired { .. } => return Ok(false),
        EditorEvent::SessionConfigUpdated { chat_id, .. } => EventScope {
            chat_id: Some(chat_id.clone()),
            ..EventScope::default()
        },
    };

    // Run rows provide the relationships omitted from compact event payloads.
    // This keeps the wire contract stable while making all three filters useful.
    if let Some(run_id) = scope.run_id.as_deref() {
        if let Some(run) = store.get_run(run_id)? {
            scope.chat_id.get_or_insert(run.chat_id);
            scope.session_id = run.acp_session_id;
        }
    }

    Ok(filter
        .chat_id
        .as_ref()
        .is_none_or(|want| scope.chat_id.as_ref() == Some(want))
        && filter
            .run_id
            .as_ref()
            .is_none_or(|want| scope.run_id.as_ref() == Some(want))
        && filter
            .session_id
            .as_ref()
            .is_none_or(|want| scope.session_id.as_ref() == Some(want)))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use tokio::io::{duplex, AsyncBufReadExt, BufReader};

    use crate::{
        protocol::{MessageRole, MessageStatus, RunStatus, TurnStatus},
        store::{AgentRun, Chat, Message},
    };

    use super::*;

    async fn test_app() -> (tempfile::TempDir, Arc<App>) {
        let directory = tempfile::tempdir().unwrap();
        let app = App::new(crate::Config {
            app_dir: directory.path().to_owned(),
            db_path: directory.path().join("store.sqlite3"),
            default_workspace_path: None,
            transport: crate::config::DaemonTransport::Tcp,
            daemon_addr: "127.0.0.1:0".into(),
            acp_registry_source: None,
            ws_token_file: None,
            ws_tls_cert: None,
            ws_tls_key: None,
            ws_client_ca: None,
            ws_client_crl: None,
            e2ee: None,
            ws_allowed_origins: vec![],
        })
        .await
        .unwrap();
        (directory, Arc::new(app))
    }

    async fn exchange(app: Arc<App>, input: &str) -> Vec<serde_json::Value> {
        use tokio::io::AsyncReadExt;

        let (mut client, server) = duplex(16 * 1024);
        let (reader, writer) = tokio::io::split(server);
        let handler = tokio::spawn(async move { handle_io(reader, writer, app, "test").await });
        client.write_all(input.as_bytes()).await.unwrap();
        client.shutdown().await.unwrap();
        let mut output = String::new();
        tokio::time::timeout(Duration::from_secs(2), client.read_to_string(&mut output))
            .await
            .expect("connection must close")
            .unwrap();
        handler.await.unwrap().unwrap();
        output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn malformed_input_closes_connection_without_dispatching_trailing_rpc() {
        let (_directory, app) = test_app().await;
        let body = "{\"method\":\"create_chat\",\"params\":{\"workspace_path\":\"/test\",\"title\":\"must not execute\"}}\n";
        for prefix in [
            format!("POST / HTTP/1.1\r\nHost: localhost\r\nOrigin: https://untrusted.example\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n", body.len()),
            "OPTIONS / HTTP/1.1\r\n\r\n".into(),
            "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n".into(),
            "not json\n".into(),
            "{\"params\":{}}\n".into(),
        ] {
            let replies = exchange(Arc::clone(&app), &format!("{prefix}{body}")).await;
            assert!(app.store.chats(None).unwrap().is_empty(), "trailing RPC executed after {prefix:?}");
            assert_eq!(replies.len(), 1, "unexpected replies after {prefix:?}");
            assert!(replies[0]["error"].as_str().unwrap().starts_with("invalid request:"));
        }
    }

    #[tokio::test]
    async fn valid_rpc_connections_survive_dispatch_errors_and_accept_multiple_requests() {
        let (_directory, app) = test_app().await;
        let replies = exchange(
            app,
            "\n{\"method\":\"health\"}\n{\"method\":\"unknown_method\"}\n{\"method\":\"health\"}\n",
        )
        .await;
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0]["result"]["status"], "ok");
        assert!(replies[1]["error"].is_string());
        assert_eq!(replies[2]["result"]["status"], "ok");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn queue_rpc_roundtrip_preserves_order_and_exposes_pending_work() {
        let (_directory, app) = test_app().await;
        let chat = app.chats.create("/test", None).unwrap();
        app.prompt_queue
            .pause(&chat.id, "Paused for review")
            .unwrap();
        let requests = [
            serde_json::json!({"method":"enqueue_prompt", "params":{
                "request_id":"follow-up", "prompt":{
                    "chat_id":chat.id,"agent_id":"agent", "text":"Later", "attachments":[], "config_values":[]
                }
            }}),
            serde_json::json!({"method":"update_prompt_queue", "params":{
                "chat_id":chat.id,"action":"edit","id":"follow-up","text":"Edited"
            }}),
            serde_json::json!({"method":"get_chat", "params":{"chat_id":chat.id}}),
            serde_json::json!({"method":"update_prompt_queue", "params":{
                "chat_id":chat.id,"action":"remove","id":"follow-up"
            }}),
        ];
        let input = requests
            .iter()
            .map(|request| format!("{request}\n"))
            .collect::<String>();
        let replies = exchange(app, &input).await;
        assert_eq!(replies.len(), 4);
        for reply in &replies {
            assert!(reply.get("error").is_none(), "{reply}");
        }
        assert_eq!(replies[0]["result"]["items"][0]["status"], "queued");
        assert_eq!(replies[1]["result"]["items"][0]["text"], "Edited");
        assert_eq!(replies[2]["result"]["prompt_queue"]["paused"], true);
        assert_eq!(
            replies[2]["result"]["prompt_queue"]["items"][0]["text"],
            "Edited"
        );
        assert_eq!(replies[3]["result"]["items"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn subscription_replays_a_prompt_that_is_already_running() {
        let (_directory, app) = test_app().await;
        app.sessions.record_running_turn_for_test("chat-1");
        let (mut client, server) = duplex(16 * 1024);
        let (reader, writer) = tokio::io::split(server);
        let app_for_handler = Arc::clone(&app);
        let handler =
            tokio::spawn(async move { handle_io(reader, writer, app_for_handler, "test").await });
        client
            .write_all(b"{\"method\":\"subscribe_events\",\"params\":{}}\n")
            .await
            .unwrap();
        let mut lines = BufReader::new(client).lines();
        let ack = lines.next_line().await.unwrap().unwrap();
        let replay = lines.next_line().await.unwrap().unwrap();
        drop(lines);
        handler.abort();
        let ack: serde_json::Value = serde_json::from_str(&ack).unwrap();
        let replay: serde_json::Value = serde_json::from_str(&replay).unwrap();
        assert_eq!(ack["result"]["subscribed"], true);
        assert_eq!(replay["event"]["type"], "sessionStateUpdated");
        assert_eq!(replay["event"]["payload"]["snapshot"]["chat_id"], "chat-1");
        assert_eq!(
            replay["event"]["payload"]["snapshot"]["turn"]["status"],
            "started"
        );
    }

    #[tokio::test]
    async fn lagged_subscriber_is_closed_for_snapshot_recovery() {
        let store = Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap());
        let (events, receiver) = broadcast::channel(1);
        for n in 0..3 {
            events
                .send(EditorEvent::ChatUpdated {
                    chat_id: n.to_string(),
                })
                .unwrap();
        }
        let (_client, server) = duplex(1024);
        let (reader, mut writer) = tokio::io::split(server);
        let mut lines = BufReader::new(reader).lines();
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            stream_events(
                &mut writer,
                &mut lines,
                SubscribeEventsParams::default(),
                receiver,
                store,
            ),
        )
        .await
        .unwrap();
        assert!(result.unwrap_err().to_string().contains("lagged"));
    }

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

    #[test]
    fn turn_event_matches_chat_and_run_filters() {
        let store = Store::open(std::path::Path::new(":memory:")).expect("in-memory store");
        let event = EditorEvent::TurnUpdated {
            chat_id: "chat-1".into(),
            run_id: "run-1".into(),
            user_message_id: "message-1".into(),
            status: TurnStatus::Started,
            stop_reason: None,
            error_message: None,
            error_kind: None,
        };

        assert!(event_matches(
            &event,
            &SubscribeEventsParams {
                chat_id: Some("chat-1".into()),
                run_id: Some("run-1".into()),
                session_id: None,
            },
            &store,
        )
        .expect("filter event"));
    }

    #[test]
    fn persisted_relationships_filter_run_message_and_session_events() {
        let store =
            Arc::new(Store::open(std::path::Path::new(":memory:")).expect("in-memory store"));
        store
            .save_agent(&crate::protocol::AgentDefinition {
                id: "test-agent".into(),
                name: "Test agent".into(),
                command: "test-agent".into(),
                arguments: vec![],
                environment: vec![],
                available: false,
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
            })
            .expect("create agent");
        let agent_id = store.agents().expect("agents")[0].id.clone();
        store
            .create_chat(&Chat {
                id: "chat-1".into(),
                workspace_path: "/tmp/workspace".into(),
                title: "test".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
                archived_at: None,
            })
            .expect("create chat");
        store
            .create_run(&AgentRun {
                id: "run-1".into(),
                chat_id: "chat-1".into(),
                agent_id,
                acp_session_id: Some("session-1".into()),
                status: RunStatus::Running,
                started_at: "2026-01-01T00:00:00Z".into(),
                finished_at: None,
                error_message: None,
                context_usage: None,
            })
            .expect("create run");
        store
            .create_message(&Message {
                id: "message-1".into(),
                chat_id: "chat-1".into(),
                agent_run_id: Some("run-1".into()),
                role: MessageRole::Assistant,
                content: "hello".into(),
                status: MessageStatus::Complete,
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
            })
            .expect("create message");

        let session_filter = SubscribeEventsParams {
            chat_id: None,
            run_id: None,
            session_id: Some("session-1".into()),
        };
        let run_event = EditorEvent::RunUpdated {
            run_id: "run-1".into(),
            status: RunStatus::Running,
            error_message: None,
            error_kind: None,
        };
        let message_event = EditorEvent::MessageUpdated {
            message_id: "message-1".into(),
            status: MessageStatus::Complete,
        };

        assert!(event_matches(&run_event, &session_filter, &store).expect("filter run"));
        assert!(event_matches(&message_event, &session_filter, &store).expect("filter message"));
        assert!(!event_matches(
            &message_event,
            &SubscribeEventsParams {
                chat_id: Some("another-chat".into()),
                ..SubscribeEventsParams::default()
            },
            &store,
        )
        .expect("filter unrelated chat"));
    }
}
