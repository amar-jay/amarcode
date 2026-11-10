//! Amarcode daemon library.
//!
//! Binary entrypoint is `main.rs`. Everything else is organized here so the
//! daemon logic can be unit-tested without spinning up the process.

pub mod acp;
pub mod app;
pub mod cleanup;
#[cfg(target_os = "linux")]
pub mod cli_path;
pub mod config;
pub mod error;
mod instance_lock;
pub mod logging;
pub mod protocol;
pub mod registry;
pub mod remote;
pub mod rpc;
mod security;
pub mod service;
pub mod service_control;
mod shutdown;
pub mod store;
pub mod update;
#[cfg(windows)]
pub mod windows;
mod workload;

pub use app::App;
pub use config::Config;
pub use error::{Error, Result};

pub mod shell;

pub(crate) mod x11;
