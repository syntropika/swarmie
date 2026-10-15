use crate::{Error, Result, SpawnOptions};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

/// Private launch material. Deliberately has no serialization or debug implementation.
#[derive(Clone)]
pub struct AgentToolExecutionOverlay {
    pub env: BTreeMap<String, String>,
    pub config_home: Option<PathBuf>,
    pub config: Value,
}
impl Default for AgentToolExecutionOverlay {
    fn default() -> Self {
        Self {
            env: BTreeMap::new(),
            config_home: None,
            config: json!({}),
        }
    }
}
impl AgentToolExecutionOverlay {
    pub fn apply(self, options: &mut SpawnOptions) -> Result<()> {
        for (name, value) in self.env {
            if options
                .execution
                .env
                .get(&name)
                .is_some_and(|explicit| explicit != &value)
            {
                return Err(Error::Invalid(
                    "Execution credentials conflict with the configured agent tool connection"
                        .into(),
                ));
            }
            options.execution.env.insert(name, value);
        }
        if let Some(home) = self.config_home {
            if options
                .execution
                .config_home
                .as_ref()
                .is_some_and(|explicit| explicit != &home)
            {
                return Err(Error::Invalid(
                    "Execution configuration conflicts with the configured agent tool connection"
                        .into(),
                ));
            }
            options.execution.config_home = Some(home);
        }
        if let Some(overlay) = self.config.as_object().filter(|config| !config.is_empty()) {
            if options.config.is_null() {
                options.config = json!({});
            }
            let config = options.config.as_object_mut().ok_or_else(|| {
                Error::Invalid("Agent tool configuration must be an object".into())
            })?;
            for (name, value) in overlay {
                if let Some(explicit) = config.get_mut(name) {
                    merge_configuration(explicit, value)?;
                } else {
                    config.insert(name.clone(), value.clone());
                }
            }
        }
        Ok(())
    }
}
fn merge_configuration(explicit: &mut Value, managed: &Value) -> Result<()> {
    if let (Some(explicit), Some(managed)) = (explicit.as_object_mut(), managed.as_object()) {
        for (name, value) in managed {
            if let Some(current) = explicit.get_mut(name) {
                merge_configuration(current, value)?;
            } else {
                explicit.insert(name.clone(), value.clone());
            }
        }
        return Ok(());
    }
    if explicit != managed {
        return Err(Error::Invalid(
            "Execution configuration conflicts with the configured agent tool connection".into(),
        ));
    }
    Ok(())
}

/// Runtime-owned connection lifecycle. Public results must contain no credentials.
#[async_trait]
pub trait AgentToolSetupPort: Send + Sync {
    async fn descriptor(&self, harness: &str) -> Result<Value>;
    async fn start(&self, params: Value) -> Result<Value>;
    async fn flow(&self, id: &str) -> Result<Value>;
    async fn input(&self, id: &str, values: Value) -> Result<Value>;
    async fn cancel(&self, id: &str) -> Result<Value>;
    async fn disconnect(&self, harness: &str) -> Result<Value>;
    async fn install(&self, _harness: &str) -> Result<Value> {
        Err(Error::Unsupported(
            "Managed installation for this agent tool".into(),
        ))
    }
    async fn execution(&self, harness: &str) -> Result<AgentToolExecutionOverlay>;
    /// Read-only history receives managed support paths without provider credentials.
    async fn history_overlay(&self, _harness: &str) -> Result<AgentToolExecutionOverlay> {
        Ok(AgentToolExecutionOverlay::default())
    }
    async fn shutdown(&self) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launch_configuration_preserves_independent_explicit_settings_and_rejects_conflicts() {
        let overlay = AgentToolExecutionOverlay {
            config: json!({"config":{"model_provider":"managed"}}),
            ..Default::default()
        };
        let mut options = SpawnOptions::new("codex", PathBuf::from("."));
        options.config = json!({"config":{"feature":true}});
        overlay.clone().apply(&mut options).unwrap();
        assert_eq!(
            options.config,
            json!({"config":{"feature":true,"model_provider":"managed"}})
        );
        options.config = json!({"config":{"model_provider":"other"}});
        assert!(overlay.apply(&mut options).is_err());
    }
}
