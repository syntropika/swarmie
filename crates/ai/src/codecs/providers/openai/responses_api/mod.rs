use crate::codecs::completion::history::Replay;
use crate::codecs::completion::options::{
    BaseInput, CacheRetention, FinalBody, RawAt, Rewrite, request_params,
};
use crate::codecs::error::EncodeError;
use crate::codecs::json_utils;
use crate::codecs::json_utils::Lenient;
use crate::codecs::message::{
    AssistantContent, Document, DocumentData, DocumentMediaType, DocumentSourceKind, Message,
    MimeType, ToolResultContent, UserContent,
};
use crate::codecs::providers::internal::object;
use crate::codecs::providers::internal::wire_ids::WireIds;
use crate::codecs::wire::Mode;
use crate::codecs::{completion, message};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value, json};
use std::sync::Arc;

pub mod streaming;

pub mod wire;

fn image_part(image: &message::Image) -> Option<Value> {
    let (key, source) = match &image.data {
        DocumentSourceKind::Base64(data) => (
            "image_url",
            format!(
                "data:{};base64,{data}",
                image.media_type.as_ref()?.to_mime_type()
            ),
        ),
        DocumentSourceKind::Url(url) => ("image_url", url.clone()),
        DocumentSourceKind::FileId(file_id) => ("file_id", file_id.clone()),
        _ => return None,
    };
    let mut part =
        json!({"type": "input_image", "detail": image.detail.clone().unwrap_or_default()});
    set(&mut part, key, Value::String(source));
    Some(part)
}

fn document_part(document: &Document) -> Option<Value> {
    let source = match &document.data {
        DocumentData::Text(text) => return Some(json!({"type": "input_text", "text": text})),
        DocumentData::File(source) => source,
    };
    Some(match source {
        DocumentSourceKind::FileId(file_id) => json!({"type": "input_file", "file_id": file_id}),
        // `input_file` reads the type of a file it fetches itself.
        DocumentSourceKind::Url(url) => json!({"type": "input_file", "file_url": url}),
        DocumentSourceKind::Base64(data) if document.media_type == Some(DocumentMediaType::PDF) => {
            object! {
                "type": "input_file",
                "file_data": format!("data:application/pdf;base64,{data}"),
                "filename": "document.pdf",
            }
        }
        _ => return None,
    })
}

fn set(value: &mut Value, key: &str, field: Value) {
    if let Some(fields) = value.as_object_mut() {
        fields.insert(key.to_owned(), field);
    }
}

fn unsendable(part: &str) -> EncodeError {
    EncodeError::request(format!(
        "the Responses API cannot carry this {part}; prepare the request first"
    ))
}

fn result_output(content: &[ToolResultContent]) -> Result<Value, EncodeError> {
    let mut parts = content
        .iter()
        .map(|part| match part {
            ToolResultContent::Text(text) => Ok(json!({"type": "input_text", "text": text.text})),
            ToolResultContent::Json { value } => {
                Ok(json!({"type": "input_text", "text": value.to_string()}))
            }
            ToolResultContent::Image(image) => {
                image_part(image).ok_or_else(|| unsendable("tool-result image"))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(match parts.as_mut_slice() {
        [part] if part.str("type") == Some("input_text") => {
            part.get_mut("text").map(Value::take).unwrap_or_default()
        }
        _ => Value::Array(parts),
    })
}

struct Custom {
    tools: std::collections::HashSet<String>,
    calls: std::collections::HashMap<String, bool>,
}

fn input(
    history: &[Arc<Message>],
    target: &wire::Responses,
    model: &str,
    custom: &mut Custom,
    stateless: bool,
) -> Result<(Vec<Value>, Layout), EncodeError> {
    let ids = WireIds::for_target(history, target, model);
    let mut items = Vec::new();
    let mut layout = Layout::default();
    for (position, message) in history.iter().enumerate() {
        if layout.after.is_empty() && !matches!(&**message, Message::System { .. }) {
            layout.after.push(items.len());
        }
        let mut parts = Vec::new();
        match &**message {
            Message::System { content } => items.push(json!({
                "type": "message",
                "role": target.provider.dialect.quirks.responses.system_role,
                "content": [{"type": "input_text", "text": content}],
            })),
            Message::User { content } => {
                for part in content {
                    parts.push(items.len());
                    let part = match part {
                        // Blank text says nothing, and some backends reject it.
                        UserContent::Text(text) if text.text.trim().is_empty() => continue,
                        UserContent::Text(text) => json!({"type": "input_text", "text": text.text}),
                        // A function output has no error field: a failed
                        // result says so in its text.
                        UserContent::ToolResult(result) => {
                            let call_id = ids.spell(&result.call);
                            let output = result_output(&result.content)?;
                            items.push(
                                if custom.calls.get(&call_id).copied().unwrap_or_else(|| {
                                    custom.tools.contains(result.name.as_str())
                                }) {
                                    object! {"type": "custom_tool_call_output", "call_id": call_id, "output": output}
                                } else {
                                    object! {"type": "function_call_output", "call_id": call_id, "output": output, "status": "completed"}
                                },
                            );
                            continue;
                        }
                        UserContent::Image(image) => {
                            image_part(image).ok_or_else(|| unsendable("image"))?
                        }
                        UserContent::Document(document) => {
                            document_part(document).ok_or_else(|| unsendable("document"))?
                        }
                        UserContent::Audio(_) => return Err(unsendable("audio")),
                        UserContent::Video(_) => return Err(unsendable("video")),
                    };
                    items.push(object! {"type": "message", "role": "user", "content": vec![part]});
                }
            }
            Message::Assistant(turn) => {
                let mut texts = 0usize;
                let mut unpaired = false;
                for block in &turn.content {
                    parts.push(items.len());
                    let mut replay = block.replay(target, &ids);
                    if unpaired && !matches!(block, AssistantContent::Reasoning(_)) {
                        unpaired = false;
                        replay = Replay::Rebuild;
                    }
                    let ciphertext = |item: &Map<String, Value>| {
                        item.get("encrypted_content")
                            .and_then(Value::as_str)
                            .is_some_and(|cipher| !cipher.is_empty())
                    };
                    let unresolvable = stateless
                        && target
                            .provider
                            .dialect
                            .quirks
                            .responses
                            .reasoning_ciphertext
                        && matches!(block, AssistantContent::Reasoning(_))
                        && match &replay {
                            Replay::Item(item) => !item.as_object().is_some_and(ciphertext),
                            Replay::Identity(identity) => !ciphertext(identity),
                            Replay::Rebuild => false,
                        };
                    if unresolvable {
                        unpaired = true;
                        continue;
                    }
                    let identity = match replay {
                        Replay::Item(item) => {
                            if let Some(call_id) = item.str("call_id") {
                                let kind = item.str("type");
                                if matches!(kind, Some("custom_tool_call" | "function_call")) {
                                    custom.calls.insert(
                                        call_id.to_owned(),
                                        kind == Some("custom_tool_call"),
                                    );
                                }
                            }
                            items.push(item.into_owned());
                            continue;
                        }
                        Replay::Identity(identity) => identity,
                        Replay::Rebuild => Map::new(),
                    };
                    let id = |prefix: &str| {
                        identity
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|id| !id.is_empty() && id.starts_with(prefix) && id.len() <= 64)
                            .map(str::to_owned)
                    };
                    let item = match block {
                        AssistantContent::Text(text) => {
                            let synthetic = match texts {
                                0 => format!("msg_rig_{position}"),
                                n => format!("msg_rig_{position}_{n}"),
                            };
                            texts += 1;
                            let mut item = json!({
                                "type": "message",
                                "role": "assistant",
                                "content": [{"type": "output_text", "text": text.text, "annotations": []}],
                                "status": "completed",
                                "id": id("").unwrap_or(synthetic),
                            });
                            if let Some(phase) =
                                identity.get("phase").filter(|phase| phase.is_string())
                            {
                                set(&mut item, "phase", phase.clone());
                            }
                            items.push(item);
                            continue;
                        }
                        AssistantContent::ToolCall(call) => {
                            let call_id = ids.spell(&call.id);
                            let name = call.function.name.as_str();
                            let kind = identity.get("type").and_then(Value::as_str);
                            let arguments = || {
                                serde_json::to_string(&call.function.arguments).unwrap_or_default()
                            };
                            let (kind, key, payload, prefix) = if kind == Some("custom_tool_call")
                                || (kind.is_none() && custom.tools.contains(name))
                            {
                                custom.calls.insert(call_id.clone(), true);
                                let input =
                                    call.function.arguments.get("input").and_then(Value::as_str);
                                let input = input.map_or_else(arguments, str::to_owned);
                                ("custom_tool_call", "input", input, "ctc_")
                            } else {
                                custom.calls.insert(call_id.clone(), false);
                                ("function_call", "arguments", arguments(), "fc_")
                            };
                            let mut item = json!({"type": kind, "call_id": call_id, "name": name});
                            set(&mut item, key, Value::String(payload));
                            if let Some(id) = id(prefix) {
                                set(&mut item, "id", json!(id));
                            }
                            item
                        }
                        // An edited reasoning block keeps its item's id and
                        // ciphertext, so the items after it stay paired.
                        AssistantContent::Reasoning(reasoning) => {
                            let Some(rs) = id("") else {
                                continue;
                            };
                            let summary: Vec<Value> = (!reasoning.text.is_empty())
                                .then(|| json!({"type": "summary_text", "text": reasoning.text}))
                                .into_iter()
                                .collect();
                            let mut item =
                                object! {"type": "reasoning", "id": rs, "summary": summary};
                            if let Some(ciphertext) = identity.get("encrypted_content") {
                                set(&mut item, "encrypted_content", ciphertext.clone());
                            }
                            item
                        }
                        AssistantContent::Opaque(opaque) if opaque.replay => opaque.item.clone(),
                        AssistantContent::Opaque(_) => continue,
                        AssistantContent::Image(_) => return Err(unsendable("assistant image")),
                    };
                    items.push(item);
                }
            }
        }
        if !matches!(&**message, Message::System { .. }) {
            layout.after.push(items.len());
            layout.parts.push(parts);
        }
    }
    if layout.after.is_empty() {
        layout.after.push(items.len());
    }
    Ok((items, layout))
}

#[derive(Default)]
struct Layout {
    after: Vec<usize>,
    parts: Vec<Vec<usize>>,
}

impl Layout {
    fn boundary(&self, point: completion::CachePoint) -> Option<usize> {
        use completion::CachePoint;
        match point {
            CachePoint::Tools => None,
            CachePoint::System => self.after.first().copied(),
            CachePoint::Messages(turns) => self.after.get(turns).copied(),
            CachePoint::Within { message, items: 0 } => self.after.get(message).copied(),
            CachePoint::Within { message, items } => self
                .parts
                .get(message)
                .and_then(|parts| parts.get(items))
                .or_else(|| self.after.get(message.checked_add(1)?))
                .copied(),
        }
    }
}

fn tool_choice(choice: message::ToolChoice) -> Result<Value, EncodeError> {
    Ok(match choice {
        message::ToolChoice::Auto => json!("auto"),
        message::ToolChoice::None => json!("none"),
        message::ToolChoice::Required => json!("required"),
        message::ToolChoice::Specific { function_names } => match function_names.as_slice() {
            [] => {
                return Err(EncodeError::request(
                    "ToolChoice::Specific requires at least one function name",
                ));
            }
            [name] => json!({"type": "function", "name": name}),
            names => json!({
                "type": "allowed_tools",
                "mode": "required",
                "tools": names.iter().map(|name| json!({"type": "function", "name": name})).collect::<Vec<_>>(),
            }),
        },
    })
}

pub(crate) fn include_ciphertext(body: &mut Map<String, Value>) {
    const CIPHERTEXT: &str = "reasoning.encrypted_content";
    let mut include = body
        .get("include")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !include.iter().any(|item| item == CIPHERTEXT) {
        include.push(json!(CIPHERTEXT));
    }
    body.insert("include".to_owned(), Value::Array(include));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Delivery {
    Http(Mode),
    WebSocket,
}

impl wire::Responses {
    pub(crate) fn responses_request(
        &self,
        request: &completion::CompletionRequest,
        delivery: Delivery,
    ) -> Result<FinalBody, EncodeError> {
        let codex =
            self.provider.dialect.quirks.responses.contract == wire::ResponsesContract::Codex;
        let mut rewrites = Vec::new();
        match delivery {
            Delivery::Http(Mode::Streaming) => rewrites.push(Rewrite::Stream(true)),
            Delivery::Http(Mode::Unary) => rewrites.push(Rewrite::Stream(false)),
            Delivery::WebSocket => rewrites.push(Rewrite::NoStream),
        }
        if delivery == Delivery::WebSocket {
            rewrites.push(Rewrite::NoBackground);
        }
        if codex {
            rewrites.push(Rewrite::CodexStore);
        }
        // Reasoning replays without stored state only with its ciphertext.
        if self.provider.dialect.quirks.responses.reasoning_ciphertext {
            rewrites.push(Rewrite::ReasoningCiphertext(codex));
        }
        // The fields that stay the same turn to turn first, then the
        // conversation, then the output cap, which one call may change: a
        // request that appended to the conversation starts with the bytes
        // of the one before, as the provider reads its prompt.
        rewrites.push(Rewrite::MoveLast("input"));
        rewrites.push(Rewrite::MoveLast("max_output_tokens"));
        let body = request_params(
            self,
            request,
            |layers| self.base(request, codex, layers),
            RawAt::Top,
            &rewrites,
        )?;
        crate::codecs::providers::openai::options::check_body(
            self,
            &self.facts,
            request,
            body,
            crate::codecs::providers::openai::options::Endpoint::Responses,
        )
    }

    fn base(
        &self,
        request: &completion::CompletionRequest,
        codex: bool,
        layers: &mut BaseInput<'_>,
    ) -> Result<Map<String, Value>, EncodeError> {
        let model = request.model.clone().unwrap_or_else(|| self.model.clone());
        let mut tools: Vec<ResponsesToolDefinition> = request
            .tools
            .iter()
            .map(|tool| ResponsesToolDefinition::from(completion::ToolDefinition::clone(tool)))
            .collect();
        let extra = layers.raw_tools()?;
        if !extra.is_empty() {
            tools.extend(
                serde_json::from_value::<Vec<ResponsesToolDefinition>>(Value::Array(extra))
                    .map_err(|err| {
                        EncodeError::request(format!(
                            "Invalid OpenAI Responses tools payload in additional_params: {err}"
                        ))
                    })?,
            );
        }
        tools.extend(self.tools.iter().cloned());
        if self.strict_tools {
            tools = tools
                .into_iter()
                .map(ResponsesToolDefinition::with_strict)
                .collect();
        }
        let mut custom = Custom {
            tools: tools
                .iter()
                .filter(|tool| tool.kind == "custom")
                .map(|tool| tool.name.clone())
                .collect(),
            calls: Default::default(),
        };
        let stateless = codex || layers.param("store") == Some(&json!(false));
        let caller = match layers.claim("input") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items,
            Some(other) => {
                return Err(EncodeError::request(format!(
                    "the OpenAI Responses `input` option must be an array of items, got {other}"
                )));
            }
        };
        // A result answering a held call takes that call's kind.
        for item in &caller {
            if let (Some(kind @ ("function_call" | "custom_tool_call")), Some(call_id)) =
                (item.str("type"), item.str("call_id"))
            {
                custom
                    .calls
                    .insert(call_id.to_owned(), kind == "custom_tool_call");
            }
        }
        let (items, layout) = input(&request.chat_history, self, &model, &mut custom, stateless)?;

        let system = |item: &Value| {
            (item.str("role") == Some(self.provider.dialect.quirks.responses.system_role)).then(
                || {
                    item.at("/content/0/text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                },
            )
        };
        let lift: Vec<bool> = match self.system_instructions {
            // The leading run of system messages, unless it is the whole
            // request, which then keeps them in `input` so it is not empty.
            SystemInstructionsPlacement::Instructions => {
                let leading = items
                    .iter()
                    .take_while(|item| system(item).is_some())
                    .count();
                let leading = if leading < items.len() || !caller.is_empty() {
                    leading
                } else {
                    0
                };
                (0..items.len()).map(|at| at < leading).collect()
            }
            SystemInstructionsPlacement::AllInstructions => {
                items.iter().map(|item| system(item).is_some()).collect()
            }
            SystemInstructionsPlacement::InputSystemMessages => vec![false; items.len()],
        };
        let mut lifted = Vec::new();
        let mut sent = caller;
        let offset = sent.len();
        for (item, lift) in items.into_iter().zip(&lift) {
            if *lift {
                lifted.extend(system(&item));
            } else {
                sent.push(item);
            }
        }
        if sent.is_empty() {
            return Err(EncodeError::request(if lifted.is_empty() {
                "OpenAI Responses request input must contain at least one item"
            } else {
                "OpenAI Responses request input must contain at least one non-system item \
             (system messages were lifted into the top-level `instructions` field)"
            }));
        }
        if self.marks_cache_points(&model) && layers.cache() != Some(CacheRetention::None) {
            // An item count of the request's own encoding, as one of `sent`.
            let at =
                |count: usize| offset + lift.iter().take(count).filter(|lifted| !**lifted).count();
            let points = request
                .cache_points
                .iter()
                .filter_map(|point| layout.boundary(*point))
                .map(at);
            mark_cache_points(&mut sent, offset, points);
        }
        let lifted: Vec<&str> = lifted
            .iter()
            .map(|text| text.trim())
            .filter(|text| !text.is_empty())
            .collect();
        let lifted = lifted.join("\n\n");
        // A gateway's own instructions go ahead of the caller's, once.
        let instructions = match &self.provider.instructions {
            Some(gateway) if lifted.is_empty() => Some(gateway.clone()),
            Some(gateway) if !lifted.contains(gateway.as_str()) => {
                Some(format!("{gateway}\n\n{lifted}"))
            }
            _ => (!lifted.is_empty()).then_some(lifted),
        };
        let text = request
            .output_schema
            .clone()
            .filter(|_| !codex)
            .map(|schema| {
                let (name, schema) = super::structured_output_schema(schema);
                json!({"format": {"type": "json_schema", "name": name, "schema": schema, "strict": true}})
            });

        Ok(crate::codecs::providers::internal::set_fields([
            ("model", Some(Value::String(model))),
            ("input", Some(Value::Array(sent))),
            ("instructions", instructions.map(Value::from)),
            (
                "max_output_tokens",
                request.max_tokens.filter(|_| !codex).map(Value::from),
            ),
            (
                "temperature",
                request.temperature.filter(|_| !codex).map(Value::from),
            ),
            (
                "tool_choice",
                request.tool_choice.clone().map(tool_choice).transpose()?,
            ),
            ("tools", (!tools.is_empty()).then(|| json!(tools))),
            ("text", text),
        ]))
    }
}

impl wire::Responses {
    fn marks_cache_points(&self, model: &str) -> bool {
        self.provider.dialect.name == super::wire::OPENAI.name
            && super::options::caches_by_options(&self.facts, model)
    }
}

fn mark_cache_points(items: &mut [Value], held: usize, ends: impl Iterator<Item = usize>) {
    for end in ends {
        let last = (held..end.min(items.len()))
            .rev()
            .find(|at| items.get(*at).is_some_and(markable));
        if let Some(item) = last.and_then(|at| items.get_mut(at)) {
            mark(item);
        }
    }
}

fn parts_field(item: &Value) -> Option<&'static str> {
    match item.str("type") {
        Some("message") if item.str("role") != Some("assistant") => Some("content"),
        Some("function_call_output" | "custom_tool_call_output") => Some("output"),
        _ => None,
    }
}

fn takes_breakpoint(part: &Value) -> bool {
    part.str("type")
        .is_some_and(|kind| ["input_text", "input_image", "input_file"].contains(&kind))
}

fn breakpoint_parts(item: &Value) -> impl Iterator<Item = &Value> {
    parts_field(item)
        .and_then(|field| item.get(field))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|part| takes_breakpoint(part))
}

fn markable(item: &Value) -> bool {
    breakpoint_parts(item).next().is_some()
        || (parts_field(item) == Some("output") && item.str("output").is_some())
}

fn mark(item: &mut Value) {
    if parts_field(item) == Some("output")
        && let Some(text) = item.str("output").map(str::to_owned)
    {
        set(
            item,
            "output",
            json!([{"type": "input_text", "text": text}]),
        );
    }
    parts_field(item)
        .and_then(|field| item.get_mut(field))
        .and_then(Value::as_array_mut)
        .into_iter()
        .filter_map(|parts| parts.iter_mut().rev().find(|part| takes_breakpoint(part)))
        .for_each(|part| set(part, "prompt_cache_breakpoint", json!({"mode": "explicit"})));
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct ResponsesToolDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
    #[serde(default, deserialize_with = "json_utils::null_or_default")]
    pub strict: bool,
    #[serde(default, deserialize_with = "json_utils::null_or_default")]
    pub description: String,
    #[serde(flatten, default)]
    pub config: Map<String, Value>,
}

impl Serialize for ResponsesToolDefinition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", &self.kind)?;
        if !self.name.is_empty() {
            map.serialize_entry("name", &self.name)?;
        }
        if !self.parameters.is_null() {
            map.serialize_entry("parameters", &self.parameters)?;
        }
        if self.kind == "function" {
            map.serialize_entry("strict", &self.strict)?;
        }
        if !self.description.is_empty() {
            map.serialize_entry("description", &self.description)?;
        }
        for (key, value) in &self.config {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl ResponsesToolDefinition {
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            kind: "function".to_string(),
            name: name.into(),
            parameters,
            strict: false,
            description: description.into(),
            config: Map::new(),
        }
    }

    pub fn strict_function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self::function(name, description, parameters).with_strict()
    }

    pub fn with_strict(mut self) -> Self {
        if self.kind == "function" {
            super::sanitize_schema(&mut self.parameters);
            self.strict = true;
        }
        self
    }

    pub fn hosted(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            name: String::new(),
            parameters: Value::Null,
            strict: false,
            description: String::new(),
            config: Map::new(),
        }
    }

    pub fn web_search() -> Self {
        Self::hosted("web_search")
    }

    pub fn file_search() -> Self {
        Self::hosted("file_search")
    }

    pub fn computer_use() -> Self {
        Self::hosted("computer_use")
    }

    pub fn with_config(mut self, key: impl Into<String>, value: Value) -> Self {
        self.config.insert(key.into(), value);
        self
    }
}

impl From<completion::ToolDefinition> for ResponsesToolDefinition {
    fn from(value: completion::ToolDefinition) -> Self {
        // The annotations are the host's: never sent.
        let completion::ToolDefinition {
            name,
            parameters,
            description,
            annotations: _,
        } = value;

        Self::function(name, description, parameters)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemInstructionsPlacement {
    #[default]
    Instructions,
    AllInstructions,
    InputSystemMessages,
}
