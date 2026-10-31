//! Exact message-profile qualifications. Public catalog flags are not sufficient
//! evidence. Each positive binding combines a publisher contract with the tested
//! swarmie-ai request/stream/tool conversion under an exact model and reasoning configuration.

use crate::ai::Provider;
use serde::{Deserialize, Serialize};
use swarmie_ai::catalog::{Capability, Evidence, Layer, UnknownReason};

pub(super) const PROFILE: &str = "swarmie.text-tools.v1";
pub(super) const REVIEWED_AT_MS: u64 = 1_793_389_892_000;

/// A provider's explicit declaration for one model at one discovered endpoint.
/// The adapter supplies its own exact qualification before dispatch. A generic
/// declaration does not qualify model-specific reasoning behavior.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeclaredCapabilities {
    pub tools: Option<bool>,
    pub streaming: Option<bool>,
    pub profile: Option<String>,
    pub evidence: Evidence,
}

fn evidence(source: &str) -> Evidence {
    Evidence {
        source: source.into(),
        revision: "swarmie-ai:v1:swarmie.text-tools.v1:2026-10-30".into(),
        layer: Layer::Adapter,
        observed_at_ms: REVIEWED_AT_MS,
        expires_at_ms: None,
    }
}

fn supported(source: &str) -> Capability<()> {
    Capability::Supported {
        evidence: evidence(source),
        value: (),
    }
}

fn unknown() -> Capability<()> {
    Capability::Unknown {
        reason: UnknownReason::Unqualified,
    }
}

fn declared(value: Option<bool>, facts: &DeclaredCapabilities, compatible: bool) -> Capability<()> {
    let mut proof = facts.evidence.clone();
    proof.source = format!("swarmie-providers;{}", proof.source);
    proof.revision = format!("swarmie-ai:v1:{PROFILE}:{}", proof.revision);
    proof.layer = Layer::Adapter;
    match value {
        Some(false) => Capability::Unsupported {
            evidence: proof,
            reason: "the exact provider/model declaration disables this capability".into(),
        },
        Some(true) if compatible => Capability::Supported {
            evidence: proof,
            value: (),
        },
        _ => unknown(),
    }
}

/// The caller verifies the configured endpoint against its explicit product
/// binding before setting `standard`. Custom routes require their own compatible
/// profile declaration. No prefixes, family inference or model-name routing occur.
pub(super) fn qualify(
    provider: Provider,
    product_provider: &str,
    model: &str,
    effort: &str,
    standard: bool,
    facts: Option<&DeclaredCapabilities>,
) -> Result<(Capability<()>, Capability<()>), &'static str> {
    let mut tools = unknown();
    let mut streaming = unknown();
    if standard {
        match (product_provider, provider, model) {
            // Typed continuation support alone does not qualify this exact
            // model's mandatory reasoning behavior for native account selection.
            ("kimi", Provider::Kimi, "kimi-k3.1-pro" | "kimi-k3.1-pro-highspeed") => {
                return Err(
                    "this exact model and reasoning profile is not qualified for native selection",
                );
            }
            ("openai", Provider::OpenAi, "gpt-6-astra" | "gpt-6.1-sol") => {
                tools = Capability::Unsupported {
                    evidence: evidence(
                        "https://developers.openai.com/api/docs/guides/function-calling",
                    ),
                    reason: "this model requires the Responses API for tool calls".into(),
                };
            }
            ("openai", Provider::OpenAi, "gpt-5" | "gpt-4.1") if effort.is_empty() => {
                let source = if model == "gpt-5" {
                    "https://developers.openai.com/api/docs/models/gpt-5"
                } else {
                    "https://developers.openai.com/api/docs/models/gpt-4.1"
                };
                tools = supported(source);
                streaming = supported(source);
            }
            ("openai", Provider::OpenAi, "gpt-5.6-terra") if matches!(effort, "none" | "zero") => {
                tools = supported("https://developers.openai.com/api/docs/guides/function-calling");
                streaming =
                    supported("https://developers.openai.com/api/docs/guides/function-calling");
            }
            (
                "anthropic",
                Provider::Anthropic,
                "claude-sonnet-4-5" | "claude-sonnet-4-6" | "claude-opus-4-5" | "claude-opus-4-6"
                | "claude-haiku-4-5",
            ) if effort.is_empty() => {
                tools = supported(
                    "https://platform.claude.com/docs/en/agents-and-tools/tool-use/overview",
                );
                streaming =
                    supported("https://platform.claude.com/docs/en/build-with-claude/streaming");
            }
            _ => {}
        }
    }
    if let Some(facts) = facts {
        let compatible = facts.profile.as_deref() == Some(PROFILE) && effort.is_empty();
        // A provider restriction can narrow a reviewed binding. A declaration
        // cannot override an incompatible reviewed adapter/model configuration.
        if facts.tools == Some(false) || matches!(tools, Capability::Unknown { .. }) {
            tools = declared(facts.tools, facts, compatible);
        }
        if facts.streaming == Some(false) || matches!(streaming, Capability::Unknown { .. }) {
            streaming = declared(facts.streaming, facts, compatible);
        }
    }
    Ok((tools, streaming))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewed_bindings_do_not_extend_to_other_models_routes_or_reasoning() {
        let (tools, streaming) =
            qualify(Provider::OpenAi, "openai", "gpt-5", "", true, None).unwrap();
        assert!(matches!(tools, Capability::Supported { .. }));
        assert!(matches!(streaming, Capability::Supported { .. }));
        for (provider, product, model, effort, standard) in [
            (Provider::OpenAi, "openai", "gpt-5-new", "", true),
            (Provider::OpenAi, "openai", "gpt-5", "", false),
            (Provider::OpenAiResponses, "openai", "gpt-5", "", true),
            (Provider::OpenAi, "openai", "gpt-5", "high", true),
        ] {
            assert!(matches!(
                qualify(provider, product, model, effort, standard, None)
                    .unwrap()
                    .0,
                Capability::Unknown { .. }
            ));
        }
        assert!(matches!(
            qualify(Provider::OpenAi, "openai", "gpt-6-astra", "", true, None)
                .unwrap()
                .0,
            Capability::Unsupported { .. }
        ));
        assert!(qualify(Provider::Kimi, "kimi", "kimi-k3.1-pro", "", true, None).is_err());
    }

    #[test]
    fn provider_flags_need_the_compatible_profile_and_cannot_raise_a_refusal() {
        let mut facts = DeclaredCapabilities {
            tools: Some(true),
            streaming: Some(true),
            profile: None,
            evidence: Evidence {
                source: "provider-discovery".into(),
                revision: "fixture".into(),
                layer: Layer::Provider,
                observed_at_ms: 1,
                expires_at_ms: None,
            },
        };
        assert!(matches!(
            qualify(
                Provider::Ollama,
                "ollama",
                "fixture",
                "",
                false,
                Some(&facts)
            )
            .unwrap()
            .0,
            Capability::Unknown { .. }
        ));
        facts.profile = Some(PROFILE.into());
        let (tools, streaming) = qualify(
            Provider::Ollama,
            "ollama",
            "fixture",
            "",
            false,
            Some(&facts),
        )
        .unwrap();
        assert!(matches!(
            tools,
            Capability::Supported {
                evidence: Evidence {
                    layer: Layer::Adapter,
                    ..
                },
                ..
            }
        ));
        assert!(matches!(streaming, Capability::Supported { .. }));
        facts.tools = Some(false);
        assert!(matches!(
            qualify(Provider::OpenAi, "openai", "gpt-5", "", true, Some(&facts))
                .unwrap()
                .0,
            Capability::Unsupported { .. }
        ));
        facts.tools = Some(true);
        assert!(matches!(
            qualify(
                Provider::OpenAi,
                "openai",
                "gpt-6-astra",
                "",
                true,
                Some(&facts)
            )
            .unwrap()
            .0,
            Capability::Unsupported { .. }
        ));
    }
}
