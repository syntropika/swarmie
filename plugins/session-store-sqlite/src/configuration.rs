use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tembra::{Result, RuntimeError};

/// Dedicated local durable file with WAL and OS locking. Its parent must exist. Renaming,
/// unlinking, hard linking or replacing the database/lock while open is unsupported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub database: PathBuf,
}

impl Configuration {
    /// Validates the file identity without opening it.
    ///
    /// # Errors
    /// Refuses empty paths, SQLite URI paths and in-memory databases.
    pub fn validate(&self) -> Result<()> {
        if self.database.as_os_str().is_empty()
            || self
                .database
                .to_str()
                .is_some_and(|p| p == ":memory:" || p.starts_with("file:"))
        {
            return Err(RuntimeError::invalid(
                "session_store_configuration",
                "a durable local SQLite path is required",
            ));
        }
        Ok(())
    }
}
