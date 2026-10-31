use serde_json::json;

use crate::codecs::catalog::{ModelFacts, ReasoningSupport};
use crate::codecs::completion::options::{Mapping, OptionFields, OptionMap};
use crate::codecs::completion::{CacheRetention, Effort, Reasoning, ServiceTier};

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Route {
    Rest,
    Vertex,
    Grpc,
}

enum Thinking<'a> {
    Levels {
        levels: &'a [Effort],
        can_disable: bool,
    },
    Budget {
        range: std::ops::RangeInclusive<u32>,
        can_disable: bool,
    },
    None,
    Unknown,
}

fn thinking<'f>(facts: &'f ModelFacts, model: &str) -> Thinking<'f> {
    let model = model.to_ascii_lowercase();
    let model = model.strip_prefix("models/").unwrap_or(&model);
    let lookup = |model: &str| facts.for_model(super::PROVIDER_NAME, model);
    let Some(spec) = lookup(model).or_else(|| lookup(versioned_model(model)?)) else {
        return named_thinking(model);
    };
    match &spec.reasoning {
        ReasoningSupport::Listed {
            levels,
            budget: Some(range),
            can_disable,
            ..
        } if levels.is_empty() => Thinking::Budget {
            range: range.clone(),
            can_disable: *can_disable,
        },
        ReasoningSupport::Listed {
            levels,
            can_disable,
            ..
        } => Thinking::Levels {
            levels,
            can_disable: *can_disable,
        },
        // A thinking row that lists no controls: its family decides, as for
        // a model the catalog does not list.
        ReasoningSupport::Unknown { .. } => named_thinking(model),
        _ => Thinking::None,
    }
}

const LOW_TO_HIGH: &[Effort] = &[Effort::Low, Effort::Medium, Effort::High];
const MINIMAL_TO_HIGH: &[Effort] = &[Effort::Minimal, Effort::Low, Effort::Medium, Effort::High];

fn named_thinking(model: &str) -> Thinking<'static> {
    let levels = |levels| Thinking::Levels {
        levels,
        can_disable: false,
    };
    match model {
        id if id.starts_with("gemini-3.8-flash") || id.starts_with("gemini-3.7-flash") => {
            levels(LOW_TO_HIGH)
        }
        id if id.starts_with("gemini-3.6-flash")
            || id.starts_with("gemini-3.5-flash-lite")
            || id.starts_with("gemini-3.5-flash") =>
        {
            levels(MINIMAL_TO_HIGH)
        }
        id if id.starts_with("gemini-3.1-pro") => levels(LOW_TO_HIGH),
        id if id.starts_with("gemini-3.1-flash-lite")
            || id.starts_with("gemini-3-flash-preview") =>
        {
            levels(MINIMAL_TO_HIGH)
        }
        id if id.starts_with("gemini-3-pro-preview") || id.starts_with("gemini-3-pro") => {
            levels(&[Effort::Low, Effort::High])
        }
        id if id.starts_with("gemini-2.5-flash-lite") => Thinking::Budget {
            range: 512..=24_576,
            can_disable: true,
        },
        id if id.starts_with("gemini-2.5-flash") => Thinking::Budget {
            range: 0..=24_576,
            can_disable: true,
        },
        id if id.starts_with("gemini-2.5-pro") => Thinking::Budget {
            range: 128..=32_768,
            can_disable: false,
        },
        _ => match super::completion::gemini_major(model) {
            Some(major) if major < 2 || model.starts_with("gemini-2.0") => Thinking::None,
            _ => Thinking::Unknown,
        },
    }
}

fn versioned_model(model: &str) -> Option<&str> {
    if let Some((base, revision)) = model.rsplit_once('-')
        && revision.len() == 3
        && revision.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Some(base);
    }
    ["-preview", "-exp"]
        .iter()
        .filter_map(|tag| model.find(tag).map(|at| &model[..at]))
        .find(|base| !base.is_empty())
}

fn config(value: serde_json::Value) -> Mapping {
    Mapping::Send(json!({ "generationConfig": value }))
}

#[doc(hidden)]
pub fn generate_content_options(
    facts: &ModelFacts,
    model: &str,
    route: Route,
    fields: OptionFields<'_>,
) -> OptionMap {
    let thinking = thinking(facts, model);
    OptionMap {
        reasoning: Mapping::of(fields.reasoning, |reasoning| match (reasoning, &thinking) {
            (
                Reasoning::Off,
                Thinking::Budget {
                    can_disable: true, ..
                },
            ) => config(json!({"thinkingConfig": {"thinkingBudget": 0}})),
            (
                Reasoning::Off,
                Thinking::Levels {
                    can_disable: true, ..
                },
            ) => config(json!({"thinkingConfig": {"thinkingLevel": "minimal"}})),
            (Reasoning::Off, Thinking::None) => Mapping::Omit("the model does not think"),
            (Reasoning::Off, _) => Mapping::unsupported("this model cannot turn thinking off"),
            (Reasoning::Effort(Effort::XHigh | Effort::Max), _) => {
                Mapping::unsupported("Gemini's thinking levels stop at `high`")
            }
            (Reasoning::Effort(effort), Thinking::Levels { levels, .. })
                if levels.contains(effort) =>
            {
                config(json!({"thinkingConfig": {"thinkingLevel": effort.as_str()}}))
            }
            (Reasoning::Effort(effort), Thinking::Levels { .. }) => Mapping::unsupported(format!(
                "this model has no `{}` thinking level",
                effort.as_str()
            )),
            (Reasoning::Effort(_), Thinking::Budget { .. }) => Mapping::unsupported(
                "Gemini 2.5 takes a thinking budget; no effort-to-budget table is configured",
            ),
            (Reasoning::Effort(_), Thinking::None) => {
                Mapping::unsupported("the model does not think")
            }
            (Reasoning::Effort(effort), Thinking::Unknown) => {
                config(json!({"thinkingConfig": {"thinkingLevel": effort.as_str()}}))
            }
            (Reasoning::Budget { tokens }, Thinking::Budget { range, .. }) => {
                if range.contains(tokens) {
                    config(json!({"thinkingConfig": {"thinkingBudget": tokens}}))
                } else {
                    Mapping::unsupported(format!(
                        "this model takes a thinking budget from {} to {}",
                        range.start(),
                        range.end()
                    ))
                }
            }
            (Reasoning::Budget { .. }, Thinking::Levels { .. }) => {
                Mapping::unsupported("this model takes a thinking level, not a budget")
            }
            (Reasoning::Budget { .. }, Thinking::None) => {
                Mapping::unsupported("the model does not think")
            }
            (Reasoning::Budget { tokens }, Thinking::Unknown) => {
                config(json!({"thinkingConfig": {"thinkingBudget": tokens}}))
            }
        }),
        cache: Mapping::of(fields.cache, |cache| match cache {
            CacheRetention::Short => Mapping::Omit("Gemini caches prompts implicitly"),
            CacheRetention::None => {
                Mapping::unsupported("implicit caching cannot be turned off per request")
            }
            CacheRetention::Long => Mapping::unsupported(
                "explicit caching is a `cachedContents` resource, not a request field",
            ),
        }),
        service_tier: Mapping::of(fields.service_tier, |tier| match (tier, route) {
            (ServiceTier::Auto, _) => Mapping::Omit("the standard tier is the default"),
            (_, Route::Grpc) => Mapping::unsupported("the gRPC request has no service tier"),
            (ServiceTier::Default, Route::Vertex) => {
                Mapping::Omit("Vertex AI serves standard pay-as-you-go by default")
            }
            (ServiceTier::Flex | ServiceTier::Priority, Route::Vertex) => Mapping::unsupported(
                "Vertex AI selects this tier by request headers its SDK transport cannot set",
            ),
            (ServiceTier::Default, Route::Rest) => {
                Mapping::Send(json!({"serviceTier": "standard"}))
            }
            (ServiceTier::Flex, Route::Rest) => Mapping::Send(json!({"serviceTier": "flex"})),
            (ServiceTier::Priority, Route::Rest) => {
                Mapping::Send(json!({"serviceTier": "priority"}))
            }
        }),
        verbosity: Mapping::of(fields.verbosity, |_| {
            Mapping::unsupported("Gemini has no verbosity setting")
        }),
        parallel_tool_calls: Mapping::of(fields.parallel_tool_calls, |parallel| match parallel {
            true => Mapping::Omit("parallel function calls are the default"),
            false => Mapping::unsupported("Gemini cannot turn parallel function calls off"),
        }),
        top_p: Mapping::of(fields.top_p, |top_p| config(json!({"topP": top_p}))),
        seed: Mapping::of(fields.seed, |seed| match i32::try_from(seed) {
            Ok(seed) => config(json!({"seed": seed})),
            Err(_) => Mapping::unsupported("Gemini's seed is a 32-bit integer"),
        }),
        stop: Mapping::of_stop(fields.stop, |stop| match stop.len() {
            0..=5 => config(json!({"stopSequences": stop})),
            _ => Mapping::unsupported("Gemini takes at most 5 stop sequences"),
        }),
        cache_key: Mapping::unrouted(fields.cache_key),
    }
}
