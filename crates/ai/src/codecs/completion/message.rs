use crate::codecs::error::ProviderError;
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, str::FromStr};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    System { content: String },

    User { content: Vec<UserContent> },

    Assistant(AssistantMessage),
}

pub mod citation;
mod identity;
mod native;

pub use citation::{Citation, DocumentRange, Source, SourceLocation, Span};

pub use identity::{CallId, EmptyCallId, EmptyToolName, LocalCallId, ProviderCallId, ToolName};
pub(crate) use native::Canonical;
pub use native::{Api, Blob, Fingerprint, Native, Opaque, Origin, StopReason};

#[non_exhaustive]
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct AssistantMessage {
    pub content: Vec<AssistantContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<StopReason>,
}

impl AssistantMessage {
    pub fn new(content: Vec<AssistantContent>) -> Self {
        Self {
            content,
            ..Self::default()
        }
    }

    pub fn rolled_back(origin: Option<Origin>, content: Vec<AssistantContent>) -> Self {
        Self {
            content,
            origin,
            stop: Some(StopReason::ToolUse),
        }
    }

    pub fn aborted(
        origin: Option<Origin>,
        content: Vec<AssistantContent>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            content,
            origin,
            stop: Some(StopReason::Aborted(reason.into())),
        }
    }

    pub fn with_content(mut self, content: Vec<AssistantContent>) -> Self {
        self.content = content;
        self
    }

    pub fn with_origin(mut self, origin: impl Into<Option<Origin>>) -> Self {
        self.origin = origin.into();
        self
    }

    pub fn with_stop(mut self, stop: impl Into<Option<StopReason>>) -> Self {
        self.stop = stop.into();
        self
    }

    pub fn tool_calls(&self) -> impl Iterator<Item = &ToolCall> {
        self.content.iter().filter_map(|part| match part {
            AssistantContent::ToolCall(call) => Some(call),
            _ => None,
        })
    }

    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|part| match part {
                AssistantContent::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn is_empty_turn(&self) -> bool {
        self.content.iter().all(AssistantContent::is_blank)
    }
}

pub fn turn_delivered_no_answer(choice: &[AssistantContent]) -> bool {
    !choice.iter().any(AssistantContent::is_answer)
}

pub fn turn_failure(
    choice: &[AssistantContent],
    stop: Option<&StopReason>,
    finish: Option<&crate::codecs::completion::FinishReason>,
) -> Option<String> {
    let failed = stop.filter(|stop| stop.is_failure());
    let calls = choice
        .iter()
        .any(|block| matches!(block, AssistantContent::ToolCall(_)));
    if let Some(StopReason::Error(reason) | StopReason::Aborted(reason)) = failed
        && calls
    {
        return Some(format!(
            "the turn failed, so none of its tool calls ran: {reason}"
        ));
    }
    let answered = !turn_delivered_no_answer(choice);
    if !answered && let Some(finish) = finish.filter(|finish| finish.truncated_output()) {
        return Some(finish.no_answer_message());
    }
    match failed {
        Some(StopReason::Error(reason)) if answered => {
            Some(format!("the provider failed the turn: {reason}"))
        }
        Some(StopReason::Error(reason)) => Some(format!(
            "the provider failed the turn without an answer: {reason}"
        )),
        _ => None,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum UserContent {
    Text(Text),
    ToolResult(ToolResult),
    Image(Image),
    Audio(Audio),
    Video(Video),
    Document(Document),
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AssistantContent {
    Text(Text),
    ToolCall(ToolCall),
    Reasoning(Reasoning),
    Image(Image),
    Opaque(Opaque),
}

impl AssistantContent {
    pub fn canonical(&self) -> Self {
        match self {
            Self::Text(text) => Self::Text(Text {
                text: text.text.clone(),
                citations: text.citations.clone(),
                native: None,
            }),
            Self::ToolCall(call) => {
                Self::ToolCall(ToolCall::new(call.id.clone(), call.function.clone()))
            }
            Self::Reasoning(reasoning) => Self::Reasoning(Reasoning {
                text: reasoning.text.clone(),
                redacted: reasoning.redacted,
                native: None,
            }),
            Self::Image(image) => Self::Image(Image {
                data: image.data.clone(),
                media_type: image.media_type.clone(),
                detail: image.detail.clone(),
                native: None,
            }),
            Self::Opaque(opaque) => Self::Opaque(opaque.clone()),
        }
    }

    pub fn into_canonical(mut self) -> Self {
        if let Some(native) = self.native_slot() {
            *native = None;
        }
        self
    }

    pub fn is_answer(&self) -> bool {
        match self {
            Self::Text(text) => !text.text.trim().is_empty(),
            Self::ToolCall(_) | Self::Image(_) => true,
            Self::Reasoning(_) | Self::Opaque(_) => false,
        }
    }

    pub fn is_blank(&self) -> bool {
        match self {
            Self::Text(Text { text, .. }) => text.trim().is_empty() && self.native_item().is_none(),
            Self::Reasoning(Reasoning { text, redacted, .. }) => {
                (*redacted || text.trim().is_empty()) && self.native_item().is_none()
            }
            Self::Opaque(opaque) => !opaque.replay,
            Self::ToolCall(_) | Self::Image(_) => false,
        }
    }

    pub fn fingerprint(&self) -> Fingerprint {
        // Projection v1 of the canonical fields, as a JSON array. An image's
        // media type and detail serialize as strings and its data as
        // `{"type", "value"}` (keys in order, no floats), so only call
        // arguments need `Canonical`.
        match self {
            Self::Text(text) => Fingerprint::of_canonical(&("v1", "text", &text.text)),
            Self::Reasoning(reasoning) => {
                Fingerprint::of_canonical(&("v1", "reasoning", &reasoning.text, reasoning.redacted))
            }
            Self::ToolCall(call) => call.fingerprint(),
            Self::Image(image) => Fingerprint::of_canonical(&(
                "v1",
                "image",
                &image.media_type,
                &image.detail,
                &image.data,
            )),
            Self::Opaque(_) => Fingerprint::of_canonical(&("v1", "opaque")),
        }
    }

    pub(crate) fn native_slot(&mut self) -> Option<&mut Option<Native>> {
        match self {
            Self::Text(text) => Some(&mut text.native),
            Self::ToolCall(call) => Some(&mut call.native),
            Self::Reasoning(reasoning) => Some(&mut reasoning.native),
            Self::Image(image) => Some(&mut image.native),
            Self::Opaque(_) => None,
        }
    }

    pub fn with_native(mut self, item: serde_json::Value) -> Self {
        let fingerprint = self.fingerprint();
        let baseline = match &self {
            Self::ToolCall(call) => call.baseline(),
            _ => None,
        };
        if let Some(native) = self.native_slot() {
            *native = Some(Native {
                item,
                fingerprint,
                baseline,
                edited: false,
                blob_slot: None,
            });
        }
        self
    }

    pub(crate) fn native(&self) -> Option<(&serde_json::Value, bool)> {
        let native = match self {
            Self::Text(text) => text.native.as_ref(),
            Self::ToolCall(call) => call.native.as_ref(),
            Self::Reasoning(reasoning) => reasoning.native.as_ref(),
            Self::Image(image) => image.native.as_ref(),
            Self::Opaque(_) => None,
        }?;
        let current = match self {
            Self::ToolCall(call) => call.native_is_current(native),
            _ => !native.edited && native.fingerprint == self.fingerprint(),
        };
        Some((&native.item, current))
    }

    pub(crate) fn stale_item(&self) -> Option<&serde_json::Value> {
        self.native()
            .and_then(|(item, current)| (!current).then_some(item))
    }

    pub fn native_item(&self) -> Option<&serde_json::Value> {
        self.native()
            .and_then(|(item, current)| current.then_some(item))
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Reasoning {
    pub text: String,
    #[serde(default, skip_serializing_if = "crate::codecs::json_utils::is_false")]
    pub redacted: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "native::lenient"
    )]
    pub native: Option<Native>,
}

impl Reasoning {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ToolResult {
    pub call: CallId,
    pub name: ToolName,
    pub content: Vec<ToolResultContent>,
    #[serde(default, skip_serializing_if = "crate::codecs::json_utils::is_false")]
    pub is_error: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolResultContent {
    Text(Text),
    Image(Image),
    Json { value: serde_json::Value },
}

impl ToolResultContent {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(&text.text),
            Self::Image(_) | Self::Json { .. } => None,
        }
    }

    pub fn as_json(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Json { value } => Some(value),
            Self::Text(_) | Self::Image(_) => None,
        }
    }

    pub fn deserialize_json<T>(&self) -> Result<T, serde_json::Error>
    where
        T: serde::de::DeserializeOwned,
    {
        match self {
            Self::Json { value } => T::deserialize(value),
            Self::Text(text) => serde_json::from_str(&text.text),
            Self::Image(_) => Err(<serde_json::Error as serde::de::Error>::custom(
                "an image tool result is not JSON",
            )),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: CallId,
    pub function: ToolFunction,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "native::lenient"
    )]
    pub native: Option<Native>,
}

impl ToolCall {
    fn fingerprint(&self) -> Fingerprint {
        let id = match &self.id {
            CallId::Provider(id) => id.as_str(),
            CallId::Local(_) => "~local",
        };
        Fingerprint::of_canonical(&(
            "v1",
            "call",
            id,
            self.function.name.as_str(),
            native::Canonical(&self.function.arguments),
            &self.function.invalid_arguments,
        ))
    }

    fn native_is_current(&self, native: &Native) -> bool {
        !native.edited
            && match &native.baseline {
                Some(baseline) => native::matches_baseline(baseline, &self.projection()),
                None => native.fingerprint == self.fingerprint(),
            }
    }

    fn baseline(&self) -> Option<serde_json::Value> {
        fn has_float(value: &serde_json::Value) -> bool {
            match value {
                serde_json::Value::Number(number) => number.is_f64(),
                serde_json::Value::Array(values) => values.iter().any(has_float),
                serde_json::Value::Object(fields) => fields.values().any(has_float),
                _ => false,
            }
        }
        self.function
            .arguments
            .values()
            .any(has_float)
            .then(|| native::baseline(self.projection()))
    }

    fn projection(&self) -> serde_json::Value {
        let id = match &self.id {
            CallId::Provider(id) => id.as_str(),
            CallId::Local(_) => "~local",
        };
        serde_json::json!([
            "v1",
            "call",
            id,
            self.function.name.as_str(),
            self.function.arguments,
            self.function.invalid_arguments,
        ])
    }

    pub fn new(id: CallId, function: ToolFunction) -> Self {
        Self {
            id,
            function,
            native: None,
        }
    }

    pub fn from_wire(wire_id: impl Into<String>, function: ToolFunction) -> Self {
        Self::new(CallId::from_wire(wire_id), function)
    }

    pub fn result(&self, content: Vec<ToolResultContent>) -> ToolResult {
        ToolResult {
            call: self.id.clone(),
            name: self.function.name.clone(),
            content,
            is_error: false,
        }
    }
}

impl Serialize for ToolCall {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Stored<'a> {
            id: &'a CallId,
            function: &'a ToolFunction,
            #[serde(skip_serializing_if = "Option::is_none")]
            native: Option<std::borrow::Cow<'a, Native>>,
        }

        let native = self.native.as_ref().map(|native| {
            // Record staleness before parsing can collapse adjacent floats.
            // The attachment baseline itself is never regenerated on save.
            if self.native_is_current(native) {
                std::borrow::Cow::Borrowed(native)
            } else {
                let mut stale = native.clone();
                stale.edited = true;
                std::borrow::Cow::Owned(stale)
            }
        });
        Stored {
            id: &self.id,
            function: &self.function,
            native,
        }
        .serialize(serializer)
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ToolFunction {
    pub name: ToolName,
    pub arguments: serde_json::Map<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_arguments: Option<String>,
}

impl ToolFunction {
    pub fn new(name: ToolName, arguments: serde_json::Value) -> Self {
        use serde_json::Value;
        let (arguments, invalid_arguments) = match arguments {
            Value::Object(arguments) => (arguments, None),
            Value::Null => (serde_json::Map::new(), None),
            Value::String(text) => {
                return Self::parse(name, &text);
            }
            other => (serde_json::Map::new(), Some(other.to_string())),
        };
        Self {
            name,
            arguments,
            invalid_arguments,
        }
    }

    pub fn parse(name: ToolName, text: &str) -> Self {
        use serde_json::Value;
        let parsed = crate::codecs::json_utils::parse_tool_arguments(text);
        let (arguments, invalid) = match parsed {
            Ok(Value::Object(arguments)) => (arguments, false),
            Ok(Value::Null) => (serde_json::Map::new(), false),
            Ok(Value::String(inner)) => match serde_json::from_str(&inner) {
                Ok(Value::Object(arguments)) => (arguments, false),
                _ => (serde_json::Map::new(), true),
            },
            Ok(_) => (serde_json::Map::new(), true),
            Err(_) => (
                crate::codecs::json_utils::parse_partial_object(text).unwrap_or_default(),
                true,
            ),
        };
        Self {
            name,
            arguments,
            invalid_arguments: invalid.then(|| text.to_owned()),
        }
    }

    pub fn arguments_value(&self) -> serde_json::Value {
        serde_json::Value::Object(self.arguments.clone())
    }

    pub fn arguments_json(&self) -> String {
        serde_json::to_string(&self.arguments).unwrap_or_default()
    }

    pub fn raw_arguments(&self) -> String {
        self.invalid_arguments
            .clone()
            .unwrap_or_else(|| self.arguments_json())
    }
}

impl<'de> Deserialize<'de> for ToolFunction {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Repr {
            name: ToolName,
            #[serde(default)]
            arguments: serde_json::Value,
            #[serde(default)]
            invalid_arguments: Option<String>,
        }
        let Repr {
            name,
            arguments,
            invalid_arguments,
        } = Repr::deserialize(deserializer)?;
        let mut function = Self::new(name, arguments);
        if invalid_arguments.is_some() {
            function.invalid_arguments = invalid_arguments;
        }
        Ok(function)
    }
}

#[non_exhaustive]
#[derive(Default, Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(from = "TextRepr")]
pub struct Text {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    citations: Option<citation::Citations>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "native::lenient"
    )]
    pub native: Option<Native>,
}

#[derive(Deserialize)]
struct TextRepr {
    text: String,
    #[serde(default, deserialize_with = "citation::lenient")]
    citations: Option<citation::Citations>,
    #[serde(default, deserialize_with = "native::lenient")]
    native: Option<Native>,
}

impl From<TextRepr> for Text {
    fn from(repr: TextRepr) -> Self {
        let TextRepr {
            text,
            citations,
            native,
        } = repr;
        Self {
            text,
            citations,
            native,
        }
        .checked()
    }
}

impl Text {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            citations: None,
            native: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

impl std::fmt::Display for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { text, .. } = self;
        write!(f, "{text}")
    }
}

#[derive(Default, Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Image {
    pub data: DocumentSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<ImageMediaType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<ImageDetail>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "native::lenient"
    )]
    pub native: Option<Native>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum DocumentSourceKind {
    Url(String),
    #[serde(alias = "string")]
    Base64(String),
    FileId(String),
    Raw(Vec<u8>),
    #[default]
    Unknown,
}

impl DocumentSourceKind {
    pub fn url(url: impl Into<String>) -> Self {
        Self::Url(url.into())
    }

    pub fn base64(base64_string: impl Into<String>) -> Self {
        Self::Base64(base64_string.into())
    }

    pub fn file_id(file_id: impl Into<String>) -> Self {
        Self::FileId(file_id.into())
    }
}

impl std::fmt::Display for DocumentSourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Url(string) => write!(f, "{string}"),
            Self::Base64(string) => write!(f, "{string}"),
            Self::FileId(string) => write!(f, "{string}"),
            Self::Raw(_) => write!(f, "<binary data>"),
            Self::Unknown => write!(f, "<unknown>"),
        }
    }
}

#[derive(Default, Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Audio {
    pub data: DocumentSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<AudioMediaType>,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Video {
    pub data: DocumentSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<VideoMediaType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional_params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum DocumentData {
    #[serde(alias = "string")]
    Text(String),
    #[serde(untagged)]
    File(DocumentSourceKind),
}

impl Default for DocumentData {
    fn default() -> Self {
        Self::File(DocumentSourceKind::Unknown)
    }
}

impl From<DocumentSourceKind> for DocumentData {
    fn from(source: DocumentSourceKind) -> Self {
        Self::File(source)
    }
}

#[derive(Default, Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Document {
    pub data: DocumentData,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<DocumentMediaType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional_params: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum MediaType {
    Image(ImageMediaType),
    Audio(AudioMediaType),
    Document(DocumentMediaType),
    Video(VideoMediaType),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ImageMediaType {
    JPEG,
    PNG,
    GIF,
    WEBP,
    HEIC,
    HEIF,
    SVG,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DocumentMediaType {
    PDF,
    TXT,
    RTF,
    HTML,
    CSS,
    MARKDOWN,
    CSV,
    XML,
    Javascript,
    Python,
}

impl ImageMediaType {
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Self::PNG)
        } else if bytes.starts_with(b"\xff\xd8\xff") {
            Some(Self::JPEG)
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            Some(Self::GIF)
        } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()) {
            Some(Self::WEBP)
        } else {
            None
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::JPEG => "jpg",
            Self::PNG => "png",
            Self::GIF => "gif",
            Self::WEBP => "webp",
            Self::HEIC => "heic",
            Self::HEIF => "heif",
            Self::SVG => "svg",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AudioMediaType {
    WAV,
    MP3,
    AIFF,
    AAC,
    OGG,
    FLAC,
    M4A,
    PCM16,
    PCM24,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum VideoMediaType {
    AVI,
    MP4,
    MPEG,
    MOV,
    WEBM,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ImageDetail {
    Low,
    High,
    #[default]
    Auto,
}

impl Message {
    pub fn final_answer(&self) -> Option<String> {
        let Message::Assistant(reply) = self else {
            return None;
        };
        let mut parts = Vec::new();
        for item in reply.content.iter() {
            match item {
                AssistantContent::Text(text) => parts.push(text.text.as_str()),
                AssistantContent::ToolCall(_) => return None,
                _ => {}
            }
        }
        let text = parts.join("\n\n");
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }

    pub fn rag_text(&self) -> Option<String> {
        match self {
            Message::User { content } => {
                for item in content.iter() {
                    if let UserContent::Text(Text { text, .. }) = item {
                        return Some(text.clone());
                    }
                }
                None
            }
            Message::System { .. } => None,
            _ => None,
        }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Message::System {
            content: text.into(),
        }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Message::User {
            content: vec![UserContent::text(text)],
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::text(text)]))
    }

    pub fn tool_result(call: CallId, name: ToolName, content: impl Into<String>) -> Self {
        Message::User {
            content: vec![UserContent::tool_result(
                call,
                name,
                vec![ToolResultContent::text(content)],
            )],
        }
    }

    pub fn tool_results(results: Vec<ToolResult>) -> Self {
        Message::User {
            content: results.into_iter().map(UserContent::ToolResult).collect(),
        }
    }

    pub fn images(&self) -> impl Iterator<Item = &Image> {
        let (user, turn): (&[UserContent], &[AssistantContent]) = match self {
            Message::User { content } => (content, &[]),
            Message::Assistant(turn) => (&[], &turn.content),
            Message::System { .. } => (&[], &[]),
        };
        let user = user.iter().flat_map(|item| {
            let (image, results): (_, &[ToolResultContent]) = match item {
                UserContent::Image(image) => (Some(image), &[]),
                UserContent::ToolResult(result) => (None, &result.content),
                _ => (None, &[]),
            };
            let results = results.iter().filter_map(|part| match part {
                ToolResultContent::Image(image) => Some(image),
                _ => None,
            });
            image.into_iter().chain(results)
        });
        let turn = turn.iter().filter_map(|block| match block {
            AssistantContent::Image(image) => Some(image),
            _ => None,
        });
        user.chain(turn)
    }

    pub fn images_mut(&mut self) -> impl Iterator<Item = &mut Image> {
        let (user, turn): (&mut [UserContent], &mut [AssistantContent]) = match self {
            Message::User { content } => (content, Default::default()),
            Message::Assistant(turn) => (Default::default(), &mut turn.content),
            Message::System { .. } => Default::default(),
        };
        let user = user.iter_mut().flat_map(|item| {
            let (image, results): (_, &mut [ToolResultContent]) = match item {
                UserContent::Image(image) => (Some(image), Default::default()),
                UserContent::ToolResult(result) => (None, &mut result.content),
                _ => Default::default(),
            };
            let results = results.iter_mut().filter_map(|part| match part {
                ToolResultContent::Image(image) => Some(image),
                _ => None,
            });
            image.into_iter().chain(results)
        });
        let turn = turn.iter_mut().filter_map(|block| match block {
            AssistantContent::Image(image) => Some(image),
            _ => None,
        });
        user.chain(turn)
    }
}

macro_rules! media_ctors {
    () => {};
    (
        $(#[$meta:meta])* $name:ident => Image($kind:ident: $data:ty);
        $($rest:tt)*
    ) => {
        $(#[$meta])*
        pub fn $name(
            data: impl Into<$data>,
            media_type: Option<ImageMediaType>,
            detail: Option<ImageDetail>,
        ) -> Self {
            Self::Image(Image {
                data: DocumentSourceKind::$kind(data.into()),
                media_type,
                detail,
                native: None,
            })
        }
        media_ctors! { $($rest)* }
    };
    (
        $(#[$meta:meta])* $name:ident => $variant:ident(params $mt:ty, $kind:path: $data:ty);
        $($rest:tt)*
    ) => {
        $(#[$meta])*
        pub fn $name(data: impl Into<$data>, media_type: Option<$mt>) -> Self {
            Self::$variant($variant {
                data: $kind(data.into()).into(),
                media_type,
                additional_params: None,
            })
        }
        media_ctors! { $($rest)* }
    };
    (
        $(#[$meta:meta])* $name:ident => $variant:ident($mt:ty, $kind:ident: $data:ty);
        $($rest:tt)*
    ) => {
        $(#[$meta])*
        pub fn $name(data: impl Into<$data>, media_type: Option<$mt>) -> Self {
            Self::$variant($variant {
                data: DocumentSourceKind::$kind(data.into()),
                media_type,
            })
        }
        media_ctors! { $($rest)* }
    };
}

impl UserContent {
    pub fn text(text: impl Into<String>) -> Self {
        UserContent::Text(text.into().into())
    }

    media_ctors! {
        image_base64 => Image(Base64: String);
        image_raw => Image(Raw: Vec<u8>);
        image_url => Image(Url: String);
        audio_base64 => Audio(AudioMediaType, Base64: String);
        audio_raw => Audio(AudioMediaType, Raw: Vec<u8>);
        audio_url => Audio(AudioMediaType, Url: String);
        video_base64 => Video(params VideoMediaType, DocumentSourceKind::Base64: String);
        video_raw => Video(params VideoMediaType, DocumentSourceKind::Raw: Vec<u8>);
        video_url => Video(params VideoMediaType, DocumentSourceKind::Url: String);
        document_base64 => Document(params DocumentMediaType, DocumentSourceKind::Base64: String);
        document_raw => Document(params DocumentMediaType, DocumentSourceKind::Raw: Vec<u8>);
        document_url => Document(params DocumentMediaType, DocumentSourceKind::Url: String);
        document_text => Document(params DocumentMediaType, DocumentData::Text: String);
    }

    pub fn tool_result(call: CallId, name: ToolName, content: Vec<ToolResultContent>) -> Self {
        UserContent::ToolResult(ToolResult {
            call,
            name,
            content,
            is_error: false,
        })
    }
}

impl AssistantContent {
    pub fn text(text: impl Into<String>) -> Self {
        AssistantContent::Text(text.into().into())
    }

    media_ctors! {
        image_base64 => Image(Base64: String);
    }

    pub fn tool_call(id: impl Into<String>, name: ToolName, arguments: serde_json::Value) -> Self {
        AssistantContent::ToolCall(ToolCall::from_wire(id, ToolFunction::new(name, arguments)))
    }

    pub fn reasoning(reasoning: impl Into<String>) -> Self {
        AssistantContent::Reasoning(Reasoning::new(reasoning))
    }
}

impl ToolResultContent {
    pub fn text(text: impl Into<String>) -> Self {
        ToolResultContent::Text(text.into().into())
    }

    pub fn json(value: serde_json::Value) -> Self {
        ToolResultContent::Json { value }
    }

    media_ctors! {
        image_base64 => Image(Base64: String);
        image_raw => Image(Raw: Vec<u8>);
        image_url => Image(Url: String);
    }
}

pub trait MimeType {
    fn from_mime_type(mime_type: &str) -> Option<Self>
    where
        Self: Sized;
    fn to_mime_type(&self) -> &'static str;
}

impl MimeType for MediaType {
    fn from_mime_type(mime_type: &str) -> Option<Self> {
        ImageMediaType::from_mime_type(mime_type)
            .map(MediaType::Image)
            .or_else(|| DocumentMediaType::from_mime_type(mime_type).map(MediaType::Document))
            .or_else(|| AudioMediaType::from_mime_type(mime_type).map(MediaType::Audio))
            .or_else(|| VideoMediaType::from_mime_type(mime_type).map(MediaType::Video))
    }

    fn to_mime_type(&self) -> &'static str {
        match self {
            MediaType::Image(media_type) => media_type.to_mime_type(),
            MediaType::Audio(media_type) => media_type.to_mime_type(),
            MediaType::Document(media_type) => media_type.to_mime_type(),
            MediaType::Video(media_type) => media_type.to_mime_type(),
        }
    }
}

// Emits both directions of a [`MimeType`] impl from a single pair list, so a
// variant's parse and emit spellings cannot drift apart. Extra `| "alias"`
// spellings parse to the same variant; only the first (canonical) string is
// emitted by `to_mime_type`.
macro_rules! impl_mime_type {
    ($ty:ident { $($variant:ident => $canonical:literal $(| $alias:literal)*),+ $(,)? }) => {
        impl MimeType for $ty {
            fn from_mime_type(mime_type: &str) -> Option<Self> {
                match mime_type {
                    $($canonical $(| $alias)* => Some($ty::$variant),)+
                    _ => None,
                }
            }

            fn to_mime_type(&self) -> &'static str {
                match self {
                    $($ty::$variant => $canonical,)+
                }
            }
        }
    };
}

impl_mime_type!(ImageMediaType {
    JPEG => "image/jpeg",
    PNG => "image/png",
    GIF => "image/gif",
    WEBP => "image/webp",
    HEIC => "image/heic",
    HEIF => "image/heif",
    SVG => "image/svg+xml",
});

impl_mime_type!(DocumentMediaType {
    PDF => "application/pdf",
    TXT => "text/plain",
    RTF => "text/rtf",
    HTML => "text/html",
    CSS => "text/css",
    MARKDOWN => "text/markdown" | "text/md",
    CSV => "text/csv",
    XML => "text/xml",
    Javascript => "application/x-javascript" | "text/x-javascript",
    Python => "application/x-python" | "text/x-python",
});

impl_mime_type!(AudioMediaType {
    WAV => "audio/wav",
    MP3 => "audio/mp3",
    AIFF => "audio/aiff",
    AAC => "audio/aac",
    OGG => "audio/ogg",
    FLAC => "audio/flac",
    M4A => "audio/m4a",
    PCM16 => "audio/pcm16",
    PCM24 => "audio/pcm24",
});

impl_mime_type!(VideoMediaType {
    AVI => "video/avi",
    MP4 => "video/mp4",
    MPEG => "video/mpeg",
    MOV => "video/mov",
    WEBM => "video/webm",
});

macro_rules! text_from {
    ($($src:ty),+ $(,)?) => {$(
        impl From<$src> for Text {
            fn from(text: $src) -> Self {
                Text::new(text)
            }
        }
    )+};
}

text_from!(String, &String, &str);

macro_rules! text_content_from_string {
    ($($ty:ident),+ $(,)?) => {$(
        impl From<String> for $ty {
            fn from(text: String) -> Self {
                $ty::text(text)
            }
        }
    )+};
}

text_content_from_string!(ToolResultContent, AssistantContent, UserContent);

macro_rules! single_content_message_from {
    (User { $($src:ty => $variant:ident),+ $(,)? }) => {$(
        impl From<$src> for Message {
            fn from(value: $src) -> Self {
                Message::User {
                    content: vec![UserContent::$variant(value.into())],
                }
            }
        }
    )+};
    (Assistant { $($src:ty => $variant:ident),+ $(,)? }) => {$(
        impl From<$src> for Message {
            fn from(value: $src) -> Self {
                Message::Assistant(AssistantMessage::new(vec![AssistantContent::$variant(
                    value.into(),
                )]))
            }
        }
    )+};
}

single_content_message_from!(User {
    String => Text,
    &str => Text,
    &String => Text,
    Text => Text,
    Image => Image,
    Audio => Audio,
    Document => Document,
    ToolResult => ToolResult,
});

single_content_message_from!(Assistant {
    ToolCall => ToolCall,
});

impl FromStr for Text {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(s.into())
    }
}

impl From<&Message> for Message {
    fn from(msg: &Message) -> Self {
        msg.clone()
    }
}

impl From<std::sync::Arc<Message>> for Message {
    fn from(msg: std::sync::Arc<Message>) -> Self {
        std::sync::Arc::unwrap_or_clone(msg)
    }
}

impl From<AssistantContent> for Message {
    fn from(content: AssistantContent) -> Self {
        Message::Assistant(AssistantMessage::new(vec![content]))
    }
}

impl From<AssistantMessage> for Message {
    fn from(message: AssistantMessage) -> Self {
        Message::Assistant(message)
    }
}

impl From<UserContent> for Message {
    fn from(content: UserContent) -> Self {
        Message::User {
            content: vec![content],
        }
    }
}

impl From<Vec<AssistantContent>> for Message {
    fn from(content: Vec<AssistantContent>) -> Self {
        Message::Assistant(AssistantMessage::new(content))
    }
}

impl From<Vec<UserContent>> for Message {
    fn from(content: Vec<UserContent>) -> Self {
        Message::User { content }
    }
}

#[derive(Default, Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    #[default]
    Auto,
    None,
    Required,
    Specific {
        function_names: Vec<ToolName>,
    },
}

#[derive(Debug, Error)]
pub enum MessageError {
    #[error("Message conversion error: {0}")]
    ConversionError(String),
}

impl From<MessageError> for ProviderError {
    fn from(error: MessageError) -> Self {
        ProviderError::request(error)
    }
}
