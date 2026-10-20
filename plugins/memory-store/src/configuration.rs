use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Selects a dedicated, durable SQLite file. Parent directories must already exist.
///
/// Paths are resolved by the embedding process. No temporary or in-memory fallback is used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub database: PathBuf,
}

/// Configuration refused before declaring or opening a database resource.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigurationError {
    #[error("database must name a nonempty SQLite file path")]
    EmptyDatabase,
    #[error("database must be a durable file path, not an in-memory name or SQLite URI")]
    NonDurableDatabase,
}

impl Configuration {
    /// Checks the path's declared form without accessing the filesystem or mutating a Host.
    ///
    /// # Errors
    /// Rejects an empty path, SQLite's special `:memory:` name, and `file:` URI syntax.
    /// Filesystem access and database compatibility errors are reported at resource startup.
    ///
    /// ```
    /// use swarmie_memory_store::{Configuration, ConfigurationError};
    /// let config = Configuration { database: "application.sqlite".into() };
    /// assert_eq!(config.validate(), Ok(()));
    /// let invalid = Configuration { database: ":memory:".into() };
    /// assert_eq!(invalid.validate(), Err(ConfigurationError::NonDurableDatabase));
    /// ```
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
