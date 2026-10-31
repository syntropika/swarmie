use super::message::{
    AssistantContent, AssistantMessage, CallId, DocumentMediaType, Origin, StopReason, ToolCall,
};
use super::options::{
    CacheRetention, GenerationOptions, OnUnsupported, Reasoning, ServiceTier, Verbosity,
};
use super::provider_options::{ExtensionOptions, ProviderExtension, ProviderOptions, ReplyExtras};
use crate::codecs::error::ProviderError;
use crate::codecs::json_utils::remove_preserving_order;
use crate::codecs::message::ToolChoice;
use crate::codecs::{
    json_utils,
    message::{Message, ToolName, UserContent},
};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::{Add, AddAssign};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Document {
    pub id: String,
    pub text: String,
    #[serde(flatten)]
    pub additional_props: HashMap<String, String>,
}

impl std::fmt::Display for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            concat!("<file id: {}>\n", "{}\n", "</file>\n"),
            self.id,
            if self.additional_props.is_empty() {
                self.text.clone()
            } else {
                let mut sorted_props = self.additional_props.iter().collect::<Vec<_>>();
                sorted_props.sort_by(|a, b| a.0.cmp(b.0));
                let metadata = sorted_props
                    .iter()
                    .map(|(k, v)| format!("{k}: {v:?}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("<metadata {} />\n{}", metadata, self.text)
            }
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: ToolName,
    pub description: String,
    pub parameters: serde_json::Value,
    #[serde(default, skip_serializing_if = "ToolAnnotations::is_empty")]
    pub annotations: ToolAnnotations,
}

impl ToolDefinition {
    pub fn new(
        name: ToolName,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            name,
            description: description.into(),
            parameters,
            annotations: ToolAnnotations::default(),
        }
    }

    pub fn with_annotations(mut self, annotations: ToolAnnotations) -> Self {
        self.annotations = annotations;
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

impl ToolAnnotations {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn read_only(&self) -> bool {
        self.read_only_hint.unwrap_or(false)
    }

    pub fn destructive(&self) -> bool {
        !self.read_only() && self.destructive_hint.unwrap_or(true)
    }

    pub fn idempotent(&self) -> bool {
        self.read_only() || self.idempotent_hint.unwrap_or(false)
    }

    pub fn open_world(&self) -> bool {
        self.open_world_hint.unwrap_or(true)
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only_hint = Some(read_only);
        self
    }

    pub fn with_destructive(mut self, destructive: bool) -> Self {
        self.destructive_hint = Some(destructive);
        self
    }

    pub fn with_idempotent(mut self, idempotent: bool) -> Self {
        self.idempotent_hint = Some(idempotent);
        self
    }

    pub fn with_open_world(mut self, open_world: bool) -> Self {
        self.open_world_hint = Some(open_world);
        self
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ProviderToolDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub config: serde_json::Map<String, serde_json::Value>,
}

impl ProviderToolDefinition {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            config: serde_json::Map::new(),
        }
    }

    pub fn with_config(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.config.insert(key.into(), value);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Other(String),
}

impl FinishReason {
    pub fn reconcile_with_output(self, has_tool_call: bool) -> Self {
        if has_tool_call && matches!(self, Self::Stop) {
            Self::ToolCalls
        } else {
            self
        }
    }

    pub fn truncated_output(&self) -> bool {
        matches!(self, Self::Length | Self::ContentFilter)
    }

    pub fn no_answer_message(&self) -> String {
        let remedy = match self {
            Self::Length => {
                "the turn ran out of output budget before producing one — \
                 raise max_tokens for this request"
            }
            Self::ContentFilter => {
                "the provider filtered the response — the content, not the \
                 budget, is what it objected to"
            }
            _ => "the turn ended before producing one",
        };
        format!(
            "the model produced no answer and stopped with \
             finish_reason={self:?}; {remedy}"
        )
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "CompletionResponseRepr")]
pub struct CompletionResponse {
    pub choice: Vec<AssistantContent>,
    pub usage: Usage,
    pub origin: Origin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aborted: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_request_id: Option<String>,
    #[serde(default)]
    finish_reason: Option<FinishReason>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    accepts_unknown_finish_reasons: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    unfinished_calls: Vec<CallId>,
    pub raw: serde_json::Value,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_request_id: Option<String>,
}

impl CompletionResponse {
    pub fn new(
        choice: Vec<AssistantContent>,
        usage: Usage,
        origin: Origin,
        raw: serde_json::Value,
    ) -> Self {
        Self {
            choice,
            usage,
            origin,
            error: None,
            aborted: None,
            provider_request_id: None,
            finish_reason: None,
            accepts_unknown_finish_reasons: false,
            unfinished_calls: Vec::new(),
            raw,
        }
    }

    pub fn provider(&self) -> &str {
        &self.origin.provider
    }

    pub fn extras<P: ProviderExtension>(&self) -> Option<Result<P::Extras, serde_json::Error>> {
        (self.origin.provider == P::PROVIDER)
            .then(|| P::Extras::from_reply(&self.origin.api, &self.raw))
    }

    pub fn extras_lossy<P: ProviderExtension>(&self) -> P::Extras
    where
        P::Extras: Default,
    {
        match self.extras::<P>() {
            Some(Ok(extras)) => extras,
            Some(Err(error)) => {
                tracing::debug!(
                    provider = P::PROVIDER,
                    api = %self.origin.api,
                    %error,
                    "reply extras did not decode; reading them as empty"
                );
                P::Extras::default()
            }
            None => P::Extras::default(),
        }
    }

    pub fn model(&self) -> Option<&str> {
        self.origin.response_model.as_deref()
    }

    pub fn response_id(&self) -> Option<&str> {
        self.origin.response_id.as_deref()
    }

    pub fn finish_reason(&self) -> Option<FinishReason> {
        self.finish_reason.clone()
    }

    pub fn accepts_unknown_finish_reasons(&self) -> bool {
        self.accepts_unknown_finish_reasons
    }

    pub fn unfinished_calls(&self) -> impl Iterator<Item = &ToolCall> {
        self.tool_calls()
            .filter(|call| self.unfinished_calls.contains(&call.id))
    }

    pub fn with_unfinished_calls(mut self, calls: impl IntoIterator<Item = CallId>) -> Self {
        self.unfinished_calls = calls.into_iter().collect();
        self
    }

    pub fn accept_unknown_finish_reasons(mut self, accept: bool) -> Self {
        self.accepts_unknown_finish_reasons = accept;
        self
    }

    pub fn stop(&self) -> StopReason {
        if let Some(error) = &self.error {
            return StopReason::Error(error.clone());
        }
        if let Some(reason) = &self.aborted {
            return StopReason::Aborted(reason.clone());
        }
        match &self.finish_reason {
            Some(FinishReason::Length) => StopReason::Length,
            Some(FinishReason::ToolCalls) => StopReason::ToolUse,
            Some(FinishReason::ContentFilter) => {
                StopReason::Error("Provider finish_reason: content_filter".to_owned())
            }
            Some(FinishReason::Other(reason)) if !self.accepts_unknown_finish_reasons => {
                StopReason::Error(format!("Provider finish_reason: {reason}"))
            }
            Some(FinishReason::Stop | FinishReason::Other(_)) | None
                if self.tool_calls().next().is_some() =>
            {
                StopReason::ToolUse
            }
            Some(FinishReason::Stop | FinishReason::Other(_)) | None => StopReason::Stop,
        }
    }

    pub fn identity(&self) -> ResponseIdentity {
        ResponseIdentity {
            response_id: self.origin.response_id.clone(),
            provider_request_id: self.provider_request_id.clone(),
        }
    }

    pub fn with_finish_reason(self, finish_reason: FinishReason) -> Self {
        self.with_optional_finish_reason(Some(finish_reason))
    }

    pub fn text(&self) -> String {
        self.choice
            .iter()
            .filter_map(|part| match part {
                AssistantContent::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn reasoning(&self) -> String {
        self.choice
            .iter()
            .filter_map(|part| match part {
                AssistantContent::Reasoning(reasoning) => Some(reasoning.text.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn message(&self) -> Option<Message> {
        if self.choice.is_empty() {
            return None;
        }
        Some(Message::Assistant(AssistantMessage {
            content: self.choice.clone(),
            ..self.head()
        }))
    }

    pub fn continued(&self, content: Vec<AssistantContent>) -> AssistantMessage {
        let content = content
            .into_iter()
            .map(|block| {
                if self.choice.contains(&block) {
                    block
                } else {
                    crate::codecs::operation::completion::canonical(block)
                }
            })
            .collect();
        AssistantMessage::rolled_back(Some(self.origin.clone()), content)
    }

    pub fn head(&self) -> AssistantMessage {
        AssistantMessage {
            content: Vec::new(),
            origin: Some(self.origin.clone()),
            stop: Some(self.stop()),
        }
    }

    pub fn tool_calls(&self) -> impl Iterator<Item = &ToolCall> {
        self.choice.iter().filter_map(|part| match part {
            AssistantContent::ToolCall(call) => Some(call),
            _ => None,
        })
    }

    pub fn with_optional_finish_reason(mut self, finish_reason: Option<FinishReason>) -> Self {
        let has_tool_call = self
            .choice
            .iter()
            .any(|content| matches!(content, AssistantContent::ToolCall(_)));
        self.finish_reason =
            finish_reason.map(|reason| reason.reconcile_with_output(has_tool_call));
        self
    }
}

#[derive(Deserialize)]
struct CompletionResponseRepr {
    choice: Vec<AssistantContent>,
    usage: Usage,
    origin: Origin,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    aborted: Option<String>,
    #[serde(default)]
    provider_request_id: Option<String>,
    #[serde(default)]
    finish_reason: Option<FinishReason>,
    #[serde(default)]
    accepts_unknown_finish_reasons: bool,
    #[serde(default)]
    unfinished_calls: Vec<CallId>,
    raw: serde_json::Value,
}

impl From<CompletionResponseRepr> for CompletionResponse {
    fn from(repr: CompletionResponseRepr) -> Self {
        let CompletionResponseRepr {
            choice,
            usage,
            mut origin,
            error,
            aborted,
            provider_request_id,
            finish_reason,
            accepts_unknown_finish_reasons,
            unfinished_calls,
            raw,
        } = repr;
        use crate::codecs::provider_response::reported;
        origin.response_id = reported(origin.response_id);
        origin.response_model = reported(origin.response_model);
        let mut response =
            Self::new(choice, usage, origin, raw).with_optional_finish_reason(finish_reason);
        response.error = error;
        response.aborted = aborted;
        response.provider_request_id = reported(provider_request_id);
        response.accepts_unknown_finish_reasons = accepts_unknown_finish_reasons;
        response.unfinished_calls = unfinished_calls;
        response
    }
}

#[non_exhaustive]
#[derive(Debug, Default, PartialEq, Clone, Copy, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_prompt_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
}

impl Usage {
    pub const fn new() -> Self {
        Self {
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            cached_input_tokens: None,
            cache_creation_input_tokens: None,
            tool_use_prompt_tokens: None,
            reasoning_tokens: None,
            cost: None,
        }
    }

    pub fn is_reported(&self) -> bool {
        *self != Self::default()
    }

    pub fn context_tokens(&self) -> Option<u64> {
        self.total_tokens.or_else(|| {
            Some(
                self.input_tokens?
                    .saturating_add(self.output_tokens.unwrap_or(0)),
            )
        })
    }

    pub fn input_tokens(mut self, tokens: impl Into<Option<u64>>) -> Self {
        self.input_tokens = tokens.into();
        self
    }

    pub fn output_tokens(mut self, tokens: impl Into<Option<u64>>) -> Self {
        self.output_tokens = tokens.into();
        self
    }

    pub fn total_tokens(mut self, tokens: impl Into<Option<u64>>) -> Self {
        self.total_tokens = tokens.into();
        self
    }

    pub fn cached_input_tokens(mut self, tokens: impl Into<Option<u64>>) -> Self {
        self.cached_input_tokens = tokens.into();
        self
    }

    pub fn cache_creation_input_tokens(mut self, tokens: impl Into<Option<u64>>) -> Self {
        self.cache_creation_input_tokens = tokens.into();
        self
    }

    pub fn tool_use_prompt_tokens(mut self, tokens: impl Into<Option<u64>>) -> Self {
        self.tool_use_prompt_tokens = tokens.into();
        self
    }

    pub fn reasoning_tokens(mut self, tokens: impl Into<Option<u64>>) -> Self {
        self.reasoning_tokens = tokens.into();
        self
    }

    pub fn cost(mut self, cost: impl Into<Option<Cost>>) -> Self {
        self.cost = cost.into();
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
    pub total: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    incomplete: bool,
}

impl Cost {
    pub fn is_complete(&self) -> bool {
        !self.incomplete
    }

    pub(crate) fn priced(
        input: f64,
        output: f64,
        cache_read: Option<f64>,
        cache_write: Option<f64>,
    ) -> Self {
        Self {
            input: Some(input),
            output: Some(output),
            cache_read,
            cache_write,
            total: input + output + cache_read.unwrap_or(0.0) + cache_write.unwrap_or(0.0),
            incomplete: cache_read.is_none() || cache_write.is_none(),
        }
    }

    pub fn from_parts(input: f64, output: f64, cache_read: f64, cache_write: f64) -> Self {
        Self {
            input: Some(input),
            output: Some(output),
            cache_read: Some(cache_read),
            cache_write: Some(cache_write),
            total: input + output + cache_read + cache_write,
            incomplete: false,
        }
    }

    pub fn from_total(total: f64) -> Self {
        Self {
            input: None,
            output: None,
            cache_read: None,
            cache_write: None,
            total,
            incomplete: false,
        }
    }

    pub fn input(mut self, input: impl Into<Option<f64>>) -> Self {
        self.input = input.into();
        self
    }

    pub fn output(mut self, output: impl Into<Option<f64>>) -> Self {
        self.output = output.into();
        self
    }

    pub fn cache_read(mut self, cache_read: impl Into<Option<f64>>) -> Self {
        self.cache_read = cache_read.into();
        self
    }

    pub fn cache_write(mut self, cache_write: impl Into<Option<f64>>) -> Self {
        self.cache_write = cache_write.into();
        self
    }
}

fn add_part(lhs: Option<f64>, rhs: Option<f64>) -> Option<f64> {
    Some(lhs? + rhs?)
}

impl Add for Cost {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        Self {
            input: add_part(self.input, other.input),
            output: add_part(self.output, other.output),
            cache_read: add_part(self.cache_read, other.cache_read),
            cache_write: add_part(self.cache_write, other.cache_write),
            total: self.total + other.total,
            incomplete: self.incomplete || other.incomplete,
        }
    }
}

fn add_counter(lhs: Option<u64>, rhs: Option<u64>) -> Option<u64> {
    match (lhs, rhs) {
        (None, None) => None,
        (lhs, rhs) => Some(lhs.unwrap_or(0) + rhs.unwrap_or(0)),
    }
}

impl Add for Usage {
    type Output = Self;

    fn add(mut self, other: Self) -> Self::Output {
        self += other;
        self
    }
}

impl AddAssign for Usage {
    fn add_assign(&mut self, other: Self) {
        if !other.is_reported() {
            return;
        }
        if !self.is_reported() {
            *self = other;
            return;
        }
        self.cost = match (self.cost, other.cost) {
            (Some(lhs), Some(rhs)) => Some(lhs + rhs),
            _ => None,
        };
        self.input_tokens = add_counter(self.input_tokens, other.input_tokens);
        self.output_tokens = add_counter(self.output_tokens, other.output_tokens);
        self.total_tokens = add_counter(self.total_tokens, other.total_tokens);
        self.cached_input_tokens = add_counter(self.cached_input_tokens, other.cached_input_tokens);
        self.cache_creation_input_tokens = add_counter(
            self.cache_creation_input_tokens,
            other.cache_creation_input_tokens,
        );
        self.tool_use_prompt_tokens =
            add_counter(self.tool_use_prompt_tokens, other.tool_use_prompt_tokens);
        self.reasoning_tokens = add_counter(self.reasoning_tokens, other.reasoning_tokens);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageTotals {
    pub tokens: Usage,
    pub cost: f64,
    pub unpriced: u64,
    pub calls: u64,
}

impl UsageTotals {
    pub fn record(&mut self, usage: &Usage) {
        self.calls += 1;
        match usage.cost {
            Some(cost) => {
                self.cost += cost.total;
                if !cost.is_complete() {
                    self.unpriced += 1;
                }
            }
            None => self.unpriced += 1,
        }
        self.tokens += usage.cost(None);
    }

    pub fn add(&mut self, other: &UsageTotals) {
        self.tokens += other.tokens;
        self.cost += other.cost;
        self.unpriced += other.unpriced;
        self.calls += other.calls;
    }

    pub fn uncached_input(&self) -> u64 {
        self.tokens
            .input_tokens
            .unwrap_or(0)
            .saturating_sub(self.tokens.cached_input_tokens.unwrap_or(0))
            .saturating_sub(self.tokens.cache_creation_input_tokens.unwrap_or(0))
    }

    pub fn total_tokens(&self) -> u64 {
        self.tokens
            .input_tokens
            .unwrap_or(0)
            .saturating_add(self.tokens.output_tokens.unwrap_or(0))
    }

    pub fn cost_label(&self) -> Option<String> {
        if self.unpriced >= self.calls {
            return None;
        }
        let more = if self.unpriced > 0 { "+" } else { "" };
        Some(format!("{}{more}", dollars_label(self.cost)))
    }

    pub fn cost_or_tokens(&self) -> String {
        self.cost_label()
            .unwrap_or_else(|| format!("{} tokens", tokens_label(self.total_tokens())))
    }
}

impl std::fmt::Display for UsageTotals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.calls {
            1 => f.write_str("1 model call")?,
            calls => write!(f, "{calls} model calls")?,
        }
        write!(f, ": {} in", tokens_label(self.uncached_input()))?;
        if let Some(read) = self.tokens.cached_input_tokens.filter(|read| *read > 0) {
            write!(f, ", {} cache read", tokens_label(read))?;
        }
        if let Some(written) = self
            .tokens
            .cache_creation_input_tokens
            .filter(|written| *written > 0)
        {
            write!(f, ", {} cache written", tokens_label(written))?;
        }
        write!(
            f,
            ", {} out",
            tokens_label(self.tokens.output_tokens.unwrap_or(0))
        )?;
        if let Some(thought) = self.tokens.reasoning_tokens.filter(|thought| *thought > 0) {
            write!(f, " ({} reasoning)", tokens_label(thought))?;
        }
        let cost = self.cost_label();
        write!(f, "; {}", cost.as_deref().unwrap_or("cost unknown"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextUse {
    pub tokens: u64,
    pub limit: Option<u32>,
}

impl ContextUse {
    pub fn percent(&self) -> Option<u64> {
        self.limit
            .filter(|limit| *limit > 0)
            .map(|limit| self.tokens.saturating_mul(100) / u64::from(limit))
    }
}

impl std::fmt::Display for ContextUse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.limit, self.percent()) {
            (Some(limit), Some(percent)) => write!(
                f,
                "{}/{} ({percent}%)",
                tokens_label(self.tokens),
                tokens_label(u64::from(limit))
            ),
            _ => f.write_str(&tokens_label(self.tokens)),
        }
    }
}

pub fn tokens_label(count: u64) -> String {
    // Shown rounded, so the float conversion's precision does not matter.
    let scaled = |unit: f64| count as f64 / unit;
    match count {
        0..1_000 => count.to_string(),
        1_000..10_000 => format!("{:.1}k", scaled(1e3)),
        10_000..1_000_000 => format!("{:.0}k", scaled(1e3)),
        1_000_000..10_000_000 => format!("{:.1}M", scaled(1e6)),
        _ => format!("{:.0}M", scaled(1e6)),
    }
}

fn dollars_label(cost: f64) -> String {
    if cost < 1.0 {
        format!("${cost:.3}")
    } else {
        format!("${cost:.2}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub composes_native_output_with_tools: bool,
    #[serde(default, skip_serializing_if = "crate::codecs::json_utils::is_false")]
    pub rejects_forced_tool_choice: bool,
}

impl ProviderCapabilities {
    pub const fn new() -> Self {
        Self {
            composes_native_output_with_tools: false,
            rejects_forced_tool_choice: false,
        }
    }

    pub const fn with_native_output_tool_composition(mut self, supported: bool) -> Self {
        self.composes_native_output_with_tools = supported;
        self
    }

    pub const fn with_forced_tool_choice_rejected(mut self, rejected: bool) -> Self {
        self.rejects_forced_tool_choice = rejected;
        self
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub model: Option<String>,
    pub chat_history: Vec<Arc<Message>>,
    pub documents: Vec<Document>,
    pub tools: Vec<Arc<ToolDefinition>>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u64>,
    pub tool_choice: Option<ToolChoice>,
    pub additional_params: Option<serde_json::Value>,
    pub output_schema: Option<schemars::Schema>,
    #[serde(skip)]
    pub record_telemetry_content: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub accept_unknown_finish_reasons: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub foreign_reasoning: bool,
    #[serde(default, skip_serializing_if = "GenerationOptions::is_default")]
    pub options: GenerationOptions,
    #[serde(default, skip_serializing_if = "ProviderOptions::is_empty")]
    pub provider_options: ProviderOptions,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cache_points: Vec<CachePoint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CachePoint {
    Tools,
    System,
    Messages(usize),
    Within { message: usize, items: usize },
}

impl CompletionRequest {
    pub fn system_instructions(&self) -> Option<&str> {
        match self.chat_history.first().map(Arc::as_ref) {
            Some(Message::System { content }) => Some(content.as_str()),
            _ => None,
        }
    }

    pub fn validate_message_content(&self) -> Result<(), ProviderError> {
        if self.chat_history.is_empty() {
            return Err(ProviderError::request(
                "request has an empty chat history; providers require at least one message",
            ));
        }

        let empty_message = |role: &str, index: usize| {
            ProviderError::request(format!(
                "{role} message at index {index} has no content; \
                 providers reject empty content blocks"
            ))
        };

        for (index, message) in self.chat_history.iter().enumerate() {
            match &**message {
                Message::System { .. } => {}
                Message::Assistant(AssistantMessage { content, .. }) => {
                    if content.is_empty() {
                        return Err(empty_message("assistant", index));
                    }
                }
                Message::User { content } => {
                    if content.is_empty() {
                        return Err(empty_message("user", index));
                    }
                }
            }
        }

        Ok(())
    }

    pub fn output_schema_name(&self) -> Option<String> {
        self.output_schema.as_ref().map(|schema| {
            schema
                .as_object()
                .and_then(|o| o.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("response_schema")
                .to_string()
        })
    }

    pub fn normalized_documents(&self) -> Option<Message> {
        Self::normalized_documents_from(&self.documents)
    }

    fn normalized_documents_from(documents: &[Document]) -> Option<Message> {
        if documents.is_empty() {
            return None;
        }

        let content = documents
            .iter()
            .map(|doc| UserContent::document_text(doc.to_string(), Some(DocumentMediaType::TXT)))
            .collect();

        Some(Message::User { content })
    }
}

pub(crate) fn insert_after_leading_system(chat_history: &mut Vec<Arc<Message>>, message: Message) {
    let insert_at = chat_history
        .iter()
        .position(|message| !matches!(&**message, Message::System { .. }))
        .unwrap_or(chat_history.len());
    let joins =
        |at: &&mut Arc<Message>| matches!(&***at, Message::User { content } if !content.is_empty());
    match (
        chat_history
            .get_mut(insert_at)
            .filter(joins)
            .map(Arc::make_mut),
        message,
    ) {
        (Some(Message::User { content }), Message::User { content: documents }) => {
            content.splice(0..0, documents);
        }
        (_, message) => chat_history.insert(insert_at, Arc::new(message)),
    }
}

fn merge_provider_tools_into_additional_params(
    additional_params: Option<serde_json::Value>,
    provider_tools: Vec<ProviderToolDefinition>,
) -> Option<serde_json::Value> {
    if provider_tools.is_empty() {
        return additional_params;
    }

    let mut provider_tools_json = provider_tools
        .into_iter()
        .map(|ProviderToolDefinition { kind, mut config }| {
            // Force the provider tool type from the strongly-typed field.
            config.insert("type".to_string(), serde_json::Value::String(kind));
            serde_json::Value::Object(config)
        })
        .collect::<Vec<_>>();

    let mut params_map = match additional_params {
        Some(serde_json::Value::Object(map)) => map,
        Some(serde_json::Value::Bool(stream)) => {
            let mut map = serde_json::Map::new();
            map.insert("stream".to_string(), serde_json::Value::Bool(stream));
            map
        }
        _ => serde_json::Map::new(),
    };

    let mut merged_tools = match remove_preserving_order(&mut params_map, "tools") {
        Some(serde_json::Value::Array(existing)) => existing,
        _ => Vec::new(),
    };
    merged_tools.append(&mut provider_tools_json);
    params_map.insert("tools".to_string(), serde_json::Value::Array(merged_tools));
    Some(serde_json::Value::Object(params_map))
}

impl CompletionRequest {
    pub fn new(prompt: impl Into<Message>) -> Self {
        Self::conversation(vec![Arc::new(prompt.into())])
    }

    fn conversation(chat_history: Vec<Arc<Message>>) -> Self {
        Self {
            model: None,
            chat_history,
            documents: Vec::new(),
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            tool_choice: None,
            additional_params: None,
            output_schema: None,
            record_telemetry_content: false,
            accept_unknown_finish_reasons: false,
            foreign_reasoning: false,
            options: GenerationOptions::default(),
            provider_options: ProviderOptions::default(),
            cache_points: Vec::new(),
        }
    }

    pub fn preamble(mut self, preamble: impl Into<String>) -> Self {
        self.chat_history
            .insert(0, Arc::new(Message::system(preamble.into())));
        self
    }

    pub fn model<S: Into<String>>(mut self, model: impl Into<Option<S>>) -> Self {
        self.model = model.into().map(Into::into);
        self.warn_if_shadowed("model", self.model.is_some());
        self
    }

    pub fn message(self, message: impl Into<Arc<Message>>) -> Self {
        self.messages([message])
    }

    pub fn messages(mut self, messages: impl IntoIterator<Item = impl Into<Arc<Message>>>) -> Self {
        let prompt = self.chat_history.pop();
        self.chat_history
            .extend(messages.into_iter().map(Into::into));
        self.chat_history.extend(prompt);
        self
    }

    pub fn document(mut self, document: Document) -> Self {
        self.documents.push(document);
        self
    }

    pub fn documents(mut self, documents: impl IntoIterator<Item = Document>) -> Self {
        self.documents.extend(documents);
        self
    }

    pub fn tool(self, tool: impl Into<Arc<ToolDefinition>>) -> Self {
        self.tools([tool])
    }

    pub fn tools(
        mut self,
        tools: impl IntoIterator<Item = impl Into<Arc<ToolDefinition>>>,
    ) -> Self {
        let first = self.tools.is_empty();
        self.tools.extend(tools.into_iter().map(Into::into));
        self.warn_if_shadowed("tools", first && !self.tools.is_empty());
        self
    }

    pub fn provider_tool(self, tool: ProviderToolDefinition) -> Self {
        self.provider_tools(vec![tool])
    }

    pub fn provider_tools(mut self, tools: Vec<ProviderToolDefinition>) -> Self {
        self.additional_params =
            merge_provider_tools_into_additional_params(self.additional_params.take(), tools);
        self
    }

    pub fn additional_params(
        mut self,
        additional_params: impl Into<Option<serde_json::Value>>,
    ) -> Self {
        let additional_params = additional_params.into();
        for key in shadowed_typed_fields(
            additional_params.as_ref(),
            &[
                ("temperature", self.temperature.is_some()),
                ("max_tokens", self.max_tokens.is_some()),
                ("tool_choice", self.tool_choice.is_some()),
                ("model", self.model.is_some()),
                ("tools", !self.tools.is_empty()),
                ("response_format", self.output_schema.is_some()),
            ],
        ) {
            warn_shadowed(key);
        }
        self.additional_params =
            json_utils::merge_params(self.additional_params.take(), additional_params);
        self
    }

    pub fn temperature(mut self, temperature: impl Into<Option<f64>>) -> Self {
        self.temperature = temperature.into();
        self.warn_if_shadowed("temperature", self.temperature.is_some());
        self
    }

    pub fn max_tokens(mut self, max_tokens: impl Into<Option<u64>>) -> Self {
        self.max_tokens = max_tokens.into();
        self.warn_if_shadowed("max_tokens", self.max_tokens.is_some());
        self
    }

    pub fn tool_choice(mut self, tool_choice: ToolChoice) -> Self {
        self.tool_choice = Some(tool_choice);
        self.warn_if_shadowed("tool_choice", true);
        self
    }

    pub fn output_schema(mut self, schema: impl Into<Option<schemars::Schema>>) -> Self {
        self.output_schema = schema.into();
        self.warn_if_shadowed("response_format", self.output_schema.is_some());
        self
    }

    pub fn record_content_telemetry(mut self, enabled: bool) -> Self {
        self.record_telemetry_content = enabled;
        self
    }

    pub fn accept_unknown_finish_reasons(mut self, accept: bool) -> Self {
        self.accept_unknown_finish_reasons = accept;
        self
    }

    pub fn options(mut self, options: GenerationOptions) -> Self {
        self.options = options;
        self
    }

    pub fn reasoning(mut self, reasoning: impl Into<Reasoning>) -> Self {
        self.options = std::mem::take(&mut self.options).reasoning(reasoning);
        self
    }

    pub fn cache(mut self, cache: CacheRetention) -> Self {
        self.options = std::mem::take(&mut self.options).cache(cache);
        self
    }

    pub fn service_tier(mut self, tier: ServiceTier) -> Self {
        self.options = std::mem::take(&mut self.options).service_tier(tier);
        self
    }

    pub fn verbosity(mut self, verbosity: Verbosity) -> Self {
        self.options = std::mem::take(&mut self.options).verbosity(verbosity);
        self
    }

    pub fn parallel_tool_calls(mut self, parallel: bool) -> Self {
        self.options = std::mem::take(&mut self.options).parallel_tool_calls(parallel);
        self
    }

    pub fn top_p(mut self, top_p: f64) -> Self {
        self.options = std::mem::take(&mut self.options).top_p(top_p);
        self
    }

    pub fn seed(mut self, seed: u64) -> Self {
        self.options = std::mem::take(&mut self.options).seed(seed);
        self
    }

    pub fn stop<S: Into<String>>(mut self, stop: impl IntoIterator<Item = S>) -> Self {
        self.options = std::mem::take(&mut self.options).stop(stop);
        self
    }

    pub fn on_unsupported(mut self, policy: OnUnsupported) -> Self {
        self.options = std::mem::take(&mut self.options).on_unsupported(policy);
        self
    }

    pub fn provider_options(mut self, options: ProviderOptions) -> Self {
        self.provider_options = options;
        self
    }

    pub fn provider_option<O: ExtensionOptions>(mut self, options: O) -> Self {
        self.provider_options = std::mem::take(&mut self.provider_options).set(options);
        self
    }

    pub fn messages_for_telemetry(&self) -> Vec<Arc<Message>> {
        let mut messages = self.chat_history.clone();
        if let Some(documents) = self.normalized_documents() {
            insert_after_leading_system(&mut messages, documents);
        }
        messages
    }

    fn warn_if_shadowed(&self, key: &'static str, set: bool) {
        if !shadowed_typed_fields(self.additional_params.as_ref(), &[(key, set)]).is_empty() {
            warn_shadowed(key);
        }
    }
}

fn warn_shadowed(key: &str) {
    if matches!(key, "tools" | "response_format") {
        tracing::warn!(
            key,
            "additional_params also carries `{key}`; the provider decides how it combines with the typed field"
        );
    } else {
        tracing::warn!(
            key,
            "additional_params overrides the typed `{key}` field set on the same request"
        );
    }
}

impl From<&str> for CompletionRequest {
    fn from(prompt: &str) -> Self {
        Self::new(prompt)
    }
}

impl From<String> for CompletionRequest {
    fn from(prompt: String) -> Self {
        Self::new(prompt)
    }
}

impl From<Message> for CompletionRequest {
    fn from(prompt: Message) -> Self {
        Self::new(prompt)
    }
}

impl From<Vec<Message>> for CompletionRequest {
    fn from(chat_history: Vec<Message>) -> Self {
        Self::conversation(chat_history.into_iter().map(Arc::new).collect())
    }
}

pub(crate) fn shadowed_typed_fields<'a>(
    additional_params: Option<&serde_json::Value>,
    typed: &[(&'a str, bool)],
) -> Vec<&'a str> {
    let Some(serde_json::Value::Object(params)) = additional_params else {
        return Vec::new();
    };
    typed
        .iter()
        .filter(|(key, set)| *set && params.contains_key(*key))
        .map(|(key, _)| *key)
        .collect()
}

#[cfg(test)]
mod plain_value_tests {

    use super::*;
    use crate::codecs::message::{Reasoning, ToolChoice};

    fn bare(chat_history: Vec<Message>) -> CompletionRequest {
        CompletionRequest {
            model: None,
            chat_history: chat_history.into_iter().map(Into::into).collect(),
            documents: Vec::new(),
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            tool_choice: None,
            additional_params: None,
            output_schema: None,
            record_telemetry_content: false,
            accept_unknown_finish_reasons: false,
            foreign_reasoning: false,
            options: GenerationOptions::default(),
            provider_options: crate::codecs::completion::ProviderOptions::default(),
            cache_points: Vec::new(),
        }
    }

    fn assert_same(actual: &CompletionRequest, expected: &CompletionRequest) {
        assert_eq!(
            serde_json::to_value(actual).ok(),
            serde_json::to_value(expected).ok()
        );
        assert_eq!(
            actual.record_telemetry_content,
            expected.record_telemetry_content
        );
    }

    fn tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: crate::codecs::message::ToolName::new(name).expect("tool name"),
            description: format!("the {name} tool"),
            parameters: serde_json::json!({"type": "object"}),
            annotations: Default::default(),
        }
    }

    fn document(id: &str) -> Document {
        Document {
            id: id.to_owned(),
            text: format!("text of {id}"),
            additional_props: HashMap::new(),
        }
    }

    #[test]
    fn new_is_the_one_user_message() {
        assert_same(
            &CompletionRequest::new("hi"),
            &bare(vec![Message::user("hi")]),
        );
    }

    #[test]
    fn preamble_is_the_first_message_and_the_prompt_the_last() {
        let request = CompletionRequest::new("prompt")
            .message(Message::user("earlier"))
            .preamble("system");
        assert_same(
            &request,
            &bare(vec![
                Message::system("system"),
                Message::user("earlier"),
                Message::user("prompt"),
            ]),
        );
    }

    #[test]
    fn a_second_preamble_goes_ahead_of_the_first() {
        let request = CompletionRequest::new("prompt")
            .preamble("first")
            .preamble("second");
        assert_same(
            &request,
            &bare(vec![
                Message::system("second"),
                Message::system("first"),
                Message::user("prompt"),
            ]),
        );
    }

    #[test]
    fn message_and_messages_go_before_the_prompt_in_order() {
        let request = CompletionRequest::new("prompt")
            .message(Message::user("one"))
            .messages([Message::assistant("two"), Message::user("three")]);
        assert_same(
            &request,
            &bare(vec![
                Message::user("one"),
                Message::assistant("two"),
                Message::user("three"),
                Message::user("prompt"),
            ]),
        );
    }

    #[test]
    fn document_and_documents_append_in_order() {
        let request = CompletionRequest::new("p")
            .document(document("a"))
            .documents([document("b"), document("c")]);
        let mut expected = bare(vec![Message::user("p")]);
        expected.documents = vec![document("a"), document("b"), document("c")];
        assert_same(&request, &expected);
    }

    #[test]
    fn tool_and_tools_append_in_order() {
        let request = CompletionRequest::new("p")
            .tool(tool("a"))
            .tools(vec![tool("b"), tool("c")]);
        let mut expected = bare(vec![Message::user("p")]);
        expected.tools = vec![tool("a").into(), tool("b").into(), tool("c").into()];
        assert_same(&request, &expected);
    }

    #[test]
    fn provider_tools_land_in_additional_params_tools() {
        let request = CompletionRequest::new("p")
            .additional_params(serde_json::json!({"top_p": 0.5}))
            .provider_tool(ProviderToolDefinition::new("web_search"))
            .provider_tools(vec![
                ProviderToolDefinition::new("code").with_config("x", serde_json::json!(1)),
            ]);
        let mut expected = bare(vec![Message::user("p")]);
        expected.additional_params = Some(serde_json::json!({
            "top_p": 0.5,
            "tools": [{"type": "web_search"}, {"type": "code", "x": 1}],
        }));
        assert_same(&request, &expected);
    }

    #[test]
    fn additional_params_after_provider_tools_replace_them() {
        let replaced = CompletionRequest::new("p")
            .provider_tool(ProviderToolDefinition::new("web_search"))
            .additional_params(serde_json::json!({"tools": [{"type": "x"}]}));
        let mut expected = bare(vec![Message::user("p")]);
        expected.additional_params = Some(serde_json::json!({"tools": [{"type": "x"}]}));
        assert_same(&replaced, &expected);

        let cleared = CompletionRequest::new("p")
            .provider_tool(ProviderToolDefinition::new("web_search"))
            .additional_params(None);
        assert_same(&cleared, &bare(vec![Message::user("p")]));

        // Set first, the parameters keep their tools and the provider tools
        // follow them, as the builder sent them.
        let kept = CompletionRequest::new("p")
            .additional_params(serde_json::json!({"tools": [{"type": "x"}]}))
            .provider_tool(ProviderToolDefinition::new("web_search"));
        expected.additional_params = Some(serde_json::json!({
            "tools": [{"type": "x"}, {"type": "web_search"}],
        }));
        assert_same(&kept, &expected);
    }

    #[test]
    fn additional_params_merge_and_none_clears() {
        let merged = CompletionRequest::new("p")
            .additional_params(serde_json::json!({"a": 1}))
            .additional_params(serde_json::json!({"b": 2}));
        let mut expected = bare(vec![Message::user("p")]);
        expected.additional_params = Some(serde_json::json!({"a": 1, "b": 2}));
        assert_same(&merged, &expected);

        let cleared = merged.additional_params(None);
        assert_same(&cleared, &bare(vec![Message::user("p")]));
    }

    #[test]
    fn sampling_setters_set_and_none_clears() {
        let request = CompletionRequest::new("p")
            .temperature(0.2)
            .max_tokens(64)
            .tool_choice(ToolChoice::Required)
            .model("gpt-5.2")
            .record_content_telemetry(true);
        let mut expected = bare(vec![Message::user("p")]);
        expected.temperature = Some(0.2);
        expected.max_tokens = Some(64);
        expected.tool_choice = Some(ToolChoice::Required);
        expected.model = Some("gpt-5.2".to_owned());
        expected.record_telemetry_content = true;
        assert_same(&request, &expected);

        let cleared = request
            .temperature(None)
            .max_tokens(None)
            .model::<String>(None)
            .record_content_telemetry(false);
        let mut expected = bare(vec![Message::user("p")]);
        expected.tool_choice = Some(ToolChoice::Required);
        assert_same(&cleared, &expected);
    }

    #[test]
    fn output_schema_sets_and_none_clears() {
        let schema = schemars::json_schema!({"type": "object", "title": "Answer"});
        let request = CompletionRequest::new("p").output_schema(schema.clone());
        let mut expected = bare(vec![Message::user("p")]);
        expected.output_schema = Some(schema);
        assert_same(&request, &expected);
        assert_same(
            &request.output_schema(None),
            &bare(vec![Message::user("p")]),
        );
    }

    #[test]
    fn every_prompt_form_converts_to_the_same_request() {
        let expected = bare(vec![Message::user("hi")]);
        assert_same(&CompletionRequest::from("hi"), &expected);
        assert_same(&CompletionRequest::from("hi".to_owned()), &expected);
        assert_same(&CompletionRequest::from(Message::user("hi")), &expected);
        assert_same(
            &CompletionRequest::from(vec![Message::user("hi")]),
            &expected,
        );
    }

    fn response(choice: Vec<AssistantContent>) -> CompletionResponse {
        CompletionResponse::new(
            choice,
            Usage::default(),
            crate::codecs::message::Origin::new("test.api", "test", ""),
            serde_json::Value::Null,
        )
    }

    #[test]
    fn reasoning_concatenates_reasoning_text_and_summaries_in_order() {
        let response = response(vec![
            AssistantContent::Reasoning(Reasoning::new("first, ")),
            AssistantContent::text("answer"),
            AssistantContent::Reasoning(Reasoning::new("then done")),
            AssistantContent::Reasoning(Reasoning {
                redacted: true,
                ..Reasoning::default()
            }),
        ]);
        assert_eq!(response.reasoning(), "first, then done");
        assert_eq!(response.text(), "answer");
    }
}

#[cfg(test)]
mod response_identity_tests {
    use super::*;

    #[test]
    fn completion_response_without_request_id_deserializes() {
        let response: CompletionResponse = serde_json::from_str(
            r#"{"choice": [{"type": "text", "text": "hi"}],
                "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2,
                          "cached_input_tokens": 0, "cache_creation_input_tokens": 0,
                          "reasoning_tokens": 0},
                "origin": {"api": "test.api", "provider": "test", "model": ""},
                "raw": null}"#,
        )
        .expect("a CompletionResponse without a request id should load");
        assert_eq!(response.provider_request_id, None);
        assert_eq!(response.identity(), ResponseIdentity::default());
    }
}
