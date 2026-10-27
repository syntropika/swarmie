use crate::error::{Reason, invalid};
use genai::adapter::AdapterKind;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    num::{NonZeroU64, NonZeroUsize},
};

macro_rules! name {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> tembra::Result<Self> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > 255
                    || value.chars().any(char::is_control)
                    || value.contains("::")
                {
                    return Err(invalid(
                        Reason::Configuration,
                        "invalid connection or model identifier",
                    ));
                }
                Ok(Self(value))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = tembra::Error;
            fn try_from(value: String) -> tembra::Result<Self> {
                Self::new(value)
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}
name!(ConnectionId);
name!(ModelId);

/// Provider routing is explicit; model names never choose another adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    OpenAi,
    OpenAiResponses,
    Anthropic,
    Gemini,
    Ollama,
    DeepSeek,
}
impl Provider {
    pub(crate) fn adapter(self) -> AdapterKind {
        match self {
            Self::OpenAi => AdapterKind::OpenAI,
            Self::OpenAiResponses => AdapterKind::OpenAIResp,
            Self::Anthropic => AdapterKind::Anthropic,
            Self::Gemini => AdapterKind::Gemini,
            Self::Ollama => AdapterKind::Ollama,
            Self::DeepSeek => AdapterKind::DeepSeek,
        }
    }
}

/// Selects a configured connection/model pair through the ordinary service key.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelKey {
    pub connection: ConnectionId,
    pub model: ModelId,
}
impl ModelKey {
    pub fn segments(&self) -> Vec<String> {
        vec![self.connection.as_str().into(), self.model.as_str().into()]
    }
}

/// Connection metadata contains credential lookup names, never secret values.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub id: ConnectionId,
    pub provider: Provider,
    pub models: Vec<ModelId>,
    pub endpoint: Option<String>,
    pub credential_key: Option<String>,
    pub timeout_ms: NonZeroU64,
    /// Limits JSON request payloads before genai conversion.
    pub request_bytes: NonZeroUsize,
    /// Limits normalized JSON responses after upstream decoding, not HTTP buffering.
    pub response_bytes: NonZeroUsize,
    /// Minimum interval between intermediate previews. First and settled text flush immediately.
    pub progress_interval_ms: NonZeroU64,
}
impl Connection {
    pub fn new(
        id: ConnectionId,
        provider: Provider,
        models: impl IntoIterator<Item = ModelId>,
    ) -> Self {
        Self {
            id,
            provider,
            models: models.into_iter().collect(),
            endpoint: None,
            credential_key: None,
            timeout_ms: NonZeroU64::new(120_000).unwrap(),
            request_bytes: NonZeroUsize::new(256 * 1024).unwrap(),
            response_bytes: NonZeroUsize::new(512 * 1024).unwrap(),
            progress_interval_ms: NonZeroU64::new(100).unwrap(),
        }
    }
    pub(crate) fn validate(&self) -> tembra::Result<()> {
        if self.models.is_empty()
            || self.models.iter().collect::<BTreeSet<_>>().len() != self.models.len()
        {
            return Err(invalid(
                Reason::Configuration,
                "connection must admit distinct model identifiers",
            ));
        }
        if self
            .credential_key
            .as_ref()
            .is_some_and(|k| k.is_empty() || k.chars().any(char::is_control))
        {
            return Err(invalid(
                Reason::Configuration,
                "invalid credential lookup key",
            ));
        }
        if let Some(endpoint) = &self.endpoint {
            let url = reqwest::Url::parse(endpoint)
                .map_err(|_| invalid(Reason::Configuration, "invalid model endpoint"))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(invalid(
                    Reason::Configuration,
                    "endpoint must be an HTTP origin/path without credentials, query or fragment",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub connections: Vec<Connection>,
}
impl Configuration {
    pub(crate) fn validate(&self) -> tembra::Result<()> {
        if self.connections.is_empty()
            || self
                .connections
                .iter()
                .map(|c| &c.id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.connections.len()
        {
            return Err(invalid(
                Reason::Configuration,
                "configure distinct model connections",
            ));
        }
        for c in &self.connections {
            c.validate()?;
        }
        Ok(())
    }
    pub(crate) fn select(&self, key: &[String]) -> tembra::Result<(Connection, ModelId)> {
        let [connection, model] = key else {
            return Err(invalid(
                Reason::Configuration,
                "model key requires connection and model",
            ));
        };
        let c = self
            .connections
            .iter()
            .find(|c| c.id.as_str() == connection)
            .ok_or_else(|| invalid(Reason::Configuration, "connection is not configured"))?;
        let m = c
            .models
            .iter()
            .find(|m| m.as_str() == model)
            .ok_or_else(|| {
                invalid(
                    Reason::Configuration,
                    "model is not admitted by this connection",
                )
            })?;
        Ok((c.clone(), m.clone()))
    }
}

pub trait Credentials {
    fn resolve(&self, key: &str) -> Option<String>;
}

pub struct EnvironmentCredentials;
impl Credentials for EnvironmentCredentials {
    fn resolve(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}
