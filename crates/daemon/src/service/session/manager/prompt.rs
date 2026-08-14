//! Prompt turns, per-chat serialization, history hydration, and session modes.

use super::*;

impl SessionManager {
    /// Persist a user message and send it to the agent (starts a run if needed).
    pub fn prompt(
        &self,
        chat_id: &str,
        agent_id: &str,
        text: impl AsRef<str>,
        session_mode: Option<&str>,
    ) -> Result<PromptResult> {
        let text = text.as_ref().trim();
        if text.is_empty() {
            return Err(Error::msg("prompt text must not be empty"));
        }

        // Keep session selection/startup and the complete ACP turn atomic for
        // this chat. Without this, concurrent RPC connections can both spawn a
        // run or can overwrite the live turn's streaming/message ownership.
        // Other chats use different locks and continue in parallel.
        let prompt_lock = self.prompt_lock(chat_id)?;
        let _prompt_guard = prompt_lock
            .lock()
            .map_err(|_| Error::msg("prompt lock poisoned"))?;

        let needs_start = {
            let guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            match guard.get(chat_id) {
                None => true,
                Some(live) => live.agent_id != agent_id,
            }
        };
        if needs_start {
            self.start_run_locked(chat_id, agent_id, session_mode)?;
        }

        let (run_id, client, session_id, needs_history_hydration) = {
            let mut guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            let live = guard
                .get_mut(chat_id)
                .ok_or_else(|| Error::msg("no live session after start_run"))?;
            (
                live.run_id.clone(),
                Arc::clone(&live.client),
                live.acp_session_id.clone(),
                std::mem::take(&mut live.needs_history_hydration),
            )
        };

        let now = timestamp();
        let user_message = Message {
            id: uuid::Uuid::new_v4().to_string(),
            chat_id: chat_id.to_owned(),
            agent_run_id: Some(run_id.clone()),
            role: MessageRole::User,
            content: text.to_owned(),
            status: MessageStatus::Complete,
            created_at: now.clone(),
            updated_at: now,
        };
        self.inner.store.create_message(&user_message)?;
        self.inner.store.replace_message_parts(
            &user_message.id,
            &[MessagePart {
                message_id: user_message.id.clone(),
                ordinal: 0,
                kind: MessagePartKind::Text,
                content_json: json!({ "text": text }).to_string(),
            }],
        )?;
        self.emit(EditorEvent::MessageUpdated {
            message_id: user_message.id.clone(),
            status: MessageStatus::Complete,
        });
        self.emit(EditorEvent::ChatUpdated {
            chat_id: chat_id.to_owned(),
        });

        // Mark the turn open before the blocking ACP prompt so subscribers can
        // show working state without waiting for the RPC to return.
        {
            let mut guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            if let Some(live) = guard.get_mut(chat_id) {
                live.active_user_message_id = Some(user_message.id.clone());
            }
        }
        self.emit(EditorEvent::TurnUpdated {
            chat_id: chat_id.to_owned(),
            run_id: run_id.clone(),
            user_message_id: user_message.id.clone(),
            status: TurnStatus::Started,
            stop_reason: None,
            error_message: None,
        });

        // ACP session/prompt: prompt is an array of content blocks.
        let prompt_text = if needs_history_hydration {
            self.emit(EditorEvent::ContextRestoration {
                chat_id: chat_id.to_owned(),
                run_id: run_id.clone(),
                source: "Restoring saved chat context".into(),
            });
            self.hydrated_prompt(chat_id, &user_message.id, text)?
        } else {
            text.to_owned()
        };
        let mut params = json!({
            "prompt": [{ "type": "text", "text": prompt_text }],
        });
        if let Some(sid) = &session_id {
            params
                .as_object_mut()
                .expect("params object")
                .insert("sessionId".into(), json!(sid));
        } else {
            let error = Error::msg("live session missing acp_session_id");
            if let Err(cleanup_error) =
                self.terminate_failed_prompt(chat_id, &run_id, &user_message.id, &error.to_string())
            {
                warn!(%run_id, error = %cleanup_error, "failed cleaning up broken prompt run");
            }
            return Err(error);
        }

        let prompt_result = match self.acp_prompt_request(&run_id, &client, params) {
            Ok(value) => value,
            Err(err) => {
                warn!(
                    %run_id,
                    %chat_id,
                    error = %err,
                    "ACP prompt failed before the agent returned a result"
                );
                if let Err(cleanup_error) = self.terminate_failed_prompt(
                    chat_id,
                    &run_id,
                    &user_message.id,
                    &err.to_string(),
                ) {
                    warn!(%run_id, error = %cleanup_error, "failed cleaning up rejected prompt run");
                }
                return Err(err);
            }
        };

        // The ACP reader sees notifications and the RPC result in order, but
        // persists notifications on a separate worker. Wait for that worker
        // before finalizing the messages from this turn.
        if let Err(err) = client.sync_inbound(ACP_REQUEST_TIMEOUT) {
            if let Err(cleanup_error) =
                self.terminate_failed_prompt(chat_id, &run_id, &user_message.id, &err.to_string())
            {
                warn!(%run_id, error = %cleanup_error, "failed cleaning up stalled inbound run");
            }
            return Err(Error::msg(err.to_string()));
        }

        // Turn finished (stopReason typically end_turn). Finalize any streaming
        // assistant message that arrived via session/update chunks.
        let message_ids = {
            let mut guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            guard
                .get_mut(chat_id)
                .map(take_streaming_messages_from_live)
                .unwrap_or_default()
        };
        for message_id in message_ids {
            finalize_message(&self.inner, &message_id, MessageStatus::Complete)?;
            self.emit(EditorEvent::MessageUpdated {
                message_id,
                status: MessageStatus::Complete,
            });
        }

        let stop_reason = extract_stop_reason(&prompt_result);
        self.finish_turn(
            chat_id,
            &run_id,
            &user_message.id,
            TurnStatus::Completed,
            stop_reason,
            None,
        );

        Ok(PromptResult {
            run_id,
            chat_id: chat_id.to_owned(),
            agent_id: agent_id.to_owned(),
            user_message_id: user_message.id,
            acp_session_id: session_id,
        })
    }

    pub(super) fn prompt_lock(&self, chat_id: &str) -> Result<Arc<std::sync::Mutex<()>>> {
        let mut locks = self
            .inner
            .prompt_locks
            .lock()
            .map_err(|_| Error::msg("prompt lock registry poisoned"))?;

        // A waiting caller owns a strong Arc, so pruning dead weak references
        // cannot split callers for the same chat across different locks.
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(chat_id).and_then(std::sync::Weak::upgrade) {
            return Ok(lock);
        }

        let lock = Arc::new(std::sync::Mutex::new(()));
        locks.insert(chat_id.to_owned(), Arc::downgrade(&lock));
        Ok(lock)
    }

    /// Change an active ACP session using the configuration it advertised.
    pub fn set_session_mode(&self, chat_id: &str, mode: &str) -> Result<()> {
        let (run_id, client, session_id, mut configuration) = {
            let guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            let live = guard
                .get(chat_id)
                .ok_or_else(|| Error::msg("no active session for this chat"))?;
            (
                live.run_id.clone(),
                Arc::clone(&live.client),
                live.acp_session_id.clone(),
                live.session_configuration.clone(),
            )
        };
        let session_id = session_id.ok_or_else(|| Error::msg("ACP session has not started"))?;
        let applied =
            configure_session(&session_id, &mut configuration, mode, |method, params| {
                self.acp_request(
                    &run_id,
                    &client,
                    AgentRpcMethod::Other(method.to_owned()),
                    params,
                )
            })?;
        if !applied {
            return Err(Error::msg(
                "agent does not advertise a compatible session mode configuration",
            ));
        }

        let mut live_runs = self
            .inner
            .by_chat
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?;
        if let Some(live) = live_runs
            .get_mut(chat_id)
            .filter(|live| live.run_id == run_id)
        {
            live.session_configuration = configuration;
            Ok(())
        } else {
            Err(Error::msg("session was replaced while changing its mode"))
        }
    }

    /// Provide an isolated fallback when an agent cannot resume its own saved
    /// session. The transcript contains only rows from this chat and excludes
    /// the message about to be sent, which is appended once as the live prompt.
    fn hydrated_prompt(
        &self,
        chat_id: &str,
        current_message_id: &str,
        prompt: &str,
    ) -> Result<String> {
        const MAX_HISTORY_CHARS: usize = 60_000;

        let mut turns = self
            .inner
            .store
            .messages(chat_id)?
            .into_iter()
            .filter(|message| {
                message.id != current_message_id && !message.content.trim().is_empty()
            })
            .filter_map(|message| match message.role {
                MessageRole::User => Some(format!("User: {}", message.content.trim())),
                MessageRole::Assistant => Some(format!("Assistant: {}", message.content.trim())),
                _ => None,
            })
            .collect::<Vec<_>>();

        let mut history = turns.join("\n\n");
        if history.len() > MAX_HISTORY_CHARS {
            while history.len() > MAX_HISTORY_CHARS && !turns.is_empty() {
                turns.remove(0);
                history = turns.join("\n\n");
            }
            history = format!("[Earlier conversation omitted for context length.]\n\n{history}");
        }

        if history.is_empty() {
            return Ok(prompt.to_owned());
        }

        Ok(format!(
            "Continue this conversation for the current chat only. Treat the transcript as prior context; respond to the final user message normally.\n\n<chat-history>\n{history}\n</chat-history>\n\nUser: {prompt}"
        ))
    }
}
