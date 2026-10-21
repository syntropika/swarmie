//! Compiled SessionStore provider. SQLite commits are authoritative; Tembra owns Resources,
//! References and reactive publication. No event bus, agent loop or model provider runs here.

mod configuration;
mod database;
mod publication;
mod reads;
mod schema;
mod service;
mod writes;

pub use configuration::Configuration;
pub use service::facet;

/// Compiled package identity.
pub const PLUGIN_ID: &str = "swarmie.session-store-sqlite";

/// Native Facet identity.
pub const FACET_ID: &str = "swarmie.session-store-sqlite.provider";

/// Compiled package version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
