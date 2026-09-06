//! ACP transport wrappers that persist envelopes around every request.

use super::*;

impl SessionManager {
    pub(super) fn acp_request(
        &self,
        run_id: &str,
        client: &AcpClient,
        method: AgentRpcMethod,
        params: Value,
    ) -> std::result::Result<Value, ClassifiedFailure> {
        let envelope = RpcEnvelope {
            direction: RpcDirection::Sent,
            method: method.as_str().to_owned(),
            payload: params.clone(),
        };
        self.inner
            .store
            .save_acp_envelope(run_id, &envelope)
            .map_err(|error| classify_message(&error.to_string()))?;

        let result = client
            .request(method, params, ACP_REQUEST_TIMEOUT)
            .map_err(|error| self.classify_client_failure(client, &error))?;

        let response_envelope = RpcEnvelope {
            direction: RpcDirection::Received,
            method: "rpc.result".into(),
            payload: result.clone(),
        };
        self.inner
            .store
            .save_acp_envelope(run_id, &response_envelope)
            .map_err(|error| classify_message(&error.to_string()))?;
        Ok(result)
    }

    pub(super) fn acp_prompt_request(
        &self,
        run_id: &str,
        client: &AcpClient,
        params: Value,
    ) -> std::result::Result<Value, ClassifiedFailure> {
        let method = AgentRpcMethod::Prompt;
        let envelope = RpcEnvelope {
            direction: RpcDirection::Sent,
            method: method.as_str().to_owned(),
            payload: params.clone(),
        };
        self.inner
            .store
            .save_acp_envelope(run_id, &envelope)
            .map_err(|error| classify_message(&error.to_string()))?;

        let result = client
            .request_with_activity_timeout(
                method,
                params,
                ACP_PROMPT_IDLE_TIMEOUT,
                ACP_PROMPT_TOTAL_TIMEOUT,
            )
            .map_err(|error| self.classify_client_failure(client, &error))?;

        self.inner
            .store
            .save_acp_envelope(
                run_id,
                &RpcEnvelope {
                    direction: RpcDirection::Received,
                    method: "rpc.result".into(),
                    payload: result.clone(),
                },
            )
            .map_err(|error| classify_message(&error.to_string()))?;
        Ok(result)
    }

    pub(super) fn acp_notify(
        &self,
        run_id: &str,
        client: &AcpClient,
        method: AgentRpcMethod,
        params: Value,
    ) -> Result<()> {
        let envelope = RpcEnvelope {
            direction: RpcDirection::Sent,
            method: method.as_str().to_owned(),
            payload: params.clone(),
        };
        self.inner.store.save_acp_envelope(run_id, &envelope)?;
        client.notify(method, params).map_err(Error::from)
    }
}
