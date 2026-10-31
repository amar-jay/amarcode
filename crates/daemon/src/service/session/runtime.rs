//! Coherent, revisioned public session state, protected with the live runs and requests.
use super::types::{LiveRun, PendingAgentRequest, SessionInner};
use crate::{
    protocol::{
        ChatSessionSnapshot, EditorEvent, RunStatus, SessionPendingRequest, SessionRequestKind,
        SessionTurnSnapshot, TurnStatus,
    },
    store::AgentRun,
};
use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

pub(super) struct SessionState {
    runs: HashMap<String, LiveRun>,
    pub pending: HashMap<String, PendingAgentRequest>,
    snapshots: HashMap<String, ChatSessionSnapshot>,
    instance_id: String,
}

impl From<HashMap<String, LiveRun>> for SessionState {
    fn from(runs: HashMap<String, LiveRun>) -> Self {
        Self {
            runs,
            pending: HashMap::new(),
            snapshots: HashMap::new(),
            instance_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}

// ACP handlers operate on the live-run map through the same guard used for public state.
impl Deref for SessionState {
    type Target = HashMap<String, LiveRun>;
    fn deref(&self) -> &Self::Target {
        &self.runs
    }
}
impl DerefMut for SessionState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.runs
    }
}

impl SessionState {
    #[cfg(test)]
    pub fn record_running_turn(&mut self, chat_id: &str) {
        let snapshot =
            self.snapshots
                .entry(chat_id.to_owned())
                .or_insert_with(|| ChatSessionSnapshot {
                    daemon_instance_id: self.instance_id.clone(),
                    chat_id: chat_id.to_owned(),
                    revision: 0,
                    run_id: None,
                    agent_id: None,
                    run_status: None,
                    turn: None,
                    pending_requests: vec![],
                    error_message: None,
                    error_kind: None,
                    context_restoration: None,
                });
        snapshot.revision = 2;
        snapshot.run_id = Some("run-1".into());
        snapshot.run_status = Some(RunStatus::Running);
        snapshot.turn = Some(SessionTurnSnapshot {
            user_message_id: "user-1".into(),
            status: TurnStatus::Started,
            stop_reason: None,
            error_message: None,
            error_kind: None,
        });
    }

    /// Snapshots retained for this daemon process. A new subscriber needs them
    /// because the event bus does not replay work that started earlier.
    pub fn retained_snapshots(&self) -> Vec<ChatSessionSnapshot> {
        let mut snapshots: Vec<_> = self.snapshots.values().cloned().collect();
        snapshots.sort_by(|left, right| left.chat_id.cmp(&right.chat_id));
        snapshots
    }

    pub fn snapshot(&mut self, chat_id: &str) -> ChatSessionSnapshot {
        self.snapshots
            .entry(chat_id.to_owned())
            .or_insert_with(|| ChatSessionSnapshot {
                daemon_instance_id: self.instance_id.clone(),
                chat_id: chat_id.to_owned(),
                revision: 0,
                run_id: None,
                agent_id: None,
                run_status: None,
                turn: None,
                pending_requests: vec![],
                error_message: None,
                error_kind: None,
                context_restoration: None,
            })
            .clone()
    }

    fn publish(&mut self, chat_id: &str, events: &tokio::sync::broadcast::Sender<EditorEvent>) {
        let snapshot = self
            .snapshots
            .get_mut(chat_id)
            .expect("snapshot initialized");
        snapshot.revision += 1;
        let _ = events.send(EditorEvent::SessionStateUpdated {
            snapshot: snapshot.clone(),
        });
    }

    /// Commit and publish public transitions while holding the state lock. Legacy events
    /// remain available for CLI consumers, but are not the desktop's source of truth.
    pub fn transition(
        &mut self,
        event: &EditorEvent,
        run: Option<&AgentRun>,
        events: &tokio::sync::broadcast::Sender<EditorEvent>,
    ) {
        let chat_id = match event {
            EditorEvent::RunUpdated { .. } => run.map(|r| r.chat_id.clone()),
            EditorEvent::TurnUpdated { chat_id, .. }
            | EditorEvent::ContextRestoration { chat_id, .. } => Some(chat_id.clone()),
            EditorEvent::ApprovalRequired { request_id, .. }
            | EditorEvent::QuestionRequired { request_id, .. } => {
                self.pending.get(request_id).map(|p| p.chat_id.clone())
            }
            _ => None,
        };
        let Some(chat_id) = chat_id else {
            return;
        };
        self.snapshot(&chat_id);
        let snapshot = self.snapshots.get_mut(&chat_id).unwrap();
        match event {
            EditorEvent::RunUpdated {
                run_id,
                status,
                error_message,
                error_kind,
            } => {
                if snapshot.run_id.as_deref() == Some(run_id)
                    && matches!(
                        snapshot.run_status,
                        Some(RunStatus::Stopped | RunStatus::Failed | RunStatus::Completed)
                    )
                    && matches!(status, RunStatus::Starting | RunStatus::Running)
                {
                    return;
                }
                if *status == RunStatus::Starting {
                    snapshot.run_id = Some(run_id.clone());
                    snapshot.agent_id = run.map(|r| r.agent_id.clone());
                    snapshot.turn = None;
                    snapshot.pending_requests.clear();
                    snapshot.context_restoration = None;
                } else if snapshot.run_id.as_deref().is_some_and(|id| id != run_id) {
                    return;
                }
                snapshot.run_id = Some(run_id.clone());
                snapshot.agent_id = run.map(|r| r.agent_id.clone());
                snapshot.run_status = Some(*status);
                snapshot.error_message = error_message.clone();
                snapshot.error_kind = *error_kind;
                if matches!(
                    status,
                    RunStatus::Failed | RunStatus::Stopped | RunStatus::Completed
                ) {
                    snapshot.pending_requests.clear();
                    snapshot.context_restoration = None;
                    if let Some(turn) = snapshot
                        .turn
                        .as_mut()
                        .filter(|t| t.status == TurnStatus::Started)
                    {
                        turn.status = if *status == RunStatus::Failed {
                            TurnStatus::Failed
                        } else {
                            TurnStatus::Cancelled
                        };
                        turn.error_message = error_message.clone();
                        turn.error_kind = *error_kind;
                    }
                    self.pending.retain(|_, p| p.run_id != *run_id);
                }
            }
            EditorEvent::TurnUpdated {
                run_id,
                user_message_id,
                status,
                stop_reason,
                error_message,
                error_kind,
                ..
            } => {
                if snapshot.run_id.as_deref().is_some_and(|id| id != run_id) {
                    return;
                }
                // A cancelled/failed run cannot be resurrected by its prompt RPC returning late.
                if *status == TurnStatus::Started
                    && matches!(
                        snapshot.run_status,
                        Some(RunStatus::Stopped | RunStatus::Failed | RunStatus::Completed)
                    )
                {
                    return;
                }
                if snapshot
                    .turn
                    .as_ref()
                    .is_some_and(|t| t.user_message_id != *user_message_id)
                    && *status != TurnStatus::Started
                {
                    return;
                }
                if snapshot.turn.as_ref().is_some_and(|t| {
                    t.user_message_id == *user_message_id
                        && matches!(t.status, TurnStatus::Cancelled | TurnStatus::Failed)
                        && t.status != *status
                }) {
                    return;
                }
                snapshot.run_id = Some(run_id.clone());
                snapshot.turn = Some(SessionTurnSnapshot {
                    user_message_id: user_message_id.clone(),
                    status: *status,
                    stop_reason: stop_reason.clone(),
                    error_message: error_message.clone(),
                    error_kind: *error_kind,
                });
                snapshot.error_message = error_message.clone();
                snapshot.error_kind = *error_kind;
                snapshot.context_restoration = None;
                if *status != TurnStatus::Started {
                    snapshot.pending_requests.clear();
                    self.pending.retain(|_, p| p.run_id != *run_id);
                }
            }
            EditorEvent::ApprovalRequired {
                run_id,
                request_id,
                details,
            }
            | EditorEvent::QuestionRequired {
                run_id,
                request_id,
                details,
            } => {
                if snapshot.run_id.as_deref().is_some_and(|id| id != run_id) {
                    return;
                }
                if !snapshot
                    .pending_requests
                    .iter()
                    .any(|p| p.request_id == *request_id)
                {
                    snapshot.pending_requests.push(SessionPendingRequest {
                        request_id: request_id.clone(),
                        kind: if matches!(event, EditorEvent::QuestionRequired { .. }) {
                            SessionRequestKind::Input
                        } else {
                            SessionRequestKind::Approval
                        },
                        details: details.clone(),
                    });
                }
            }
            EditorEvent::ContextRestoration { run_id, source, .. } => {
                if snapshot.run_id.as_deref() != Some(run_id) {
                    return;
                }
                snapshot.context_restoration = Some(source.clone());
            }
            _ => return,
        }
        self.publish(&chat_id, events);
    }

    pub fn claim_request(
        &mut self,
        request_id: &str,
        events: &tokio::sync::broadcast::Sender<EditorEvent>,
    ) -> Option<PendingAgentRequest> {
        let request = self.pending.remove(request_id)?;
        if let Some(snapshot) = self.snapshots.get_mut(&request.chat_id) {
            snapshot
                .pending_requests
                .retain(|p| p.request_id != request_id);
            self.publish(&request.chat_id, events);
        }
        Some(request)
    }

    pub fn remove_requests(
        &mut self,
        run_id: &str,
        events: &tokio::sync::broadcast::Sender<EditorEvent>,
    ) {
        self.pending.retain(|_, p| p.run_id != run_id);
        let chats = self
            .snapshots
            .iter_mut()
            .filter_map(|(chat, snapshot)| {
                if snapshot.run_id.as_deref() == Some(run_id)
                    && !snapshot.pending_requests.is_empty()
                {
                    snapshot.pending_requests.clear();
                    Some(chat.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for chat in chats {
            self.publish(&chat, events);
        }
    }

    pub fn forget_chat(&mut self, chat_id: &str) {
        self.snapshots.remove(chat_id);
    }
}

pub(super) fn emit(inner: &SessionInner, event: EditorEvent) {
    if !matches!(
        &event,
        EditorEvent::RunUpdated { .. }
            | EditorEvent::TurnUpdated { .. }
            | EditorEvent::ApprovalRequired { .. }
            | EditorEvent::QuestionRequired { .. }
            | EditorEvent::ContextRestoration { .. }
    ) {
        let _ = inner.events.send(event);
        return;
    }
    // Never acquire the SQLite mutex while holding the session mutex.
    let run = match &event {
        EditorEvent::RunUpdated { run_id, .. } => inner.store.get_run(run_id).ok().flatten(),
        _ => None,
    };
    let mut state = inner.state.lock().expect("session state poisoned");
    state.transition(&event, run.as_ref(), &inner.events);
    let _ = inner.events.send(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        acp::RpcId,
        protocol::{AgentFailureKind, SessionRequestKind},
    };
    use serde_json::json;
    fn run(id: &str) -> AgentRun {
        AgentRun {
            id: id.into(),
            chat_id: "chat".into(),
            agent_id: "agent".into(),
            acp_session_id: Some("session".into()),
            status: RunStatus::Running,
            started_at: "now".into(),
            finished_at: None,
            error_message: None,
            context_usage: None,
        }
    }
    fn run_event(id: &str, status: RunStatus) -> EditorEvent {
        EditorEvent::RunUpdated {
            run_id: id.into(),
            status,
            error_message: None,
            error_kind: None,
        }
    }
    fn turn(id: &str, status: TurnStatus) -> EditorEvent {
        EditorEvent::TurnUpdated {
            chat_id: "chat".into(),
            run_id: id.into(),
            user_message_id: "user".into(),
            status,
            stop_reason: None,
            error_message: None,
            error_kind: None,
        }
    }
    fn pending(
        state: &mut SessionState,
        id: &str,
        input: bool,
        events: &tokio::sync::broadcast::Sender<EditorEvent>,
    ) {
        state.pending.insert(
            id.into(),
            PendingAgentRequest {
                request_id: id.into(),
                run_id: "run".into(),
                chat_id: "chat".into(),
                acp_id: RpcId::Number(99),
                method: "session/request_permission".into(),
                params: json!({"message":"continue?"}),
            },
        );
        let event = if input {
            EditorEvent::QuestionRequired {
                run_id: "run".into(),
                request_id: id.into(),
                details: json!({"message":"question"}),
            }
        } else {
            EditorEvent::ApprovalRequired {
                run_id: "run".into(),
                request_id: id.into(),
                details: json!({"options":[]}),
            }
        };
        state.transition(&event, None, events);
    }
    #[test]
    fn snapshots_recover_ordered_requests_and_exclude_transport_ids() {
        let mut state = SessionState::from(HashMap::new());
        let (events, mut receiver) = tokio::sync::broadcast::channel(32);
        state.transition(
            &run_event("run", RunStatus::Starting),
            Some(&run("run")),
            &events,
        );
        state.transition(&turn("run", TurnStatus::Started), None, &events);
        pending(&mut state, "second-alphabetically", false, &events);
        pending(&mut state, "first-alphabetically", true, &events);
        let snapshot = state.snapshot("chat");
        assert_eq!(snapshot.revision, 4);
        assert_eq!(snapshot.turn.unwrap().status, TurnStatus::Started);
        assert_eq!(
            snapshot.pending_requests[0].request_id,
            "second-alphabetically"
        );
        assert!(matches!(
            snapshot.pending_requests[1].kind,
            SessionRequestKind::Input
        ));
        let serialized = serde_json::to_string(&state.snapshot("chat")).unwrap();
        assert!(!serialized.contains("acp_id"));
        assert!(!serialized.contains("99"));
        for revision in 1..=4 {
            assert!(
                matches!(receiver.try_recv().unwrap(), EditorEvent::SessionStateUpdated { snapshot } if snapshot.revision == revision)
            );
        }
        let retained = state.retained_snapshots();
        assert_eq!(retained.len(), 1);
        assert_eq!(
            retained[0].turn.as_ref().unwrap().status,
            TurnStatus::Started
        );
    }
    #[test]
    fn only_one_client_claims_a_request_and_every_client_observes_resolution() {
        let mut state = SessionState::from(HashMap::new());
        let (events, _) = tokio::sync::broadcast::channel(32);
        pending(&mut state, "request", false, &events);
        let mut first = events.subscribe();
        let mut second = events.subscribe();
        let shared = std::sync::Arc::new(std::sync::Mutex::new(state));
        let clients = (0..2)
            .map(|_| {
                let shared = shared.clone();
                let events = events.clone();
                std::thread::spawn(move || {
                    shared
                        .lock()
                        .unwrap()
                        .claim_request("request", &events)
                        .is_some()
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            clients
                .into_iter()
                .map(|client| client.join().unwrap() as usize)
                .sum::<usize>(),
            1
        );
        for receiver in [&mut first, &mut second] {
            assert!(
                matches!(receiver.try_recv().unwrap(), EditorEvent::SessionStateUpdated { snapshot } if snapshot.pending_requests.is_empty())
            );
        }
    }
    #[test]
    fn terminal_and_replaced_runs_cannot_be_resurrected_by_late_events() {
        let mut state = SessionState::from(HashMap::new());
        let (events, _) = tokio::sync::broadcast::channel(32);
        state.transition(
            &run_event("run", RunStatus::Starting),
            Some(&run("run")),
            &events,
        );
        state.transition(&turn("run", TurnStatus::Started), None, &events);
        pending(&mut state, "approval", false, &events);
        state.transition(&turn("run", TurnStatus::Cancelled), None, &events);
        state.transition(
            &run_event("run", RunStatus::Stopped),
            Some(&run("run")),
            &events,
        );
        let revision = state.snapshot("chat").revision;
        state.transition(&turn("run", TurnStatus::Completed), None, &events);
        state.transition(
            &run_event("run", RunStatus::Running),
            Some(&run("run")),
            &events,
        );
        assert_eq!(state.snapshot("chat").revision, revision);
        assert!(state.pending.is_empty());
        state.transition(
            &run_event("new", RunStatus::Starting),
            Some(&run("new")),
            &events,
        );
        state.transition(&turn("run", TurnStatus::Failed), None, &events);
        let snapshot = state.snapshot("chat");
        assert_eq!(snapshot.run_id.as_deref(), Some("new"));
        assert!(snapshot.turn.is_none());
        assert!(snapshot.pending_requests.is_empty());
    }
    #[test]
    fn failure_and_restart_snapshots_clear_active_state() {
        let mut state = SessionState::from(HashMap::new());
        let (events, _) = tokio::sync::broadcast::channel(16);
        state.transition(
            &run_event("run", RunStatus::Starting),
            Some(&run("run")),
            &events,
        );
        state.transition(&turn("run", TurnStatus::Started), None, &events);
        let failure = EditorEvent::RunUpdated {
            run_id: "run".into(),
            status: RunStatus::Failed,
            error_message: Some("Disconnected".into()),
            error_kind: Some(AgentFailureKind::Error),
        };
        state.transition(&failure, Some(&run("run")), &events);
        let stopped = state.snapshot("chat");
        assert_eq!(stopped.turn.unwrap().status, TurnStatus::Failed);
        assert_eq!(stopped.error_message.as_deref(), Some("Disconnected"));
        let restarted = SessionState::from(HashMap::new()).snapshot("chat");
        assert_ne!(stopped.daemon_instance_id, restarted.daemon_instance_id);
        assert!(restarted.turn.is_none());
        assert!(restarted.pending_requests.is_empty());
    }
}
