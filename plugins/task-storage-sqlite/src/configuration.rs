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
