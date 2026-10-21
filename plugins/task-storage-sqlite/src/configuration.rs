use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Dedicated durable database file. Its parent directory must already exist.
///
/// Symlink aliases resolve to one ownership lock. Hard-linked database files are refused
/// where link counts are available. Renaming, unlinking or replacing an open database or its
/// lock file is unsupported. Use a local filesystem with working OS locks and SQLite WAL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub database: PathBuf,
}

/// Invalid configuration detected before a Resource opens the database.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigurationError {
    #[error("database must name a nonempty durable SQLite file")]
    EmptyDatabase,
    #[error("in-memory databases and SQLite URI paths are not supported")]
    NonDurableDatabase,
    #[error("outbox limits require positive portable event and byte counts and 1..=64 consumers")]
    OutboxLimits,
}

impl Configuration {
    /// Checks that the supplied path names a durable file rather than a SQLite special name.
    ///
    /// # Errors
    /// Rejects empty paths, SQLite URI syntax and the special in-memory database name.
    pub fn validate(&self) -> Result<(), ConfigurationError> {
        if self.database.as_os_str().is_empty() {
            return Err(ConfigurationError::EmptyDatabase);
        }
        if let Some(path) = self.database.to_str()
            && (path == ":memory:" || path.starts_with("file:"))
        {
            return Err(ConfigurationError::NonDurableDatabase);
        }
        Ok(())
    }
}

/// Hard retained outbox capacity. A full outbox refuses the complete task transaction before
/// any task write; it never deletes pending history. Operators must acknowledge and compact,
/// increase capacity, or explicitly remove an obsolete consumer. No automatic expiry exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskOutboxLimits {
    pub max_events: u64,
    pub max_bytes: u64,
    pub max_consumers: u32,
}

impl Default for TaskOutboxLimits {
    fn default() -> Self {
        Self {
            max_events: 1_000_000,
            max_bytes: 1_073_741_824,
            max_consumers: 64,
        }
    }
}

impl TaskOutboxLimits {
    pub(crate) fn validate(self) -> Result<(), ConfigurationError> {
        if self.max_events == 0
            || self.max_events > swarmie_task_contracts::MAX_PORTABLE_INTEGER
            || self.max_bytes == 0
            || self.max_bytes > swarmie_task_contracts::MAX_PORTABLE_INTEGER
            || !(1..=64).contains(&self.max_consumers)
        {
            return Err(ConfigurationError::OutboxLimits);
        }
        Ok(())
    }
}
