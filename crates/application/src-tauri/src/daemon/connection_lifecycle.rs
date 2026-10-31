//! Owns SSH retry timing; RPCs wait for a verified endpoint and are never replayed.
use super::{
    remote::{self, ConnectionSettings},
    ssh::{self, SshTunnelEndpoint, SshTunnelManager},
};
use serde::Serialize;
use std::{
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tokio::{sync::watch, task::JoinHandle};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub status: ConnectionPhase,
    pub error: Option<String>,
    pub reconnect_attempt: u32,
    pub retry_in_ms: Option<u64>,
    /// Changes when a verified endpoint is lost or replaced.
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionPhase {
    #[default]
    Idle,
    Connecting,
    Connected,
    Reconnecting,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub status: ConnectionStatus,
    settings: Option<ConnectionSettings>,
    endpoint: Option<SshTunnelEndpoint>,
}

fn state() -> &'static watch::Sender<Snapshot> {
    static STATE: OnceLock<watch::Sender<Snapshot>> = OnceLock::new();
    STATE.get_or_init(|| watch::channel(Snapshot::default()).0)
}

pub(crate) fn generations() -> &'static watch::Sender<u64> {
    static GENERATIONS: OnceLock<watch::Sender<u64>> = OnceLock::new();
    GENERATIONS.get_or_init(|| watch::channel(0).0)
}

#[derive(Default)]
struct Supervisor {
    task: Option<JoinHandle<()>>,
    stopped: bool,
}

fn supervisor() -> &'static Mutex<Supervisor> {
    static SUPERVISOR: OnceLock<Mutex<Supervisor>> = OnceLock::new();
    SUPERVISOR.get_or_init(|| Mutex::new(Supervisor::default()))
}

/// Must be called inside the native async runtime. It is safe to call repeatedly.
pub(crate) fn start() {
    let mut supervisor = supervisor().lock().expect("SSH supervisor lock");
    if supervisor.stopped || supervisor.task.is_some() {
        return;
    }
    supervisor.task = Some(tokio::spawn(supervise(
        ssh::shared(),
        remote::load,
        remote::changes(),
        state(),
        generations(),
        Duration::from_secs(1),
        Duration::from_millis(250),
    )));
}

pub(crate) fn status() -> ConnectionStatus {
    state().borrow().status.clone()
}

pub(crate) async fn shutdown() {
    let task = {
        let mut supervisor = supervisor().lock().expect("SSH supervisor lock");
        supervisor.stopped = true;
        supervisor.task.take()
    };
    if let Some(task) = task {
        task.abort();
        let _ = task.await;
    }
    ssh::shared().shutdown().await;
}

pub(super) async fn endpoint(settings: &ConnectionSettings) -> Result<SshTunnelEndpoint, String> {
    start();
    let mut updates = state().subscribe();
    tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            remote::ensure_selected(settings)?;
            let snapshot = updates.borrow_and_update().clone();
            if snapshot.settings.as_ref() == Some(settings) {
                if let Some(endpoint) = snapshot.endpoint {
                    return Ok(endpoint);
                }
                if snapshot.status.status == ConnectionPhase::Reconnecting {
                    return Err(snapshot
                        .status
                        .error
                        .unwrap_or_else(|| "SSH is reconnecting".into()));
                }
            }
            updates
                .changed()
                .await
                .map_err(|_| "SSH connection supervisor stopped")?;
        }
    })
    .await
    .map_err(|_| "SSH connection is still unavailable; reconnecting in the background")?
}

fn publish(
    state: &watch::Sender<Snapshot>,
    generations: &watch::Sender<u64>,
    settings: Option<ConnectionSettings>,
    endpoint: Option<SshTunnelEndpoint>,
    mut status: ConnectionStatus,
) {
    let previous = state.borrow().clone();
    if previous.endpoint != endpoint {
        generations.send_modify(|generation| *generation = generation.wrapping_add(1));
    }
    status.generation = *generations.borrow();
    let next = Snapshot {
        status,
        settings,
        endpoint,
    };
    if previous != next {
        state.send_replace(next);
    }
}

pub(super) async fn supervise(
    manager: &SshTunnelManager,
    read: impl Fn() -> Result<ConnectionSettings, String> + Send + Sync,
    mut changes: watch::Receiver<u64>,
    state: &watch::Sender<Snapshot>,
    generations: &watch::Sender<u64>,
    poll_interval: Duration,
    initial_retry: Duration,
) {
    let mut selected = None;
    let mut attempt: u32 = 0;
    loop {
        changes.borrow_and_update();
        let settings = read();
        if settings.as_ref().ok() != selected.as_ref() {
            selected = settings.as_ref().ok().cloned();
            attempt = 0;
        }
        let mut delay = poll_interval;
        match settings {
            Ok(settings) if settings.ssh.is_some() => {
                let was_connected = {
                    let previous = state.borrow();
                    previous.settings.as_ref() == Some(&settings) && previous.endpoint.is_some()
                };
                let already_connected = was_connected
                    && manager
                        .is_running(settings.ssh.as_ref().unwrap(), &settings.token)
                        .await;
                match changes.has_changed() {
                    Ok(true) => continue,
                    Err(_) => return,
                    Ok(false) => {}
                }
                if !already_connected {
                    let error = {
                        let previous = state.borrow();
                        if was_connected {
                            Some("SSH connection closed; reconnecting".into())
                        } else if previous.settings.as_ref() == Some(&settings) {
                            previous.status.error.clone()
                        } else {
                            None
                        }
                    };
                    publish(
                        state,
                        generations,
                        Some(settings.clone()),
                        None,
                        ConnectionStatus {
                            status: if attempt == 0 && !was_connected {
                                ConnectionPhase::Connecting
                            } else {
                                ConnectionPhase::Reconnecting
                            },
                            reconnect_attempt: attempt,
                            error,
                            ..Default::default()
                        },
                    );
                }
                let result = tokio::select! {
                    result = manager.connect_transaction(settings.ssh.as_ref().unwrap(), &settings.token, true, || {
                        if read()? == settings { Ok(()) } else { Err("Daemon connection settings changed".into()) }
                    }) => result,
                    changed = changes.changed() => { if changed.is_err() { return; } continue; },
                };
                match result {
                    Ok((endpoint, ())) => {
                        attempt = 0;
                        publish(
                            state,
                            generations,
                            Some(settings),
                            Some(endpoint),
                            ConnectionStatus {
                                status: ConnectionPhase::Connected,
                                ..Default::default()
                            },
                        );
                    }
                    Err(error) => {
                        attempt = attempt.saturating_add(1);
                        delay = initial_retry
                            .saturating_mul(1u32 << (attempt - 1).min(5))
                            .min(Duration::from_secs(5));
                        publish(
                            state,
                            generations,
                            Some(settings),
                            None,
                            ConnectionStatus {
                                status: ConnectionPhase::Reconnecting,
                                error: Some(error),
                                reconnect_attempt: attempt,
                                retry_in_ms: Some(delay.as_millis() as u64),
                                ..Default::default()
                            },
                        );
                    }
                }
            }
            Ok(settings) => {
                publish(
                    state,
                    generations,
                    Some(settings),
                    None,
                    ConnectionStatus::default(),
                );
            }
            Err(error) => {
                publish(
                    state,
                    generations,
                    None,
                    None,
                    ConnectionStatus {
                        status: ConnectionPhase::Reconnecting,
                        error: Some(error),
                        retry_in_ms: Some(delay.as_millis() as u64),
                        ..Default::default()
                    },
                );
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(delay) => {},
            changed = changes.changed() => { if changed.is_err() { return; } },
        }
    }
}
