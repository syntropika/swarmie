//! Explicit MCP connections, independent of PluginManager and Agent Loop.
//!
//! The embedding host supplies a running Tokio executor and I/O permissions.
//! [`orb`] ties the connection and catalog leases to Tembra's physical lifetime.

mod client;
mod configuration;
mod discovery;
mod error;
mod mounted;
mod orb;
mod projection;
pub mod settings;
mod transport;

pub use client::{Connection, ConnectionStatus, Observation};
pub use configuration::{Configuration, Endpoint, Limits, Protocol, ServerId};
pub use error::Reason;
pub use orb::{McpMethod, SERVICE, ServerBinding, orb};

pub(crate) use error::{invalid, unavailable};
