//! Tauri IPC adapters for events operations.
use crate::daemon::DaemonClient;
use amarcode_protocol::rpc::SubscribeEventsParams;
use amarcode_protocol::EditorEvent;
use serde::Serialize;
use tauri::{ipc::Channel, State};

#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum DaemonEventStreamStatus {
    Connected,
    Disconnected { error: String },
}

/// Run one daemon event stream for the lifetime of its dedicated TCP
/// connection. Keeping this command pending is intentional: if the daemon
/// disconnects, the returned error rejects the frontend `invoke` promise so
/// React can reconnect instead of silently retaining a dead subscription.
#[tauri::command]
pub async fn subscribe_events(
    client: State<'_, DaemonClient>,
    filter: SubscribeEventsParams,
    on_event: Channel<EditorEvent>,
    on_status: Channel<DaemonEventStreamStatus>,
) -> Result<(), String> {
    let mut subscription = client.subscribe_events(filter).await?;
    on_status
        .send(DaemonEventStreamStatus::Connected)
        .map_err(|error| error.to_string())?;

    loop {
        match subscription.next::<EditorEvent>().await {
            Ok(event) => on_event.send(event).map_err(|error| error.to_string())?,
            Err(error) => {
                let _ = on_status.send(DaemonEventStreamStatus::Disconnected {
                    error: error.clone(),
                });
                return Err(error);
            }
        }
    }
}
