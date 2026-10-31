//! Native Vault engine, private interactions and protected custody composition.
//! Embeddings supply an authenticated private channel; Tembra owns resource lifetime.
mod cards;
mod configuration;
mod control;

pub use control::{NativeAgent, NativeExecution, NativeVault};

/// Private Vault service, excluded from ordinary agent transports.
pub const SERVICE: &str = control::SERVICE;

/// Identity selected only by an authenticated private embedding.
pub const PRINCIPAL: &str = control::PRINCIPAL;
