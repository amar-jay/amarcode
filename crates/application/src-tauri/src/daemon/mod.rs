mod bridge;
mod client;
pub(crate) mod connection_lifecycle;
mod manager;
pub mod ssh;

pub use bridge::EventSubscription;
pub use client::DaemonClient;
pub use manager::{
    DaemonBootstrapStatus, DaemonManager, DaemonRemovalResult, DaemonRemovalStatus,
    DaemonUpdateCheck, DaemonUpdateStatus,
};

pub mod remote;
