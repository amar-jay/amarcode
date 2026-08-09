//! Stable wire protocol between the daemon and clients (desktop app, CLI).
//!
//! Submodules:
//! - [`rpc`] — request/response envelopes and method names
//! - [`events`] — payloads streamed after `subscribe_events`
//! - [`types`] — shared domain enums and DTOs (status, roles, ACP methods)
//!
//! Clients depend on this surface. Keep it versioned and boring.
//! Raw ACP JSON-RPC for agent subprocesses lives under `acp`, not here.

pub mod events;
pub mod rpc;
pub mod types;

pub use events::{EditorEvent, EventLine};
pub use types::{
    AgentEventMethod, AgentRpcMethod, MessagePartKind, MessageRole, MessageStatus, RpcDirection,
    RpcEnvelope, RunStatus, TurnStatus,
};
