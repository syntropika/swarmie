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

/// Public provider-authorization session service installed by this composition.
pub const AUTHORIZATION_SERVICE: &str = providers::oauth::SERVICE;
