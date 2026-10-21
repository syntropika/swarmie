use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use swarmie_task_contracts::TaskOutboxLimits;

/// Dedicated durable database file. Its parent directory must already exist.
///
/// Symlink aliases resolve to one ownership lock. Hard-linked database files are refused
/// where link counts are available. Renaming, unlinking or replacing an open database or its
/// lock file is unsupported. Use a local filesystem with working OS locks and SQLite WAL.
///
/// Outbox limits are read from JSON when the Host-owned Resource opens. To change them, close
/// the existing Host and reopen the same database with updated configuration; recompilation and
/// data deletion are unnecessary. Lowering capacity below retained usage preserves every event
/// and rejects new task writes until explicit consumption/compaction or a capacity increase.
/// Retained JSON byte limits do not limit physical database pages, indexes, WAL, or filesystem
/// free space. Operators must monitor those physical resources separately.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub database: PathBuf,
    /// Hard retained capacity; omitted JSON fields use defaults. Pending history never expires.
    #[serde(default)]
    pub outbox: TaskOutboxLimits,
    /// Explicit operator adoption of legacy unowned consumers, applied atomically on opening.
    /// Maps existing consumer IDs to installed Facet names. Existing owners are never changed;
    /// missing consumers or conflicting mappings refuse opening without modifying storage.
    #[serde(default)]
    pub outbox_legacy_owners: BTreeMap<String, String>,
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
    #[error("legacy owner adoption requires at most 64 valid consumer IDs and bounded Facet names")]
    OutboxLegacyOwners,
}

impl Configuration {
    /// Checks that the supplied path names a durable file rather than a SQLite special name.
    ///
    /// # Errors
    /// Rejects empty paths, SQLite URI syntax, the special in-memory database name,
    /// invalid outbox capacity limits, and invalid legacy-owner adoption mappings.
    pub fn validate(&self) -> Result<(), ConfigurationError> {
        if self.database.as_os_str().is_empty() {
            return Err(ConfigurationError::EmptyDatabase);
        }
        if let Some(path) = self.database.to_str()
            && (path == ":memory:" || path.starts_with("file:"))
        {
            return Err(ConfigurationError::NonDurableDatabase);
        }
        self.outbox
            .validate()
            .map_err(|_| ConfigurationError::OutboxLimits)?;
        if self.outbox_legacy_owners.len() > 64
            || self.outbox_legacy_owners.iter().any(|(id, owner)| {
                swarmie_task_contracts::validate_identity(id).is_err()
                    || swarmie_task_contracts::validate_task_outbox_owner_facet(owner).is_err()
            })
        {
            return Err(ConfigurationError::OutboxLegacyOwners);
        }
        Ok(())
    }
}
