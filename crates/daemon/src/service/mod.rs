//! Service layer: product use-cases over store + ACP.
//!
//! This is where the daemon is "heavy". RPC handlers call managers here;
//! managers call `store` and `acp`.
//!
//! Submodules:
//! - [`agent_manager`] — agent definitions and binary resolution
//! - [`chat_manager`] — chat CRUD and workspace-scoped listing
//! - [`session`] — live runs, prompts, ACP sessions, event fan-out
//!
//! ## Orchestration contract
//!
//! `store` and `acp` do not call each other. **Service** is the join point.
//!
//! When a client RPC involves an agent, or when the agent pushes inbound
//! traffic, durable product state is written to the store **before** any
//! live `EditorEvent` or RPC `result` that depends on that state.
//! See [`session`] and the crate README ("Store-first write path").

pub mod agent_manager;
pub mod chat_manager;
pub mod session;

pub use agent_manager::{AgentManager, ResolvedAgent};
pub use chat_manager::{ChatDetail, ChatManager, MessageDetail};
pub use session::{PendingAgentRequest, PromptResult, SessionManager};
