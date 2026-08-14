//! ACP transport wrappers that persist envelopes around every request.

use super::*;

impl SessionManager {
    pub(super) fn acp_request(
        &self,
        run_id: &str,
        client: &AcpClient,
        method: AgentRpcMethod,
        params: Value,
    ) -> Result<Value> {
        let envelope = RpcEnvelope {
            direction: RpcDirection::Sent,
            method: method.as_str().to_owned(),
            payload: params.clone(),
        };
        self.inner.store.save_acp_envelope(run_id, &envelope)?;

        let result = client
            .request(method, params, ACP_REQUEST_TIMEOUT)
            .map_err(Error::from)?;

        let response_envelope = RpcEnvelope {
            direction: RpcDirection::Received,
            method: "rpc.result".into(),
            payload: result.clone(),
        };
        self.inner
            .store
            .save_acp_envelope(run_id, &response_envelope)?;
        Ok(result)
    }

    pub(super) fn acp_prompt_request(
        &self,
        run_id: &str,
        client: &AcpClient,
        params: Value,
    ) -> Result<Value> {
        let method = AgentRpcMethod::Prompt;
        let envelope = RpcEnvelope {
            direction: RpcDirection::Sent,
            method: method.as_str().to_owned(),
            payload: params.clone(),
        };
        self.inner.store.save_acp_envelope(run_id, &envelope)?;

        let result = client
            .request_with_activity_timeout(
                method,
                params,
                ACP_PROMPT_IDLE_TIMEOUT,
                ACP_PROMPT_TOTAL_TIMEOUT,
            )
            .map_err(Error::from)?;

        self.inner.store.save_acp_envelope(
            run_id,
            &RpcEnvelope {
                direction: RpcDirection::Received,
                method: "rpc.result".into(),
                payload: result.clone(),
            },
        )?;
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
