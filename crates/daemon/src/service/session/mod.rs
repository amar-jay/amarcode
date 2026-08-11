//! Live session / agent-run coordination.
//!
//! The **only** place that joins ACP I/O, SQLite, and the `EditorEvent` bus.
//! `store` and `acp` stay segmented; this module owns ordering.
//!
//! ## Store-first rule
//!
//! For every meaningful ACP outcome (request result or inbound notification):
//!
//! 1. **Persist** to `store` (run/message/parts/`acp_events`)
//! 2. **Then** publish `EditorEvent` / complete the client RPC result
//!
//! Never fan out or return durable claims that SQLite does not yet contain.
//!
//! ## Layout
//!
//! - [`manager`] — public `SessionManager` API (start/prompt/cancel/respond)
//! - [`inbound`] — ACP inbound worker + notification routing
//! - [`messages`] — streaming assistant messages, parts, run completion
//! - [`types`] — shared live-run state and public result types
//! - [`util`] — pure helpers (timestamps, payload extraction, emit)

mod inbound;
mod manager;
mod messages;
mod session_config;
mod types;
mod util;

pub use manager::SessionManager;
pub use types::{PendingAgentRequest, PromptResult};
