use crate::codecs::wire::Wire;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::borrow::Cow;
use std::sync::Arc;

use crate::codecs::catalog::ModelFacts;
use crate::codecs::completion::options::{BaseInput, RawAt, Rewrite, request_params};
use crate::codecs::completion::{CompletionRequest, FinishReason, ProviderCapabilities, Replay};
use crate::codecs::error::{EncodeError, ProviderError};
use crate::codecs::json_utils::Lenient;
use crate::codecs::json_utils::remove_preserving_order;
use crate::codecs::message::{
    AssistantContent, AssistantMessage, DocumentData, DocumentMediaType,
    DocumentSourceKind as Source, Message, MimeType, ToolCall, ToolResult, ToolResultContent,
    UserContent,
};
use crate::codecs::observe::ObservedError;
use crate::codecs::operation::{Block, CallFragment, Completion, Finish};
use crate::codecs::providers::internal::object;
use crate::codecs::providers::internal::wire::classify_chat_completions_frame;
use crate::codecs::providers::internal::wire_ids::WireIds;
use crate::codecs::wire::{
    AdapterVerdict, Capabilities, Decoder, Descriptor, Encoded, Flow, Framing, Mode,
    ObservationSink, Out, WireEvent, WireFrame,
};
use finish::{finish_reason, native_finish_reason, provider_error_envelope};

use super::dto::merge_fields;
use super::{BodyRewrite, OpenAIConfig, OutputCap, Quirks};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chat {
    pub provider: OpenAIConfig,
    pub model: String,
    pub strict_tools: bool,
    pub tool_result_array_content: bool,
    #[serde(skip)]
    pub facts: ModelFacts,
}

fn unsendable(what: &str) -> EncodeError {
    EncodeError::request(format!("Chat Completions cannot carry {what} in this form"))
}

fn media_url(data: &Source, mime: Option<&str>) -> Option<String> {
    match (data, mime) {
        (Source::Url(url), _) => Some(url.clone()),
        (Source::Base64(data), Some(mime)) => Some(format!("data:{mime};base64,{data}")),
        _ => None,
    }
}

fn image_url(image: &crate::codecs::message::Image, user: bool) -> Result<Value, EncodeError> {
    let mime = image.media_type.as_ref().map(MimeType::to_mime_type);
    let url = media_url(&image.data, mime).ok_or_else(|| unsendable("an image"))?;
    let mut part = Map::from_iter([("url".to_owned(), Value::String(url))]);
    let detail = match &image.detail {
        Some(detail) => Some(detail.clone()),
        None => user.then(Default::default),
    };
    if let Some(detail) = detail {
        part.insert("detail".to_owned(), serde_json::to_value(detail)?);
    }
    Ok(object! {"type": "image_url", "image_url": part})
}

fn user_part(part: &UserContent) -> Result<Value, EncodeError> {
    let file = |file: Value| object! {"type": "file", "file": file};
    Ok(match part {
        UserContent::Text(text) => json!({"type": "text", "text": text.text}),
        UserContent::Image(image) => image_url(image, true)?,
        UserContent::Document(document) => {
            let pdf = document.media_type == Some(DocumentMediaType::PDF);
            match &document.data {
                DocumentData::Text(text) => json!({"type": "text", "text": text}),
                DocumentData::File(Source::FileId(id)) => file(json!({ "file_id": id })),
                DocumentData::File(Source::Base64(data)) if pdf => file(object! {
                    "file_data": format!("data:application/pdf;base64,{data}"),
                    "filename": "document.pdf",
                }),
                // OpenRouter and Mistral fetch a PDF a URL names.
                DocumentData::File(Source::Url(url)) if pdf => {
                    file(json!({"file_data": url, "filename": "document.pdf"}))
                }
                DocumentData::File(_) => return Err(unsendable("a document")),
            }
        }
        UserContent::Audio(audio) => match &audio.data {
            Source::Base64(data) => json!({"type": "input_audio", "input_audio": {
                "data": data,
                "format": audio.media_type.clone().unwrap_or(crate::codecs::message::AudioMediaType::MP3),
            }}),
            _ => return Err(unsendable("audio")),
        },
        UserContent::Video(video) => {
            let mime = video.media_type.as_ref().map(MimeType::to_mime_type);
            let url = media_url(&video.data, mime).ok_or_else(|| unsendable("a video"))?;
            object! {"type": "video_url", "video_url": object! {"url": url}}
        }
        UserContent::ToolResult(_) => return Err(unsendable("a tool result as a content part")),
    })
}

fn tool_message(result: &ToolResult, ids: &WireIds, array: bool) -> Result<Value, EncodeError> {
    fn text(part: &ToolResultContent) -> Option<Cow<'_, str>> {
        match part {
            ToolResultContent::Text(text) => Some(Cow::Borrowed(&text.text)),
            ToolResultContent::Json { value } => Some(Cow::Owned(value.to_string())),
            ToolResultContent::Image(_) => None,
        }
    }
    let image = |part: &ToolResultContent| matches!(part, ToolResultContent::Image(_));
    let content = if array || result.content.iter().any(image) {
        let parts = result.content.iter().map(|part| match part {
            ToolResultContent::Image(image) => image_url(image, false),
            _ => Ok(object! {"type": "text", "text": text(part).unwrap_or_default()}),
        });
        Value::Array(parts.collect::<Result<_, EncodeError>>()?)
    } else {
        let texts: Vec<Cow<'_, str>> = result.content.iter().filter_map(text).collect();
        Value::String(texts.join("\n"))
    };
    let id = ids.spell(&result.call);
    Ok(object! {"role": "tool", "tool_call_id": id, "content": content})
}

fn user_message(messages: &mut Vec<Value>, parts: &mut Vec<Value>) {
    let content = match parts.as_mut_slice() {
        [] => return,
        [part] if part.str("type") == Some("text") => {
            part.get_mut("text").map(Value::take).unwrap_or_default()
        }
        _ => Value::Array(std::mem::take(parts)),
    };
    parts.clear();
    messages.push(object! {"role": "user", "content": content});
}

impl Chat {
    fn reasoning_field<'a>(&'a self, model: &str) -> Option<&'a str> {
        use crate::codecs::providers::openai::wire::{MOONSHOT, OPENROUTER};
        let deepseek = self
            .provider
            .base_url
            .to_ascii_lowercase()
            .contains("deepseek.com");
        let name = model.rsplit('/').next().unwrap_or(model);
        let field = |vendor: &str, id: &str| {
            self.facts
                .for_model(vendor, id)
                .and_then(|spec| spec.compat.reasoning_field.as_deref())
        };
        let listed = field(self.provider.dialect.name, model)
            .or_else(|| field(MOONSHOT.name, name))
            .or_else(|| field(OPENROUTER.name, model));
        self.provider
            .dialect
            .quirks
            .reasoning_field
            .or(deepseek.then_some("reasoning_content"))
            .or(listed)
    }

    pub fn new(provider: OpenAIConfig, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
            strict_tools: false,
            tool_result_array_content: false,
            facts: ModelFacts::default(),
        }
    }

    pub fn with_facts(mut self, facts: ModelFacts) -> Self {
        self.facts = facts;
        self
    }

    pub fn with_strict_tools(mut self) -> Self {
        self.strict_tools = true;
        self
    }

    pub fn with_tool_result_array_content(mut self) -> Self {
        self.tool_result_array_content = true;
        self
    }

    fn base(
        &self,
        request: &CompletionRequest,
        input: &mut BaseInput<'_>,
    ) -> Result<Map<String, Value>, EncodeError> {
        let quirks = &self.provider.dialect.quirks;
        let mut model = request.model.clone().unwrap_or_else(|| self.model.clone());
        if quirks.rewrite == BodyRewrite::HuggingFaceRouter {
            // Some sub-providers (Fireworks) address models by a qualified id.
            model = self.provider.route().model_identifier(&model);
        }
        let passthrough = input.raw_tools()?;
        // A call to a custom tool the request declares is a custom call (the protocol).
        let custom: Vec<String> = passthrough
            .iter()
            .filter(|tool| tool.str("type") == Some("custom"))
            .filter_map(|tool| tool.at("/custom/name").and_then(Value::as_str))
            .map(str::to_owned)
            .collect();
        let messages = self.messages(&request.chat_history, &model, &custom)?;

        let mut tools: Vec<Value> = Vec::new();
        let mut tool_choice = None;
        if quirks.supports_tools {
            for tool in &request.tools {
                let mut parameters = tool.parameters.clone();
                let mut function = json!({"name": tool.name, "description": tool.description});
                let mut strict = self.strict_tools;
                if strict {
                    let mut sanitized = parameters.clone();
                    match crate::codecs::providers::openai::sanitize_tool_schema(
                        &mut sanitized,
                        quirks,
                    ) {
                        Ok(()) => parameters = sanitized,
                        Err(reason) => {
                            tracing::warn!(
                                tool = %tool.name,
                                "sending the tool without strict mode: its schema holds {reason}, \
                                 which this provider's strict mode cannot express"
                            );
                            strict = false;
                        }
                    }
                }
                if let Some(function) = function.as_object_mut() {
                    function.insert("parameters".to_owned(), parameters);
                    if strict {
                        function.insert("strict".to_owned(), Value::Bool(true));
                    }
                }
                tools.push(object! {"type": "function", "function": function});
            }
            // Passthrough tools, function or native (Groq's `browser_search`),
            // join the typed ones in one array.
            tools.extend(passthrough);
            tool_choice = request
                .tool_choice
                .clone()
                .map(tool_choice_value)
                .transpose()?
                .filter(|_| !tools.is_empty());
        } else {
            if !request.tools.is_empty() {
                tracing::warn!("Tool use is not supported by this provider; tools will be ignored");
            }
            if request.tool_choice.is_some() {
                tracing::warn!("Tool choice is not supported by this provider and will be ignored");
            }
            // The caller's own tools still reach a dialect that takes none,
            // as `additional_params` always did.
            tools.extend(passthrough);
        }

        if request.output_schema.is_some() && !quirks.supports_response_format {
            tracing::warn!(
                "Structured outputs are not supported by this provider; ignoring output_schema"
            );
        }
        // Defer schemas until a tool result exists unless the dialect supports both at once.
        let answered = messages
            .iter()
            .any(|message| message.str("role") == Some("tool"));
        let response_format = match request.output_schema.clone() {
            Some(schema)
                if quirks.supports_response_format
                    && (quirks.response_format_with_tools || tools.is_empty() || answered) =>
            {
                let (name, schema) =
                    crate::codecs::providers::openai::structured_output_schema(schema);
                Some(json!({"type": "json_schema",
                    "json_schema": {"name": name, "strict": true, "schema": schema}}))
            }
            _ => None,
        };

        Ok(crate::codecs::providers::internal::set_fields([
            ("model", Some(Value::String(model))),
            ("messages", Some(Value::Array(messages))),
            ("tools", (!tools.is_empty()).then_some(Value::Array(tools))),
            ("tool_choice", tool_choice),
            ("temperature", request.temperature.map(Value::from)),
            ("max_tokens", request.max_tokens.map(Value::from)),
            ("response_format", response_format),
        ]))
    }

    fn messages(
        &self,
        history: &[Arc<Message>],
        model: &str,
        custom: &[String],
    ) -> Result<Vec<Value>, EncodeError> {
        let ids = WireIds::for_target(history, self, model);
        let reasoning_field = self.reasoning_field(model);
        let mut messages = Vec::new();
        for message in history {
            match &**message {
                Message::System { content } => messages.push(json!({"role": "system",
                    "content": [{"type": "text", "text": content}]})),
                Message::User { content } => {
                    let mut parts = Vec::new();
                    for part in content {
                        if let UserContent::ToolResult(result) = part {
                            user_message(&mut messages, &mut parts);
                            let array = self.tool_result_array_content;
                            messages.push(tool_message(result, &ids, array)?);
                        } else {
                            parts.push(user_part(part)?);
                        }
                    }
                    user_message(&mut messages, &mut parts);
                }
                Message::Assistant(turn) => {
                    messages.extend(self.assistant(turn, &ids, custom, reasoning_field));
                }
            }
        }
        if messages.is_empty() {
            return Err(EncodeError::request(
                "OpenAI Chat Completions request has no provider-compatible messages after \
                 conversion",
            ));
        }
        Ok(messages)
    }

    fn assistant(
        &self,
        turn: &AssistantMessage,
        ids: &WireIds,
        custom: &[String],
        reasoning_field: Option<&str>,
    ) -> Option<Value> {
        let (mut text, mut parts, mut has_parts) = (String::new(), Vec::new(), false);
        let mut reasoning: Vec<(String, String)> = Vec::new();
        let (mut fields, mut calls) = (Map::new(), Vec::new());
        for block in &turn.content {
            let replay = block.replay(self, ids);
            match block {
                AssistantContent::Text(block) => {
                    text.push_str(&block.text);
                    parts.push(json!({"type": "text", "text": block.text}));
                    // A text item is message fields: an answer's audio.
                    if let Replay::Item(item) = replay
                        && let Value::Object(item) = item.into_owned()
                    {
                        fields.extend(item);
                    }
                }
                AssistantContent::Reasoning(block) => {
                    let field = match replay {
                        Replay::Item(item) if item.get("type").is_some() => {
                            has_parts = true;
                            parts.push(item.into_owned());
                            None
                        }
                        Replay::Item(item) => {
                            let mut field = None;
                            for (key, value) in item.as_object().into_iter().flatten() {
                                if value.is_string() {
                                    field.get_or_insert_with(|| key.clone());
                                } else {
                                    let detail = Map::from_iter([(key.clone(), value.clone())]);
                                    merge_fields(&mut fields, &detail);
                                }
                            }
                            field
                        }
                        Replay::Identity(identity)
                            if identity.get("type").and_then(Value::as_str) == Some("thinking") =>
                        {
                            has_parts = true;
                            parts.push(json!({"type": "thinking",
                                "thinking": [{"type": "text", "text": block.text}]}));
                            None
                        }
                        Replay::Identity(identity) => identity.keys().next().cloned(),
                        Replay::Rebuild => reasoning_field.map(str::to_owned),
                    };
                    if let Some(field) = field {
                        match reasoning.iter_mut().find(|(name, _)| *name == field) {
                            Some((_, joined)) => {
                                joined.push('\n');
                                joined.push_str(&block.text);
                            }
                            None => reasoning.push((field, block.text.clone())),
                        }
                    }
                }
                AssistantContent::ToolCall(call) => {
                    calls.push(call_item(call, replay, ids, custom));
                }
                // An opaque is a content part; one with no `type` is not
                // sent, so its keys never land on the message.
                AssistantContent::Opaque(opaque) if opaque.replay => {
                    if opaque.item.get("type").is_some() {
                        has_parts = true;
                        parts.push(opaque.item.clone());
                    }
                }
                AssistantContent::Opaque(_) | AssistantContent::Image(_) => {}
            }
        }
        let mut message = Map::from_iter([("role".to_owned(), Value::from("assistant"))]);
        if has_parts {
            message.insert("content".to_owned(), Value::Array(parts));
        } else if !text.is_empty() {
            message.insert("content".to_owned(), Value::String(text));
        }
        for (field, text) in reasoning {
            message.insert(field, Value::String(text));
        }
        message.extend(fields);
        if !calls.is_empty() {
            message.insert("tool_calls".to_owned(), Value::Array(calls));
        }
        let has_content = message.contains_key("audio")
            || message.contains_key("tool_calls")
            || match message.get("content") {
                Some(Value::String(text)) => !text.is_empty(),
                Some(Value::Array(parts)) => !parts.is_empty(),
                _ => false,
            };
        if let Some(field) = reasoning_field {
            message
                .entry(field)
                .or_insert_with(|| Value::String(String::new()));
        }
        has_content.then_some(Value::Object(message))
    }
}

pub(crate) fn rewrite_body(
    kind: BodyRewrite,
    map: &mut Map<String, Value>,
) -> Result<(), EncodeError> {
    let forced = map
        .get("tool_choice")
        .and_then(|choice| choice.at("/function/name"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    match kind {
        BodyRewrite::LlamaCpp => {
            if let Some(name) = forced {
                return Err(EncodeError::request(format!(
                    "llama.cpp cannot force a specific tool: `llama-server` accepts only \
                     `auto`, `none` or `required` for tool_choice and silently treats \
                     anything else as `auto`, so requesting `{name}` would return whichever \
                     tool the model picked. Use `ToolChoice::Required` to force a call, or \
                     advertise only `{name}` in `tools`."
                )));
            }
        }
        BodyRewrite::Moonshot => {
            if forced.is_some() {
                return Err(EncodeError::request(
                    "Moonshot does not support forcing a specific tool",
                ));
            }
            if map.get("tool_choice").and_then(Value::as_str) == Some("required") {
                tracing::warn!(
                    "Moonshot does not support tool_choice=required; coercing to auto with an \
                     additional steering message"
                );
                map.insert("tool_choice".to_owned(), Value::from("auto"));
                if let Some(Value::Array(messages)) = map.get_mut("messages") {
                    messages.push(json!({"role": "user",
                        "content": "Please select a tool to handle the current issue."}));
                }
            }
        }
        // The gateway takes every message's content as one string.
        // Perplexity spends output budget on a text-part array (a short
        // `max_tokens` comes back empty at `length`, checked live), so
        // text-only arrays go as one string; the gateway takes every
        // content as one string.
        BodyRewrite::Perplexity | BodyRewrite::Mira => {
            let all = kind == BodyRewrite::Mira;
            for content in messages_mut(map).filter_map(|message| message.get_mut("content")) {
                if let Value::Array(parts) = content
                    && (all || parts.iter().all(|part| part.str("type") == Some("text")))
                {
                    let texts: Vec<&str> =
                        parts.iter().filter_map(|part| part.str("text")).collect();
                    *content = Value::String(texts.join("\n"));
                }
            }
        }
        BodyRewrite::DeepSeek => finalize_deepseek(map),
        BodyRewrite::Mistral => finalize_mistral(map),
        BodyRewrite::Ollama => finalize_ollama(map)?,
        BodyRewrite::None | BodyRewrite::OpenRouter | BodyRewrite::HuggingFaceRouter => {}
    }
    Ok(())
}

fn call_item(call: &ToolCall, replay: Replay<'_>, ids: &WireIds, custom: &[String]) -> Value {
    let mut item = match replay {
        Replay::Item(item) => match item.into_owned() {
            Value::Object(item) => item,
            _ => Map::new(),
        },
        Replay::Identity(identity) => identity,
        Replay::Rebuild => Map::new(),
    };
    let name = call.function.name.as_str();
    let custom = match item.get("type").and_then(Value::as_str) {
        Some(kind) => kind == "custom",
        None => {
            let declared = custom.iter().any(|tool| tool == name);
            let kind = if declared { "custom" } else { "function" };
            item.insert("type".to_owned(), Value::from(kind));
            declared
        }
    };
    let id = ids.spell(&call.id);
    item.insert("id".to_owned(), Value::String(id));
    let (slot, key, value) = if custom {
        let input = match call.function.arguments.get("input") {
            Some(Value::String(input)) => input.clone(),
            Some(input) => input.to_string(),
            None => String::new(),
        };
        ("custom", "input", input)
    } else {
        let arguments = serde_json::to_string(&call.function.arguments).unwrap_or_default();
        ("function", "arguments", arguments)
    };
    let slot = item
        .entry(slot)
        .or_insert_with(|| Value::Object(Map::new()));
    if !slot.is_object() {
        *slot = Value::Object(Map::new());
    }
    if let Value::Object(fields) = slot {
        fields.insert("name".to_owned(), Value::from(name));
        fields.insert(key.to_owned(), Value::String(value));
    }
    Value::Object(item)
}

fn tool_choice_value(choice: crate::codecs::message::ToolChoice) -> Result<Value, EncodeError> {
    use crate::codecs::message::ToolChoice;
    Ok(match choice {
        ToolChoice::Auto => Value::from("auto"),
        ToolChoice::None => Value::from("none"),
        ToolChoice::Required => Value::from("required"),
        ToolChoice::Specific { function_names } => {
            let [name] = function_names.as_slice() else {
                return Err(EncodeError::request(
                    "Provider only supports forcing exactly one specific tool",
                ));
            };
            json!({"type": "function", "function": {"name": name}})
        }
    })
}

fn finalize_ollama(map: &mut Map<String, Value>) -> Result<(), EncodeError> {
    if let Some(key) = ["num_ctx", "options"]
        .into_iter()
        .find(|key| map.contains_key(*key))
    {
        return Err(EncodeError::request(format!(
            "Ollama's OpenAI-compatible API ignores `{key}`; send it through the native \
             route (`Ollama::native_completion`)"
        )));
    }
    Ok(())
}

fn messages_mut(map: &mut Map<String, Value>) -> impl Iterator<Item = &mut Map<String, Value>> {
    let messages = map.get_mut("messages").and_then(Value::as_array_mut);
    messages
        .into_iter()
        .flatten()
        .filter_map(Value::as_object_mut)
}

fn finalize_deepseek(map: &mut Map<String, Value>) {
    let thinking = match map
        .get("thinking")
        .and_then(|thinking| thinking.str("type"))
    {
        Some(mode) => !mode.eq_ignore_ascii_case("disabled"),
        None => map.get("model").and_then(Value::as_str) != Some("deepseek-chat"),
    };
    let forced = map
        .get("tool_choice")
        .is_some_and(|choice| choice.is_object() || choice.as_str() == Some("required"));
    if thinking && forced {
        tracing::debug!(
            "dropping tool_choice: DeepSeek rejects a forced tool choice while thinking"
        );
        remove_preserving_order(map, "tool_choice");
    }
}

fn finalize_mistral(map: &mut Map<String, Value>) {
    // Mistral rejects forced tool calls beside JSON response formats: the
    // choice is relaxed rather than the schema discarded.
    let forces_a_tool_call = map
        .get("tool_choice")
        .is_some_and(|choice| !matches!(choice.as_str(), Some("auto" | "none")));
    let has_tools = map
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    let structured = map
        .get("response_format")
        .and_then(|format| format.str("type"))
        .is_some_and(|kind| matches!(kind, "json_schema" | "json_object"));
    if forces_a_tool_call && has_tools && structured {
        tracing::debug!(
            "relaxing tool_choice to `auto`: Mistral rejects a forced tool choice \
             alongside a response format"
        );
        map.insert("tool_choice".to_owned(), Value::from("auto"));
    }
    for content in messages_mut(map).filter_map(|message| message.get_mut("content")) {
        for part in content.as_array_mut().into_iter().flatten() {
            *part = mistral_chunk(part);
        }
    }
}

fn mistral_chunk(part: &Value) -> Value {
    let field = |pointer: &str| part.at(pointer).and_then(Value::as_str);
    match part.str("type") {
        Some("image_url") => {
            json!({"type": "image_url", "image_url": part.get("image_url")})
        }
        Some("input_audio") => json!({"type": "input_audio",
            "input_audio": field("/input_audio/data")}),
        Some("file") => match field("/file/file_data") {
            Some(data) => json!({"type": "document_url", "document_url": data,
                "document_name": field("/file/filename")}),
            None => json!({"type": "file", "file_id": field("/file/file_id")}),
        },
        _ => part.clone(),
    }
}

impl Wire for Chat {
    type Op = crate::codecs::operation::Completion;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = ChatEvent;
    type Decoder<'id> = ChatDecoder;
    type Reassembler = document::ChatCompletion;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(self.provider.dialect.name)
            .model(self.model.as_str())
            .facts(&self.facts)
            .capabilities(Capabilities::completion(
                ProviderCapabilities::default().with_native_output_tool_composition(
                    self.provider.dialect.quirks.supports_response_format,
                ),
            ))
            .replay(self)
    }

    fn encode(&self, request: CompletionRequest, mode: Mode) -> Result<Encoded, EncodeError> {
        let quirks = &self.provider.dialect.quirks;
        let base = self.provider.base_url.trim_end_matches('/');
        let prefix = quirks
            .strict_tools_prefix
            .filter(|prefix| self.strict_tools && !base.ends_with(prefix))
            .unwrap_or_default();
        let path = format!("{prefix}{}", quirks.completion_path);
        // Azure's deployment URL remains pinned to the handle, not a request override.
        let uri = self
            .provider
            .uri(&path, self.provider.deployment(&self.model));
        let builder = self.provider.completion_headers(
            &request,
            http::Request::post(uri).header("Content-Type", "application/json"),
        );
        let mut rewrites = Vec::new();
        if quirks.output_cap == OutputCap::OpenAiReasoningFamilies {
            rewrites.push(Rewrite::OutputCapRename);
        }
        if mode == Mode::Streaming {
            // A caller's own stream options, `include_usage` among them, stand.
            if quirks.stream_include_usage {
                rewrites.push(Rewrite::StreamUsage);
            }
            rewrites.push(Rewrite::Stream(true));
        }
        rewrites.push(Rewrite::ChatDialect(quirks.rewrite));
        let raw_at = match quirks.rewrite {
            BodyRewrite::Mira => RawAt::Ignored(
                "Additional parameters are not supported by Mira and will be ignored",
            ),
            _ => RawAt::Top,
        };
        let body = request_params(
            self,
            &request,
            |input| self.base(&request, input),
            raw_at,
            &rewrites,
        )?;
        let body = crate::codecs::providers::openai::options::check_body(
            self,
            &self.facts,
            &request,
            body,
            crate::codecs::providers::openai::options::Endpoint::ChatCompletions,
        )?;
        crate::codecs::providers::internal::trace_json(
            crate::codecs::providers::internal::LogTarget::Completions,
            "OpenAI Chat Completions request",
            &body,
        );
        let request = builder.body(body.into_body())?;
        let framing = match mode {
            Mode::Streaming => Framing::Sse,
            Mode::Unary => Framing::Whole,
        };
        Ok(Encoded::new(request, framing)
            .with_request_id_header(self.provider.dialect.request_id_header)
            .with_projection(ChatDecoder::project)
            .with_route(Some(quirks.completion_path)))
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        ChatDecoder::new(self.provider.dialect.quirks)
    }
}

impl crate::codecs::completion::ReplayTarget for Chat {
    fn catalog_priced(&self) -> bool {
        self.provider.dialect.quirks.catalog_priced
    }

    fn map_options(
        &self,
        request: &CompletionRequest,
        fields: crate::codecs::completion::options::OptionFields<'_>,
    ) -> crate::codecs::completion::options::OptionMap {
        crate::codecs::providers::openai::options::chat_options(self, request, fields)
    }

    fn facts(&self) -> Option<&ModelFacts> {
        Some(&self.facts)
    }

    fn api(&self) -> crate::codecs::message::Api {
        crate::codecs::message::Api::from_static("openai.chat")
    }

    fn provider(&self) -> &str {
        self.provider.dialect.name
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn accepts(&self, model: &str) -> crate::codecs::completion::Accepts {
        let quirks = &self.provider.dialect.quirks;
        let user_images = reads_images(&self.facts, &self.provider.dialect, model);
        crate::codecs::completion::Accepts {
            user_images,
            assistant_images: false,
            tool_result_images: user_images && quirks.supports_image_tool_results,
            tools: quirks.supports_tools,
            foreign_reasoning: self.reasoning_field(model).is_some(),
        }
    }

    fn encodes(&self, _model: &str, media: crate::codecs::completion::Media<'_>) -> bool {
        use crate::codecs::completion::Media;
        let dialect = &self.provider.dialect;
        let rewrite = dialect.quirks.rewrite;
        let parts = !matches!(rewrite, BodyRewrite::DeepSeek | BodyRewrite::Mira);
        // Cohere and Ollama read images and text, and no other media.
        let files = parts
            && rewrite != BodyRewrite::Perplexity
            && ![super::dialects::COHERE.name, super::dialects::OLLAMA.name]
                .contains(&dialect.name);
        let linked = |source: &Source, typed: bool| match source {
            Source::Url(_) => true,
            Source::Base64(_) => typed,
            Source::Raw(_) | Source::FileId(_) | Source::Unknown => false,
        };
        match media {
            Media::Image(image, place) => {
                parts
                    && place != crate::codecs::completion::Place::Assistant
                    && linked(&image.data, image.media_type.is_some())
                    // Ollama reads an image as data and never fetches a URL.
                    && !(dialect.name == super::dialects::OLLAMA.name
                        && matches!(image.data, Source::Url(_)))
            }
            Media::Audio(audio) => files && matches!(audio.data, Source::Base64(_)),
            Media::Video(video) => {
                files
                    && rewrite != BodyRewrite::Mistral
                    && ![super::dialects::OPENAI.name, super::dialects::AZURE.name]
                        .contains(&dialect.name)
                    && linked(&video.data, video.media_type.is_some())
            }
            Media::Document(document) => {
                let pdf = document.media_type == Some(DocumentMediaType::PDF);
                match &document.data {
                    DocumentData::Text(_) => true,
                    DocumentData::File(Source::FileId(_)) => {
                        files && dialect.quirks.accepts_file_ids
                    }
                    DocumentData::File(Source::Base64(_)) => files && pdf,
                    DocumentData::File(Source::Url(_)) => {
                        pdf && matches!(rewrite, BodyRewrite::OpenRouter | BodyRewrite::Mistral)
                    }
                    DocumentData::File(Source::Raw(_) | Source::Unknown) => false,
                }
            }
        }
    }

    fn normalize_tool_call_id(
        &self,
        id: &str,
        _model: &str,
        _: Option<&crate::codecs::message::Origin>,
    ) -> String {
        let sanitized = |part: &str| {
            crate::codecs::providers::internal::wire_ids::legal_call_id(part, usize::MAX)
        };
        if let Some((call, item)) = id.split_once('|') {
            let call = sanitized(call);
            let item = sanitized(item);
            let combined = if item.is_empty() {
                call.clone()
            } else {
                format!("{call}_{item}")
            };
            if combined.len() <= 40 {
                return combined;
            }
            let hash: String = crate::codecs::providers::internal::wire_ids::short_hash(id)
                .chars()
                .take(8)
                .collect();
            let prefix: String = call.chars().take((40 - hash.len() - 1).max(1)).collect();
            return format!("{prefix}_{hash}");
        }
        if self.provider.dialect.name == super::dialects::OPENAI.name {
            return id.chars().take(40).collect();
        }
        id.to_owned()
    }

    fn identity(&self, item: &Value) -> Map<String, Value> {
        if let Some(kind @ ("custom" | "thinking")) = item.str("type") {
            return Map::from_iter([("type".to_owned(), Value::from(kind))]);
        }
        REASONING_TEXT_KEYS
            .iter()
            .find(|key| item.get(**key).is_some_and(Value::is_string))
            .map(|key| Map::from_iter([((*key).to_owned(), Value::String(String::new()))]))
            .unwrap_or_default()
    }

    fn call_id_slot(&self) -> Option<&'static str> {
        Some("/id")
    }

    fn later_system(&self, _model: &str) -> crate::codecs::completion::LaterSystem {
        self.provider.dialect.quirks.later_system
    }

    fn result_parts(&self, _model: &str) -> bool {
        self.tool_result_array_content
    }

    fn sends_alone(&self, block: &AssistantContent) -> bool {
        let replay = || block.replay(self, &WireIds::default());
        match block {
            AssistantContent::ToolCall(_) => true,
            AssistantContent::Text(text) => {
                !text.text.is_empty()
                    || matches!(replay(), Replay::Item(item) if item.get("audio").is_some())
            }
            AssistantContent::Reasoning(_) => match replay() {
                Replay::Item(item) => item.get("type").is_some(),
                Replay::Identity(identity) => {
                    identity.get("type").and_then(Value::as_str) == Some("thinking")
                }
                Replay::Rebuild => false,
            },
            AssistantContent::Opaque(opaque) => opaque.item.get("type").is_some(),
            AssistantContent::Image(_) => false,
        }
    }
}

type ImageRule<'a> = (&'a str, fn(&str) -> bool);

fn image_rule(vendor: &str) -> Option<ImageRule<'_>> {
    use crate::codecs::providers::{minimax, moonshot, xiaomimimo, zai};
    Some(match vendor {
        "deepseek" | "mira" => return None,
        "groq" => ("groq", |model| model.contains("llama-4")),
        "mistral" | "mistralai" => ("mistral", |model| {
            !(model.starts_with("codestral") || model.starts_with("devstral"))
        }),
        "openai" | "azure.openai" => ("openai", crate::codecs::providers::openai::reads_images),
        "xai" | "x-ai" => ("xai", crate::codecs::providers::xai::reads_images),
        "cohere" => ("cohere", crate::codecs::providers::cohere::reads_images),
        "zai" | "z-ai" => ("zai", zai::reads_images),
        "moonshot" | "moonshotai" => ("moonshot", moonshot::reads_images),
        "minimax" => ("minimax", minimax::reads_images),
        "xiaomimimo" | "xiaomi" => ("xiaomimimo", xiaomimimo::reads_images),
        vendor => (vendor, |_| true),
    })
}

fn reads_images(facts: &ModelFacts, dialect: &super::Dialect, model: &str) -> bool {
    match model.split_once('/') {
        Some((vendor, upstream)) if dialect.quirks.rewrite == BodyRewrite::OpenRouter => facts
            .reads_images_or(dialect.name, model, |_| {
                image_rule(vendor).is_some_and(|(_, rule)| rule(upstream))
            }),
        _ => image_rule(dialect.name)
            .is_some_and(|(vendor, rule)| facts.reads_images_or(vendor, model, rule)),
    }
}

pub enum ChatEvent {
    Chunk(Value),
    Whole(Value),
    Done,
    Failure(ProviderError),
    BareText(String),
}

const REASONING_TEXT_KEYS: [&str; 3] = ["reasoning_content", "reasoning", "reasoning_text"];

const REASONING_DETAILS: &str = "reasoning_details";

#[derive(Debug, PartialEq)]
pub(crate) enum Part {
    Text(String),
    Thinking(String),
    Image,
    Unknown,
}

impl Part {
    pub(crate) fn of(part: &Value) -> Self {
        let text = |key: &str| part.str(key).unwrap_or_default().to_owned();
        match part.str("type") {
            Some("text") => Self::Text(text("text")),
            None if part.str("text").is_some() => Self::Text(text("text")),
            Some("refusal") => Self::Text(text("refusal")),
            Some("thinking") => Self::Thinking(match part.get("thinking") {
                Some(Value::Array(chunks)) => chunks
                    .iter()
                    .filter_map(|chunk| chunk.str("text"))
                    .collect(),
                _ => text("thinking"),
            }),
            Some("image_url") => Self::Image,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum CallKind {
    Function,
    Custom,
    Unknown,
}

impl CallKind {
    pub(crate) fn of(call: &Value) -> Self {
        match call.str("type") {
            None | Some("function") => Self::Function,
            Some("custom") => Self::Custom,
            Some(_) => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Writing {
    Text,
    Thinking,
}

struct OpenCall {
    at: usize,
    id: Option<String>,
    opaque: bool,
    arguments: String,
}

#[derive(Default)]
pub struct ChatDecoder {
    quirks: Quirks,
    writing: Option<(Writing, usize)>,
    thinking_text: String,
    reasoning: Option<usize>,
    reasoning_field: Option<&'static str>,
    reasoning_text: String,
    reasoning_details: Map<String, Value>,
    audio_id: Option<Value>,
    calls: Vec<OpenCall>,
    usage: Option<Value>,
    reply_fields: Map<String, Value>,
    first_text: Option<usize>,
    last_text: Option<usize>,
    turn_citations: Vec<crate::codecs::wire::WireCitation>,
    finish: Option<FinishReason>,
    response_id: Option<String>,
    response_model: Option<String>,
    ended: bool,
    chunked: bool,
}

impl ChatDecoder {
    fn new(quirks: Quirks) -> Self {
        Self {
            quirks,
            ..Self::default()
        }
    }

    fn absorb<'f>(&mut self, frame: &'f Value) -> Option<&'f Value> {
        // Every chunk restates these; each is stored only when it changes.
        keep_text(&mut self.response_id, frame.str("id"));
        keep_text(&mut self.response_model, frame.str("model"));
        // `n > 1` streams interleave candidates told apart only by
        // `choices[].index`; candidate 0 is the turn, as in a whole reply.
        let choice = frame.arr("choices").iter().find(|choice| {
            let index = choice.get("index").and_then(Value::as_u64);
            index.is_none_or(|index| index == 0)
        });
        if let Some(usage) = frame
            .at("/usage")
            .or_else(|| choice.and_then(|choice| choice.at("/usage")))
            && self.usage.as_ref() != Some(usage)
        {
            self.usage = Some(usage.clone());
        }
        for key in reported::REPLY_FIELDS {
            if let Some(value) = frame.get(key).filter(|value| !value.is_null())
                && self.reply_fields.get(key) != Some(value)
            {
                self.reply_fields.insert(key.to_owned(), value.clone());
            }
        }
        let choice = choice?;
        // A gateway's upstream-native reason is consulted only when the
        // normalized field is absent or empty (OpenRouter's precedence).
        let reason = match choice
            .str("finish_reason")
            .filter(|reason| !reason.is_empty())
        {
            Some(reason) => Some(finish_reason(reason, &self.quirks)),
            None => choice
                .str("native_finish_reason")
                .filter(|reason| self.quirks.native_finish_reason && !reason.is_empty())
                .map(native_finish_reason),
        };
        if let Some(reason) = reason {
            self.finish = Some(reason);
            self.ended = true;
        }
        Some(choice)
    }

    fn delta(
        &mut self,
        delta: &Map<String, Value>,
        out: &mut Out<'_, Completion>,
    ) -> Result<(), ProviderError> {
        let reasoning = REASONING_TEXT_KEYS.iter().find_map(|key| {
            delta
                .get(*key)
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(|text| (*key, text))
        });
        if let Some((key, text)) = reasoning {
            self.reason(text, out)?;
            self.reasoning_field.get_or_insert(key);
        }
        if let Some(Value::Array(details)) = delta.get(REASONING_DETAILS)
            && !details.is_empty()
        {
            self.reason("", out)?;
            merge_details(&mut self.reasoning_details, details.clone());
        }
        let audio = delta.get("audio");
        if let Some(id) = audio.and_then(|audio| audio.at("/id")) {
            self.audio_id = Some(id.clone());
        }
        match delta.get("content") {
            Some(Value::Array(parts)) => {
                for part in parts {
                    self.part(part, out)?;
                }
            }
            Some(part @ Value::Object(_)) => self.part(part, out)?,
            _ => {
                let text = ["content", "refusal"]
                    .iter()
                    .find_map(|key| {
                        delta
                            .get(*key)
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                    })
                    .or_else(|| audio.and_then(|audio| audio.str("transcript")))
                    .filter(|text| !text.is_empty());
                if let Some(text) = text {
                    self.write(Writing::Text, text, out)?;
                }
            }
        }
        for image in delta
            .get("images")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            self.image(image, out)?;
        }
        for call in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            self.call(call, out)?;
        }
        self.turn_citations.extend(
            delta
                .get("annotations")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(reported::annotation),
        );
        self.cite_turn(out);
        Ok(())
    }

    fn cite_turn(&mut self, out: &mut Out<'_, Completion>) {
        if let Some(text) = self.first_text {
            for citation in self.turn_citations.drain(..) {
                out.cite(text, citation);
            }
        }
    }

    fn reason(&mut self, text: &str, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
        let at = match self.reasoning {
            Some(at) => at,
            None => {
                let at = out.fresh_index();
                out.open(at, Block::Reasoning { redacted: false }, Value::Null)?;
                out.lead(at)?;
                self.reasoning = Some(at);
                at
            }
        };
        self.reasoning_text.push_str(text);
        out.push(at, text)
    }

    fn close_reasoning(&mut self, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
        let Some(at) = self.reasoning.take() else {
            return Ok(());
        };
        let text = std::mem::take(&mut self.reasoning_text);
        let mut item = Map::new();
        if let Some(field) = self.reasoning_field.take() {
            item.insert(field.to_owned(), text.into());
        }
        item.append(&mut self.reasoning_details);
        out.edit(at, |slot| *slot = Value::Object(item))?;
        out.finish(at)
    }

    fn write(
        &mut self,
        writing: Writing,
        text: &str,
        out: &mut Out<'_, Completion>,
    ) -> Result<(), ProviderError> {
        let index = match self.writing {
            Some((current, index)) if current == writing => index,
            _ => {
                self.close_writing(out)?;
                let index = out.fresh_index();
                let block = match writing {
                    Writing::Thinking => Block::Reasoning { redacted: false },
                    Writing::Text => {
                        self.first_text.get_or_insert(index);
                        self.last_text = Some(index);
                        Block::Text
                    }
                };
                out.open(index, block, Value::Null)?;
                self.writing = Some((writing, index));
                index
            }
        };
        if writing == Writing::Thinking {
            self.thinking_text.push_str(text);
        }
        out.push(index, text)
    }

    fn close_writing(&mut self, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
        let (index, item) = match self.writing.take() {
            None => return Ok(()),
            Some((Writing::Text, index)) => (
                index,
                self.audio_id
                    .take()
                    .map(|id| json!({ "audio": { "id": id } })),
            ),
            Some((Writing::Thinking, index)) => {
                let text = std::mem::take(&mut self.thinking_text);
                (
                    index,
                    Some(json!({"type": "thinking", "thinking": [{"type": "text", "text": text}]})),
                )
            }
        };
        if let Some(item) = item {
            out.edit(index, |slot| *slot = item)?;
        }
        out.finish(index)
    }

    #[deny(clippy::wildcard_enum_match_arm)]
    fn part(&mut self, part: &Value, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
        match Part::of(part) {
            Part::Text(text) => self.write(Writing::Text, &text, out),
            Part::Thinking(text) => self.write(Writing::Thinking, &text, out),
            Part::Image => self.image(part, out),
            Part::Unknown => {
                self.close_writing(out)?;
                if let Some(citation) = reported::reference(part) {
                    match self.last_text {
                        Some(text) => out.cite(text, citation),
                        None => self.turn_citations.push(citation),
                    }
                }
                let index = out.fresh_index();
                out.whole(index, Block::Opaque { replay: true }, part.clone(), "")
            }
        }
    }

    fn image(&mut self, part: &Value, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
        use crate::codecs::message::{Image, ImageMediaType};
        let url = part
            .at("/image_url/url")
            .or_else(|| part.get("image_url"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let inline = url
            .strip_prefix("data:")
            .and_then(|rest| rest.split_once(";base64,"));
        let mut image = Image::default();
        let data = match inline {
            Some((mime, data)) => {
                image.data = Source::Base64(String::new());
                image.media_type = ImageMediaType::from_mime_type(mime);
                data
            }
            None => {
                image.data = Source::Url(url.to_owned());
                ""
            }
        };
        self.close_writing(out)?;
        let index = out.fresh_index();
        out.whole(index, Block::Image(image), part.clone(), data)
    }

    fn call(&mut self, call: &Value, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
        self.close_writing(out)?;
        let id = match call.get("id") {
            Some(Value::String(id)) if !id.is_empty() && id != "null" => Some(id.clone()),
            Some(id @ Value::Number(_)) => Some(id.to_string()),
            _ => None,
        };
        let index = call
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok());
        let arguments = call
            .at("/function/arguments")
            .map(crate::codecs::json_utils::value_to_json_string);
        let name = call
            .at("/function/name")
            .or_else(|| call.at("/custom/name"))
            .and_then(Value::as_str);
        let starts = name.is_some_and(|name| !name.is_empty())
            || arguments.as_deref().is_some_and(|text| !text.is_empty());
        let at = match index {
            Some(index) => index,
            None => match id.as_deref() {
                Some(id) => self
                    .calls
                    .iter()
                    .find(|open| open.id.as_deref() == Some(id)),
                None => self.calls.last().filter(|open| {
                    open.opaque
                        || !crate::codecs::json_utils::is_whole_object(&open.arguments)
                        || !starts
                }),
            }
            .map_or_else(|| out.fresh_index(), |open| open.at),
        };
        let opaque = match self.calls.iter_mut().find(|open| open.at == at) {
            Some(open) => {
                if id.is_some() {
                    open.id.clone_from(&id);
                }
                open.arguments
                    .push_str(arguments.as_deref().unwrap_or_default());
                open.opaque
            }
            None => {
                let opaque = CallKind::of(call) == CallKind::Unknown;
                if opaque {
                    out.open(at, Block::Opaque { replay: false }, Value::Null)?;
                }
                self.calls.push(OpenCall {
                    at,
                    id: id.clone(),
                    opaque,
                    arguments: arguments.clone().unwrap_or_default(),
                });
                opaque
            }
        };
        if !opaque {
            out.fragment(
                Some(at),
                CallFragment {
                    id: id.as_deref(),
                    name,
                    arguments: arguments.as_deref(),
                },
            )?;
        }
        out.edit(at, |item| {
            if !item.is_object() {
                *item = Value::Object(Map::new());
            }
            if let (Value::Object(item), Some(fields)) = (item, call.as_object()) {
                merge_fields(item, fields);
                // The index orders the stream; the call it assembles has none.
                remove_preserving_order(item, "index");
            }
        })
    }

    fn close_calls(
        &mut self,
        out: &mut Out<'_, Completion>,
        all: bool,
    ) -> Result<(), ProviderError> {
        let cut = self.finish == Some(FinishReason::Length);
        let mut waiting = Vec::new();
        for call in std::mem::take(&mut self.calls) {
            if call.opaque {
                out.finish(call.at)?;
                continue;
            }
            let (mut input, mut complete) = (None, false);
            out.edit(call.at, |item| {
                if CallKind::of(item) == CallKind::Custom {
                    let custom = item.at("/custom/input").cloned();
                    input = Some(custom.unwrap_or_else(|| Value::from("")));
                }
                complete = match item.at("/function/arguments") {
                    Some(Value::String(text)) => matches!(
                        crate::codecs::json_utils::parse_tool_arguments(text),
                        Ok(Value::Object(_))
                    ),
                    Some(arguments) => arguments.is_object(),
                    None => input.is_some(),
                };
            })?;
            if !all && !complete {
                waiting.push(call);
                continue;
            }
            if let Some(input) = input {
                out.announce(call.at, json!({ "input": input }))?;
            }
            if cut && !complete {
                out.close(call.at)?;
            } else {
                out.finish(call.at)?;
            }
        }
        self.calls = waiting;
        Ok(())
    }

    fn chunk(&mut self, frame: &Value, out: &mut Out<'_, Completion>) -> Result<(), ProviderError> {
        let Some(choice) = self.absorb(frame) else {
            return Ok(());
        };
        if let Some(delta) = choice.obj("delta") {
            self.delta(delta, out)?;
        }
        // Text and reasoning stay open until the stream ends, as the protocol closes
        // its blocks (`openai-completions.ts` around 680): a provider may
        // still send either after the finish chunk.
        if self.finish == Some(FinishReason::ToolCalls) {
            self.close_calls(out, false)?;
        }
        Ok(())
    }

    fn whole(
        &mut self,
        frame: &Value,
        mut out: Out<'_, Completion>,
    ) -> Result<Flow, ProviderError> {
        let Some(choice) = self.absorb(frame) else {
            return Err(ProviderError::Response(
                "Response contained no choices".to_owned(),
            ));
        };
        let Some(mut message) = choice.obj("message").cloned() else {
            return Err(ProviderError::Response(
                "Response did not contain a valid message or tool call".to_owned(),
            ));
        };
        self.ended = true;
        if let Some(Value::Array(calls)) = message.get_mut("tool_calls") {
            for (index, call) in calls.iter_mut().enumerate() {
                if let Some(call) = call.as_object_mut() {
                    call.insert("index".to_owned(), index.into());
                }
            }
        }
        self.delta(&message, &mut out)?;
        self.close_calls(&mut out, true)?;
        self.end(out)
    }

    fn end(&mut self, mut out: Out<'_, Completion>) -> Result<Flow, ProviderError> {
        self.close_writing(&mut out)?;
        self.close_reasoning(&mut out)?;
        self.cite_turn(&mut out);
        if let Some(text) = self.first_text {
            for citation in reported::listed(&self.reply_fields) {
                out.cite(text, citation);
            }
        }
        let cost = reported::cost(self.usage.as_ref(), &self.reply_fields);
        let usage = self
            .usage
            .as_ref()
            .map(|usage| normalized_usage(usage, &self.quirks))
            .unwrap_or_default()
            .cost(cost);
        Ok(out.end(Finish {
            usage,
            reason: self.finish.take(),
            response_id: self.response_id.take(),
            model: self.response_model.take(),
            ..Finish::default()
        }))
    }

    fn finish(&mut self, mut out: Out<'_, Completion>) -> Result<Flow, ProviderError> {
        if !self.ended {
            return Err(ProviderError::Truncated);
        }
        self.close_calls(&mut out, true)?;
        self.end(out)
    }

    fn done(&mut self, out: Out<'_, Completion>) -> Result<Flow, ProviderError> {
        if !self.ended && self.chunked && self.quirks.done_without_finish_reason {
            self.finish = Some(if self.calls.is_empty() {
                FinishReason::Stop
            } else {
                FinishReason::ToolCalls
            });
            self.ended = true;
        }
        self.finish(out)
    }
}

fn keep_text(slot: &mut Option<String>, text: Option<&str>) {
    if let Some(text) = text
        && slot.as_deref() != Some(text)
    {
        *slot = Some(text.to_owned());
    }
}

fn merge_details(block: &mut Map<String, Value>, details: Vec<Value>) {
    let Value::Array(merged) = block
        .entry(REASONING_DETAILS)
        .or_insert_with(|| Value::Array(Vec::new()))
    else {
        return;
    };
    for detail in details {
        let kind = detail.str("type");
        let continues = matches!(kind, Some("reasoning.text" | "reasoning.summary"))
            && merged.last().is_some_and(|last| {
                last.str("type") == kind && last.get("index") == detail.get("index")
            });
        match (continues, merged.last_mut(), detail) {
            (true, Some(Value::Object(last)), Value::Object(fields)) => {
                for (key, value) in fields {
                    let missing = last
                        .get(&key)
                        .is_none_or(|existing| existing.is_null() || existing.as_str() == Some(""));
                    match (last.get_mut(&key), value) {
                        (Some(Value::String(text)), Value::String(more))
                            if matches!(key.as_str(), "text" | "summary") =>
                        {
                            text.push_str(&more);
                        }
                        (_, value) if missing => {
                            last.insert(key, value);
                        }
                        _ => {}
                    }
                }
            }
            (_, _, detail) => merged.push(detail),
        }
    }
}

#[deny(clippy::wildcard_enum_match_arm)]
impl<'id> Decoder<'id, Completion> for ChatDecoder {
    type Event = ChatEvent;

    fn classify(&self, frame: WireFrame) -> WireEvent<ChatEvent> {
        let data = frame.as_str();
        // `[DONE]` is the wire's terminal sentinel, not JSON.
        if data == "[DONE]" {
            return WireEvent::Known(ChatEvent::Done);
        }
        // The in-band error envelope arrives with a 200 status and is this
        // wire's own terminal failure, whatever else the frame is.
        let envelope = |value: &Value| provider_error_envelope(value, &data);
        match classify_chat_completions_frame::<Value>(&data) {
            WireEvent::Known(frame) => WireEvent::Known(match envelope(&frame) {
                Some(error) => ChatEvent::Failure(error),
                None => Self::chunk_or_whole(frame),
            }),
            WireEvent::Unknown { event_type, value } => {
                if let Some(error) = envelope(value.value()) {
                    WireEvent::Known(ChatEvent::Failure(error))
                } else if let (true, Some(text)) = (
                    self.quirks.accepts_bare_string_reply,
                    value.value().as_str(),
                ) {
                    WireEvent::Known(ChatEvent::BareText(text.to_owned()))
                } else {
                    WireEvent::Unknown { event_type, value }
                }
            }
            // A frame with a duplicate `object` or `choices` is still JSON.
            WireEvent::Corrupt(error) => match serde_json::from_str::<Value>(&data)
                .ok()
                .and_then(|value| envelope(&value))
            {
                Some(error) => WireEvent::Known(ChatEvent::Failure(error)),
                None => WireEvent::Corrupt(error),
            },
        }
    }

    fn decode(
        &mut self,
        event: ChatEvent,
        mut out: Out<'id, Completion>,
    ) -> Result<Flow, ProviderError> {
        match event {
            ChatEvent::Chunk(frame) => {
                self.chunked = true;
                self.chunk(&frame, &mut out)?;
                Ok(Flow::More)
            }
            ChatEvent::Whole(frame) => self.whole(&frame, out),
            ChatEvent::Done => self.done(out),
            // A bare string is the whole answer, ended: the protocol's stop for a
            // reply that names no reason.
            ChatEvent::BareText(text) => {
                self.ended = true;
                self.finish = Some(FinishReason::Stop);
                self.write(Writing::Text, &text, &mut out)?;
                self.end(out)
            }
            ChatEvent::Failure(error) => Err(error),
        }
    }

    fn eof(&mut self, out: Out<'id, Completion>) -> Result<Flow, ProviderError> {
        self.finish(out)
    }
}

fn normalized_usage(usage: &Value, quirks: &Quirks) -> crate::codecs::completion::Usage {
    let count = |pointer: &str| usage.at(pointer).and_then(Value::as_u64);
    let detail = |object: &str, key: &str| {
        count(&format!("/{object}/{key}")).or(usage.obj(object).map(|_| 0))
    };
    let (prompt, completion, total) = (
        count("/prompt_tokens"),
        count("/completion_tokens"),
        count("/total_tokens"),
    );
    let audio = count("/prompt_tokens_details/audio_tokens").unwrap_or(0);
    let input = prompt.map(|prompt| {
        let beside = prompt.saturating_add(audio);
        let accounted = beside.saturating_add(completion.unwrap_or(0));
        if audio != 0 && Some(accounted) == total {
            beside
        } else {
            prompt
        }
    });
    crate::codecs::completion::Usage {
        input_tokens: input,
        output_tokens: completion
            .or_else(|| total.map(|total| total.saturating_sub(input.unwrap_or(0)))),
        total_tokens: total,
        cached_input_tokens: detail("prompt_tokens_details", "cached_tokens")
            .or(count("/num_cached_tokens"))
            .or(count("/prompt_cache_hit_tokens")),
        cache_creation_input_tokens: count("/prompt_tokens_details/cache_write_tokens"),
        reasoning_tokens: detail("completion_tokens_details", "reasoning_tokens")
            .filter(|_| quirks.reliable_reasoning_count),
        ..Default::default()
    }
}

impl ChatDecoder {
    fn chunk_or_whole(frame: Value) -> ChatEvent {
        let whole = match frame.str("object") {
            Some(object) => object == "chat.completion",
            None => frame
                .arr("choices")
                .iter()
                .any(|choice| choice.obj("message").is_some()),
        };
        if whole {
            ChatEvent::Whole(frame)
        } else {
            ChatEvent::Chunk(frame)
        }
    }

    pub(crate) fn project(payload: &[u8], sink: &mut ObservationSink<'_>) {
        let Ok(payload) = serde_json::from_slice::<Value>(payload) else {
            return;
        };
        // Only the chunk that carries the finish reason is a verdict, so the
        // model rides with it rather than on each delta.
        let verdict = match payload
            .at("/choices/0/finish_reason")
            .and_then(Value::as_str)
        {
            Some(reason) => AdapterVerdict {
                finish_reason: Some(sink.scrub(reason)),
                block_reason: None,
                detail: None,
                model: payload.str("model").map(|value| sink.scrub(value)),
            },
            None => AdapterVerdict::default(),
        };
        let response_id = payload.str("id").map(|value| sink.scrub(value));
        sink.provider(verdict, response_id);
        if let Some(error) = ObservedError::of(&payload) {
            error.emit(sink);
        }
    }
}

mod document;
pub(crate) mod finish;
mod reported;
