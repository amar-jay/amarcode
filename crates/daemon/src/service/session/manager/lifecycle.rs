//! Live run and turn shutdown lifecycle.

use super::*;

impl SessionManager {
    /// Cancel the live run for a chat.
    pub fn cancel(&self, chat_id: &str) -> Result<()> {
        let live = {
            let mut guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            guard.remove(chat_id)
        };

        let Some(live) = live else {
            return Err(Error::msg(format!("no live run for chat: {chat_id}")));
        };

        let active_user_message_id = live.active_user_message_id.clone();
        let run_id = live.run_id.clone();
        remove_pending_requests_for_run(&self.inner, &run_id);

        let _ = self.acp_notify(
            &run_id,
            &live.client,
            AgentRpcMethod::Cancel,
            json!({ "sessionId": live.acp_session_id }),
        );
        // Prefer session/close when the agent advertised it; cancel is enough for now.
        let _ = live.client.kill();

        if let Some(user_message_id) = active_user_message_id {
            self.emit(EditorEvent::TurnUpdated {
                chat_id: chat_id.to_owned(),
                run_id: run_id.clone(),
                user_message_id,
                status: TurnStatus::Cancelled,
                stop_reason: Some("cancelled".into()),
                error_message: None,
            });
        }

        self.inner
            .store
            .update_run(&run_id, RunStatus::Stopped, None, None)?;
        self.emit(EditorEvent::RunUpdated {
            run_id,
            status: RunStatus::Stopped,
            error_message: None,
        });
        self.emit(EditorEvent::AgentConnectionChanged {
            agent_id: live.agent_id,
            connected: false,
            error_message: None,
        });
        Ok(())
    }

    /// Permanently delete a chat after its current turn has settled and its
    /// live ACP process, if any, has been stopped.
    pub fn delete_chat(&self, chat_id: &str) -> Result<()> {
        self.inner
            .store
            .get_chat(chat_id)?
            .ok_or_else(|| Error::msg(format!("chat not found: {chat_id}")))?;

        // Interrupt an active turn first, then wait for its prompt guard so all
        // cleanup writes finish before the cascading database deletion.
        if self.has_live_run(chat_id)? {
            self.cancel(chat_id)?;
        }
        let prompt_lock = self.prompt_lock(chat_id)?;
        let _prompt_guard = prompt_lock
            .lock()
            .map_err(|_| Error::msg("prompt lock poisoned"))?;

        // A prompt could have won the lock race immediately before deletion.
        if self.has_live_run(chat_id)? {
            self.cancel(chat_id)?;
        }

        if !self.inner.store.delete_chat(chat_id)? {
            return Err(Error::msg(format!("chat not found: {chat_id}")));
        }
        if let Ok(mut locks) = self.inner.prompt_locks.lock() {
            locks.remove(chat_id);
        }
        self.emit(EditorEvent::ChatUpdated {
            chat_id: chat_id.to_owned(),
        });
        Ok(())
    }

    fn has_live_run(&self, chat_id: &str) -> Result<bool> {
        Ok(self
            .inner
            .by_chat
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?
            .contains_key(chat_id))
    }

    pub(super) fn fail_run(&self, run_id: &str, error: &str) -> Result<()> {
        // If a prompt turn is open on this run, close it as failed first.
        if let Ok(mut guard) = self.inner.by_chat.lock() {
            let hit = guard.iter_mut().find_map(|(chat_id, live)| {
                if live.run_id == run_id {
                    live.active_user_message_id
                        .take()
                        .map(|user_message_id| (chat_id.clone(), user_message_id))
                } else {
                    None
                }
            });
            if let Some((chat_id, user_message_id)) = hit {
                drop(guard);
                self.emit(EditorEvent::TurnUpdated {
                    chat_id,
                    run_id: run_id.to_owned(),
                    user_message_id,
                    status: TurnStatus::Failed,
                    stop_reason: None,
                    error_message: Some(error.to_owned()),
                });
            }
        }
        self.inner
            .store
            .update_run(run_id, RunStatus::Failed, None, Some(error))?;
        self.emit(EditorEvent::RunUpdated {
            run_id: run_id.to_owned(),
            status: RunStatus::Failed,
            error_message: Some(error.to_owned()),
        });
        Ok(())
    }

    /// Clear the open turn on the live chat (if it matches) and notify clients.
    pub(super) fn finish_turn(
        &self,
        chat_id: &str,
        run_id: &str,
        user_message_id: &str,
        status: TurnStatus,
        stop_reason: Option<String>,
        error_message: Option<&str>,
    ) {
        if let Ok(mut guard) = self.inner.by_chat.lock() {
            if let Some(live) = guard.get_mut(chat_id) {
                if live.run_id == run_id
                    && live.active_user_message_id.as_deref() == Some(user_message_id)
                {
                    live.active_user_message_id = None;
                }
            }
        }
        self.emit(EditorEvent::TurnUpdated {
            chat_id: chat_id.to_owned(),
            run_id: run_id.to_owned(),
            user_message_id: user_message_id.to_owned(),
            status,
            stop_reason,
            error_message: error_message.map(str::to_owned),
        });
    }

    /// A prompt transport failure leaves the ACP process's state ambiguous.
    /// Detach it before killing the process so late notifications cannot touch
    /// this chat, then make every durable row terminal before publishing the
    /// corresponding editor events.
    pub(super) fn terminate_failed_prompt(
        &self,
        chat_id: &str,
        run_id: &str,
        user_message_id: &str,
        error: &str,
    ) -> Result<()> {
        let mut live = {
            let mut live_runs = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            if live_runs
                .get(chat_id)
                .is_none_or(|live| live.run_id != run_id)
            {
                return Ok(());
            }
            live_runs.remove(chat_id).expect("live run checked above")
        };

        remove_pending_requests_for_run(&self.inner, run_id);
        let _ = live.client.notify(
            AgentRpcMethod::Cancel,
            json!({ "sessionId": live.acp_session_id }),
        );
        let _ = live.client.kill();

        let mut first_message_error = None;
        for message_id in take_streaming_messages_from_live(&mut live) {
            match finalize_message(&self.inner, &message_id, MessageStatus::Interrupted) {
                Ok(()) => self.emit(EditorEvent::MessageUpdated {
                    message_id,
                    status: MessageStatus::Interrupted,
                }),
                Err(message_error) => {
                    first_message_error.get_or_insert(message_error);
                }
            }
        }

        self.inner
            .store
            .update_run(run_id, RunStatus::Failed, None, Some(error))?;
        self.emit(EditorEvent::RunUpdated {
            run_id: run_id.to_owned(),
            status: RunStatus::Failed,
            error_message: Some(error.to_owned()),
        });
        self.emit(EditorEvent::TurnUpdated {
            chat_id: chat_id.to_owned(),
            run_id: run_id.to_owned(),
            user_message_id: user_message_id.to_owned(),
            status: TurnStatus::Failed,
            stop_reason: None,
            error_message: Some(error.to_owned()),
        });
        self.emit(EditorEvent::AgentConnectionChanged {
            agent_id: live.agent_id,
            connected: false,
            error_message: Some(error.to_owned()),
        });

        if let Some(message_error) = first_message_error {
            return Err(message_error);
        }
        Ok(())
    }

    pub(super) fn detach_chat(&self, chat_id: &str) {
        let removed = self
            .inner
            .by_chat
            .lock()
            .ok()
            .and_then(|mut guard| guard.remove(chat_id));
        if let Some(live) = removed {
            remove_pending_requests_for_run(&self.inner, &live.run_id);
            let _ = live.client.kill();
            if let Some(user_message_id) = live.active_user_message_id {
                let _ = self.inner.events.send(EditorEvent::TurnUpdated {
                    chat_id: chat_id.to_owned(),
                    run_id: live.run_id.clone(),
                    user_message_id,
                    status: TurnStatus::Cancelled,
                    stop_reason: Some("replaced".into()),
                    error_message: Some("replaced by new run".into()),
                });
            }
            let _ = self.inner.store.update_run(
                &live.run_id,
                RunStatus::Stopped,
                None,
                Some("replaced by new run"),
            );
            let _ = self.inner.events.send(EditorEvent::RunUpdated {
                run_id: live.run_id,
                status: RunStatus::Stopped,
                error_message: Some("replaced by new run".into()),
            });
        }
    }

    pub(super) fn remove_live_run(&self, chat_id: &str, run_id: &str) {
        if let Ok(mut live_runs) = self.inner.by_chat.lock() {
            if live_runs
                .get(chat_id)
                .is_some_and(|live| live.run_id == run_id)
            {
                live_runs.remove(chat_id);
            }
        }
        remove_pending_requests_for_run(&self.inner, run_id);
    }
}
