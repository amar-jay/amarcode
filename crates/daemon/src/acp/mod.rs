//! Agent Client Protocol (ACP) subprocess layer.
//!
//! Owns how the daemon talks to an ACP-compatible agent over stdio JSON-RPC.
//! Higher layers (`service`) decide *when* to prompt or cancel; this layer
//! decides *how* bytes move to the child process.
//!
//! Implementation lives in [`client`] (`AcpClient`): process spawn, line
//! transport, and request correlation in one place. Split only if the file
//! grows past what is easy to reason about.

pub mod client;

pub use client::{AcpClient, AcpError, AcpInbound, AcpResult};
