use crate::codecs::wire::Flow;
use serde::{Deserialize, Serialize};

use crate::codecs::embeddings;
use crate::codecs::error::EncodeError;
use crate::codecs::error::ProviderError;
use crate::codecs::model::{ModelInfo, ModelList};
use crate::codecs::operation::{
    Embedding, ModelListing, ModelPage, Rerank as RerankOp, Transcription, Verify as VerifyOp,
};
use crate::codecs::providers::internal::wire::classify_untyped_line;
use crate::codecs::providers::openai::embedding::Usage;
use crate::codecs::providers::openai::embedding::{
    CompatibleEmbeddingResponse, EncodingFormat, model_dimensions_from_identifier,
};
use crate::codecs::transcription::TranscriptionRequest;
use crate::codecs::wire::{
    Body, Capabilities, Decoder, Descriptor, DocumentDecoder, Encoded, Framing, Mode, Out, Wire,
    WireEvent, WireFrame,
};

use super::OpenAIConfig;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageBody {
    #[default]
    OpenAi,
    Xai,
    Hyperbolic,
    Venice,
    HuggingFace,
    MiniMax,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpeechBody {
    #[default]
    OpenAi,
    Xai,
    Hyperbolic,
    MiniMax,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranscriptionBody {
    Multipart,
    InputAudioJson,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DimensionsField {
    Dimensions,
    OutputDimension,
    Ignored,
}

impl DimensionsField {
    pub const fn name(self) -> Option<&'static str> {
        match self {
            Self::Dimensions => Some("dimensions"),
            Self::OutputDimension => Some("output_dimension"),
            Self::Ignored => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptedWidths {
    Fixed,
    Range {
        min: usize,
        max: usize,
        requirement: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ModelWidth {
    pub model: &'static str,
    pub default: Option<usize>,
    pub accepted: AcceptedWidths,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RerankQuirks {
    pub path: &'static str,
    pub max_documents: usize,
    pub sends_model_field: bool,
}

impl RerankQuirks {
    pub const fn unsupported() -> Self {
        Self {
            path: "",
            max_documents: 0,
            sends_model_field: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmbeddingQuirks {
    pub max_documents: usize,
    pub requires_usage: bool,
    pub supports_encoding_format: bool,
    pub supports_user: bool,
    pub sends_model_field: bool,
    pub dimensions: DimensionsField,
    pub widths: &'static [ModelWidth],
    pub refuse_zero_width: Option<&'static str>,
}

impl EmbeddingQuirks {
    pub const fn openai() -> Self {
        Self {
            max_documents: 1024,
            requires_usage: true,
            supports_encoding_format: true,
            supports_user: true,
            sends_model_field: true,
            dimensions: DimensionsField::Dimensions,
            // Shared OpenAI model widths are resolved separately.
            widths: &[],
            // Zero represents an unknown width, not a requested dimension.
            refuse_zero_width: None,
        }
    }
}

impl super::SubRoute {
    pub fn serves_model_routed_endpoints(&self) -> bool {
        matches!(self, Self::HFInference)
    }
}

impl OpenAIConfig {
    pub fn with_audio_api_version(mut self, api_version: impl Into<String>) -> Self {
        self.audio_api_version = Some(api_version.into());
        self
    }

    pub fn embedding(&self, model: impl Into<String>, ndims: Option<usize>) -> Embeddings {
        Embeddings::new(self.clone(), model, ndims)
    }

    pub fn rerank(&self, model: impl Into<String>) -> Rerank {
        Rerank::new(self.clone(), model)
    }

    pub fn transcription(&self, model: impl Into<String>) -> Transcriptions {
        Transcriptions::new(self.clone(), model)
    }

    pub fn models(&self) -> Models {
        Models::new(self.clone())
    }

    pub fn verify(&self) -> Verify {
        Verify::new(self.clone())
    }

    pub(crate) fn modality_uri(
        &self,
        endpoint: &str,
        fixed: &'static str,
        model: &str,
    ) -> Result<String, String> {
        if !self.dialect.quirks.model_is_modality_path {
            return Ok(self.uri(fixed, self.deployment(model)));
        }
        let route = self.route();
        if !route.serves_model_routed_endpoints() {
            return Err(format!(
                "{endpoint} endpoint is not supported yet for {route}"
            ));
        }
        Ok(format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            model.trim_start_matches('/')
        ))
    }
}

pub(super) const MISTRAL_EMBEDDING_WIDTHS: &[ModelWidth] = &[
    ModelWidth {
        model: "mistral-embed",
        default: Some(1_024),
        accepted: AcceptedWidths::Fixed,
    },
    ModelWidth {
        model: "mistral-embed-2312",
        default: Some(1_024),
        accepted: AcceptedWidths::Fixed,
    },
    ModelWidth {
        model: "codestral-embed",
        // Configurable with no documented native width, so a handle that
        // names none reports 0 rather than inventing one.
        default: None,
        accepted: AcceptedWidths::Range {
            // Mistral documents only a ceiling. The floor is the codec's own
            // "unknown" sentinel, which never reaches the wire.
            min: 0,
            max: 3_072,
            requirement: "to be at most 3072 for Codestral Embed",
        },
    },
    ModelWidth {
        model: "codestral-embed-2505",
        default: None,
        accepted: AcceptedWidths::Range {
            min: 0,
            max: 3_072,
            requirement: "to be at most 3072 for Codestral Embed",
        },
    },
];

pub(super) const DOUBLEWORD_EMBEDDING_WIDTHS: &[ModelWidth] = &[ModelWidth {
    model: "Qwen/Qwen3-Embedding-8B",
    default: Some(4_096),
    accepted: AcceptedWidths::Range {
        min: 32,
        max: 4_096,
        requirement: "to be between 32 and 4096",
    },
}];

pub(crate) fn json_post(
    provider: &OpenAIConfig,
    path: &str,
    deployment: Option<&str>,
    body: &serde_json::Value,
) -> Result<Encoded, EncodeError> {
    json_post_to(provider, provider.uri(path, deployment), body)
}

fn json_post_to(
    provider: &OpenAIConfig,
    uri: String,
    body: &serde_json::Value,
) -> Result<Encoded, EncodeError> {
    let bytes = serde_json::to_vec(body)?;
    let builder = http::Request::post(uri).header("Content-Type", "application/json");
    encoded(provider, builder, Body::Bytes(bytes))
}

fn get(provider: &OpenAIConfig, path: &str) -> Result<Encoded, EncodeError> {
    encoded(
        provider,
        http::Request::get(provider.uri(path, None)),
        Body::empty(),
    )
}

pub(crate) fn encoded(
    provider: &OpenAIConfig,
    builder: http::request::Builder,
    body: Body,
) -> Result<Encoded, EncodeError> {
    let mut request = provider.authenticate(builder).body(body)?;
    if let Some(envelope) = provider
        .dialect
        .quirks
        .hooks
        .and_then(|hooks| hooks.modality_envelope)
    {
        envelope(provider, &mut request)?;
    }
    Ok(Encoded::new(request, Framing::Whole)
        .with_request_id_header(provider.dialect.request_id_header))
}

fn unsupported_parameter(provider: &str, parameter: &str) -> EncodeError {
    EncodeError::request(format!(
        "{provider} embeddings do not support the `{parameter}` parameter"
    ))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Embeddings {
    pub provider: OpenAIConfig,
    pub model: String,
    pub ndims: Option<usize>,
    pub encoding_format: Option<EncodingFormat>,
    pub user: Option<String>,
}

impl Embeddings {
    pub fn new(provider: OpenAIConfig, model: impl Into<String>, ndims: Option<usize>) -> Self {
        Self {
            provider,
            model: model.into(),
            ndims,
            encoding_format: None,
            user: None,
        }
    }

    pub fn with_encoding_format(mut self, encoding_format: EncodingFormat) -> Self {
        self.encoding_format = Some(encoding_format);
        self
    }

    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    fn model_width(&self) -> Option<&'static ModelWidth> {
        self.provider
            .dialect
            .quirks
            .embedding
            .widths
            .iter()
            .find(|width| width.model == self.model)
    }

    fn resolved_ndims(&self) -> usize {
        self.ndims
            .or_else(|| self.model_width().and_then(|width| width.default))
            .or_else(|| model_dimensions_from_identifier(&self.model))
            .unwrap_or_default()
    }

    fn refuse_unhonourable_width(&self) -> Result<(), EncodeError> {
        let quirks = &self.provider.dialect.quirks.embedding;
        let provider = self.provider.dialect.name;
        let invalid = |requirement, parameter| {
            EncodeError::request(format!(
                "{provider} embeddings require `{parameter}` {requirement}"
            ))
        };
        // A dialect that reads no width field has nothing to refuse: the
        // caller's number never reaches the wire, and the shared driver
        // catches the disagreement against the reply instead.
        let Some(parameter) = quirks.dimensions.name() else {
            return Ok(());
        };
        let Some(declared) = self.ndims else {
            return Ok(());
        };
        if declared == 0 {
            return match quirks.refuse_zero_width {
                Some(requirement) => Err(invalid(requirement, parameter)),
                None => Ok(()),
            };
        }
        // A model the dialect does not document: the caller's width is the
        // only width there is, so it goes out unvalidated and the API rules.
        let Some(width) = self.model_width() else {
            return Ok(());
        };
        // Native widths require no truncation parameter.
        if width.default == Some(declared) {
            return Ok(());
        }
        match width.accepted {
            AcceptedWidths::Fixed => Err(unsupported_parameter(provider, parameter)),
            AcceptedWidths::Range { min, max, .. } if (min..=max).contains(&declared) => Ok(()),
            AcceptedWidths::Range { requirement, .. } => Err(invalid(requirement, parameter)),
        }
    }

    fn requested_width(&self) -> Option<(&'static str, usize)> {
        let field = self.provider.dialect.quirks.embedding.dimensions.name()?;
        if self.model == crate::codecs::providers::openai::embedding::TEXT_EMBEDDING_ADA_002 {
            return None;
        }
        // Unknown widths are metadata sentinels, not request parameters.
        let ndims = match self.resolved_ndims() {
            0 => return None,
            ndims => ndims,
        };
        // At a documented model's native width, send nothing: that width is
        // what the model emits unasked, so the field would only restate the
        // default and the vector is identical either way.
        if self
            .model_width()
            .is_some_and(|width| width.default == Some(ndims))
        {
            return None;
        }
        Some((field, ndims))
    }
}

#[derive(Default)]
pub struct EmbeddingsDecoder {
    requires_usage: bool,
    provider: &'static str,
    model: String,
}

impl<'id> Decoder<'id, Embedding> for EmbeddingsDecoder {
    type Event = CompatibleEmbeddingResponse;

    fn classify(&self, frame: WireFrame) -> WireEvent<Self::Event> {
        classify_untyped_line(frame.as_str().as_bytes())
    }

    fn decode(
        &mut self,
        event: Self::Event,
        out: Out<'id, Embedding>,
    ) -> Result<Flow, ProviderError> {
        if event.usage.is_none() && self.requires_usage {
            return Err(ProviderError::Response(format!(
                "{} embedding response omitted required usage",
                self.provider
            )));
        }
        let usage = event
            .usage
            .as_ref()
            .map(Usage::to_normalized)
            .unwrap_or_default();
        let vectors = event.data.into_iter().map(|datum| datum.embedding);
        let model = if event.model.is_empty() {
            self.model.clone()
        } else {
            event.model
        };
        Ok(out.end(embeddings::EmbeddingResponse {
            model: Some(model),
            usage,
            ..embeddings::EmbeddingResponse::from_vectors(vectors)
        }))
    }
}

impl Wire for Embeddings {
    type Op = Embedding;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = CompatibleEmbeddingResponse;
    type Decoder<'id> = EmbeddingsDecoder;
    type Reassembler = crate::codecs::wire::document::Unreassembled;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(self.provider.dialect.name)
            .model(self.model.as_str())
            .capabilities(
                Capabilities::embedding(
                    self.provider.dialect.quirks.embedding.max_documents,
                    self.resolved_ndims(),
                )
                .declaring(self.ndims),
            )
    }

    fn encode(&self, request: Vec<String>, _mode: Mode) -> Result<Encoded, EncodeError> {
        let quirks = &self.provider.dialect.quirks.embedding;
        // Base64 vectors are not decoded anywhere, so asking for them would
        // answer 200 with a payload the codec cannot read.
        if self.encoding_format == Some(EncodingFormat::Base64) {
            return Err(EncodeError::request(format!(
                "Cannot decode {} embedding responses encoded as `base64`",
                self.provider.dialect.name
            )));
        }
        if self.encoding_format.is_some() && !quirks.supports_encoding_format {
            return Err(unsupported_parameter(
                self.provider.dialect.name,
                "encoding_format",
            ));
        }
        if self.user.is_some() && !quirks.supports_user {
            return Err(unsupported_parameter(self.provider.dialect.name, "user"));
        }
        self.refuse_unhonourable_width()?;

        let mut body = serde_json::json!({ "input": request });
        let Some(object) = body.as_object_mut() else {
            return Err(EncodeError::request(
                "embedding request body must be an object",
            ));
        };
        if quirks.sends_model_field {
            object.insert("model".to_owned(), serde_json::json!(self.model));
        }
        if let Some((field, ndims)) = self.requested_width() {
            object.insert(field.to_owned(), serde_json::json!(ndims));
        }
        if let Some(encoding_format) = self.encoding_format {
            object.insert(
                "encoding_format".to_owned(),
                serde_json::to_value(encoding_format)?,
            );
        }
        if let Some(user) = &self.user {
            object.insert("user".to_owned(), serde_json::json!(user));
        }

        json_post(
            &self.provider,
            self.provider.dialect.quirks.embeddings_path,
            self.provider.deployment(&self.model),
            &body,
        )
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        EmbeddingsDecoder {
            requires_usage: self.provider.dialect.quirks.embedding.requires_usage,
            provider: self.provider.dialect.name,
            model: self.model.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transcriptions {
    pub provider: OpenAIConfig,
    pub model: String,
}

impl Transcriptions {
    pub fn new(provider: OpenAIConfig, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
        }
    }

    fn multipart_body(&self, request: TranscriptionRequest) -> Result<Body, EncodeError> {
        use crate::codecs::http_client::MultipartForm;
        use crate::codecs::http_client::multipart::Part;

        let mut form = MultipartForm::new();
        // Azure addresses its deployment through the URL rather than the form.
        if self.provider.deployment(&self.model).is_none() {
            form = form.text("model", self.model.clone());
        }
        form = form.part(Part::bytes("file", request.data).filename(request.filename));
        if let Some(language) = request.language {
            form = form.text("language", language);
        }
        if let Some(prompt) = request.prompt {
            form = form.text("prompt", prompt);
        }
        if let Some(temperature) = request.temperature {
            form = form.text("temperature", temperature.to_string());
        }
        if let Some(additional_params) = request.additional_params {
            for (name, value) in additional_params_object(&additional_params)? {
                match value {
                    // Arrays go as one `name[]` field per item, the form
                    // encoding these endpoints read for lists such as
                    // `timestamp_granularities`.
                    serde_json::Value::Array(items) => {
                        for item in items {
                            form = form.text(format!("{name}[]"), form_text(item));
                        }
                    }
                    other => form = form.text(name.clone(), form_text(other)),
                }
            }
        }
        Ok(Body::Multipart(form))
    }

    fn input_audio_body(&self, request: TranscriptionRequest) -> Result<Body, EncodeError> {
        use base64::Engine;

        if request.prompt.is_some() {
            return Err(EncodeError::request(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "OpenRouter STT does not support a top-level prompt field. \
                     Provider-specific prompt options can be passed via `additional_params`. \
                     Example: {\"provider\": {\"options\": {\"<provider>\": {\"prompt\": \"<text>\"}}}}",
            )));
        }

        let mut body = serde_json::Map::new();
        body.insert("model".to_owned(), serde_json::json!(self.model));
        body.insert(
            "input_audio".to_owned(),
            serde_json::json!({
                "data": base64::engine::general_purpose::STANDARD.encode(&request.data),
                "format": audio_format_of(&request.filename),
            }),
        );
        if let Some(language) = request.language {
            body.insert("language".to_owned(), serde_json::json!(language));
        }
        if let Some(temperature) = request.temperature {
            body.insert("temperature".to_owned(), serde_json::json!(temperature));
        }
        if let Some(additional_params) = request.additional_params {
            for (name, value) in additional_params_object(&additional_params)? {
                body.insert(name.clone(), value.clone());
            }
        }
        Ok(Body::Bytes(serde_json::to_vec(
            &serde_json::Value::Object(body),
        )?))
    }
}

fn form_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn additional_params_object(
    params: &serde_json::Value,
) -> Result<&serde_json::Map<String, serde_json::Value>, EncodeError> {
    params.as_object().ok_or_else(|| {
        EncodeError::request(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "additional transcription parameters must be a JSON object",
        ))
    })
}

fn audio_format_of(filename: &str) -> &'static str {
    let extension = std::path::Path::new(filename)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("mp3") => "mp3",
        Some("flac") => "flac",
        Some("m4a") => "m4a",
        Some("ogg") => "ogg",
        Some("webm") => "webm",
        Some("aac") => "aac",
        _ => "wav",
    }
}

impl Wire for Transcriptions {
    type Op = Transcription;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = crate::codecs::providers::openai::transcription::TranscriptionResponse;
    type Decoder<'id> = DocumentDecoder<Transcription, Self::Event>;
    type Reassembler = crate::codecs::wire::document::Unreassembled;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(self.provider.dialect.name).model(self.model.as_str())
    }

    fn encode(&self, request: TranscriptionRequest, _mode: Mode) -> Result<Encoded, EncodeError> {
        let uri = self
            .provider
            .modality_uri(
                "transcription",
                self.provider.dialect.quirks.transcription_path,
                &self.model,
            )
            .map_err(EncodeError::request)?;
        let builder = http::Request::post(uri);
        let (builder, body) = match self.provider.dialect.quirks.transcription_body {
            TranscriptionBody::Multipart => (builder, self.multipart_body(request)?),
            TranscriptionBody::InputAudioJson => (
                builder.header(http::header::CONTENT_TYPE, "application/json"),
                self.input_audio_body(request)?,
            ),
        };
        encoded(&self.provider, builder, body)
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        DocumentDecoder::new(
            |body| classify_untyped_line(body.as_bytes()),
            |event| event.normalize(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Models {
    pub provider: OpenAIConfig,
}

impl Models {
    pub fn new(provider: OpenAIConfig) -> Self {
        Self { provider }
    }
}

#[derive(Debug, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub created: Option<u64>,
    #[serde(default)]
    pub owned_by: Option<String>,
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub context_length: Option<u32>,
    #[serde(default)]
    pub max_context_length: Option<u32>,
    #[serde(default)]
    pub max_completion_tokens: Option<u32>,
    #[serde(default)]
    pub top_provider: Option<TopProvider>,
}

#[derive(Debug, Deserialize)]
pub struct TopProvider {
    #[serde(default)]
    pub max_completion_tokens: Option<u32>,
}

impl From<ModelEntry> for ModelInfo {
    fn from(entry: ModelEntry) -> Self {
        let mut model = ModelInfo::from_id(entry.id);
        model.name = entry.name;
        model.description = entry.description;
        model.r#type = entry.kind;
        model.created_at = entry.created;
        model.owned_by = entry.owned_by;
        model.context_length = entry
            .context_window
            .or(entry.context_length)
            .or(entry.max_context_length);
        model.max_output_tokens = entry.max_completion_tokens.or_else(|| {
            entry
                .top_provider
                .and_then(|provider| provider.max_completion_tokens)
        });
        model
    }
}

#[derive(Debug, Deserialize)]
pub struct ModelsReply {
    #[serde(default)]
    pub data: Vec<ModelEntry>,
}

impl Wire for Models {
    type Op = ModelListing;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = ModelsReply;
    type Decoder<'id> = DocumentDecoder<ModelListing, Self::Event>;
    type Reassembler = crate::codecs::wire::document::Unreassembled;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(self.provider.dialect.name)
    }

    fn encode(&self, _cursor: Option<String>, _mode: Mode) -> Result<Encoded, EncodeError> {
        get(&self.provider, self.provider.dialect.quirks.models_path)
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        DocumentDecoder::new(
            |body| classify_untyped_line(body.as_bytes()),
            |event| {
                let models = event.data.into_iter().map(ModelInfo::from).collect();
                Ok(ModelPage {
                    models: ModelList::new(models),
                    next: None,
                })
            },
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rerank {
    pub provider: OpenAIConfig,
    pub model: String,
    pub top_n: Option<usize>,
}

impl Rerank {
    pub fn new(provider: OpenAIConfig, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
            top_n: None,
        }
    }

    pub fn with_top_n(mut self, top_n: usize) -> Self {
        self.top_n = Some(top_n);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankResultEntry {
    pub index: usize,
    #[serde(alias = "score")]
    pub relevance_score: f64,
    #[serde(default, alias = "text")]
    pub document: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RerankUsage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankReply {
    #[serde(default)]
    pub model: Option<String>,
    pub results: Vec<RerankResultEntry>,
    #[serde(default)]
    pub usage: Option<RerankUsage>,
}

impl Wire for Rerank {
    type Op = RerankOp;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = RerankReply;
    type Decoder<'id> = DocumentDecoder<RerankOp, Self::Event>;
    type Reassembler = crate::codecs::wire::document::Unreassembled;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(self.provider.dialect.name)
            .model(self.model.as_str())
            .capabilities(Capabilities::rerank(
                self.provider.dialect.quirks.rerank.max_documents,
            ))
    }

    fn encode(
        &self,
        request: crate::codecs::operation::RerankRequest,
        _mode: Mode,
    ) -> Result<Encoded, EncodeError> {
        let quirks = &self.provider.dialect.quirks.rerank;
        // An empty path explicitly disables reranking.
        if quirks.path.is_empty() {
            return Err(EncodeError::request(format!(
                "{} offers no reranking endpoint",
                self.provider.dialect.name
            )));
        }
        let mut body = serde_json::json!({
            "query": request.query,
            "documents": request.documents,
        });
        let Some(object) = body.as_object_mut() else {
            return Err(EncodeError::request(
                "rerank request body must be an object",
            ));
        };
        if quirks.sends_model_field {
            object.insert("model".to_owned(), serde_json::json!(self.model));
        }
        if let Some(top_n) = self.top_n {
            object.insert("top_n".to_owned(), serde_json::json!(top_n));
        }

        json_post(
            &self.provider,
            quirks.path,
            self.provider.deployment(&self.model),
            &body,
        )
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        DocumentDecoder::new(
            |body| classify_untyped_line(body.as_bytes()),
            |event| {
                let usage = event
                    .usage
                    .map(|usage| crate::codecs::completion::Usage {
                        input_tokens: Some(usage.prompt_tokens),
                        total_tokens: Some(usage.total_tokens),
                        ..Default::default()
                    })
                    .unwrap_or_default();
                let results = event
                    .results
                    .into_iter()
                    .map(|result| crate::codecs::rerank::RerankResult {
                        index: result.index,
                        document: result.document,
                        relevance_score: result.relevance_score,
                    })
                    .collect();
                Ok(crate::codecs::rerank::RerankResponse {
                    model: event.model,
                    usage,
                    ..crate::codecs::rerank::RerankResponse::new(results)
                })
            },
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Verify {
    pub provider: OpenAIConfig,
}

impl Verify {
    pub fn new(provider: OpenAIConfig) -> Self {
        Self { provider }
    }
}

pub use crate::codecs::operation::VerifyDecoder;

impl Wire for Verify {
    type Op = VerifyOp;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = ();
    type Decoder<'id> = VerifyDecoder;
    type Reassembler = crate::codecs::wire::document::Unreassembled;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(self.provider.dialect.name)
    }

    fn encode(&self, _request: (), _mode: Mode) -> Result<Encoded, EncodeError> {
        let path = self.provider.dialect.quirks.verify_path;
        if path.is_empty() {
            return Err(EncodeError::request(format!(
                "{} offers no endpoint that checks a credential without consuming tokens",
                self.provider.dialect.name
            )));
        }
        get(&self.provider, path)
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        VerifyDecoder
    }
}
