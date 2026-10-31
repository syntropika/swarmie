use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::codecs::completion::provider_options::reply_field;
use crate::codecs::completion::{ExtensionOptions, ProviderExtension, ReplyExtras};
use crate::codecs::message::Api;

mod chat;
mod responses;

pub use chat::{
    ApproximateLocation, AudioFormat, AudioOutput, ChatOptions, CompletionTokensDetails, Modality,
    Prediction, PromptTokensDetails, SearchContextSize, UserLocation, WebSearchOptions,
};

pub use responses::{
    AccessPrograms, ContextManagement, CyberAccess, Include, ItemPhase, OpenAiResponsesOptions,
    PromptCacheOptions, ReasoningContext, ReasoningMode, ReasoningOptions, ReasoningSummary,
    Truncation,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenAiExt;

impl ProviderExtension for OpenAiExt {
    const PROVIDER: &'static str = crate::codecs::providers::openai::PROVIDER_NAME;
    type Options = OpenAiOptions;
    type Extras = OpenAiExtras;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct OpenAiOptions {
    #[serde(rename = "*")]
    pub shared: OpenAiShared,
    #[serde(rename = "openai.chat")]
    pub chat: ChatOptions,
    #[serde(rename = "openai.responses")]
    pub responses: OpenAiResponsesOptions,
}

impl OpenAiOptions {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn chat(mut self, chat: ChatOptions) -> Self {
        self.chat = chat;
        self
    }

    #[must_use]
    pub fn shared(mut self, shared: OpenAiShared) -> Self {
        self.shared = shared;
        self
    }

    #[must_use]
    pub fn responses(mut self, responses: OpenAiResponsesOptions) -> Self {
        self.responses = responses;
        self
    }

    fn with_shared(mut self, set: impl FnOnce(OpenAiShared) -> OpenAiShared) -> Self {
        self.shared = set(std::mem::take(&mut self.shared));
        self
    }

    fn with_responses(
        mut self,
        set: impl FnOnce(OpenAiResponsesOptions) -> OpenAiResponsesOptions,
    ) -> Self {
        self.responses = set(std::mem::take(&mut self.responses));
        self
    }

    #[must_use]
    pub fn store(self, store: bool) -> Self {
        self.with_shared(|shared| shared.store(store))
    }

    #[must_use]
    pub fn metadata(self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.with_shared(|shared| shared.metadata(key, value))
    }

    #[must_use]
    pub fn prompt_cache_key(self, key: impl Into<String>) -> Self {
        self.with_shared(|shared| shared.prompt_cache_key(key))
    }

    #[must_use]
    pub fn safety_identifier(self, id: impl Into<String>) -> Self {
        self.with_shared(|shared| shared.safety_identifier(id))
    }

    #[must_use]
    pub fn reasoning_summary(self, summary: ReasoningSummary) -> Self {
        self.with_responses(|responses| responses.reasoning_summary(summary))
    }

    #[must_use]
    pub fn include(self, include: impl IntoIterator<Item = Include>) -> Self {
        self.with_responses(|responses| responses.include(include))
    }

    #[must_use]
    pub fn conversation(self, id: impl Into<String>) -> Self {
        self.with_responses(|responses| responses.conversation(id))
    }

    #[must_use]
    pub fn truncation(self, truncation: Truncation) -> Self {
        self.with_responses(|responses| responses.truncation(truncation))
    }

    #[must_use]
    pub fn background(self, background: bool) -> Self {
        self.with_responses(|responses| responses.background(background))
    }

    #[must_use]
    pub fn max_tool_calls(self, max: u32) -> Self {
        self.with_responses(|responses| responses.max_tool_calls(max))
    }
}

impl ExtensionOptions for OpenAiOptions {
    type Ext = OpenAiExt;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct OpenAiShared {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_identifier: Option<String>,
}

impl OpenAiShared {
    #[must_use]
    pub fn store(mut self, store: bool) -> Self {
        self.store = Some(store);
        self
    }

    #[must_use]
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn prompt_cache_key(mut self, key: impl Into<String>) -> Self {
        self.prompt_cache_key = Some(key.into());
        self
    }

    #[must_use]
    pub fn safety_identifier(mut self, id: impl Into<String>) -> Self {
        self.safety_identifier = Some(id.into());
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenAiExtras {
    pub service_tier: Option<String>,
    pub reasoning_effort: Option<String>,
    pub reasoning_summary: Option<String>,
    pub reasoning_mode: Option<String>,
    pub reasoning_context: Option<String>,
    pub prompt_cache_retention: Option<String>,
    pub incomplete_reason: Option<String>,
    pub phases: Option<Vec<ItemPhase>>,
    pub billing_payer: Option<String>,
    pub system_fingerprint: Option<String>,
    pub prompt_tokens_details: Option<PromptTokensDetails>,
    pub completion_tokens_details: Option<CompletionTokensDetails>,
    pub annotations: Option<Vec<Value>>,
}

impl ReplyExtras for OpenAiExtras {
    fn from_reply(api: &Api, raw: &Value) -> Result<Self, serde_json::Error> {
        if api.as_str() == "openai.chat" {
            return Ok(Self {
                service_tier: reply_field(raw, "/service_tier")?,
                system_fingerprint: reply_field(raw, "/system_fingerprint")?,
                prompt_tokens_details: reply_field(raw, "/usage/prompt_tokens_details")?,
                completion_tokens_details: reply_field(raw, "/usage/completion_tokens_details")?,
                annotations: reply_field(raw, "/choices/0/message/annotations")?,
                ..Self::default()
            });
        }
        let envelope = responses::Envelope::from_reply(raw)?;
        Ok(Self {
            service_tier: envelope.service_tier,
            reasoning_effort: envelope.reasoning_effort,
            reasoning_summary: envelope.reasoning_summary,
            reasoning_mode: envelope.reasoning_mode,
            reasoning_context: envelope.reasoning_context,
            prompt_cache_retention: envelope.prompt_cache_retention,
            incomplete_reason: envelope.incomplete_reason,
            phases: envelope.phases,
            billing_payer: reply_field(raw, "/billing/payer")?,
            ..Self::default()
        })
    }
}
