//! Agent-initiated request replies and live-session queries.

use super::*;

impl SessionManager {
    /// Run ACP `authenticate` for an agent (live session if present, else a short probe).
    pub fn authenticate_agent(&self, agent_id: &str, method_id: Option<&str>) -> Result<()> {
        let method_id = method_id.unwrap_or("agent");
        let params = json!({ "methodId": method_id });

        if let Some((run_id, client)) = self.live_client_for_agent(agent_id)? {
            self.acp_request(&run_id, &client, AgentRpcMethod::Authenticate, params)
                .map_err(Error::from)?;
            return Ok(());
        }

        let resolved = self.agents.resolve(agent_id)?;
        let (client, _inbound) = AcpClient::spawn(
            &resolved.command.to_string_lossy(),
            &resolved.arguments,
            &resolved.environment,
            None,
        )?;
        let initialize = client.request(
            AgentRpcMethod::Initialize,
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": {
                    "name": "amarcode-daemon",
                    "title": "Amarcode Daemon",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
            ACP_REQUEST_TIMEOUT,
        );
        if let Err(error) = initialize {
            let failure = self.classify_client_failure(&client, &error);
            let _ = client.kill();
            return Err(failure.into());
        }
        let auth = client.request(AgentRpcMethod::Authenticate, params, ACP_REQUEST_TIMEOUT);
        let failure = auth
            .as_ref()
            .err()
            .map(|error| self.classify_client_failure(&client, error));
        let _ = client.kill();
        match failure {
            Some(failure) => Err(failure.into()),
            None => Ok(()),
        }
    }

    fn live_client_for_agent(
        &self,
        agent_id: &str,
    ) -> Result<Option<(String, Arc<AcpClient>)>> {
        let guard = self
            .inner
            .by_chat
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?;
        Ok(guard.values().find_map(|live| {
            (live.agent_id == agent_id)
                .then(|| (live.run_id.clone(), Arc::clone(&live.client)))
        }))
    }

    /// Answer an agent-initiated request (`ApprovalRequired` / `QuestionRequired`).
    pub fn respond_to_agent(&self, request_id: &str, result: Value) -> Result<()> {
        let pending = {
            let mut guard = self
                .inner
                .pending
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            guard.remove(request_id)
        }
        .ok_or_else(|| Error::msg(format!("unknown pending agent request: {request_id}")))?;

        let client = {
            let guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            guard
                .get(&pending.chat_id)
                .filter(|live| live.run_id == pending.run_id)
                .map(|live| Arc::clone(&live.client))
                .ok_or_else(|| Error::msg("pending request belongs to a replaced run"))?
        };

        // ACP permission replies must use `outcome.optionId`. Translate
        // convenience shapes from the UI/CLI so agents don't abort the turn.
        let result = if pending.method == AgentEventMethod::PermissionRequested.as_str()
            || pending.method == "permission.requested"
        {
            normalize_permission_result(&pending.params, result)
        } else {
            result
        };
        self.inner.terminals.record_permission(
            &pending.run_id,
            &pending.chat_id,
            &pending.params,
            &result,
        );

        let envelope = RpcEnvelope {
            direction: RpcDirection::Sent,
            method: format!("response:{}", pending.method),
            payload: result.clone(),
        };
        self.inner
            .store
            .save_acp_envelope(&pending.run_id, &envelope)?;

        client
            .respond(pending.acp_id, result)
            .map_err(Error::from)?;
        Ok(())
    }

    pub fn respond_error_to_agent(
        &self,
        request_id: &str,
        code: i64,
        message: &str,
        data: Option<Value>,
    ) -> Result<()> {
        let pending = {
            let mut guard = self
                .inner
                .pending
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            guard.remove(request_id)
        }
        .ok_or_else(|| Error::msg(format!("unknown pending agent request: {request_id}")))?;

        let client = {
            let guard = self
                .inner
                .by_chat
                .lock()
                .map_err(|_| Error::msg("session lock poisoned"))?;
            guard
                .get(&pending.chat_id)
                .filter(|live| live.run_id == pending.run_id)
                .map(|live| Arc::clone(&live.client))
                .ok_or_else(|| Error::msg("pending request belongs to a replaced run"))?
        };

        client
            .respond_error(pending.acp_id, code, message, data)
            .map_err(Error::from)?;
        Ok(())
    }

    /// `(run_id, agent_id, acp_session_id)` for the live chat session, if any.
    pub fn live_run_for_chat(
        &self,
        chat_id: &str,
    ) -> Result<Option<(String, String, Option<String>)>> {
        let guard = self
            .inner
            .by_chat
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?;
        Ok(guard.get(chat_id).map(|live| {
            (
                live.run_id.clone(),
                live.agent_id.clone(),
                live.acp_session_id.clone(),
            )
        }))
    }

    pub fn pending_requests(&self) -> Result<Vec<PendingAgentRequest>> {
        let guard = self
            .inner
            .pending
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?;
        Ok(guard.values().cloned().collect())
    }
}
