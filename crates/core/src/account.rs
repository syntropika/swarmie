use crate::{Error, Result, SessionExecutionOptions, SpawnOptions, ToolPolicy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

/// Connection options for an account query, with no conversational operations.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountQuery {
    pub harness: String,
    pub cwd: PathBuf,
    /// Native connection configuration, including executable and diagnostic overrides.
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub execution: AccountExecutionOptions,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountExecutionOptions {
    #[serde(default, skip_serializing)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub config_home: Option<PathBuf>,
}

impl AccountQuery {
    pub fn new(harness: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            harness: harness.into(),
            cwd: cwd.into(),
            config: Value::Null,
            execution: AccountExecutionOptions::default(),
        }
    }
    /// Supplies the shared process launcher without requesting a runtime session.
    pub fn connection_options(&self) -> SpawnOptions {
        SpawnOptions {
            project_id: None,
            project_revision: None,
            harness: self.harness.clone(),
            cwd: self.cwd.clone(),
            model: None,
            config: self.config.clone(),
            execution: SessionExecutionOptions {
                env: self.execution.env.clone(),
                config_home: self.execution.config_home.clone(),
                tools: ToolPolicy::Disabled,
                ephemeral: true,
                ..Default::default()
            },
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if self.harness.trim().is_empty() {
            return Err(Error::Invalid("harness must not be empty".into()));
        }
        if !self.cwd.is_dir() {
            return Err(Error::Invalid(
                "account query cwd must be a directory".into(),
            ));
        }
        if !self.config.is_null() && !self.config.is_object() {
            return Err(Error::Invalid(
                "account query config must be an object".into(),
            ));
        }
        Ok(())
    }
}

/// Aggregate queries retain unsupported and failed providers independently.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AccountQuotaResult {
    Available { harness: String, quotas: Value },
    Unsupported { harness: String, message: String },
    Error { harness: String, message: String },
}
