use crate::error::{Reason, invalid};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    num::{NonZeroU64, NonZeroUsize},
};
use swarmie_ai::{ChatProfile, Protocol};

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
    Kimi,
}
impl Provider {
    /// Stable provider identity; a model name never selects this route.
    pub const fn id(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::OpenAiResponses => "openai-responses",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::Ollama => "ollama",
            Self::DeepSeek => "deepseek",
            Self::Kimi => "kimi",
        }
    }

    pub(crate) const fn protocol(self) -> Protocol {
        match self {
            Self::OpenAi => Protocol::ChatCompletions(ChatProfile::OpenAi),
            Self::OpenAiResponses => Protocol::Responses,
            Self::Anthropic => Protocol::Anthropic,
            Self::Gemini => Protocol::Gemini,
            Self::Ollama => Protocol::Ollama,
            Self::DeepSeek => Protocol::ChatCompletions(ChatProfile::DeepSeek),
            Self::Kimi => Protocol::ChatCompletions(ChatProfile::Kimi),
        }
    }

    fn default_endpoint(self) -> &'static str {
        match self {
            Self::OpenAi | Self::OpenAiResponses => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com",
            Self::Gemini => "https://generativelanguage.googleapis.com",
            Self::Ollama => "http://localhost:11434",
            Self::DeepSeek => "https://api.deepseek.com",
            Self::Kimi => "https://api.moonshot.ai/v1",
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
    /// Completion deadline, streaming header wait, and each streaming idle read.
    /// Streaming responses have no total deadline while bytes keep arriving.
    /// After cancellation, this also limits the physical settlement wait.
    pub timeout_ms: NonZeroU64,
    /// Limits JSON request payloads before protocol encoding.
    pub request_bytes: NonZeroUsize,
    /// Limits normalized JSON responses after upstream decoding, not HTTP buffering.
    pub response_bytes: NonZeroUsize,
    /// Minimum interval between intermediate previews. First and settled text flush immediately.
    pub progress_interval_ms: NonZeroU64,
}
impl Connection {
    /// Exact non-secret catalog route, using this adapter's actual default endpoint
    /// when no explicit endpoint is configured. Trailing slashes follow the same
    /// normalization as the provider client. Connection identity distinguishes deployments.
    pub fn model_identity(
        &self,
        model: &ModelId,
    ) -> tembra::Result<swarmie_model_catalog::ModelIdentity> {
        self.validate()?;
        if !self.models.contains(model) {
            return Err(invalid(
                Reason::Configuration,
                "model is not admitted by this connection",
            ));
        }
        let base = self
            .endpoint
            .as_deref()
            .unwrap_or_else(|| self.provider.default_endpoint())
            .trim_end_matches('/');
        let endpoint = match self.provider {
            Provider::OpenAi | Provider::DeepSeek | Provider::Kimi => {
                format!("{base}/chat/completions")
            }
            Provider::OpenAiResponses => format!("{base}/responses"),
            Provider::Anthropic => format!("{}/v1/messages", base.trim_end_matches("/v1")),
            Provider::Gemini => format!(
                "{}/v1beta/models/{}",
                base.trim_end_matches("/v1beta"),
                model.as_str()
            ),
            Provider::Ollama => format!("{base}/api/chat"),
        };
        let identity = swarmie_model_catalog::ModelIdentity {
            provider: self.provider.id().into(),
            adapter: self.provider.protocol().adapter().into(),
            endpoint,
            deployment: Some(self.id.as_str().into()),
            model: model.as_str().into(),
            revision: None,
        };
        identity
            .validate()
            .map_err(|_| invalid(Reason::Configuration, "invalid operational model identity"))?;
        Ok(identity)
    }

    /// Content identity for configuration and explicit reasoning, without credential
    /// values. Credential lookup keys are configuration; resolved secrets are not read.
    pub fn configuration_revision(
        &self,
        model: &ModelId,
        effort: Option<&str>,
    ) -> tembra::Result<String> {
        self.model_identity(model)?;
        let effort = effort.map(reasoning).transpose()?;
        let encoded = serde_json::to_vec(&(self, model, effort)).map_err(|_| {
            invalid(
                Reason::Configuration,
                "model configuration could not be encoded",
            )
        })?;
        Ok(format!("{:x}", Sha256::digest(encoded)))
    }

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
            request_bytes: NonZeroUsize::new(16 * 1024 * 1024).unwrap(),
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

/// Native credential custody resolves only at final physical attempt admission.
///
/// Context cancellation or explicit Stop may drop observation of this future.
/// Independently started I/O must retain its physical owner and secret cleanup
/// until settlement; a dropped observer proves neither cancellation nor completion.
/// Authorization, renewal and mutating credential workflows retain their own owners
/// outside this custody call. Cancelling a model wait grants no replay authority.
pub trait Credentials {
    fn acquire<'a>(
        &'a self,
        context: &'a tembra::Context,
        admitted: &'a crate::model::AdmittedAttempt,
        key: &'a str,
    ) -> futures::future::LocalBoxFuture<'a, tembra::Result<crate::model::ModelCredential>>;
}

pub struct EnvironmentCredentials;
impl Credentials for EnvironmentCredentials {
    fn acquire<'a>(
        &'a self,
        context: &'a tembra::Context,
        _admitted: &'a crate::model::AdmittedAttempt,
        key: &'a str,
    ) -> futures::future::LocalBoxFuture<'a, tembra::Result<crate::model::ModelCredential>> {
        Box::pin(async move {
            context.check()?;
            let value = std::env::var(key).map_err(|_| {
                tembra::Error::new(
                    tembra::Code::Unavailable,
                    "swarmie.models.credentials",
                    "model credential is unavailable",
                )
            })?;
            crate::model::ModelCredential::new(value, "[secret]".into())
        })
    }
}

pub(crate) fn reasoning(value: &str) -> tembra::Result<swarmie_ai::codecs::completion::Reasoning> {
    use swarmie_ai::codecs::completion::{Effort, Reasoning};
    let effort = match value.to_ascii_lowercase().as_str() {
        "none" | "off" | "zero" => return Ok(Reasoning::Off),
        "minimal" => Effort::Minimal,
        "low" => Effort::Low,
        "medium" => Effort::Medium,
        "high" => Effort::High,
        "xhigh" => Effort::XHigh,
        "max" => Effort::Max,
        _ => {
            return Err(invalid(
                Reason::Configuration,
                "unsupported reasoning effort",
            ));
        }
    };
    Ok(Reasoning::Effort(effort))
}
