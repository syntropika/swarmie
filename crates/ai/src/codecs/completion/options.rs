use std::borrow::Cow;

use serde::{Deserialize, Serialize};

mod mapping;
mod merge;

pub(crate) use mapping::OptionName;
pub use mapping::{Mapping, OptionFields, OptionMap};

pub use merge::{BaseInput, FinalBody, RawAt, Rewrite, check, param, request_params};
pub(crate) use merge::{BodyRefusal, CatalogRefusal, catalog_refusals};

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GenerationOptions {
    #[serde(default)]
    pub reasoning: Option<Reasoning>,
    #[serde(default)]
    pub cache: Option<CacheRetention>,
    #[serde(default)]
    pub service_tier: Option<ServiceTier>,
    #[serde(default)]
    pub verbosity: Option<Verbosity>,
    #[serde(default)]
    pub parallel_tool_calls: Option<bool>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_unsupported: Option<OnUnsupported>,
}

impl GenerationOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn unsupported_policy(&self) -> OnUnsupported {
        self.on_unsupported.unwrap_or_default()
    }

    pub fn reasoning(mut self, reasoning: impl Into<Reasoning>) -> Self {
        self.reasoning = Some(reasoning.into());
        self
    }

    pub fn cache(mut self, cache: CacheRetention) -> Self {
        self.cache = Some(cache);
        self
    }

    pub fn service_tier(mut self, tier: ServiceTier) -> Self {
        self.service_tier = Some(tier);
        self
    }

    pub fn verbosity(mut self, verbosity: Verbosity) -> Self {
        self.verbosity = Some(verbosity);
        self
    }

    pub fn parallel_tool_calls(mut self, parallel: bool) -> Self {
        self.parallel_tool_calls = Some(parallel);
        self
    }

    pub fn top_p(mut self, top_p: f64) -> Self {
        self.top_p = Some(top_p);
        self
    }

    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    pub fn stop<S: Into<String>>(mut self, stop: impl IntoIterator<Item = S>) -> Self {
        self.stop = stop.into_iter().map(Into::into).collect();
        self
    }

    pub fn cache_key(mut self, key: impl Into<String>) -> Self {
        self.cache_key = Some(key.into());
        self
    }

    pub fn on_unsupported(mut self, policy: OnUnsupported) -> Self {
        self.on_unsupported = Some(policy);
        self
    }

    pub fn fields(&self) -> OptionFields<'_> {
        let Self {
            reasoning,
            cache,
            service_tier,
            verbosity,
            parallel_tool_calls,
            top_p,
            seed,
            stop,
            cache_key,
            on_unsupported: _,
        } = self;
        OptionFields {
            reasoning: reasoning.as_ref(),
            cache: cache.as_ref(),
            service_tier: service_tier.as_ref(),
            verbosity: verbosity.as_ref(),
            parallel_tool_calls: *parallel_tool_calls,
            top_p: *top_p,
            seed: *seed,
            stop,
            cache_key: cache_key.as_deref(),
        }
    }

    pub fn overlay(self, over: &GenerationOptions) -> GenerationOptions {
        let GenerationOptions {
            reasoning,
            cache,
            service_tier,
            verbosity,
            parallel_tool_calls,
            top_p,
            seed,
            stop,
            cache_key,
            on_unsupported,
        } = over;
        GenerationOptions {
            reasoning: reasoning.or(self.reasoning),
            cache: cache.or(self.cache),
            service_tier: service_tier.or(self.service_tier),
            verbosity: verbosity.or(self.verbosity),
            parallel_tool_calls: parallel_tool_calls.or(self.parallel_tool_calls),
            top_p: top_p.or(self.top_p),
            seed: seed.or(self.seed),
            stop: if stop.is_empty() {
                self.stop
            } else {
                stop.clone()
            },
            cache_key: cache_key.clone().or(self.cache_key),
            on_unsupported: on_unsupported.or(self.on_unsupported),
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reasoning {
    Off,
    Effort(Effort),
    Budget { tokens: u32 },
}

impl From<Effort> for Reasoning {
    fn from(effort: Effort) -> Self {
        Self::Effort(effort)
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl Effort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheRetention {
    None,
    Short,
    Long,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceTier {
    Auto,
    Default,
    Flex,
    Priority,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verbosity {
    Low,
    Medium,
    High,
}

impl Verbosity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnUnsupported {
    #[default]
    Error,
    Ignore,
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("`{option}` is not supported by {provider} model `{model}`: {reason}")]
pub struct UnsupportedOption {
    pub option: Cow<'static, str>,
    pub provider: String,
    pub model: String,
    pub reason: String,
}

impl UnsupportedOption {
    pub fn new(
        option: impl Into<Cow<'static, str>>,
        provider: impl Into<String>,
        model: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            option: option.into(),
            provider: provider.into(),
            model: model.into(),
            reason: reason.into(),
        }
    }
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("{}", unsupported_list(.0))]
    Unsupported(Vec<UnsupportedOption>),
    #[error(transparent)]
    Invalid(crate::codecs::error::ProviderError),
}

fn unsupported_list(refusals: &[UnsupportedOption]) -> String {
    refusals
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}
