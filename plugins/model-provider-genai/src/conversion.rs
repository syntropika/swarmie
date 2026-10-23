mod fidelity;
use crate::{Connection, Limits, backend::ClientConnection, error::failure};
pub(crate) use fidelity::StreamCapture;
use fidelity::{
    prepare_replay, provider_content, provider_reasoning, replay_extension, replay_text,
    validate_provider_content,
};
use genai::chat::{self as g, ChatOptions, ChatRequest};
use std::collections::{BTreeMap, BTreeSet};
use swarmie_model_contracts::*;

type Checked<T> = std::result::Result<T, ProviderError>;
fn invalid() -> ProviderError {
    failure(ProviderErrorKind::InvalidRequest, Submission::NotSent)
}
fn unsupported() -> ProviderError {
    failure(ProviderErrorKind::Unsupported, Submission::NotSent)
}
fn parse_error() -> ProviderError {
    failure(ProviderErrorKind::Parsing, Submission::ResponseReceived)
}

pub(crate) fn bytes<T: serde::Serialize>(value: &T) -> usize {
    tembra::contracts::to_portable_value(value)
        .ok()
        .and_then(|v| tembra::contracts::canonical_json(&v).ok())
        .map_or(usize::MAX, |v| v.len())
}

fn namespace(c: &Connection) -> String {
    format!("swarmie.continuation.{}", c.provider)
}
fn extension(c: &Connection, kind: &str, value: serde_json::Value) -> Extension {
    Extension {
        namespace: namespace(c),
        version: 1,
        value: serde_json::json!({"kind":kind,"value":value}),
    }
}

pub(crate) fn request(
    request: &ModelRequest,
    c: &ClientConnection,
    limits: &Limits,
) -> Checked<(ChatRequest, ChatOptions)> {
    if bytes(request) > limits.request_bytes {
        return Err(failure(
            ProviderErrorKind::PayloadLimit,
            Submission::NotSent,
        ));
    }
    if request.request_id.is_empty()
        || request.request_id.len() > 256
        || request.messages.is_empty()
    {
        return Err(invalid());
    }
    let capabilities = c.capabilities();
    if !request.tools.is_empty() && capabilities.tools == Support::Unsupported {
        return Err(unsupported());
    }
    let mut tools = BTreeSet::new();
    for t in &request.tools {
        if t.name.is_empty()
            || !tools.insert(&t.name)
            || t.parameters.get("type").and_then(|v| v.as_str()) != Some("object")
        {
            return Err(invalid());
        }
    }
    let mut pending = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut messages = Vec::new();
    for message in &request.messages {
        if message.role == Role::Assistant && pending.is_empty() {
            seen.clear();
        }
        let role = match message.role {
            Role::System => g::ChatRole::System,
            Role::Developer
                if matches!(
                    c.kind,
                    genai::adapter::AdapterKind::OpenAI | genai::adapter::AdapterKind::OpenAIResp
                ) =>
            {
                g::ChatRole::Developer
            }
            Role::Developer => return Err(unsupported()),
            Role::User => g::ChatRole::User,
            Role::Assistant => g::ChatRole::Assistant,
            Role::Tool => g::ChatRole::Tool,
        };
        let mut parts = Vec::new();
        let mut content = message.content.iter().peekable();
        while let Some(part) = content.next() {
            // Each replay block stays beside the visible reasoning it represents. Removing
            // visible reasoning for persistence does not move its continuation position.
            if let ContentPart::Reasoning { text } = part
                && let Some(ContentPart::Extension { extension: ext }) = content.peek()
                && ext.namespace == namespace(&c.configuration)
                && ext.version == 2
                && matches!(
                    ext.value["kind"].as_str(),
                    Some("reasoning_replay" | "provider_content")
                )
            {
                if replay_text(c.kind, ext)?.as_deref() != Some(text) {
                    return Err(invalid());
                }
                continue;
            }
            if let ContentPart::Extension { extension: ext } = part
                && ext.namespace == namespace(&c.configuration)
                && ext.version == 2
            {
                if message.role != Role::Assistant {
                    return Err(invalid());
                }
                parts.push(match ext.value["kind"].as_str() {
                    Some("reasoning_replay") => g::ContentPart::ReasoningContent(
                        ext.value["value"].as_str().ok_or_else(invalid)?.to_owned(),
                    ),
                    Some("provider_content") => {
                        validate_provider_content(c.kind, &ext.value["value"])
                            .map_err(|_| unsupported())?;
                        g::ContentPart::from_custom(ext.value["value"].clone(), None)
                    }
                    _ => return Err(unsupported()),
                });
                continue;
            }
            parts.push(match part {
                ContentPart::Text { text } if message.role != Role::Tool => {
                    g::ContentPart::Text(text.clone())
                }
                ContentPart::Reasoning { text } if message.role == Role::Assistant => {
                    g::ContentPart::ReasoningContent(text.clone())
                }
                ContentPart::Image { media_type, source } if message.role == Role::User => {
                    if capabilities.images == Support::Unsupported
                        || !media_type.starts_with("image/")
                    {
                        return Err(unsupported());
                    }
                    match source {
                        MediaSource::Url { url } => {
                            g::ContentPart::from_binary_url(media_type, url, None)
                        }
                        MediaSource::Base64 { data } => {
                            g::ContentPart::from_binary_base64(media_type, data.as_str(), None)
                        }
                    }
                }
                ContentPart::ToolCall { call } if message.role == Role::Assistant => {
                    if call.id.is_empty()
                        || call.name.is_empty()
                        || !call.arguments.is_object()
                        || !seen.insert(call.id.clone())
                    {
                        return Err(invalid());
                    }
                    pending.insert(call.id.clone(), call.name.clone());
                    let mut signatures = None;
                    for ext in &call.extensions {
                        if ext.namespace != namespace(&c.configuration)
                            || ext.version != 1
                            || ext.value["kind"] != "tool_thought_signatures"
                        {
                            return Err(unsupported());
                        }
                        if signatures.is_some() {
                            return Err(invalid());
                        }
                        signatures = Some(
                            serde_json::from_value(ext.value["value"].clone())
                                .map_err(|_| invalid())?,
                        );
                    }
                    g::ContentPart::ToolCall(g::ToolCall {
                        call_id: call.id.clone(),
                        fn_name: call.name.clone(),
                        fn_arguments: call.arguments.clone(),
                        thought_signatures: signatures,
                    })
                }
                ContentPart::ToolResult {
                    call_id,
                    value,
                    is_error,
                } if message.role == Role::Tool => {
                    let name = pending.remove(call_id).ok_or_else(invalid)?;
                    g::ContentPart::ToolResponse(
                        g::ToolResponse::new(
                            call_id,
                            serde_json::to_string(&if *is_error {
                                serde_json::json!({"is_error":true,"value":value})
                            } else {
                                value.clone()
                            })
                            .map_err(|_| invalid())?,
                        )
                        .with_fn_name(name),
                    )
                }
                ContentPart::Extension { extension: ext } if message.role == Role::Assistant => {
                    if ext.namespace != namespace(&c.configuration) || ext.version != 1 {
                        return Err(unsupported());
                    }
                    match ext.value["kind"].as_str() {
                        Some("thought_signature") => g::ContentPart::ThoughtSignature(
                            ext.value["value"].as_str().ok_or_else(invalid)?.into(),
                        ),
                        _ => return Err(unsupported()),
                    }
                }
                _ => return Err(invalid()),
            });
        }
        if parts.is_empty() {
            return Err(invalid());
        }
        if matches!(message.role, Role::System | Role::Developer)
            && parts.iter().any(|p| !matches!(p, g::ContentPart::Text(_)))
        {
            return Err(invalid());
        }
        if message.role == Role::Assistant {
            parts = prepare_replay(c.kind, &request.model.model, parts)?;
        }
        messages.push(g::ChatMessage::new(
            role,
            g::MessageContent::from_parts(parts),
        ));
    }
    if !pending.is_empty() {
        return Err(invalid());
    }
    let o = &request.options;
    let defaults = &c.configuration.defaults;
    let mut options = ChatOptions::default()
        .with_capture_raw_body(true)
        .with_capture_content(true)
        .with_capture_usage(true)
        .with_capture_reasoning_content(true)
        .with_capture_tool_calls(true)
        .with_stream_header_timeout(std::time::Duration::from_millis(c.configuration.timeout_ms))
        .with_stream_read_timeout(std::time::Duration::from_millis(c.configuration.timeout_ms));
    if let Some(v) = o.temperature.or(defaults.temperature) {
        if !v.is_finite() || !(0.0..=2.0).contains(&v) {
            return Err(invalid());
        }
        options.temperature = Some(v);
    }
    if let Some(v) = o.top_p.or(defaults.top_p) {
        if !v.is_finite() || !(0.0..=1.0).contains(&v) {
            return Err(invalid());
        }
        options.top_p = Some(v);
    }
    options.max_tokens = o.max_output_tokens.or(defaults.max_output_tokens);
    if options.max_tokens == Some(0) {
        return Err(invalid());
    }
    if let Some(v) = o
        .reasoning_effort
        .as_ref()
        .or(defaults.reasoning_effort.as_ref())
    {
        if capabilities.reasoning == Support::Unsupported {
            return Err(unsupported());
        }
        options.reasoning_effort = Some(g::ReasoningEffort::from_keyword(v).ok_or_else(invalid)?);
    }
    if !request.tools.is_empty()
        && o.response_format
            .as_ref()
            .or(defaults.response_format.as_ref())
            .is_some()
        && !matches!(
            c.kind,
            genai::adapter::AdapterKind::OpenAI
                | genai::adapter::AdapterKind::OpenAIResp
                | genai::adapter::AdapterKind::Anthropic
        )
    {
        // The current Gemini generateContent path does not implement Interactions' tool/JSON
        // combination; Ollama and other adapters have no qualified combination policy.
        return Err(unsupported());
    }
    if let Some(format) = o
        .response_format
        .as_ref()
        .or(defaults.response_format.as_ref())
    {
        options.response_format = Some(match format {
            ResponseFormat::Json
                if capabilities.json != Support::Unsupported
                    && !matches!(
                        c.kind,
                        genai::adapter::AdapterKind::Anthropic
                            | genai::adapter::AdapterKind::Gemini
                    ) =>
            {
                g::ChatResponseFormat::JsonMode
            }
            ResponseFormat::JsonSchema { name, schema }
                if capabilities.json_schema != Support::Unsupported =>
            {
                if name.is_empty() || !schema.is_object() {
                    return Err(invalid());
                }
                g::ChatResponseFormat::JsonSpec(g::JsonSpec::new(name, schema.clone()))
            }
            _ => return Err(unsupported()),
        });
    }
    if matches!(
        c.kind,
        genai::adapter::AdapterKind::OpenAI | genai::adapter::AdapterKind::OpenAIResp
    ) && !request.tools.is_empty()
    {
        options = options.with_extra_body(serde_json::json!({"parallel_tool_calls":false}));
    }
    let tools = request
        .tools
        .iter()
        .map(|t| {
            g::Tool::new(t.name.clone())
                .with_description(&t.description)
                .with_schema(t.parameters.clone())
        })
        .collect::<Vec<_>>();
    Ok((ChatRequest::new(messages).with_tools(tools), options))
}

pub(crate) struct Completion {
    pub content: g::MessageContent,
    pub raw_body: Option<serde_json::Value>,
    pub reasoning: Option<String>,
    pub stop: Option<g::StopReason>,
    pub usage: g::Usage,
    pub response_id: Option<String>,
    pub raw_usage: Option<serde_json::Value>,
    pub reported_model: Option<String>,
}

impl From<g::ChatResponse> for Completion {
    fn from(response: g::ChatResponse) -> Self {
        let raw_usage = response
            .captured_raw_body
            .as_ref()
            .and_then(|v| v.get("usage").or_else(|| v.get("usageMetadata")))
            .cloned();
        let response_id = response.response_id.or_else(|| {
            response
                .captured_raw_body
                .as_ref()
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        });
        Self {
            content: response.content,
            raw_body: response.captured_raw_body,
            reasoning: response.reasoning_content,
            stop: response.stop_reason,
            usage: response.usage,
            response_id,
            raw_usage,
            reported_model: Some(response.provider_model_iden.model_name.to_string()),
        }
    }
}

pub(crate) fn response(
    request: &ModelRequest,
    c: &ClientConnection,
    completion: Completion,
    limits: &Limits,
) -> Checked<ModelResponse> {
    let Completion {
        mut content,
        raw_body,
        reasoning,
        stop,
        usage,
        response_id,
        raw_usage,
        reported_model,
    } = completion;
    if let Some(raw) = raw_body {
        if bytes(&raw) > limits.response_bytes {
            return Err(failure(
                ProviderErrorKind::PayloadLimit,
                Submission::ResponseReceived,
            ));
        }
        if let Some(native) = provider_content(c.kind, &raw)? {
            content = native;
        }
        if raw
            .pointer("/choices/0/message/reasoning_details")
            .is_some()
        {
            return Err(failure(
                ProviderErrorKind::DataLoss,
                Submission::ResponseReceived,
            ));
        }
    }
    let mut parts = Vec::new();
    if content.parts().is_empty() && reasoning.is_none() && stop.is_none() {
        return Err(parse_error());
    }
    if let Some(text) = reasoning.filter(|text| !text.is_empty())
        && !content
            .parts()
            .iter()
            .any(|p| matches!(p, g::ContentPart::ReasoningContent(_)) || matches!(p, g::ContentPart::Custom(custom) if provider_reasoning(c.kind, &custom.data).is_some()))
    {
        parts.push(ContentPart::Reasoning { text: text.clone() });
        parts.push(ContentPart::Extension { extension: replay_extension(&c.configuration, "reasoning_replay", text.into()) });
    }
    let mut ids = BTreeSet::new();
    for part in content.into_parts() {
        match part {
            g::ContentPart::ReasoningContent(text) => {
                parts.push(ContentPart::Reasoning { text: text.clone() });
                parts.push(ContentPart::Extension {
                    extension: replay_extension(&c.configuration, "reasoning_replay", text.into()),
                });
            }
            g::ContentPart::Custom(custom) => {
                validate_provider_content(c.kind, &custom.data)?;
                if let Some(text) =
                    provider_reasoning(c.kind, &custom.data).filter(|text| !text.is_empty())
                {
                    parts.push(ContentPart::Reasoning { text });
                }
                parts.push(ContentPart::Extension {
                    extension: replay_extension(&c.configuration, "provider_content", custom.data),
                });
            }
            other => parts.push(match other {
                g::ContentPart::Text(text) => ContentPart::Text { text },

                g::ContentPart::ThoughtSignature(s) => ContentPart::Extension {
                    extension: extension(&c.configuration, "thought_signature", s.into()),
                },

                g::ContentPart::ToolCall(call) => {
                    if call.call_id.is_empty()
                        || !ids.insert(call.call_id.clone())
                        || call.fn_name.is_empty()
                        || !call.fn_arguments.is_object()
                    {
                        return Err(parse_error());
                    }
                    let extensions = call
                        .thought_signatures
                        .map(|s| {
                            vec![extension(
                                &c.configuration,
                                "tool_thought_signatures",
                                serde_json::json!(s),
                            )]
                        })
                        .unwrap_or_default();
                    ContentPart::ToolCall {
                        call: ToolCall {
                            id: call.call_id,
                            name: call.fn_name,
                            arguments: call.fn_arguments,
                            extensions,
                        },
                    }
                }
                _ => {
                    return Err(failure(
                        ProviderErrorKind::DataLoss,
                        Submission::ResponseReceived,
                    ));
                }
            }),
        }
    }
    let stop = match stop {
        Some(g::StopReason::Completed(raw))
            if matches!(
                c.kind,
                genai::adapter::AdapterKind::OpenAIResp | genai::adapter::AdapterKind::Gemini
            ) && parts
                .iter()
                .any(|p| matches!(p, ContentPart::ToolCall { .. })) =>
        {
            Some(g::StopReason::ToolCall(raw))
        }
        stop => stop,
    };
    if request
        .options
        .response_format
        .as_ref()
        .or(c.configuration.defaults.response_format.as_ref())
        .is_some()
    {
        let text = parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>();
        let tool_branch = matches!(stop, Some(g::StopReason::ToolCall(_)))
            && parts
                .iter()
                .any(|p| matches!(p, ContentPart::ToolCall { .. }));
        let unfinished = matches!(
            stop,
            Some(g::StopReason::MaxTokens(_) | g::StopReason::ContentFilter(_))
        ) || matches!(&stop, Some(g::StopReason::Other(raw)) if raw == "refusal");
        if !tool_branch && !unfinished {
            serde_json::from_str::<serde_json::Value>(&text).map_err(|_| parse_error())?;
        }
    }
    let finish_reason = match &stop {
        Some(g::StopReason::Completed(_)) => FinishReason::Stop,
        Some(g::StopReason::MaxTokens(_)) => FinishReason::Length,
        Some(g::StopReason::ToolCall(_)) => FinishReason::ToolCalls,
        Some(g::StopReason::ContentFilter(_)) => FinishReason::ContentFilter,
        Some(g::StopReason::StopSequence(_)) => FinishReason::StopSequence,
        Some(g::StopReason::Other(_)) => FinishReason::Other,
        None => FinishReason::Unknown,
    };
    let n = |v: Option<i32>| v.and_then(|n| u64::try_from(n).ok());
    let mut extensions = vec![Extension {
        namespace: format!("swarmie.usage.{}", c.configuration.provider),
        version: 1,
        value: serde_json::to_value(&usage).map_err(|_| parse_error())?,
    }];
    if let Some(value) = raw_usage {
        extensions.push(Extension {
            namespace: format!("swarmie.provider_usage.{}", c.configuration.provider),
            version: 1,
            value,
        });
    }
    if let Some(model) = reported_model {
        extensions.push(Extension {
            namespace: "swarmie.reported-model".into(),
            version: 1,
            value: model.into(),
        });
    }
    let response = ModelResponse {
        request_id: request.request_id.clone(),
        model: request.model.clone(),
        message: Message {
            role: Role::Assistant,
            content: parts,
        },
        finish_reason,
        raw_finish_reason: stop.map(|s| s.raw().to_owned()),
        provider_response_id: response_id,
        usage: TokenUsage {
            input_tokens: n(usage.prompt_tokens),
            output_tokens: n(usage.completion_tokens),
            total_tokens: n(usage.total_tokens),
            cached_input_tokens: n(usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens)),
            reasoning_tokens: n(usage
                .completion_tokens_details
                .as_ref()
                .and_then(|d| d.reasoning_tokens)),
        },
        extensions,
    };
    if bytes(&response) > limits.response_bytes {
        return Err(failure(
            ProviderErrorKind::PayloadLimit,
            Submission::ResponseReceived,
        ));
    }
    Ok(response)
}
