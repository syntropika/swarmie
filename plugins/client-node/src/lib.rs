//! Reusable client-facing node services over the existing Swarmie capabilities.
//!
//! The embedding supplies a Tembra host, executor, authenticated principal and an exclusively
//! owned profile directory. This crate does not start a host worker, listener or desktop shell.
mod account_file;
mod composition;
mod conversations;
mod credential_keys;
mod definitions;
mod directories;
mod execution;
mod provider_io;
mod providers;

pub use account_file::NativeAccountFile;
pub use composition::{NodeConfiguration, orbs};
pub use credential_keys::{NativeCredentialKeys, NativeCredentialKeyspace};
pub use execution::{ConversationExecution, ExecutionOwner, ExecutionScope};
pub use provider_io::{NativeProviderIo, NativeProviderLease};

/// Enable recovery and scheduling after the host has installed the complete node composition.
/// Repeated calls do not retry held tasks. A new host recovers admitted tasks from
/// their confirmed checkpoints; local interruption holds do not survive a restart.
pub async fn start(host: &tembra::Host) -> tembra::Result<()> {
    conversations::tasks::start(host).await
}

/// Public provider-authorization session service installed by this composition.
pub const AUTHORIZATION_SERVICE: &str = providers::oauth::SERVICE;

pub use swarmie_context::environment::Configuration as EnvironmentConfiguration;
