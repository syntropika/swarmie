use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tembra::{Result, RuntimeError};

/// Explicitly selected durable stores and consumer. Persisted binding database metadata pins
/// this tuple, preventing accidental reuse against a replacement source or destination.
/// Use a separate database and explicit configuration for a different store pair. Closing the
/// Host preserves bindings and the consumer; no automatic compaction or retirement occurs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub database: PathBuf,
    pub task_store_id: String,
    pub session_store_id: String,
    pub consumer_id: String,
    #[serde(default = "enabled")]
    pub auto_publish: bool,
    #[serde(default = "interval")]
    pub poll_interval_ms: u64,
}

fn enabled() -> bool {
    true
}
fn interval() -> u64 {
    100
}

impl Configuration {
    /// Validates durable paths, bounded stable identities and the managed activity interval.
    ///
    /// # Errors
    /// Refuses SQLite memory/URI paths and invalid values before opening files or installing.
    pub fn validate(&self) -> Result<()> {
        let path = self.database.to_string_lossy();
        if path.is_empty()
            || path == ":memory:"
            || path.starts_with("file:")
            || !(1..=60_000).contains(&self.poll_interval_ms)
        {
            return Err(RuntimeError::invalid(
                "binding_configuration",
                "a durable database and 1..=60000ms poll interval are required",
            ));
        }
        for id in [
            &self.task_store_id,
            &self.session_store_id,
            &self.consumer_id,
        ] {
            swarmie_task_contracts::validate_identity(id)?;
        }
        Ok(())
    }
}
