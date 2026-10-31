use crate::codecs::catalog::ModelFacts;
use crate::codecs::completion::options::FinalBody;
use crate::codecs::completion::{CompletionRequest, ProviderCapabilities};
use crate::codecs::error::EncodeError;
use crate::codecs::operation::Completion;
use crate::codecs::providers::internal::named_dialect;
use crate::codecs::wire::{Body, Capabilities, Descriptor, Encoded, Framing, Mode, Secret, Wire};
use serde::{Deserialize, Serialize};

use super::completion::{
    CacheTtl, default_max_tokens_for_model, document_source, image_source,
    rejects_forced_tool_choice,
};
use super::streaming::MessagesDecoder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dialect {
    pub name: &'static str,
    pub base_url: &'static str,
    pub api_key_env: &'static str,
    pub base_url_env: Option<&'static str>,
    pub request_id_header: Option<&'static str>,
    pub quirks: Quirks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Quirks {
    pub max_tokens: MaxTokens,
    pub strict_tool_schemas: bool,
    pub unsigned_thinking: bool,
    pub tool_input_streaming: ToolInputStreaming,
    pub auth: Auth,
    pub session_header: Option<&'static str>,
    pub supports_output_format: bool,
    pub catalog_priced: bool,
    pub cache_ttl_pricing: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Auth {
    #[default]
    ApiKey,
    OptionalBearer,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolInputStreaming {
    #[default]
    Eager,
    BetaHeader,
    Off,
}

impl Quirks {
    pub const fn anthropic() -> Self {
        Self {
            max_tokens: MaxTokens::ByModel,
            strict_tool_schemas: true,
            unsigned_thinking: false,
            tool_input_streaming: ToolInputStreaming::Eager,
            auth: Auth::ApiKey,
            session_header: None,
            supports_output_format: true,
            catalog_priced: true,
            cache_ttl_pricing: true,
        }
    }

    pub const fn gateway() -> Self {
        Self {
            max_tokens: MaxTokens::Fixed(4096),
            strict_tool_schemas: false,
            unsigned_thinking: false,
            tool_input_streaming: ToolInputStreaming::Eager,
            auth: Auth::ApiKey,
            session_header: None,
            supports_output_format: true,
            catalog_priced: true,
            cache_ttl_pricing: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaxTokens {
    ByModel,
    Fixed(u64),
}

pub(crate) const MESSAGES_API: &str = "anthropic.messages";

pub const ANTHROPIC: Dialect = Dialect {
    name: super::PROVIDER_NAME,
    base_url: "https://api.anthropic.com",
    api_key_env: "ANTHROPIC_API_KEY",
    base_url_env: Some("ANTHROPIC_BASE_URL"),
    request_id_header: Some("request-id"),
    quirks: Quirks::anthropic(),
};

const ALL: &[&Dialect] = &[
    &ANTHROPIC,
    &ZAI,
    &MINIMAX,
    &MOONSHOT,
    &XIAOMIMIMO,
    &OLLAMA,
    &OPENCODE,
    &OPENCODE_GO,
];

pub fn all() -> impl Iterator<Item = &'static Dialect> {
    ALL.iter().copied()
}

impl Dialect {
    pub fn by_name(name: &str) -> Option<Self> {
        all().copied().find(|dialect| dialect.name == name)
    }
}

impl Serialize for Dialect {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let registered = Self::by_name(self.name).as_ref() == Some(self);
        named_dialect::serialize(serializer, "Anthropic", self.name, registered)
    }
}

impl<'de> Deserialize<'de> for Dialect {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        named_dialect::deserialize(deserializer, "Anthropic", Self::by_name)
    }
}

pub const fn compatible(
    name: &'static str,
    base_url: &'static str,
    api_key_env: &'static str,
    base_url_env: Option<&'static str>,
) -> Dialect {
    Dialect {
        name,
        base_url,
        api_key_env,
        base_url_env,
        request_id_header: Some("request-id"),
        quirks: Quirks::gateway(),
    }
}

impl Dialect {
    pub fn default_max_tokens(&self, facts: &ModelFacts, model: &str) -> Option<u64> {
        match self.quirks.max_tokens {
            MaxTokens::ByModel => default_max_tokens_for_model(facts, model),
            MaxTokens::Fixed(tokens) => Some(tokens),
        }
    }
}

pub const ZAI: Dialect = compatible(
    crate::codecs::providers::zai::PROVIDER_NAME,
    "https://api.z.ai/api/anthropic",
    "ZAI_API_KEY",
    Some("ZAI_ANTHROPIC_API_BASE"),
);

pub const MINIMAX: Dialect = compatible(
    crate::codecs::providers::minimax::PROVIDER_NAME,
    "https://api.minimax.io/anthropic",
    "MINIMAX_API_KEY",
    Some("MINIMAX_ANTHROPIC_API_BASE"),
);

pub const MOONSHOT: Dialect = Dialect {
    quirks: Quirks {
        unsigned_thinking: true,
        ..Quirks::gateway()
    },
    ..compatible(
        crate::codecs::providers::moonshot::PROVIDER_NAME,
        "https://api.moonshot.ai/anthropic",
        "MOONSHOT_API_KEY",
        Some("MOONSHOT_ANTHROPIC_API_BASE"),
    )
};

pub const XIAOMIMIMO: Dialect = compatible(
    crate::codecs::providers::xiaomimimo::PROVIDER_NAME,
    "https://api.xiaomimimo.com/anthropic",
    "XIAOMI_MIMO_API_KEY",
    Some("XIAOMI_MIMO_ANTHROPIC_API_BASE"),
);

pub const OLLAMA: Dialect = Dialect {
    quirks: Quirks {
        auth: Auth::OptionalBearer,
        unsigned_thinking: true,
        // Its `output_config` reads only `effort`.
        supports_output_format: false,
        // A daemon is usually local; the catalog's prices are Ollama Cloud's.
        catalog_priced: false,
        ..Quirks::gateway()
    },
    ..compatible(
        crate::codecs::providers::ollama::PROVIDER_NAME,
        "http://localhost:11434",
        "OLLAMA_API_KEY",
        Some("OLLAMA_API_BASE_URL"),
    )
};

pub const OPENCODE: Dialect = compatible(
    "opencode",
    "https://opencode.ai/inference/anthropic",
    "OPENCODE_API_KEY",
    Some("OPENCODE_ANTHROPIC_BASE_URL"),
);

pub const OPENCODE_GO: Dialect = Dialect {
    quirks: Quirks {
        session_header: Some("x-opencode-session"),
        ..Quirks::gateway()
    },
    ..compatible(
        "opencode-go",
        "https://opencode.ai/inference/go/anthropic",
        "OPENCODE_API_KEY",
        Some("OPENCODE_GO_ANTHROPIC_BASE_URL"),
    )
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnthropicConfig {
    pub api_key: Secret,
    pub base_url: String,
    pub version: String,
    pub betas: Vec<String>,
    pub dialect: Dialect,
    #[serde(default, skip_serializing_if = "Addressing::is_body")]
    pub addressing: Addressing,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Addressing {
    #[default]
    Body,
    Url,
}

impl Addressing {
    fn is_body(&self) -> bool {
        *self == Self::Body
    }
}

impl AnthropicConfig {
    pub fn new(api_key: impl Into<Secret>) -> Self {
        Self::with_key(&ANTHROPIC, api_key)
    }

    pub fn with_key(dialect: &Dialect, api_key: impl Into<Secret>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: normalize_base_url(dialect.base_url),
            version: super::completion::ANTHROPIC_VERSION_LATEST.to_owned(),
            betas: Vec::new(),
            dialect: *dialect,
            addressing: Addressing::Body,
        }
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    pub fn with_beta(mut self, beta: impl Into<String>) -> Self {
        self.betas.push(beta.into());
        self
    }

    pub fn with_base_url(mut self, base_url: impl AsRef<str>) -> Self {
        self.base_url = normalize_base_url(base_url.as_ref());
        self
    }

    pub fn completion(&self, model: impl Into<String>) -> Messages {
        let model = model.into();
        Messages {
            default_max_tokens: self
                .dialect
                .default_max_tokens(ModelFacts::builtin(), &model),
            provider: self.clone(),
            model,
            prompt_caching: false,
            static_prefix_cache_ttl: None,
            strict_tools: false,
            tool_input_streaming: self.dialect.quirks.tool_input_streaming,
            facts: ModelFacts::default(),
        }
    }

    pub(super) fn headers(&self, builder: http::request::Builder) -> http::request::Builder {
        self.headers_with(builder, &self.betas)
    }

    fn headers_with(
        &self,
        builder: http::request::Builder,
        betas: &[String],
    ) -> http::request::Builder {
        let builder = match self.dialect.quirks.auth {
            Auth::ApiKey => builder.header("x-api-key", self.api_key.expose()),
            Auth::OptionalBearer if self.api_key.is_empty() => builder,
            Auth::OptionalBearer => builder.header(
                http::header::AUTHORIZATION,
                format!("Bearer {}", self.api_key.expose()),
            ),
        };
        let builder = builder.header("anthropic-version", &self.version);
        if betas.is_empty() {
            builder
        } else {
            builder.header("anthropic-beta", betas.join(","))
        }
    }

    fn betas_with(&self, extra: &[&str]) -> Vec<String> {
        let mut betas = self.betas.clone();
        for extra in extra {
            if !betas.iter().any(|beta| beta == extra) {
                betas.push((*extra).to_owned());
            }
        }
        betas
    }
}

pub fn normalize_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    for suffix in ["/v1/messages", "/messages", "/v1"] {
        if let Some(stripped) = trimmed.strip_suffix(suffix) {
            return stripped.to_owned();
        }
    }
    trimmed.to_owned()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Messages {
    pub provider: AnthropicConfig,
    pub model: String,
    pub default_max_tokens: Option<u64>,
    pub prompt_caching: bool,
    pub static_prefix_cache_ttl: Option<CacheTtl>,
    pub strict_tools: bool,
    #[serde(default)]
    pub tool_input_streaming: ToolInputStreaming,
    #[serde(skip)]
    pub facts: ModelFacts,
}

impl Messages {
    pub fn with_facts(mut self, facts: ModelFacts) -> Self {
        self.default_max_tokens = self
            .provider
            .dialect
            .default_max_tokens(&facts, &self.model);
        self.facts = facts;
        self
    }

    pub fn with_default_max_tokens(mut self, tokens: u64) -> Self {
        self.default_max_tokens = Some(tokens);
        self
    }

    pub fn with_prompt_caching(mut self) -> Self {
        self.prompt_caching = true;
        self
    }

    pub fn with_static_prefix_cache_ttl(mut self, ttl: CacheTtl) -> Self {
        self.static_prefix_cache_ttl = Some(ttl);
        self
    }

    pub fn with_strict_tools(mut self) -> Self {
        self.strict_tools = true;
        self
    }

    pub fn with_tool_input_streaming(mut self, streaming: ToolInputStreaming) -> Self {
        self.tool_input_streaming = streaming;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Prepared {
    pub model: String,
    pub body: FinalBody,
    pub betas: Vec<String>,
}

impl Messages {
    pub fn encode_with(
        &self,
        request: CompletionRequest,
        mode: Mode,
        envelope: impl FnOnce(Prepared) -> Result<http::Request<Body>, EncodeError>,
    ) -> Result<Encoded, EncodeError> {
        let model = request.model.clone().unwrap_or_else(|| self.model.clone());
        let fine_grained = !request.tools.is_empty()
            && self.tool_input_streaming == ToolInputStreaming::BetaHeader;
        let body = super::completion::body(self, &request, mode)?;
        let betas: Vec<&str> = [
            body.get("thinking")
                .and_then(|thinking| thinking.get("block_binding"))
                .is_some_and(serde_json::Value::is_object)
                .then_some(super::completion::THINKING_BINDING_BETA),
            fine_grained.then_some(super::completion::FINE_GRAINED_TOOL_STREAMING_BETA),
        ]
        .into_iter()
        .flatten()
        .collect();
        crate::codecs::providers::internal::trace_json(
            crate::codecs::providers::internal::LogTarget::Completions,
            "Anthropic completion request",
            &body,
        );
        let request = envelope(Prepared {
            model,
            body,
            betas: self.provider.betas_with(&betas),
        })?;
        Ok(Encoded::new(
            request,
            match mode {
                Mode::Unary => Framing::Whole,
                Mode::Streaming => Framing::Sse,
            },
        )
        .with_request_id_header(self.provider.dialect.request_id_header)
        .with_projection(MessagesDecoder::project))
    }
}

impl Wire for Messages {
    type Op = Completion;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = super::streaming::MessagesEvent;
    type Decoder<'id> = MessagesDecoder;
    type Reassembler = super::streaming::document::Message;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(self.provider.dialect.name)
            .model(self.model.as_str())
            .facts(&self.facts)
            .capabilities(Capabilities::completion(
                ProviderCapabilities::default()
                    .with_native_output_tool_composition(
                        self.provider.dialect.quirks.supports_output_format,
                    )
                    .with_forced_tool_choice_rejected(rejects_forced_tool_choice(
                        &self.facts,
                        &self.model,
                    )),
            ))
            .replay(self)
    }

    fn encode(&self, request: CompletionRequest, mode: Mode) -> Result<Encoded, EncodeError> {
        let session = self.provider.dialect.quirks.session_header.map(|name| {
            let id = crate::codecs::providers::internal::request_session_id(&request);
            (name, id)
        });
        self.encode_with(request, mode, |prepared| {
            let mut builder = self
                .provider
                .headers_with(
                    http::Request::post(format!("{}/v1/messages", self.provider.base_url)),
                    &prepared.betas,
                )
                .header(http::header::CONTENT_TYPE, "application/json");
            if let Some((name, id)) = session {
                builder = builder.header(name, id);
            }
            Ok(builder.body(prepared.body.into_body())?)
        })
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        MessagesDecoder::new(self.provider.dialect.quirks)
    }
}

impl crate::codecs::completion::ReplayTarget for Messages {
    fn catalog_priced(&self) -> bool {
        self.provider.dialect.quirks.catalog_priced
    }

    fn map_options(
        &self,
        request: &CompletionRequest,
        fields: crate::codecs::completion::options::OptionFields<'_>,
    ) -> crate::codecs::completion::options::OptionMap {
        super::options::map_options(self, request, fields)
    }

    fn facts(&self) -> Option<&ModelFacts> {
        Some(&self.facts)
    }

    fn api(&self) -> crate::codecs::message::Api {
        crate::codecs::message::Api::from_static(MESSAGES_API)
    }

    fn provider(&self) -> &str {
        self.provider.dialect.name
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn accepts(&self, model: &str) -> crate::codecs::completion::Accepts {
        let rule: Option<fn(&str) -> bool> = match self.provider.dialect.name {
            name if name == ANTHROPIC.name => None,
            name if name == ZAI.name => Some(crate::codecs::providers::zai::reads_images),
            name if name == MOONSHOT.name => Some(crate::codecs::providers::moonshot::reads_images),
            name if name == MINIMAX.name => Some(crate::codecs::providers::minimax::reads_images),
            name if name == XIAOMIMIMO.name => {
                Some(crate::codecs::providers::xiaomimimo::reads_images)
            }
            _ => Some(|_| true),
        };
        let images = rule.is_none_or(|rule| {
            self.facts
                .reads_images_or(self.provider.dialect.name, model, rule)
        });
        crate::codecs::completion::Accepts {
            user_images: images,
            assistant_images: false,
            tool_result_images: images,
            tools: true,
            // Thinking replays only with its signature.
            foreign_reasoning: false,
        }
    }

    fn encodes(&self, _model: &str, media: crate::codecs::completion::Media<'_>) -> bool {
        use crate::codecs::completion::{Media, Place};
        match media {
            Media::Image(image, Place::User | Place::ToolResult) => image_source(image).is_some(),
            Media::Document(document) => document_source(document).is_some(),
            Media::Image(_, Place::Assistant) | Media::Audio(_) | Media::Video(_) => false,
        }
    }

    fn normalize_tool_call_id(
        &self,
        id: &str,
        _model: &str,
        _source: Option<&crate::codecs::message::Origin>,
    ) -> String {
        crate::codecs::providers::internal::wire_ids::legal_call_id(id, 64)
    }

    fn identity(&self, item: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        item.get("caller")
            .filter(|_| item.get("type").and_then(serde_json::Value::as_str) == Some("tool_use"))
            .map(|caller| serde_json::Map::from_iter([("caller".to_owned(), caller.clone())]))
            .unwrap_or_default()
    }

    fn later_system(&self, model: &str) -> crate::codecs::completion::LaterSystem {
        if super::completion::takes_mid_conversation_system(&self.facts, model) {
            crate::codecs::completion::LaterSystem::InPlace
        } else {
            crate::codecs::completion::LaterSystem::Leading
        }
    }

    fn binds_context(&self, model: &str) -> bool {
        self.provider.dialect.name == ANTHROPIC.name
            && super::completion::binds_context(&self.facts, model)
    }

    fn drops_unbound_items(&self, request: &CompletionRequest) -> bool {
        let model = request.model.as_deref().unwrap_or(&self.model);
        let thinking = crate::codecs::completion::options::param(self, request, "thinking");
        super::completion::drops_unbound_thinking(self, model, thinking.as_ref())
    }

    fn sends_alone(&self, block: &crate::codecs::message::AssistantContent) -> bool {
        let ids = crate::codecs::providers::internal::wire_ids::WireIds::default();
        super::completion::assistant_part(block, self, &ids).is_some()
    }

    fn call_id_slot(&self) -> Option<&'static str> {
        Some("/id")
    }

    fn blob_slots(&self) -> &'static [&'static str] {
        &["/signature", "/data"]
    }

    fn hosted_pair(
        &self,
        item: &serde_json::Value,
    ) -> Option<(crate::codecs::completion::Pairing, String)> {
        use crate::codecs::completion::Pairing;
        let kind = item.get("type")?.as_str()?;
        let (side, key) = if kind.ends_with("_tool_use") {
            (Pairing::Use, "id")
        } else if kind.ends_with("_tool_result") {
            (Pairing::Result, "tool_use_id")
        } else {
            return None;
        };
        Some((side, item.get(key)?.as_str()?.to_owned()))
    }
}
