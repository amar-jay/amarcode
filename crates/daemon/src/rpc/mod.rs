//! TCP JSON-line transport for clients.
//!
//! Intentionally thin: accept connections, parse one JSON object per line,
//! dispatch methods, write results/errors, stream subscription events.
//!
//! Submodules:
//! - [`server`] — bind/accept loop
//! - [`connection`] — per-socket read/write and subscribe mode
//! - [`handler`] — method dispatch into `service` / `store`
//!
//! No SQL and no ACP stdio in this module tree.

pub mod connection;
pub mod handler;
pub mod server;
