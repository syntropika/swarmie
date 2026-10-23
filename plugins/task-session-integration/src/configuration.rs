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

/// Bridge settings for an explicitly selected shared physical database. The bridge owns
/// routing pins and publication policy; the database owner alone selects its file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedConfiguration {
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
    /// Copies domain settings without the independently owned database path.
    pub fn shared(&self) -> SharedConfiguration {
        SharedConfiguration {
            task_store_id: self.task_store_id.clone(),
            session_store_id: self.session_store_id.clone(),
            consumer_id: self.consumer_id.clone(),
            auto_publish: self.auto_publish,
            poll_interval_ms: self.poll_interval_ms,
        }
    }

    /// Validates durable paths, bounded stable identities and the managed activity interval.
    ///
    /// # Errors
    /// Refuses SQLite memory/URI paths and invalid values before opening files or installing.
    pub fn validate(&self) -> Result<()> {
        let path = self.database.to_string_lossy();
        if path.is_empty() || path == ":memory:" || path.starts_with("file:") {
            return Err(RuntimeError::invalid(
                "binding_configuration",
                "a durable database path is required",
            ));
        }
        self.shared().validate()
    }
}

impl SharedConfiguration {
    /// Validates stable routing identities and the managed publication interval.
    ///
    /// # Errors
    /// Refuses invalid identities and intervals outside 1..=60000ms before installation.
    pub fn validate(&self) -> Result<()> {
        if !(1..=60_000).contains(&self.poll_interval_ms) {
            return Err(RuntimeError::invalid(
                "binding_configuration",
                "a 1..=60000ms poll interval is required",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_settings_need_no_file_and_reject_ambient_database_paths() {
        let value = serde_json::json!({"task_store_id":"tasks","session_store_id":"sessions","consumer_id":"history"});
        let configuration: SharedConfiguration = serde_json::from_value(value.clone()).unwrap();
        configuration.validate().unwrap();
        assert!(configuration.auto_publish);
        assert_eq!(configuration.poll_interval_ms, 100);
        let mut with_path = value;
        with_path["database"] = serde_json::json!("unselected.sqlite");
        assert!(serde_json::from_value::<SharedConfiguration>(with_path).is_err());
        let mut invalid = configuration;
        invalid.poll_interval_ms = 0;
        assert!(invalid.validate().is_err());
    }
}
