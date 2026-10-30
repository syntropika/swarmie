//! Reusable client-facing node services over the existing Swarmie capabilities.
//!
//! The embedding supplies a Tembra host, executor, authenticated principal and an exclusively
//! owned profile directory. This crate does not start a host worker, listener or desktop shell.
mod composition;
mod conversations;
mod definitions;
mod directories;
mod providers;

pub use composition::{NodeConfiguration, install};

/// Enable recovery and scheduling after the host has installed the complete node composition.
/// Repeated calls do not retry held tasks. A new host recovers admitted tasks from
/// their confirmed checkpoints; local interruption holds do not survive a restart.
pub async fn start(host: &tembra::Host) -> tembra::Result<()> {
    conversations::tasks::start(host).await
}

/// Public provider-authorization session service installed by this composition.
pub const AUTHORIZATION_SERVICE: &str = providers::oauth::SERVICE;
