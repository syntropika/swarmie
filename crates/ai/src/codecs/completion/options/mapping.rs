use serde_json::Value;

use super::{CacheRetention, Reasoning, ServiceTier, Verbosity};
use crate::codecs::completion::CompletionRequest;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OptionFields<'a> {
    pub reasoning: Option<&'a Reasoning>,
    pub cache: Option<&'a CacheRetention>,
    pub service_tier: Option<&'a ServiceTier>,
    pub verbosity: Option<&'a Verbosity>,
    pub parallel_tool_calls: Option<bool>,
    pub top_p: Option<f64>,
    pub seed: Option<u64>,
    pub stop: &'a [String],
    pub cache_key: Option<&'a str>,
}

impl OptionFields<'_> {
    pub(crate) fn is_set(&self, option: OptionName) -> bool {
        match option {
            OptionName::Reasoning => self.reasoning.is_some(),
            OptionName::Cache => self.cache.is_some(),
            OptionName::ServiceTier => self.service_tier.is_some(),
            OptionName::Verbosity => self.verbosity.is_some(),
            OptionName::ParallelToolCalls => self.parallel_tool_calls.is_some(),
            OptionName::TopP => self.top_p.is_some(),
            OptionName::Seed => self.seed.is_some(),
            OptionName::Stop => !self.stop.is_empty(),
            OptionName::CacheKey => self.cache_key.is_some(),
            OptionName::Temperature => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OptionName {
    Reasoning,
    Cache,
    ServiceTier,
    Verbosity,
    ParallelToolCalls,
    TopP,
    Seed,
    Stop,
    CacheKey,
    Temperature,
}

impl OptionName {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Reasoning => "reasoning",
            Self::Cache => "cache",
            Self::ServiceTier => "service_tier",
            Self::Verbosity => "verbosity",
            Self::ParallelToolCalls => "parallel_tool_calls",
            Self::TopP => "top_p",
            Self::Seed => "seed",
            Self::Stop => "stop",
            Self::CacheKey => "cache_key",
            Self::Temperature => "temperature",
        }
    }

    pub(crate) fn clear(self, request: &mut CompletionRequest) {
        let options = &mut request.options;
        match self {
            Self::Reasoning => options.reasoning = None,
            Self::Cache => options.cache = None,
            Self::ServiceTier => options.service_tier = None,
            Self::Verbosity => options.verbosity = None,
            Self::ParallelToolCalls => options.parallel_tool_calls = None,
            Self::TopP => options.top_p = None,
            Self::Seed => options.seed = None,
            Self::Stop => options.stop.clear(),
            Self::CacheKey => options.cache_key = None,
            Self::Temperature => request.temperature = None,
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum Mapping {
    Nothing,
    Send(Value),
    Omit(&'static str),
    Place,
    Unsupported(String),
}

impl Mapping {
    pub fn of<T>(value: Option<T>, map: impl FnOnce(T) -> Mapping) -> Mapping {
        value.map_or(Mapping::Nothing, map)
    }

    pub fn of_stop(stop: &[String], map: impl FnOnce(&[String]) -> Mapping) -> Mapping {
        if stop.is_empty() {
            Mapping::Nothing
        } else {
            map(stop)
        }
    }

    pub fn unsupported(reason: impl Into<String>) -> Mapping {
        Mapping::Unsupported(reason.into())
    }

    pub fn unrouted(cache_key: Option<&str>) -> Mapping {
        Mapping::of(cache_key, |_| {
            Mapping::Omit("the provider routes its prompt cache without a key")
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OptionMap {
    pub reasoning: Mapping,
    pub cache: Mapping,
    pub service_tier: Mapping,
    pub verbosity: Mapping,
    pub parallel_tool_calls: Mapping,
    pub top_p: Mapping,
    pub seed: Mapping,
    pub stop: Mapping,
    pub cache_key: Mapping,
}

impl OptionMap {
    pub(super) fn into_slots(self) -> [(OptionName, Mapping); 9] {
        let OptionMap {
            reasoning,
            cache,
            service_tier,
            verbosity,
            parallel_tool_calls,
            top_p,
            seed,
            stop,
            cache_key,
        } = self;
        [
            (OptionName::Reasoning, reasoning),
            (OptionName::Cache, cache),
            (OptionName::ServiceTier, service_tier),
            (OptionName::Verbosity, verbosity),
            (OptionName::ParallelToolCalls, parallel_tool_calls),
            (OptionName::TopP, top_p),
            (OptionName::Seed, seed),
            (OptionName::Stop, stop),
            (OptionName::CacheKey, cache_key),
        ]
    }
}
