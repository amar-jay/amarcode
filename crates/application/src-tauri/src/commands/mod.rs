//! IPC boundary: preserve frontend command names and arguments, adapt them to
//! shared daemon requests or native desktop services, and report channel status.
//! Domain behavior and transport framing belong outside this module.

pub mod agents;
pub mod application;
pub mod chats;
pub mod daemon;
pub mod events;
pub mod sessions;
pub mod workspace;

use serde::Serialize;
use tauri::ipc::Channel;

/// Commands own publishing terminal failures to the frontend. Services publish
/// progress and return errors; they do not publish the same failure a second time.
fn report_failure<T, S: Serialize + Clone>(
    result: Result<T, String>,
    channel: &Channel<S>,
    failed: impl FnOnce(String) -> S,
) -> Result<T, String> {
    if let Err(error) = &result {
        let _ = channel.send(failed(error.clone()));
    }
    result
}
