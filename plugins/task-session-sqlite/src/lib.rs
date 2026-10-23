//! Same-owner task decisions and associated history committed in one SQLite transaction.
mod commit;

mod schema;

mod service;

pub use schema::sqlite_schema;

pub use service::{facet_with_database, facets_with_database};

use swarmie_task_contracts::TaskOutboxLimits;

/// Shared local provider configuration. Physical paths belong to the selected database owner.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    #[serde(default)]
    pub outbox: TaskOutboxLimits,
}

/// Compiled package identity.
pub const PLUGIN_ID: &str = "swarmie.task-session-sqlite";

/// Native joint-commit provider identity. It never schedules or executes handlers.
pub const FACET_ID: &str = "swarmie.task-session-sqlite.commit";
