use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::codecs::completion::provider_options::reply_field;

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct OpenAiResponsesOptions {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub input: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningOptions>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<Include>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<Truncation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_management: Vec<ContextManagement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_options: Option<PromptCacheOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_programs: Option<AccessPrograms>,
}

impl OpenAiResponsesOptions {
    #[must_use]
    pub fn input(mut self, items: impl IntoIterator<Item = Value>) -> Self {
        self.input.extend(items);
        self
    }

    #[must_use]
    pub fn reasoning_summary(mut self, summary: ReasoningSummary) -> Self {
        self.reasoning.get_or_insert_with(Default::default).summary = Some(summary);
        self
    }

    #[must_use]
    pub fn reasoning_mode(mut self, mode: ReasoningMode) -> Self {
        self.reasoning.get_or_insert_with(Default::default).mode = Some(mode);
        self
    }

    #[must_use]
    pub fn reasoning_context(mut self, context: ReasoningContext) -> Self {
        self.reasoning.get_or_insert_with(Default::default).context = Some(context);
        self
    }

    #[must_use]
    pub fn include(mut self, include: impl IntoIterator<Item = Include>) -> Self {
        self.include.extend(include);
        self
    }

    #[must_use]
    pub fn conversation(mut self, id: impl Into<String>) -> Self {
        self.conversation = Some(id.into());
        self
    }

    #[must_use]
    pub fn truncation(mut self, truncation: Truncation) -> Self {
        self.truncation = Some(truncation);
        self
    }

    #[must_use]
    pub fn context_management(
        mut self,
        entries: impl IntoIterator<Item = ContextManagement>,
    ) -> Self {
        self.context_management.extend(entries);
        self
    }

    #[must_use]
    pub fn prompt_cache_comparison(mut self, response_id: impl Into<String>) -> Self {
        self.prompt_cache_options = Some(PromptCacheOptions {
            comparison_response_id: Some(response_id.into()),
        });
        self
    }

    #[must_use]
    pub fn background(mut self, background: bool) -> Self {
        self.background = Some(background);
        self
    }

    #[must_use]
    pub fn max_tool_calls(mut self, max: u32) -> Self {
        self.max_tool_calls = Some(max);
        self
    }

    #[must_use]
    pub fn top_logprobs(mut self, count: u8) -> Self {
        self.top_logprobs = Some(count);
        self
    }

    #[must_use]
    pub fn access_programs(mut self, programs: AccessPrograms) -> Self {
        self.access_programs = Some(programs);
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ReasoningOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<ReasoningSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ReasoningMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ReasoningContext>,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningSummary {
    Auto,
    Concise,
    Detailed,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningMode {
    Standard,
    Pro,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningContext {
    Auto,
    AllTurns,
    CurrentTurn,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Include {
    #[serde(rename = "file_search_call.results")]
    FileSearchCallResults,
    #[serde(rename = "web_search_call.results")]
    WebSearchCallResults,
    #[serde(rename = "web_search_call.action.sources")]
    WebSearchCallActionSources,
    #[serde(rename = "message.input_image.image_url")]
    MessageInputImageImageUrl,
    #[serde(rename = "computer_call_output.output.image_url")]
    ComputerCallOutputOutputImageUrl,
    #[serde(rename = "code_interpreter_call.outputs")]
    CodeInterpreterCallOutputs,
    #[serde(rename = "reasoning.encrypted_content")]
    ReasoningEncryptedContent,
    #[serde(rename = "message.output_text.logprobs")]
    MessageOutputTextLogprobs,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Truncation {
    Auto,
    Disabled,
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContextManagement {
    Compaction {
        #[serde(skip_serializing_if = "Option::is_none")]
        compact_threshold: Option<u32>,
    },
}

impl ContextManagement {
    pub fn compaction(threshold: Option<u32>) -> Self {
        Self::Compaction {
            compact_threshold: threshold,
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PromptCacheOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison_response_id: Option<String>,
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AccessPrograms {
    pub cyber: CyberAccess,
}

impl AccessPrograms {
    pub fn cyber(access: CyberAccess) -> Self {
        Self { cyber: access }
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CyberAccess {
    Standard,
    DaybreakBlue,
    DaybreakRed,
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ItemPhase {
    pub id: String,
    #[serde(default)]
    pub phase: Option<String>,
}

pub(crate) struct Envelope {
    pub(crate) service_tier: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) reasoning_summary: Option<String>,
    pub(crate) reasoning_mode: Option<String>,
    pub(crate) reasoning_context: Option<String>,
    pub(crate) prompt_cache_retention: Option<String>,
    pub(crate) incomplete_reason: Option<String>,
    pub(crate) phases: Option<Vec<ItemPhase>>,
}

impl Envelope {
    pub(crate) fn from_reply(raw: &Value) -> Result<Self, serde_json::Error> {
        let reasoning = raw.get("reasoning").filter(|value| value.is_object());
        let reasoning = |key: &str| match reasoning {
            Some(reasoning) => reply_field::<String>(reasoning, &format!("/{key}")),
            None => Ok(None),
        };
        let messages = raw
            .get("output")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
            .map(ItemPhase::deserialize)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            service_tier: reply_field(raw, "/service_tier")?,
            reasoning_effort: reasoning("effort")?,
            reasoning_summary: reasoning("summary")?,
            reasoning_mode: reasoning("mode")?,
            reasoning_context: reasoning("context")?,
            prompt_cache_retention: reply_field(raw, "/prompt_cache_retention")?,
            incomplete_reason: reply_field(raw, "/incomplete_details/reason")?,
            phases: (!messages.is_empty()).then_some(messages),
        })
    }
}
