//! Durable per-chat FIFO. A single worker owns all deliveries for each chat.
use super::PromptResult;
use crate::{
    protocol::{
        rpc::{PromptParams, PromptQueueState, UpdatePromptQueueParams},
        EditorEvent,
    },
    store::Store,
    Error, Result,
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
};
use tokio::sync::broadcast;
type Execute = dyn Fn(PromptParams, &AtomicBool) -> Result<PromptResult> + Send + Sync;
type Waiter = mpsc::SyncSender<Result<PromptResult>>;
#[derive(Default)]
struct Workers {
    active: HashMap<String, Arc<AtomicBool>>,
    waiters: HashMap<String, Waiter>,
}
pub struct PromptQueue {
    store: Arc<Store>,
    events: broadcast::Sender<EditorEvent>,
    execute: Arc<Execute>,
    workers: Mutex<Workers>,
}
impl PromptQueue {
    pub fn new(
        store: Arc<Store>,
        events: broadcast::Sender<EditorEvent>,
        execute: impl Fn(PromptParams, &AtomicBool) -> Result<PromptResult> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            events,
            execute: Arc::new(execute),
            workers: Mutex::new(Workers::default()),
        })
    }
    pub fn state(&self, chat_id: &str) -> Result<PromptQueueState> {
        self.store.prompt_queue(chat_id)
    }
    fn changed(&self, chat_id: &str) {
        let _ = self.events.send(EditorEvent::ChatUpdated {
            chat_id: chat_id.into(),
        });
    }
    fn kick(self: &Arc<Self>, chat_id: &str, workers: &mut Workers) -> Result<()> {
        if workers.active.contains_key(chat_id) || self.state(chat_id)?.paused {
            return Ok(());
        }
        let stopped = Arc::new(AtomicBool::new(false));
        workers.active.insert(chat_id.into(), Arc::clone(&stopped));
        let queue = Arc::clone(self);
        let chat = chat_id.to_owned();
        if let Err(error) = std::thread::Builder::new()
            .name("prompt-queue".into())
            .spawn(move || queue.work(&chat, stopped))
        {
            workers.active.remove(chat_id);
            self.store.pause_prompt_queue(
                chat_id,
                Some("Could not start queue worker. Resume to retry."),
            )?;
            return Err(error.into());
        }
        Ok(())
    }
    pub fn enqueue(self: &Arc<Self>, id: &str, prompt: PromptParams) -> Result<PromptQueueState> {
        let chat = prompt.chat_id.clone();
        let mut workers = self
            .workers
            .lock()
            .map_err(|_| Error::msg("Queue lock poisoned"))?;
        self.store.insert_prompt_job(id, &prompt)?;
        self.kick(&chat, &mut workers)?;
        let state = self.state(&chat)?;
        self.changed(&chat);
        Ok(state)
    }
    /// Preserve the original blocking prompt RPC while using the same FIFO.
    pub fn prompt(self: &Arc<Self>, prompt: PromptParams) -> Result<PromptResult> {
        let id = uuid::Uuid::new_v4().to_string();
        let chat = prompt.chat_id.clone();
        let (send, receive) = mpsc::sync_channel(1);
        {
            let mut workers = self
                .workers
                .lock()
                .map_err(|_| Error::msg("Queue lock poisoned"))?;
            let queue = self.state(&chat)?;
            if queue.paused && !queue.items.is_empty() {
                return Err(Error::msg(
                    "Follow-up queue is paused. Resume it before sending another prompt.",
                ));
            }
            if queue.items.is_empty() {
                self.store.pause_prompt_queue(&chat, None)?;
            }
            self.store.insert_prompt_job(&id, &prompt)?;
            workers.waiters.insert(id.clone(), send);
            if let Err(error) = self.kick(&chat, &mut workers) {
                workers.waiters.remove(&id);
                return Err(error);
            }
        }
        self.changed(&chat);
        receive
            .recv()
            .map_err(|_| Error::msg("Prompt queue worker stopped"))?
    }
    pub fn update(self: &Arc<Self>, params: UpdatePromptQueueParams) -> Result<PromptQueueState> {
        let mut workers = self
            .workers
            .lock()
            .map_err(|_| Error::msg("Queue lock poisoned"))?;
        match params.action.as_str() {
            "pause" => self
                .store
                .pause_prompt_queue(&params.chat_id, Some("Queue paused."))?,
            "resume" => {
                self.store.pause_prompt_queue(&params.chat_id, None)?;
                self.kick(&params.chat_id, &mut workers)?;
            }
            "remove" => {
                let id = params
                    .id
                    .as_deref()
                    .ok_or_else(|| Error::msg("Missing queued message ID"))?;
                self.store.remove_prompt_job(&params.chat_id, id)?;
                if let Some(waiter) = workers.waiters.remove(id) {
                    let _ = waiter.send(Err(Error::msg("Queued message removed")));
                }
            }
            "edit" => self.store.edit_prompt_job(
                &params.chat_id,
                params
                    .id
                    .as_deref()
                    .ok_or_else(|| Error::msg("Missing queued message ID"))?,
                params
                    .text
                    .as_deref()
                    .ok_or_else(|| Error::msg("Missing queued text"))?,
            )?,
            _ => return Err(Error::msg("Unknown queue action")),
        }
        let state = self.state(&params.chat_id)?;
        self.changed(&params.chat_id);
        Ok(state)
    }
    pub fn pause(&self, chat_id: &str, reason: &str) -> Result<bool> {
        let guard = self
            .workers
            .lock()
            .map_err(|_| Error::msg("Queue lock poisoned"))?;
        if let Some(signal) = guard.active.get(chat_id) {
            signal.store(true, Ordering::SeqCst);
        }
        self.store.pause_prompt_queue(chat_id, Some(reason))?;
        self.changed(chat_id);
        Ok(guard.active.contains_key(chat_id))
    }
    fn work(self: Arc<Self>, chat: &str, stopped: Arc<AtomicBool>) {
        loop {
            let next = (|| -> Result<_> {
                let mut workers = self
                    .workers
                    .lock()
                    .map_err(|_| Error::msg("Queue lock poisoned"))?;
                if self.state(chat)?.paused {
                    workers.active.remove(chat);
                    return Ok(None);
                }
                stopped.store(false, Ordering::SeqCst);
                let next = self.store.claim_prompt_job(chat)?;
                if next.is_none() {
                    workers.active.remove(chat);
                }
                Ok(next)
            })();
            let (id, prompt) = match next {
                Ok(Some(job)) => job,
                Ok(None) => break,
                Err(error) => {
                    tracing::error!(%error,%chat,"Queue claim failed");
                    self.fail_worker(chat, &error.to_string());
                    break;
                }
            };
            self.changed(chat);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (self.execute)(prompt, &stopped)
            }))
            .unwrap_or_else(|_| Err(Error::msg("Prompt worker panicked")));
            let failure = if stopped.load(Ordering::SeqCst) {
                Some("Stopped by you. Queue paused.".to_owned())
            } else {
                match &result {
                    Err(error) => Some(error.to_string()),
                    Ok(result) => result
                        .stop_reason
                        .as_deref()
                        .filter(|reason| *reason != "end_turn")
                        .map(|reason| format!("Turn stopped: {reason}. Queue paused.")),
                }
            };
            let completed = (|| -> Result<()> {
                let mut workers = self
                    .workers
                    .lock()
                    .map_err(|_| Error::msg("Queue lock poisoned"))?;
                self.store.finish_prompt_job(&id, failure.as_deref())?;
                if let Some(error) = failure.as_deref() {
                    self.store.pause_prompt_queue(chat, Some(error))?;
                }
                if let Some(waiter) = workers.waiters.remove(&id) {
                    if failure.is_some() {
                        self.store.remove_prompt_job(chat, &id)?;
                    }
                    let _ = waiter.send(result);
                }
                Ok(())
            })();
            self.changed(chat);
            if let Err(error) = completed {
                self.fail_worker(chat, &error.to_string());
                break;
            }
        }
    }
    fn fail_worker(&self, chat: &str, error: &str) {
        let _ = self.store.pause_prompt_queue(chat, Some(error));
        if let Ok(mut workers) = self.workers.lock() {
            workers.active.remove(chat);
            // Wake direct RPCs belonging to this chat; jobs stay saved for recovery.
            if let Ok(state) = self.state(chat) {
                for job in state.items {
                    if job.status == "running" {
                        let _ = self.store.finish_prompt_job(&job.id, Some(error));
                    }
                    if let Some(waiter) = workers.waiters.remove(&job.id) {
                        let _ = waiter.send(Err(Error::msg(error)));
                    }
                }
            }
        }
        self.changed(chat);
    }
}
