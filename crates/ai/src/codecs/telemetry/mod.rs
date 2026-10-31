use crate::codecs::completion::{AssistantContent, CompletionRequest, Message, Usage};
use crate::codecs::message::{
    DocumentData, DocumentSourceKind, Image, MimeType, Opaque, ToolResult, ToolResultContent,
    UserContent,
};
use base64::Engine;
use serde::Serialize;
use std::borrow::Borrow;
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use tracing::callsite::Identifier;

#[doc(hidden)]
pub use tracing as __tracing;

pub use tracing::field::Empty;

/// Whether this library was built with optional instrumentation enabled.
pub const ENABLED: bool = cfg!(feature = "telemetry");

#[doc(hidden)]
#[macro_export]
macro_rules! __swarmie_canonical_completion_span {
    (
        target: $target:literal,
        $(parent: $parent:expr,)?
        name: $name:literal,
        // Both blocks are spliced verbatim into `info_span!`: the header block
        // must end with a trailing comma, the extras block must begin with one.
        // Violating either surfaces as an `info_span!` parse error at the call
        // site, not here.
        { $($header:tt)* }
        { $($extra:tt)* }
    ) => {{
        if !$crate::codecs::telemetry::ENABLED {
            $crate::codecs::telemetry::__tracing::Span::none()
        } else {
            $crate::codecs::telemetry::__tracing::info_span!(
            target: $target,
            $(parent: $parent,)?
            $name,
            $($header)*
            gen_ai.request.stream = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.response.id = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.response.model = $crate::codecs::telemetry::__tracing::field::Empty,
            swarmie.provider_request_id = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.usage.input_tokens = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.usage.output_tokens = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.usage.cache_read.input_tokens = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.usage.cache_creation.input_tokens = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.usage.tool_use_prompt_tokens = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.usage.reasoning_tokens = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.input.messages = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.output.messages = $crate::codecs::telemetry::__tracing::field::Empty
            $($extra)*
            )
        }
    }};
}

macro_rules! new_completion_span {
    ($name:literal, $provider:expr, $request_model:expr, $operation:expr, $system:expr) => {
        $crate::__swarmie_canonical_completion_span!(
            target: "swarmie_ai::completions",
            name: $name,
            {
                gen_ai.operation.name = $operation,
                gen_ai.provider.name = $provider,
                gen_ai.request.model = $request_model,
                gen_ai.system_instructions = $system,
            }
            {}
        )
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenAiOperation {
    Chat,
    GenerateContent,
    Embeddings,
    Rerank,
    Transcription,
    ImageGeneration,
    AudioGeneration,
}

impl GenAiOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::GenerateContent => "generate_content",
            Self::Embeddings => "embeddings",
            Self::Rerank => "rerank",
            Self::Transcription => "transcription",
            Self::ImageGeneration => "image_generation",
            Self::AudioGeneration => "audio_generation",
        }
    }

    pub(crate) fn is_completion(self) -> bool {
        matches!(self, Self::Chat | Self::GenerateContent)
    }
}

pub const PROVIDER_REQUEST_ID_FIELD: &str = "swarmie.provider_request_id";

pub const COMPLETION_PARENT_MARKER_FIELD: &str = "swarmie.completion_parent";

pub const COMPLETION_PARENT_REQUIRED_FIELDS: &[&str] = &[
    "gen_ai.operation.name",
    "gen_ai.provider.name",
    "gen_ai.request.model",
    "gen_ai.system_instructions",
    "gen_ai.response.id",
    "gen_ai.response.model",
    "gen_ai.usage.input_tokens",
    "gen_ai.usage.output_tokens",
    "gen_ai.usage.cache_read.input_tokens",
    "gen_ai.usage.cache_creation.input_tokens",
    "gen_ai.usage.tool_use_prompt_tokens",
    "gen_ai.usage.reasoning_tokens",
    "gen_ai.input.messages",
    "gen_ai.output.messages",
];

#[macro_export]
macro_rules! completion_parent_span {
    (
        target: $target:literal,
        parent: $parent:expr,
        name: $name:literal,
        operation: $operation:expr,
        system_instructions: $system:expr
        $(, $($extra:tt)*)?
    ) => {
        $crate::__swarmie_canonical_completion_span!(
            target: $target,
            parent: $parent,
            name: $name,
            {
                swarmie.completion_parent = true,
                gen_ai.operation.name = $operation,
                gen_ai.system_instructions = $system,
                gen_ai.provider.name = $crate::codecs::telemetry::__tracing::field::Empty,
                gen_ai.request.model = $crate::codecs::telemetry::__tracing::field::Empty,
            }
            { $(, $($extra)*)? }
        )
    };
    // Default arm: delegates to the explicit-parent arm so the two cannot
    // drift in the fields they declare.
    (
        target: $target:literal,
        name: $name:literal,
        operation: $operation:expr,
        system_instructions: $system:expr
        $(, $($extra:tt)*)?
    ) => {
        $crate::completion_parent_span!(
            target: $target,
            parent: $crate::codecs::telemetry::__tracing::Span::current(),
            name: $name,
            operation: $operation,
            system_instructions: $system
            $(, $($extra)*)?
        )
    };
}

// `#[macro_export]` places the macro at the crate root; re-export it here so
// it is also reachable at its documented home alongside the contract
// constants it implements.
pub use crate::completion_parent_span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionParentVerdict {
    Adopt,
    RejectMissingFields,
    NotAParent,
}

fn missing_required_fields(metadata: &tracing::Metadata<'_>) -> Vec<&'static str> {
    let fields = metadata.fields();
    COMPLETION_PARENT_REQUIRED_FIELDS
        .iter()
        .copied()
        .filter(|name| fields.field(name).is_none())
        .collect()
}

fn classify_completion_parent(metadata: &tracing::Metadata<'_>) -> CompletionParentVerdict {
    let fields = metadata.fields();
    // Exact match, never a prefix: a runtime field that merely starts with the
    // marker name (`swarmie.completion_parent.id`, say) is not the marker and must
    // not make its span a rejected parent.
    if fields.field(COMPLETION_PARENT_MARKER_FIELD).is_none() {
        return CompletionParentVerdict::NotAParent;
    }
    if COMPLETION_PARENT_REQUIRED_FIELDS
        .iter()
        .all(|name| fields.field(name).is_some())
    {
        CompletionParentVerdict::Adopt
    } else {
        CompletionParentVerdict::RejectMissingFields
    }
}

static NEAR_MISS_WARNED: LazyLock<Mutex<HashSet<Identifier>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

fn warn_once_on_completion_parent_verdict(
    verdict: CompletionParentVerdict,
    metadata: &tracing::Metadata<'_>,
) {
    match verdict {
        CompletionParentVerdict::Adopt | CompletionParentVerdict::NotAParent => {}
        CompletionParentVerdict::RejectMissingFields => {
            let first_sighting = {
                let mut warned = NEAR_MISS_WARNED
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                warned.insert(metadata.callsite())
            };
            // Release the lock before invoking subscribers, which may re-enter
            // the builder and deadlock if the guard is still held.
            if !first_sighting {
                return;
            }
            tracing::warn!(
                marker = COMPLETION_PARENT_MARKER_FIELD,
                missing_fields = ?missing_required_fields(metadata),
                "completion-parent span declares the marker but not every required field \
                 and is not adopted; provider telemetry lands on a fresh child span \
                 instead — declare the span with \
                 `swarmie_ai::codecs::telemetry::completion_parent_span!`"
            );
        }
    }
}

macro_rules! new_modality_span {
    ($name:literal, $provider:expr, $request_model:expr, $operation:expr) => {
        $crate::codecs::telemetry::__tracing::info_span!(
            target: "swarmie_ai::modalities",
            $name,
            gen_ai.operation.name = $operation,
            gen_ai.provider.name = $provider,
            gen_ai.request.model = $request_model,
            gen_ai.response.id = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.response.model = $crate::codecs::telemetry::__tracing::field::Empty,
            swarmie.provider_request_id = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.usage.input_tokens = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.usage.output_tokens = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.usage.cache_read.input_tokens = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.usage.cache_creation.input_tokens = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.usage.tool_use_prompt_tokens = $crate::codecs::telemetry::__tracing::field::Empty,
            gen_ai.usage.reasoning_tokens = $crate::codecs::telemetry::__tracing::field::Empty,
        )
    };
}

pub struct SpanBuilder<'a> {
    provider: &'a str,
    request_model: &'a str,
    operation: GenAiOperation,
    system_instructions: Option<String>,
    streaming: Option<bool>,
}

impl<'a> SpanBuilder<'a> {
    pub fn new(provider: &'a str, request_model: &'a str, operation: GenAiOperation) -> Self {
        Self {
            provider,
            request_model,
            operation,
            system_instructions: None,
            streaming: None,
        }
    }

    pub fn streaming(mut self, streaming: bool) -> Self {
        self.streaming = Some(streaming);
        self
    }

    pub fn system_instructions(
        mut self,
        system_instructions: Option<&'a str>,
        record_content: bool,
    ) -> Self {
        self.system_instructions = system_instructions_json(system_instructions, record_content);
        self
    }

    pub fn build(self) -> tracing::Span {
        if !cfg!(feature = "telemetry") {
            return tracing::Span::none();
        }
        if self.operation.is_completion()
            && let Some(parent) = self.adopt_completion_parent()
        {
            return parent;
        }

        let (provider, model) = (self.provider, self.request_model);
        let op = self.operation.as_str();
        let sys = self.system_instructions.as_deref();
        let span = match self.operation {
            GenAiOperation::Chat => new_completion_span!("chat", provider, model, op, sys),
            GenAiOperation::GenerateContent => {
                new_completion_span!("generate_content", provider, model, op, sys)
            }
            GenAiOperation::Embeddings => new_modality_span!("embeddings", provider, model, op),
            GenAiOperation::Rerank => new_modality_span!("rerank", provider, model, op),
            GenAiOperation::Transcription => {
                new_modality_span!("transcription", provider, model, op)
            }
            GenAiOperation::ImageGeneration => {
                new_modality_span!("image_generation", provider, model, op)
            }
            GenAiOperation::AudioGeneration => {
                new_modality_span!("audio_generation", provider, model, op)
            }
        };
        if self.operation.is_completion() {
            self.record_streaming(&span);
        }
        span
    }

    fn record_streaming(&self, span: &tracing::Span) {
        if let Some(streaming) = self.streaming {
            span.record("gen_ai.request.stream", streaming);
        }
    }

    fn adopt_completion_parent(&self) -> Option<tracing::Span> {
        let current = tracing::Span::current();
        let metadata = current.metadata()?;
        let verdict = classify_completion_parent(metadata);
        warn_once_on_completion_parent_verdict(verdict, metadata);
        if verdict != CompletionParentVerdict::Adopt {
            return None;
        }
        current.record("gen_ai.operation.name", self.operation.as_str());
        current.record("gen_ai.provider.name", self.provider);
        current.record("gen_ai.request.model", self.request_model);
        self.record_streaming(&current);
        if let Some(system_instructions) = self.system_instructions.as_deref() {
            current.record("gen_ai.system_instructions", system_instructions);
        }
        Some(current)
    }
}

#[derive(Serialize)]
struct TelemetryChatMessage {
    role: &'static str,
    parts: Vec<TelemetryPart>,
}

#[derive(Serialize)]
struct TelemetryOutputMessage {
    role: &'static str,
    parts: Vec<TelemetryPart>,
    finish_reason: &'static str,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TelemetryPart {
    Text {
        content: String,
    },
    ToolCall {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        name: String,
        arguments: serde_json::Value,
    },
    ToolCallResponse {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        response: serde_json::Value,
    },
    Reasoning {
        content: String,
    },
    Opaque {
        kind: String,
    },
    Uri {
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        modality: &'static str,
        uri: String,
    },
    File {
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        modality: &'static str,
        file_id: String,
    },
    Blob {
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        modality: &'static str,
        content: String,
    },
}

fn media_part<T>(
    data: &DocumentSourceKind,
    media_type: Option<&T>,
    modality: &'static str,
) -> Option<TelemetryPart>
where
    T: MimeType,
{
    let mime_type = media_type.map(|media_type| media_type.to_mime_type().to_string());
    match data {
        DocumentSourceKind::Url(uri) => Some(TelemetryPart::Uri {
            mime_type,
            modality,
            uri: uri.clone(),
        }),
        DocumentSourceKind::FileId(file_id) => Some(TelemetryPart::File {
            mime_type,
            modality,
            file_id: file_id.clone(),
        }),
        DocumentSourceKind::Base64(content) => Some(TelemetryPart::Blob {
            mime_type,
            modality,
            content: content.clone(),
        }),
        DocumentSourceKind::Raw(content) => Some(TelemetryPart::Blob {
            mime_type,
            modality,
            content: base64::engine::general_purpose::STANDARD.encode(content),
        }),
        DocumentSourceKind::Unknown => None,
    }
}

fn image_part(image: &Image) -> Option<TelemetryPart> {
    media_part(&image.data, image.media_type.as_ref(), "image")
}

fn opaque_part(opaque: &Opaque) -> TelemetryPart {
    TelemetryPart::Opaque {
        kind: opaque.kind().unwrap_or("unknown").to_owned(),
    }
}

fn tool_result_response(result: &ToolResult) -> serde_json::Value {
    let mut content = result
        .content
        .iter()
        .filter_map(|content| match content {
            ToolResultContent::Text(text) => Some(serde_json::Value::String(text.text.clone())),
            ToolResultContent::Json { value } => Some(value.clone()),
            ToolResultContent::Image(image) => {
                image_part(image).and_then(|part| serde_json::to_value(part).ok())
            }
        })
        .collect::<Vec<_>>();

    if content.len() == 1 {
        content.pop().unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Array(content)
    }
}

fn user_parts(content: &[UserContent]) -> Vec<TelemetryPart> {
    content
        .iter()
        .filter_map(|content| match content {
            UserContent::Text(text) => Some(TelemetryPart::Text {
                content: text.text.clone(),
            }),
            UserContent::ToolResult(result) => Some(TelemetryPart::ToolCallResponse {
                id: Some(result.call.to_string()),
                response: tool_result_response(result),
            }),
            UserContent::Image(image) => image_part(image),
            UserContent::Audio(audio) => {
                media_part(&audio.data, audio.media_type.as_ref(), "audio")
            }
            UserContent::Video(video) => {
                media_part(&video.data, video.media_type.as_ref(), "video")
            }
            UserContent::Document(document) => match &document.data {
                DocumentData::Text(content) => Some(TelemetryPart::Text {
                    content: content.clone(),
                }),
                DocumentData::File(data) => {
                    media_part(data, document.media_type.as_ref(), "document")
                }
            },
        })
        .collect()
}

fn assistant_parts(content: &[AssistantContent]) -> Vec<TelemetryPart> {
    content
        .iter()
        .flat_map(|content| match content {
            AssistantContent::Text(text) => vec![TelemetryPart::Text {
                content: text.text.clone(),
            }],
            AssistantContent::ToolCall(tool_call) => vec![TelemetryPart::ToolCall {
                id: Some(tool_call.id.to_string()),
                name: tool_call.function.name.clone().into(),
                arguments: tool_call.function.arguments_value(),
            }],
            AssistantContent::Reasoning(reasoning) => vec![TelemetryPart::Reasoning {
                content: reasoning.text.clone(),
            }],
            AssistantContent::Image(image) => image_part(image).into_iter().collect(),
            AssistantContent::Opaque(opaque) => vec![opaque_part(opaque)],
        })
        .collect()
}

fn input_messages(messages: &[impl Borrow<Message>]) -> Vec<TelemetryChatMessage> {
    messages
        .iter()
        .map(|message| match message.borrow() {
            Message::System { content } => TelemetryChatMessage {
                role: "system",
                parts: vec![TelemetryPart::Text {
                    content: content.clone(),
                }],
            },
            Message::User { content } => TelemetryChatMessage {
                role: "user",
                parts: user_parts(content),
            },
            Message::Assistant(turn) => TelemetryChatMessage {
                role: "assistant",
                parts: assistant_parts(&turn.content),
            },
        })
        .collect()
}

fn output_messages(content: &[AssistantContent]) -> Vec<TelemetryOutputMessage> {
    let finish_reason = if content
        .iter()
        .any(|content| matches!(content, AssistantContent::ToolCall(_)))
    {
        "tool_call"
    } else {
        // the provider library's normalized assistant content does not retain provider finish
        // reasons such as length or content filtering. Avoid claiming a clean
        // stop when the actual reason is unavailable.
        "unknown"
    };
    vec![TelemetryOutputMessage {
        role: "assistant",
        parts: assistant_parts(content),
        finish_reason,
    }]
}

pub fn system_instructions_json(instructions: Option<&str>, enabled: bool) -> Option<String> {
    if !enabled {
        return None;
    }

    instructions.and_then(|instructions| {
        serde_json::to_string(&vec![TelemetryPart::Text {
            content: instructions.to_string(),
        }])
        .ok()
    })
}

pub fn record_model_input(span: &tracing::Span, messages: &[impl Borrow<Message>], enabled: bool) {
    if !enabled || span.is_disabled() {
        return;
    }

    if let Ok(messages) = serde_json::to_string(&input_messages(messages)) {
        span.record("gen_ai.input.messages", messages);
    }
}

pub fn record_model_output(span: &tracing::Span, content: &[AssistantContent], enabled: bool) {
    if !enabled || span.is_disabled() {
        return;
    }

    let messages = output_messages(content);
    if let Ok(messages) = serde_json::to_string(&messages) {
        span.record("gen_ai.output.messages", messages);
    }
}

pub fn record_tool_definitions(span: &tracing::Span, request: &CompletionRequest, enabled: bool) {
    if !enabled || span.is_disabled() {
        return;
    }

    let functions = request.tools.iter().map(|tool| {
        serde_json::json!({
            "type": "function",
            "name": tool.name.as_str(),
            "description": tool.description,
            "parameters": tool.parameters,
        })
    });
    let hosted = crate::codecs::completion::history::raw_tools(request)
        .iter()
        .filter_map(serde_json::Value::as_object)
        .flat_map(|tool| {
            let kinds: Vec<&str> = match (tool.get("type"), tool.get("name")) {
                (Some(kind), _) => kind.as_str().into_iter().collect(),
                (None, Some(_)) => vec!["function"],
                (None, None) => tool.keys().map(String::as_str).collect(),
            };
            kinds.into_iter().map(move |kind| {
                let name = [
                    tool.get("name"),
                    tool.get(kind).and_then(|body| body.get("name")),
                ]
                .into_iter()
                .find_map(|name| name?.as_str());
                serde_json::json!({ "type": kind, "name": name.unwrap_or(kind) })
            })
        });
    let definitions: Vec<_> = functions.chain(hosted).collect();
    if definitions.is_empty() {
        return;
    }
    if let Ok(definitions) = serde_json::to_string(&definitions) {
        span.record("gen_ai.tool.definitions", definitions);
    }
}

pub trait SpanCombinator {
    fn record_token_usage(&self, usage: &Usage);

    fn record_response(&self, response_id: Option<&str>, model: Option<&str>, usage: &Usage);
}

impl SpanCombinator for tracing::Span {
    fn record_token_usage(&self, usage: &Usage) {
        if self.is_disabled() {
            return;
        }

        // A counter the provider did not report leaves its span field unset;
        // a reported zero is recorded as zero.
        let fields = [
            ("gen_ai.usage.input_tokens", usage.input_tokens),
            ("gen_ai.usage.output_tokens", usage.output_tokens),
            (
                "gen_ai.usage.cache_read.input_tokens",
                usage.cached_input_tokens,
            ),
            (
                "gen_ai.usage.cache_creation.input_tokens",
                usage.cache_creation_input_tokens,
            ),
            (
                "gen_ai.usage.tool_use_prompt_tokens",
                usage.tool_use_prompt_tokens,
            ),
            ("gen_ai.usage.reasoning_tokens", usage.reasoning_tokens),
        ];
        for (field, value) in fields {
            if let Some(value) = value {
                self.record(field, value);
            }
        }
    }

    fn record_response(&self, response_id: Option<&str>, model: Option<&str>, usage: &Usage) {
        if self.is_disabled() {
            return;
        }
        if let Some(id) = response_id {
            self.record("gen_ai.response.id", id);
        }
        if let Some(model) = model {
            self.record("gen_ai.response.model", model);
        }
        self.record_token_usage(usage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<String>>>);

    impl tracing::field::Visit for Capture {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.lock().unwrap().push(format!("{field}={value:?}"));
        }
    }

    impl tracing::Subscriber for Capture {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            attributes.record(&mut self.clone());
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _: &tracing::span::Id, values: &tracing::span::Record<'_>) {
            values.record(&mut self.clone());
        }

        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            event.record(&mut self.clone());
        }

        fn enter(&self, _: &tracing::span::Id) {}

        fn exit(&self, _: &tracing::span::Id) {}
    }

    #[test]
    fn instrumentation_and_content_have_separate_gates_and_raw_json_is_never_traced() {
        let capture = Capture::default();
        tracing::subscriber::with_default(capture.clone(), || {
            let span = SpanBuilder::new("fixture", "fixture-model", GenAiOperation::Chat)
                .system_instructions(Some("private-system"), false)
                .build();
            let request = CompletionRequest::new("private-user");
            record_model_input(&span, &request.chat_history, false);
            crate::codecs::providers::internal::trace_json(
                crate::codecs::providers::internal::LogTarget::Completions,
                "raw",
                &serde_json::json!({"credential":"private-raw"}),
            );
            let records = capture.0.lock().unwrap().join("\n");
            assert!(!records.contains("private-"));
            assert_eq!(span.is_disabled(), !ENABLED);
            drop(records);
            record_model_input(&span, &request.chat_history, true);
            let records = capture.0.lock().unwrap().join("\n");
            assert_eq!(records.contains("private-user"), ENABLED);
            assert!(!records.contains("private-system"));
            assert!(!records.contains("private-raw"));
        });
    }
}
