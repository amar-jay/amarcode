//! Shared editor wire contract plus daemon-private ACP vocabulary.

mod acp_types;

pub use acp_types::{AgentEventMethod, AgentRpcMethod, RpcDirection, RpcEnvelope};
pub use amarcode_protocol::*;
