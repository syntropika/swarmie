//! Durable scheduling through ordinary Tembra application Facets and transactional TaskStorage.
//!
//! Every handler runs through a lifecycle-owned Registry operation. Committed records are
//! authoritative; phase futures and cancellation tokens are ephemeral execution observations.
//! External effects may repeat after an interrupted phase and require stable operation identities.

mod cancellation;
mod configuration;
mod execution;
mod mutation;
mod observation;
mod ownership;
mod reads;
mod recovery;
mod registry;
mod scheduler;
mod service;
mod work;

pub use configuration::Configuration;
pub use service::facet;

/// Logical identity of the independently composable compiled scheduler plugin.
pub const PLUGIN_ID: &str = "swarmie.durable-tasks";

/// Executable identity of the compiled scheduler Facet.
pub const FACET_ID: &str = "swarmie.durable-tasks.scheduler";

/// Version of this compiled plugin package.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
