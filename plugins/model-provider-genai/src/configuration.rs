use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use swarmie_model_contracts::GenerationOptions;
use tembra::{Result, RuntimeError};

/// Deployable connection metadata. credential_env names a private environment entry; inline
/// secrets are deliberately not a configuration field. Multiple connections may use one vendor.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub id: String,
    pub provider: String,
    pub models: Vec<String>,
    pub base_url: Option<String>,
    pub credential_env: Option<String>,
    #[serde(default = "timeout")]
    pub timeout_ms: u64,
    #[serde(default = "parallel")]
    pub max_concurrent: u32,
    #[serde(default)]
    pub defaults: GenerationOptions,
}

fn timeout() -> u64 {
    120_000
}
fn parallel() -> u32 {
    4
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub request_bytes: usize,
    pub response_bytes: usize,
    pub event_bytes: usize,
    pub window_bytes: usize,
    pub window_events: usize,
    pub batch_events: usize,
    pub batch_interval_ms: u64,
    pub max_generations: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            request_bytes: 256 * 1024,
            response_bytes: 512 * 1024,
            event_bytes: 32 * 1024,
            window_bytes: 128 * 1024,
            window_events: 128,
            batch_events: 32,
            batch_interval_ms: 20,
            max_generations: 128,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub limits: Limits,
}

pub(crate) fn invalid() -> RuntimeError {
    RuntimeError::invalid(
        "model_configuration",
        "invalid model provider configuration",
    )
}

impl Configuration {
    pub(crate) fn validate(&self) -> Result<()> {
        let l = &self.limits;
        if self.connections.is_empty()
            || self.connections.len() > 128
            || !(1024..=512 * 1024).contains(&l.request_bytes)
            || !(1024..=1024 * 1024).contains(&l.response_bytes)
            || !(256..=64 * 1024).contains(&l.event_bytes)
            || l.window_bytes < l.event_bytes * 2
            || l.window_bytes > 512 * 1024
            || l.response_bytes.saturating_add(l.window_bytes) > 896 * 1024
            || !(2..=1024).contains(&l.window_events)
            || l.batch_events == 0
            || l.batch_events > l.window_events / 2
            || !(1..=1000).contains(&l.batch_interval_ms)
            || !(1..=1024).contains(&l.max_generations)
        {
            return Err(invalid());
        }
        let mut ids = BTreeSet::new();
        for c in &self.connections {
            if c.id.is_empty()
                || c.id.len() > 128
                || !ids.insert(&c.id)
                || genai::adapter::AdapterKind::from_lower_str(&c.provider).is_none()
                || c.models.is_empty()
                || c.models.len() > 1024
                || c.models
                    .iter()
                    .any(|m| m.is_empty() || m.len() > 256 || m.contains("::"))
                || !(1..=128).contains(&c.max_concurrent)
                || !(1..=3_600_000).contains(&c.timeout_ms)
            {
                return Err(invalid());
            }
            if let Some(url) = &c.base_url {
                let url = reqwest::Url::parse(url).map_err(|_| invalid())?;
                if !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
                {
                    return Err(invalid());
                }
            }
            if c.credential_env.as_ref().is_some_and(|name| {
                name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            }) {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

/// Supervisor-owned credential resolution. Values never pass through a Tembra DTO or State.
/// Implementations must not log credentials or include them in errors.
pub trait Credentials {
    fn resolve(&self, name: &str) -> Option<String>;
}

pub struct EnvironmentCredentials;

impl Credentials for EnvironmentCredentials {
    fn resolve(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}
