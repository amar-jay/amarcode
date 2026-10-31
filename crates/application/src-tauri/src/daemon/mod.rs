mod bridge;
mod client;
pub(crate) mod connection_lifecycle;
mod manager;
pub mod ssh;

pub use bridge::EventSubscription;
pub use client::DaemonClient;
pub use manager::{
    ApplicationCleanupResult, ApplicationCleanupStatus, DaemonBootstrapStatus, DaemonManager,
    DaemonUpdateCheck, DaemonUpdateStatus,
};

pub mod remote;
