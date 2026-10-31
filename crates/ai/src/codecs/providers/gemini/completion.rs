pub const GEMINI_3_8_FLASH: &str = "gemini-3.8-flash";
pub const GEMINI_3_1_FLASH_LITE_PREVIEW: &str = "gemini-3.1-flash-lite-preview";
pub const GEMINI_3_FLASH_PREVIEW: &str = "gemini-3-flash-preview";
pub const GEMINI_2_5_PRO_PREVIEW_06_05: &str = "gemini-2.5-pro-preview-06-05";
pub const GEMINI_2_5_PRO_EXP_03_25: &str = "gemini-2.5-pro-exp-03-25";
pub const GEMINI_2_5_FLASH: &str = "gemini-2.5-flash";
pub const GEMINI_2_0_FLASH: &str = "gemini-2.0-flash";

use serde_json::{Map, Value, json};

#[doc(hidden)]
pub use super::options::{Route, generate_content_options};
use crate::codecs::completion::options::{BaseInput, FinalBody, RawAt, Rewrite, request_params};
use crate::codecs::completion::{Accepts, CompletionRequest, Media, Place, Replay, ReplayTarget};
use crate::codecs::error::{EncodeError, ProviderError};
use crate::codecs::json_utils::Lenient;
use crate::codecs::json_utils::remove_preserving_order;
use crate::codecs::message::{
    AssistantContent, DocumentData, DocumentMediaType, DocumentSourceKind as Source, Message,
    MimeType, ToolChoice, ToolResultContent, UserContent,
};
use crate::codecs::operation::Completion;
use crate::codecs::providers::internal::set_fields;
use crate::codecs::providers::internal::wire_ids::WireIds;
use crate::codecs::telemetry::GenAiOperation;
use crate::codecs::wire::{Descriptor, Encoded, Framing, Mode, Wire};

pub const PROVIDER_NAME: &str = "gcp.gemini";

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GenerateContent {
    pub provider: super::GeminiConfig,
    pub model: String,
    pub cached_content: Option<String>,
    #[serde(default)]
    pub thought_replay: ThoughtReplay,
    #[serde(skip)]
    pub facts: crate::codecs::catalog::ModelFacts,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ThoughtReplay {
    #[default]
    All,
    CurrentTurn,
}

impl GenerateContent {
    pub fn new(provider: super::GeminiConfig, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
            cached_content: None,
            thought_replay: ThoughtReplay::All,
            facts: crate::codecs::catalog::ModelFacts::default(),
        }
    }

    pub fn with_facts(mut self, facts: crate::codecs::catalog::ModelFacts) -> Self {
        self.facts = facts;
        self
    }

    pub fn thought_replay(mut self, replay: ThoughtReplay) -> Self {
        self.thought_replay = replay;
        self
    }

    pub fn with_cached_content(mut self, name: impl Into<String>) -> Self {
        self.cached_content = Some(name.into());
        self
    }
}

fn drop_finished_signatures(contents: &mut [Value]) {
    let current = contents.iter().rposition(|content| {
        let parts = content.arr("parts");
        content.str("role") == Some("user")
            && parts.iter().any(|part| part.get("text").is_some())
            && !parts
                .iter()
                .any(|part| part.get("functionResponse").is_some())
    });
    for content in contents.iter_mut().take(current.unwrap_or(0)) {
        let parts = content.get_mut("parts").and_then(Value::as_array_mut);
        for part in parts.into_iter().flatten().filter_map(Value::as_object_mut) {
            remove_preserving_order(part, "thoughtSignature");
        }
    }
}

impl Wire for GenerateContent {
    type Op = Completion;
    type Payload = Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = super::streaming::GenerateContentChunk;
    type Decoder<'id> = super::streaming::GenerateContentDecoder;
    type Reassembler = super::streaming::document::GenerateContentResponse;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(PROVIDER_NAME)
            .model(self.model.as_str())
            .facts(&self.facts)
            .telemetry(|_| GenAiOperation::GenerateContent)
            .replay(self)
    }

    fn encode(&self, request: CompletionRequest, mode: Mode) -> Result<Encoded, EncodeError> {
        // The request may name a model of its own; the wire's is the default.
        let model = request.model.clone().unwrap_or_else(|| self.model.clone());
        let replay = self.thought_replay;
        let body = request_body(
            &request,
            self,
            &model,
            self.cached_content.as_deref(),
            |body| {
                if let (ThoughtReplay::CurrentTurn, Some(Value::Array(contents))) =
                    (replay, body.get_mut("contents"))
                {
                    drop_finished_signatures(contents);
                }
            },
        )?;
        use crate::codecs::providers::internal::LogTarget;
        // `alt=sse` is what makes the streamed reply an event stream rather
        // than a JSON array of the same chunks.
        let (verb, framing, target) = match mode {
            Mode::Unary => ("generateContent", Framing::Whole, LogTarget::Completions),
            Mode::Streaming => (
                "streamGenerateContent?alt=sse",
                Framing::Sse,
                LogTarget::Streaming,
            ),
        };
        crate::codecs::providers::internal::trace_json(target, "Gemini completion request", &body);
        let uri = self.provider.uri(&format!("/v1beta/models/{model}:{verb}"));
        let request = http::Request::post(uri)
            .header("Content-Type", "application/json")
            .body(body.into_body())?;
        // Gemini supplies no transport request-id response header.
        Ok(Encoded::new(request, framing)
            .with_projection(super::streaming::GenerateContentDecoder::project)
            .with_analysis_only(super::streaming::GenerateContentDecoder::is_analysis_only))
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        super::streaming::GenerateContentDecoder::default()
    }
}

impl ReplayTarget for GenerateContent {
    fn facts(&self) -> Option<&crate::codecs::catalog::ModelFacts> {
        Some(&self.facts)
    }

    fn api(&self) -> crate::codecs::message::Api {
        crate::codecs::message::Api::from_static("gemini.generate_content")
    }

    fn provider(&self) -> &str {
        PROVIDER_NAME
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn accepts(&self, model: &str) -> Accepts {
        accepts(model)
    }

    fn encodes(&self, _model: &str, media: Media<'_>) -> bool {
        encodes(media, false)
    }

    fn normalize_tool_call_id(
        &self,
        id: &str,
        model: &str,
        _source: Option<&crate::codecs::message::Origin>,
    ) -> String {
        normalize_tool_call_id(model, id)
    }

    fn later_system(&self, _model: &str) -> crate::codecs::completion::LaterSystem {
        crate::codecs::completion::LaterSystem::Leading
    }

    fn call_id_slot(&self) -> Option<&'static str> {
        CALL_ID_SLOT
    }

    fn blob_slots(&self) -> &'static [&'static str] {
        BLOB_SLOTS
    }

    fn declares_tools(&self, request: &crate::codecs::completion::CompletionRequest) -> bool {
        self.cached_content.is_some() || declares_tools(self, request)
    }

    fn map_options(
        &self,
        request: &CompletionRequest,
        fields: crate::codecs::completion::options::OptionFields<'_>,
    ) -> crate::codecs::completion::options::OptionMap {
        let model = request.model.as_deref().unwrap_or(&self.model);
        generate_content_options(&self.facts, model, Route::Rest, fields)
    }
}

pub fn declares_tools(
    target: &dyn ReplayTarget,
    request: &crate::codecs::completion::CompletionRequest,
) -> bool {
    let param = |key: &str| crate::codecs::completion::options::param(target, request, key);
    !request.tools.is_empty()
        || param("tools").is_some_and(|tools| !tools.as_array().is_none_or(Vec::is_empty))
        || CACHED_CONTENT
            .iter()
            .any(|spelling| param(spelling).is_some_and(|value| !value.is_null()))
}

pub const CALL_ID_SLOT: Option<&str> = Some("/functionCall/id");

pub const BLOB_SLOTS: &[&str] = &["/thoughtSignature"];

pub(super) fn gemini_major(model: &str) -> Option<u32> {
    let model = model.to_ascii_lowercase();
    let model = model.strip_prefix("models/").unwrap_or(&model);
    let rest = model.strip_prefix("gemini-")?;
    let rest = rest.strip_prefix("live-").unwrap_or(rest);
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    Some(
        rest.get(..end)
            .and_then(|major| major.parse().ok())
            .unwrap_or(u32::MAX),
    )
}

pub(super) fn gemini_3_or_later(model: &str) -> bool {
    gemini_major(model).is_some_and(|major| major >= 3)
}

pub fn requires_tool_call_id(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    model.starts_with("claude-") || model.starts_with("gpt-oss-") || gemini_3_or_later(&model)
}

pub fn accepts(model: &str) -> Accepts {
    Accepts {
        tool_result_images: gemini_major(model).is_none_or(|major| major >= 3),
        ..Accepts::ALL
    }
}

pub fn encodes(media: Media<'_>, response_files: bool) -> bool {
    match media {
        Media::Image(image, place) => {
            reads_image(image.media_type.as_ref(), place)
                && match image.data {
                    Source::Base64(_) => true,
                    Source::Url(_) => place != Place::ToolResult || response_files,
                    _ => false,
                }
        }
        Media::Audio(audio) => {
            audio.media_type.is_some() && matches!(audio.data, Source::Url(_) | Source::Base64(_))
        }
        Media::Video(video) => match &video.data {
            Source::Url(url) if url.starts_with("https://www.youtube.com") => true,
            data => {
                video.media_type.is_some() && matches!(data, Source::Url(_) | Source::Base64(_))
            }
        },
        Media::Document(document) => match (&document.media_type, &document.data) {
            (_, DocumentData::Text(_)) => true,
            (Some(_), DocumentData::File(Source::Url(_))) => true,
            (Some(media_type), DocumentData::File(Source::Base64(_))) => {
                *media_type == DocumentMediaType::PDF
            }
            _ => false,
        },
    }
}

pub(crate) fn reads_image(
    media_type: Option<&crate::codecs::message::ImageMediaType>,
    place: Place,
) -> bool {
    use crate::codecs::message::ImageMediaType::{HEIC, HEIF, JPEG, PNG, WEBP};
    match place {
        Place::ToolResult => matches!(media_type, Some(JPEG | PNG | WEBP)),
        _ => matches!(media_type, Some(JPEG | PNG | WEBP | HEIC | HEIF)),
    }
}

pub fn normalize_tool_call_id(model: &str, id: &str) -> String {
    if !requires_tool_call_id(model) {
        return id.to_owned();
    }
    crate::codecs::providers::internal::wire_ids::legal_call_id(id, 64)
}

pub fn request_body(
    request: &CompletionRequest,
    target: &dyn ReplayTarget,
    model: &str,
    cached_content: Option<&str>,
    adjust: impl FnOnce(&mut Map<String, Value>),
) -> Result<FinalBody, EncodeError> {
    request_params(
        target,
        request,
        |input| {
            let mut body = base(request, target, model, input)?;
            adjust(&mut body);
            Ok(body)
        },
        RawAt::Top,
        &[Rewrite::GeminiCachedContent(
            cached_content.map(str::to_owned),
        )],
    )
}

fn base(
    request: &CompletionRequest,
    target: &dyn ReplayTarget,
    model: &str,
    input: &mut BaseInput<'_>,
) -> Result<Map<String, Value>, EncodeError> {
    let extra_tools = input.raw_tools()?;
    let schema = request
        .output_schema
        .clone()
        .map(|schema| schema.to_value());
    let mime = schema.as_ref().map(|_| json!("application/json"));
    let (temperature, max_tokens) = (
        request.temperature.map(Value::from),
        request.max_tokens.map(Value::from),
    );
    let config = set_fields([
        ("responseMimeType", mime),
        ("responseJsonSchema", schema),
        ("temperature", temperature),
        ("maxOutputTokens", max_tokens),
    ]);
    let (mut system, mut history) = (Vec::new(), Vec::new());
    for message in &request.chat_history {
        match &**message {
            Message::System { content } if content.is_empty() => {}
            Message::System { content } => system.push(text_part(content)),
            message => history.push(message),
        }
    }
    // Gemini rejects a field set twice, and merges a tool choice set twice,
    // which unions the allowed functions.
    let preamble = "system instruction twice: as a preamble or system message";
    let choice = "tool choice twice: as `tool_choice`, which Gemini merges,";
    let twice = [
        (!system.is_empty(), &SYSTEM_INSTRUCTION, preamble),
        (request.tool_choice.is_some(), &TOOL_CONFIG, choice),
    ];
    for (set, spellings, what) in twice {
        let raw = spellings
            .iter()
            .find(|spelling| input.param(spelling).is_some_and(|value| !value.is_null()));
        if let (true, Some(spelling)) = (set, raw) {
            return Err(EncodeError::request(format!(
                "a Gemini request set the {what} and as `additional_params.{spelling}`. Set it \
                 one way or the other"
            )));
        }
    }
    let declared: Vec<Value> = request.tools.iter().map(|tool| declaration(tool)).collect();
    let declared = (!declared.is_empty()).then(|| {
        Value::Object(set_fields([(
            "functionDeclarations",
            Some(Value::Array(declared)),
        )]))
    });
    let tools: Vec<Value> = declared.into_iter().chain(extra_tools).collect();
    let contents = Value::Array(contents(&history, target, model)?);
    let system = (!system.is_empty()).then(|| content(system, "model"));
    let tool_config = request
        .tool_choice
        .clone()
        .map_or(Value::Null, calling_config);
    Ok(set_fields([
        ("contents", Some(contents)),
        (
            "generationConfig",
            Some(if config.is_empty() {
                Value::Null
            } else {
                Value::Object(config)
            }),
        ),
        ("safetySettings", Some(Value::Null)),
        ("toolConfig", Some(tool_config)),
        ("systemInstruction", Some(system.unwrap_or(Value::Null))),
        ("tools", (!tools.is_empty()).then_some(Value::Array(tools))),
    ]))
}

fn declaration(tool: &crate::codecs::completion::ToolDefinition) -> Value {
    let parameters = (!tool.parameters.is_null()).then(|| tool.parameters.clone());
    Value::Object(set_fields([
        ("name", Some(Value::from(tool.name.as_str()))),
        ("description", Some(Value::from(tool.description.as_str()))),
        ("parametersJsonSchema", parameters),
    ]))
}

pub(super) const SYSTEM_INSTRUCTION: [&str; 2] = ["systemInstruction", "system_instruction"];
pub(super) const TOOL_CONFIG: [&str; 2] = ["toolConfig", "tool_config"];
const CACHED_CONTENT: [&str; 2] = ["cachedContent", "cached_content"];

pub(super) fn present<'a>(body: &Map<String, Value>, spellings: &[&'a str]) -> Option<&'a str> {
    let set = |spelling: &&&str| body.get(**spelling).is_some_and(|value| !value.is_null());
    spellings.iter().find(set).copied()
}

fn calling_config(choice: ToolChoice) -> Value {
    let (mode, names) = match choice {
        ToolChoice::Auto => ("AUTO", None),
        ToolChoice::None => ("NONE", None),
        ToolChoice::Required => ("ANY", None),
        ToolChoice::Specific { function_names } => ("ANY", Some(json!(function_names))),
    };
    let config = set_fields([
        ("mode", Some(json!(mode))),
        ("allowed_function_names", names),
    ]);
    json!({ "functionCallingConfig": config })
}

fn contents(
    history: &[&Message],
    target: &dyn ReplayTarget,
    model: &str,
) -> Result<Vec<Value>, EncodeError> {
    let ids = WireIds::for_target(history, target, model);
    let (with_ids, gemini3) = (requires_tool_call_id(model), gemini_3_or_later(model));
    let mut contents = Vec::with_capacity(history.len());
    let mut push = |role: &str, parts: Vec<Value>| {
        // Gemini rejects content with no parts.
        if !parts.is_empty() {
            contents.push(content(parts, role));
        }
    };
    for message in history {
        match message {
            Message::System { content } => push("user", vec![text_part(content)]),
            Message::User { content } => {
                let mut run = Vec::new();
                let mut responses = false;
                for part in content {
                    let id = match part {
                        UserContent::ToolResult(result) => {
                            Some(ids.of(&result.call).filter(|_| with_ids))
                        }
                        _ => None,
                    };
                    if id.is_some() != responses {
                        push("user", std::mem::take(&mut run));
                    }
                    responses = id.is_some();
                    run.push(user_part(part, id.flatten())?);
                }
                push("user", run);
            }
            Message::Assistant(turn) => {
                let parts = turn
                    .content
                    .iter()
                    .map(|block| assistant_part(block, target, &ids, with_ids, gemini3));
                push(
                    "model",
                    parts
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .flatten()
                        .collect(),
                );
            }
        }
    }
    Ok(contents)
}

fn content(parts: Vec<Value>, role: &str) -> Value {
    Value::Object(set_fields([
        ("parts", Some(Value::Array(parts))),
        ("role", Some(Value::from(role))),
    ]))
}

fn text_part(text: &str) -> Value {
    json!({ "text": text, "thought": false })
}

pub(super) fn mime<M: MimeType>(media_type: Option<M>) -> Option<String> {
    media_type.map(|media_type| media_type.to_mime_type().to_owned())
}

pub(super) fn carried(source: &Source) -> Result<(bool, &str), EncodeError> {
    match source {
        Source::Url(uri) => Ok((true, uri)),
        Source::Base64(data) => Ok((false, data)),
        _ => Err(EncodeError::request(
            "Gemini cannot receive this media in its form",
        )),
    }
}

pub(super) fn media(
    mime_type: Option<String>,
    source: &Source,
    part: bool,
) -> Result<Value, EncodeError> {
    let (uri, data) = carried(source)?;
    let data = match (uri, mime_type) {
        (true, mime) => ("fileData", json!({ "mimeType": mime, "fileUri": data })),
        (false, Some(mime)) => ("inlineData", json!({ "mimeType": mime, "data": data })),
        (false, None) => {
            return Err(EncodeError::request(
                "Gemini cannot receive media without its type",
            ));
        }
    };
    Ok(Value::Object(set_fields([
        (data.0, Some(data.1)),
        ("thought", part.then_some(Value::Bool(false))),
    ])))
}

fn user_part(part: &UserContent, id: Option<&str>) -> Result<Value, EncodeError> {
    Ok(match part {
        UserContent::Text(text) => text_part(&text.text),
        UserContent::ToolResult(result) => {
            let (mut values, mut parts) = (Vec::new(), Vec::new());
            for item in &result.content {
                match item {
                    ToolResultContent::Text(text) => values.push(Value::from(text.text.as_str())),
                    ToolResultContent::Json { value } => values.push(value.clone()),
                    ToolResultContent::Image(image) => {
                        parts.push(media(mime(image.media_type.clone()), &image.data, false)?);
                    }
                }
            }
            let key = if result.is_error { "error" } else { "result" };
            let value = match values.len() {
                0 => None,
                1 => values.pop(),
                _ => Some(Value::Array(values)),
            };
            let response = set_fields([
                ("name", Some(json!(result.name))),
                ("id", id.map(Value::from)),
                (
                    "response",
                    value.map(|value| Value::Object(Map::from_iter([(key.into(), value)]))),
                ),
                ("parts", (!parts.is_empty()).then_some(Value::Array(parts))),
            ]);
            Value::Object(set_fields([
                ("functionResponse", Some(Value::Object(response))),
                ("thought", Some(Value::Bool(false))),
            ]))
        }
        UserContent::Image(image) => media(mime(image.media_type.clone()), &image.data, true)?,
        // A text document goes as text, so that RAG context reads as prose.
        UserContent::Document(document) => match &document.data {
            DocumentData::Text(text) => text_part(text),
            DocumentData::File(data) => media(mime(document.media_type.clone()), data, true)?,
        },
        UserContent::Audio(audio) => media(mime(audio.media_type.clone()), &audio.data, true)?,
        UserContent::Video(video) => {
            let mut part = media(mime(video.media_type.clone()), &video.data, true)?;
            if let (Some(Value::Object(extra)), Some(part)) =
                (&video.additional_params, part.as_object_mut())
            {
                part.extend(extra.clone());
            }
            part
        }
    })
}

fn assistant_part(
    block: &AssistantContent,
    target: &dyn ReplayTarget,
    ids: &WireIds,
    with_ids: bool,
    gemini3: bool,
) -> Result<Option<Value>, EncodeError> {
    if let Replay::Item(item) = block.replay(target, ids) {
        let mut item = item.into_owned();
        if let Some(part) = item.as_object_mut() {
            if let (false, Some(Value::Object(call))) = (with_ids, part.get_mut("functionCall")) {
                remove_preserving_order(call, "id");
            }
            // Gemini rejects the whole request over a signature that is
            // not base64 ("Base64 decoding failed").
            if part
                .get("thoughtSignature")
                .and_then(Value::as_str)
                .is_some_and(|signature| !is_base64(signature))
            {
                return Err(EncodeError::request(
                    "Gemini thought signatures must be valid base64",
                ));
            }
        }
        return Ok(Some(item));
    }
    Ok(Some(match block {
        AssistantContent::Text(text) => json!({ "text": text.text }),
        AssistantContent::Reasoning(reasoning)
            if reasoning.redacted || reasoning.text.trim().is_empty() =>
        {
            return Ok(None);
        }
        AssistantContent::Reasoning(reasoning) => {
            json!({ "thought": true, "text": reasoning.text })
        }
        AssistantContent::ToolCall(call) => {
            let id = ids.of(&call.id).filter(|_| with_ids).map(Value::from);
            let name = Some(json!(call.function.name));
            let args = Some(json!(call.function.arguments));
            if gemini3 {
                return Err(EncodeError::request(
                    "Gemini requires the original thought signature for this tool call",
                ));
            }
            let signature = None::<Value>;
            let call = Value::Object(set_fields([("name", name), ("args", args), ("id", id)]));
            Value::Object(set_fields([
                ("functionCall", Some(call)),
                ("thoughtSignature", signature),
            ]))
        }
        AssistantContent::Image(image) => media(mime(image.media_type.clone()), &image.data, true)?,
        AssistantContent::Opaque(opaque) => opaque.item.clone(),
    }))
}

fn is_base64(text: &str) -> bool {
    let body = text.trim_end_matches('=');
    let alphabet = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/');
    !body.is_empty()
        && text.len().is_multiple_of(4)
        && text.len() - body.len() <= 2
        && body.bytes().all(alphabet)
}

pub(crate) fn blocked_prompt_error(feedback: &Value) -> Option<ProviderError> {
    let reason = match feedback.get("blockReason")? {
        Value::String(reason) => reason.clone(),
        Value::Number(number) => format!("BLOCK_REASON_{number}"),
        _ => return None,
    };
    if matches!(
        reason.as_str(),
        "BLOCK_REASON_UNSPECIFIED" | "BLOCKED_REASON_UNSPECIFIED"
    ) {
        return None;
    }
    let spelled = |value: Option<&Value>| match value {
        Some(Value::String(name)) => name.clone(),
        Some(other) => other.to_string(),
        None => "<unset>".to_owned(),
    };
    let ratings = feedback.arr("safetyRatings").iter();
    let ratings = ratings.map(|rating| {
        format!(
            "{}={}",
            spelled(rating.get("category")),
            spelled(rating.get("probability"))
        )
    });
    let ratings: Vec<String> = ratings.collect();
    let ratings = match ratings.is_empty() {
        true => String::new(),
        false => format!(", safety_ratings=[{}]", ratings.join(", ")),
    };
    let message = format!("Gemini blocked the prompt: block_reason={reason}{ratings}");
    let error = crate::codecs::provider_response::ProviderResponseError::without_status(message)
        .with_code(Some(reason.clone()));
    Some(ProviderError::ProviderResponse(Box::new(
        match reason.as_str() {
            "SAFETY" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "IMAGE_SAFETY" | "MODEL_ARMOR"
            | "JAILBREAK" => error.with_refusal(true),
            _ => error.with_transient(Some(true)),
        },
    )))
}

pub fn map_google_finish_reason(wire_name: &str) -> crate::codecs::completion::FinishReason {
    use crate::codecs::completion::FinishReason;
    match wire_name {
        "STOP" => FinishReason::Stop,
        "MAX_TOKENS" => FinishReason::Length,
        "SAFETY"
        | "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "IMAGE_SAFETY"
        | "IMAGE_PROHIBITED_CONTENT"
        | "MODEL_ARMOR" => FinishReason::ContentFilter,
        other => FinishReason::Other(other.to_owned()),
    }
}

pub(crate) fn classify_reply(body: &str) -> crate::codecs::wire::WireEvent<Value> {
    crate::codecs::providers::internal::wire::classify_marker_keyed_frame(
        body,
        &["candidates", "promptFeedback", "usageMetadata"],
    )
}

pub fn usage_of(usage: &Value) -> crate::codecs::completion::Usage {
    let count = |key: &str| usage.get(key).and_then(Value::as_u64);
    let tool_use = count("toolUsePromptTokenCount");
    let thoughts = count("thoughtsTokenCount");
    let input = count("promptTokenCount")
        .unwrap_or(0)
        .saturating_add(tool_use.unwrap_or(0));
    let output = count("candidatesTokenCount")
        .unwrap_or(0)
        .saturating_add(thoughts.unwrap_or(0));
    crate::codecs::completion::Usage {
        input_tokens: Some(input),
        output_tokens: Some(output),
        cached_input_tokens: count("cachedContentTokenCount"),
        reasoning_tokens: thoughts,
        tool_use_prompt_tokens: tool_use,
        total_tokens: Some(input.saturating_add(output)),
        cache_creation_input_tokens: None,
        cost: None,
    }
}

pub fn with_cached_content(
    body: &mut serde_json::Map<String, Value>,
    name: &str,
) -> Result<(), EncodeError> {
    let fail = |message: String| Err(EncodeError::request(message));
    if !name.starts_with("cachedContents/") {
        return fail(format!(
            "gemini cached content handle should look like `cachedContents/<id>`, got `{name}`"
        ));
    }
    if let Some(existing) = body
        .get("cachedContent")
        .and_then(Value::as_str)
        .filter(|existing| *existing != name)
    {
        return fail(format!(
            "a Gemini request set cached content twice, to `{existing}` and `{name}`: set it \
             one way or the other"
        ));
    }
    let conflicts: Vec<&str> = [
        (
            &super::completion::SYSTEM_INSTRUCTION[..],
            "a system instruction (preamble)",
        ),
        (&["tools"][..], "tools"),
        (&super::completion::TOOL_CONFIG[..], "a tool choice"),
    ]
    .into_iter()
    .filter_map(|(spellings, what)| super::completion::present(body, spellings).map(|_| what))
    .collect();
    if conflicts.is_empty() {
        body.insert("cachedContent".to_owned(), Value::String(name.to_owned()));
        return Ok(());
    }
    // Cached function declarations need caller-side dispatch; hosted tools
    // run on Gemini's side.
    let tools = body
        .get("tools")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let declares_functions = tools.iter().any(|tool| {
        !tool.arr("functionDeclarations").is_empty()
            || !tool.arr("function_declarations").is_empty()
    });
    let caveat = match declares_functions {
        true => {
            " Function declarations in a cache are declarations only: an `Agent` dispatches \
             only tools it advertised, so a cached function tool runs only when you drive \
             `GenerateContent` yourself. Hosted tools such as `codeExecution` are fine to cache."
        }
        false => "",
    };
    fail(format!(
        "a Gemini request using cached content `{name}` also set {}. The cached content owns \
         the system instruction, tools and tool choice of every request that uses it: move \
         them into the cache, or drop the cache handle.{caveat}",
        conflicts.join(" and ")
    ))
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    use crate::codecs::message::{AssistantMessage, ToolName};
    use std::sync::Arc;

    #[test]
    fn a_gemini_3_tool_call_requires_original_signature_without_a_placeholder() {
        let wire = GenerateContent::new(super::super::GeminiConfig::new(""), "gemini-3-pro");
        let mut request = CompletionRequest::new("Continue.").max_tokens(100);
        let call = AssistantContent::tool_call(
            "call-1",
            ToolName::new("lookup").unwrap(),
            json!({"key":1}),
        );
        request
            .chat_history
            .push(Arc::new(Message::Assistant(AssistantMessage::new(vec![
                call,
            ]))));
        let error = wire
            .encode(request, Mode::Unary)
            .expect_err("a signature is required");
        assert!(error.to_string().contains("original thought signature"));
    }
}
