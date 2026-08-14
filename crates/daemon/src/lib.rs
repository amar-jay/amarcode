//! Amarcode daemon library.
//!
//! Binary entrypoint is `main.rs`. Everything else is organized here so the
//! daemon logic can be unit-tested without spinning up the process.

pub mod acp;
pub mod app;
pub mod app_dir;
pub mod cleanup;
pub mod config;
pub mod error;
mod instance_lock;
pub mod logging;
pub mod protocol;
pub mod rpc;
pub mod service;
pub mod service_control;
pub mod store;

pub use app::App;
pub use config::Config;
pub use error::{Error, Result};
