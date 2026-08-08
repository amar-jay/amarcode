//! Shared error type for the daemon crate.
//!
//! Prefer returning `Result<T>` from library boundaries (store, service, rpc,
//! acp). Map I/O, SQLite, and protocol failures into `Error` so callers do not
//! depend on concrete third-party error types.
//!
//! RPC handlers turn `Error` into `{ "error": "..." }` lines for clients.

use std::fmt;

/// Daemon error.
#[derive(Debug)]
pub struct Error {
    message: String,
}

/// Convenience alias used across the crate.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn msg(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::msg(value.to_string())
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Self::msg(value)
    }
}

impl From<&str> for Error {
    fn from(value: &str) -> Self {
        Self::msg(value)
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::msg(value.to_string())
    }
}

impl From<crate::acp::AcpError> for Error {
    fn from(value: crate::acp::AcpError) -> Self {
        Self::msg(value.to_string())
    }
}
