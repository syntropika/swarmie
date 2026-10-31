use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use crate::codecs::completion::options::{CatalogRefusal, OptionName};
use crate::codecs::completion::{
    CacheRetention, CompletionRequest, Cost, Effort, GenerationOptions, Reasoning,
    UnsupportedOption, Usage,
};
use crate::codecs::providers::registry::{Format, ProviderId};

use super::details::ModelDetails;

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelSpec {
    pub id: String,
    pub provider: ProviderId,
    pub display_name: String,
    pub context_window: Option<u32>,
    #[serde(default)]
    pub max_input_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub input: Modalities,
    pub reasoning: ReasoningSupport,
    pub caching: CacheSupport,
    pub tools: bool,
    pub structured_output: bool,
    pub pricing: Option<Pricing>,
    pub deprecated: bool,
    pub sampling: Option<Sampling>,
    pub compat: Compat,
    #[serde(default)]
    pub details: ModelDetails,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Modalities {
    pub text: bool,
    pub image: bool,
    pub audio: bool,
    pub video: bool,
    pub pdf: bool,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningSupport {
    #[default]
    None,
    Unknown {
        default: Option<Effort>,
    },
    Listed {
        levels: Vec<Effort>,
        budget: Option<RangeInclusive<u32>>,
        can_disable: bool,
        default: Option<Effort>,
    },
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheSupport {
    pub retention: Vec<CacheRetention>,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pricing {
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

impl Pricing {
    pub fn new(input: f64, output: f64) -> Self {
        Self {
            input,
            output,
            cache_read: None,
            cache_write: None,
        }
    }

    pub fn with_cache_read(mut self, price: f64) -> Self {
        self.cache_read = Some(price);
        self
    }

    pub fn with_cache_write(mut self, price: f64) -> Self {
        self.cache_write = Some(price);
        self
    }

    pub fn cost(&self, usage: &Usage) -> Option<Cost> {
        let written = usage.cache_creation_input_tokens.unwrap_or(0);
        let write_cost = match written {
            0 => Some(0.0),
            tokens => self
                .cache_write
                .map(|rate| tokens as f64 * rate / 1_000_000.0),
        };
        self.cost_with_cache_write(usage, write_cost)
    }

    pub(crate) fn cost_with_cache_write(
        &self,
        usage: &Usage,
        write_cost: Option<f64>,
    ) -> Option<Cost> {
        let input = usage.input_tokens?;
        let output = usage.output_tokens?;
        let read = usage.cached_input_tokens.unwrap_or(0);
        let written = usage.cache_creation_input_tokens.unwrap_or(0);
        let uncached = input.saturating_sub(read).saturating_sub(written);
        // Token counts stay far below 2^53, so the conversion is exact.
        let price = |tokens: u64, per_million: f64| tokens as f64 * per_million / 1_000_000.0;
        // No tokens cost nothing, whether or not the rate is listed.
        let cache = |tokens: u64, rate: Option<f64>| match (tokens, rate) {
            (0, _) => Some(0.0),
            (tokens, rate) => rate.map(|rate| price(tokens, rate)),
        };
        Some(Cost::priced(
            price(uncached, self.input),
            price(output, self.output),
            cache(read, self.cache_read),
            write_cost,
        ))
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sampling {
    Any,
    ReasoningOff,
    Never,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compat {
    pub reasoning_field: Option<String>,
    pub adaptive_thinking: bool,
    pub thinking_off: Option<String>,
    pub mid_conversation_system: bool,
    pub rejects_forced_tool_choice: bool,
    pub binds_context: bool,
    pub prompt_cache_options: bool,
    pub chat_tools_need_reasoning_off: bool,
    pub prefill: bool,
}

impl Compat {
    pub fn with_reasoning_field(mut self, field: impl Into<String>) -> Self {
        self.reasoning_field = Some(field.into());
        self
    }

    pub fn with_prefill(mut self, on: bool) -> Self {
        self.prefill = on;
        self
    }
}

impl CacheSupport {
    pub fn new(retention: impl IntoIterator<Item = CacheRetention>) -> Self {
        Self {
            retention: retention.into_iter().collect(),
        }
    }
}

impl ModelSpec {
    pub fn new(provider: ProviderId, id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            display_name: id.clone(),
            id,
            provider,
            context_window: None,
            max_input_tokens: None,
            max_output_tokens: None,
            input: Modalities {
                text: true,
                ..Modalities::default()
            },
            reasoning: ReasoningSupport::default(),
            caching: CacheSupport::default(),
            tools: false,
            structured_output: false,
            pricing: None,
            deprecated: false,
            sampling: None,
            compat: Compat::default(),
            details: ModelDetails::default(),
        }
    }

    pub fn format(&self) -> Option<Format> {
        self.provider.format()
    }

    pub fn reference(&self) -> String {
        format!("{}/{}", self.provider.vendor(), self.id)
    }

    pub fn default_options(&self, reasoning: Option<Reasoning>) -> GenerationOptions {
        let options = GenerationOptions {
            reasoning,
            ..GenerationOptions::default()
        };
        if self.caching.retention.contains(&CacheRetention::Short) {
            options.cache(CacheRetention::Short)
        } else {
            options
        }
    }

    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = name.into();
        self
    }

    pub fn with_context_window(mut self, tokens: u32) -> Self {
        self.context_window = Some(tokens);
        self
    }

    pub fn with_max_input_tokens(mut self, tokens: u32) -> Self {
        self.max_input_tokens = Some(tokens);
        self
    }

    pub fn input_limit(&self) -> Option<u32> {
        match (self.max_input_tokens, self.context_window) {
            (Some(input), Some(window)) => Some(input.min(window)),
            (input, window) => input.or(window),
        }
    }

    pub fn with_max_output_tokens(mut self, tokens: u32) -> Self {
        self.max_output_tokens = Some(tokens);
        self
    }

    pub fn with_reasoning(mut self, reasoning: ReasoningSupport) -> Self {
        self.reasoning = reasoning;
        self
    }

    pub fn with_caching(mut self, caching: CacheSupport) -> Self {
        self.caching = caching;
        self
    }

    pub fn with_tools(mut self, tools: bool) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_pricing(mut self, pricing: Pricing) -> Self {
        self.pricing = Some(pricing);
        self
    }

    pub fn with_sampling(mut self, sampling: Sampling) -> Self {
        self.sampling = Some(sampling);
        self
    }

    pub fn with_compat(mut self, compat: Compat) -> Self {
        self.compat = compat;
        self
    }

    pub fn with_details(mut self, details: ModelDetails) -> Self {
        self.details = details;
        self
    }

    pub fn validate(&self, options: &GenerationOptions) -> Result<(), UnsupportedOption> {
        match self.rules(options, None).into_iter().next() {
            Some(refusal) => Err(self.unsupported(refusal)),
            None => Ok(()),
        }
    }

    pub fn refusals(&self, request: &CompletionRequest) -> Vec<UnsupportedOption> {
        self.request_rules(request)
            .into_iter()
            .map(|refusal| self.unsupported(refusal))
            .collect()
    }

    pub fn fit(&self, request: &mut CompletionRequest) -> Vec<UnsupportedOption> {
        let mut dropped = Vec::new();
        // Only a set option is refused, and each pass unsets one, so this ends.
        while let Some(refusal) = self.request_rules(request).into_iter().next() {
            refusal.option.clear(request);
            dropped.push(self.unsupported(refusal));
        }
        dropped
    }

    pub(crate) fn request_rules(&self, request: &CompletionRequest) -> Vec<CatalogRefusal> {
        if request.options.is_default() {
            return Vec::new();
        }
        self.rules(&request.options, request.temperature)
    }

    fn rules(&self, options: &GenerationOptions, temperature: Option<f64>) -> Vec<CatalogRefusal> {
        let mut refusals = Vec::new();
        if let Some(reason) = options
            .reasoning
            .as_ref()
            .and_then(|reasoning| self.reasoning.refusal(reasoning))
        {
            refusals.push(CatalogRefusal {
                option: OptionName::Reasoning,
                reason,
            });
        }
        if let Some(reason) = options
            .cache
            .as_ref()
            .and_then(|cache| self.caching.refusal(cache))
        {
            refusals.push(CatalogRefusal {
                option: OptionName::Cache,
                reason,
            });
        }
        let reasons = self.reasons_with(options.reasoning.as_ref());
        let sampled = [
            (OptionName::Temperature, temperature.is_some()),
            (OptionName::TopP, options.top_p.is_some()),
        ];
        for (option, set) in sampled {
            let refusal = set.then(|| self.sampling_refusal(option.as_str(), reasons));
            if let Some(reason) = refusal.flatten() {
                refusals.push(CatalogRefusal { option, reason });
            }
        }
        refusals
    }

    pub(crate) fn sampling_refusal(&self, field: &str, reasons: bool) -> Option<String> {
        match self.sampling? {
            Sampling::Never => Some(format!("the model takes no `{field}`")),
            Sampling::ReasoningOff if reasons => {
                Some(match self.reasoning.can_disable() == Some(true) {
                    true => format!(
                        "the model rejects `{field}` while it reasons; remove `{field}` or set \
                         `reasoning` to `Off`"
                    ),
                    false => format!("the model rejects `{field}` while it reasons"),
                })
            }
            Sampling::ReasoningOff | Sampling::Any => None,
        }
    }

    pub(crate) fn reasons_with(&self, reasoning: Option<&Reasoning>) -> bool {
        match reasoning {
            Some(Reasoning::Off) => {
                self.reasoning.supported() && self.reasoning.can_disable() == Some(false)
            }
            Some(_) => self.reasoning.supported(),
            None => self.reasons_by_default(),
        }
    }

    pub(crate) fn reasons_by_default(&self) -> bool {
        self.reasoning.supported()
            && (self.reasoning.default_effort().is_some()
                || self.reasoning.can_disable() == Some(false))
    }

    fn unsupported(&self, refusal: CatalogRefusal) -> UnsupportedOption {
        UnsupportedOption::new(
            refusal.option.as_str(),
            self.provider.vendor(),
            &self.id,
            refusal.reason,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReasoningChoice {
    pub name: &'static str,
    pub reasoning: Option<Reasoning>,
}

impl ReasoningChoice {
    pub fn label(&self) -> String {
        match self.reasoning {
            Some(Reasoning::Budget { tokens }) => format!("{} ({tokens} tokens)", self.name),
            _ => self.name.to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the reasoning settings are {}, not `{name}`", choices.join(", "))]
pub struct UnknownReasoning {
    pub name: String,
    pub choices: Vec<&'static str>,
}

const NAMED_BUDGETS: [(Effort, u32); 3] = [
    (Effort::Low, 2048),
    (Effort::Medium, 8192),
    (Effort::High, 16384),
];

impl ReasoningSupport {
    pub fn choices(&self) -> Vec<ReasoningChoice> {
        let mut choices = vec![ReasoningChoice {
            name: "default",
            reasoning: None,
        }];
        let Self::Listed {
            levels,
            budget,
            can_disable,
            ..
        } = self
        else {
            return choices;
        };
        if *can_disable {
            choices.push(ReasoningChoice {
                name: "off",
                reasoning: Some(Reasoning::Off),
            });
        }
        choices.extend(levels.iter().map(|level| ReasoningChoice {
            name: level.as_str(),
            reasoning: Some(Reasoning::Effort(*level)),
        }));
        if levels.is_empty()
            && let Some(range) = budget
        {
            choices.extend(
                NAMED_BUDGETS
                    .iter()
                    .map(|&(effort, tokens)| ReasoningChoice {
                        name: effort.as_str(),
                        // Not `clamp`, which panics on an inverted range.
                        reasoning: Some(Reasoning::Budget {
                            tokens: tokens.max(*range.start()).min(*range.end()),
                        }),
                    }),
            );
        }
        choices
    }

    pub fn named(&self, name: &str) -> Result<ReasoningChoice, UnknownReasoning> {
        let choices = self.choices();
        choices
            .iter()
            .find(|choice| choice.name == name)
            .copied()
            .ok_or_else(|| UnknownReasoning {
                name: name.to_owned(),
                choices: choices.iter().map(|choice| choice.name).collect(),
            })
    }

    pub fn supported(&self) -> bool {
        !matches!(self, Self::None)
    }

    pub fn levels(&self) -> Option<&[Effort]> {
        match self {
            Self::None => Some(&[]),
            Self::Unknown { .. } => None,
            Self::Listed { levels, .. } => Some(levels),
        }
    }

    pub fn budget(&self) -> Option<&RangeInclusive<u32>> {
        match self {
            Self::Listed { budget, .. } => budget.as_ref(),
            _ => None,
        }
    }

    pub fn can_disable(&self) -> Option<bool> {
        match self {
            Self::None => Some(false),
            Self::Unknown { .. } => None,
            Self::Listed { can_disable, .. } => Some(*can_disable),
        }
    }

    pub fn default_effort(&self) -> Option<Effort> {
        match self {
            Self::None => None,
            Self::Unknown { default } | Self::Listed { default, .. } => *default,
        }
    }

    pub fn refusal(&self, reasoning: &Reasoning) -> Option<String> {
        let (levels, budget, can_disable) = match self {
            Self::None => {
                return (!matches!(reasoning, Reasoning::Off))
                    .then(|| "the model does not reason".to_owned());
            }
            Self::Unknown { .. } => return None,
            Self::Listed {
                levels,
                budget,
                can_disable,
                ..
            } => (levels, budget, *can_disable),
        };
        match reasoning {
            Reasoning::Off if can_disable => None,
            Reasoning::Off => Some("reasoning cannot be turned off on this model".to_owned()),
            Reasoning::Effort(effort) if levels.contains(effort) => None,
            Reasoning::Effort(_) if levels.is_empty() && budget.is_some() => {
                Some("the model takes a reasoning budget, not an effort level".to_owned())
            }
            Reasoning::Effort(_) if levels.is_empty() => {
                Some("the model takes no effort level".to_owned())
            }
            Reasoning::Effort(effort) => Some(format!(
                "the model has no `{}` effort level",
                effort.as_str()
            )),
            Reasoning::Budget { tokens } => match budget {
                Some(range) if range.contains(tokens) => None,
                Some(range) => Some(format!(
                    "the model takes a reasoning budget from {} to {} tokens",
                    range.start(),
                    range.end()
                )),
                None if levels.is_empty() => Some("the model takes no reasoning budget".to_owned()),
                None => Some("the model takes an effort level, not a reasoning budget".to_owned()),
            },
        }
    }
}

impl CacheSupport {
    pub fn refusal(&self, cache: &CacheRetention) -> Option<String> {
        (!self.retention.is_empty() && !self.retention.contains(cache)).then(|| {
            format!(
                "the model does not honour `{}` cache retention",
                retention_word(cache)
            )
        })
    }
}

pub(super) fn retention_word(cache: &CacheRetention) -> &'static str {
    match cache {
        CacheRetention::None => "none",
        CacheRetention::Short => "short",
        CacheRetention::Long => "long",
    }
}
