use serde::{Deserialize, Serialize};

use crate::codecs::wire::Secret;

use super::responses_api::SystemInstructionsPlacement;
use super::responses_api::wire::Responses;

mod auth;
pub(crate) mod chat;
mod dialects;
pub(crate) mod dto;
mod modality;
mod route;

use auth::default_user_agent;
pub use auth::{Auth, AuthAlternative, CallerIdentity, Identity};
pub use chat::Chat;
pub use dialects::*;
pub use modality::{
    AcceptedWidths, DimensionsField, EmbeddingQuirks, Embeddings, EmbeddingsDecoder, ImageBody,
    ModelEntry, ModelWidth, Models, ModelsReply, Rerank, RerankQuirks, RerankReply,
    RerankResultEntry, RerankUsage, SpeechBody, TranscriptionBody, Transcriptions, Verify,
    VerifyDecoder,
};
pub use route::{OpenAiDecoder, OpenAiEvent, OpenAiReassembler, OpenAiWire, Route};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubRoute {
    #[default]
    HFInference,
    Together,
    SambaNova,
    Fireworks,
    Hyperbolic,
    Nebius,
    Novita,
    Custom(String),
}

impl SubRoute {
    pub fn slug(&self) -> &str {
        match self {
            Self::HFInference => "hf-inference/models",
            Self::Together => "together",
            Self::SambaNova => "sambanova",
            Self::Fireworks => "fireworks-ai",
            Self::Hyperbolic => "hyperbolic",
            Self::Nebius => "nebius",
            Self::Novita => "novita",
            Self::Custom(route) => route,
        }
    }

    pub fn model_identifier(&self, model: &str) -> String {
        const FIREWORKS_PREFIX: &str = "accounts/fireworks/models/";
        match self {
            Self::Fireworks if !model.starts_with(FIREWORKS_PREFIX) => {
                format!("{FIREWORKS_PREFIX}{model}")
            }
            _ => model.to_owned(),
        }
    }
}

impl std::fmt::Display for SubRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.slug())
    }
}

impl From<&str> for SubRoute {
    fn from(route: &str) -> Self {
        Self::Custom(route.to_owned())
    }
}

impl From<String> for SubRoute {
    fn from(route: String) -> Self {
        Self::Custom(route)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Routing {
    Path,
    AzureDeployment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputCap {
    Legacy,
    OpenAiReasoningFamilies,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StrictSchema {
    OpenAi,
    DeepSeek,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BodyRewrite {
    None,
    HuggingFaceRouter,
    DeepSeek,
    Mira,
    Perplexity,
    Mistral,
    LlamaCpp,
    Moonshot,
    OpenRouter,
    Ollama,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponsesContract {
    OpenAi,
    Xai,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ResponsesQuirks {
    pub path: &'static str,
    pub system_instructions: SystemInstructionsPlacement,
    pub system_role: &'static str,
    pub contract: ResponsesContract,
    pub strict_tools_by_default: bool,
    pub reasoning_ciphertext: bool,
}

impl ResponsesQuirks {
    pub const fn openai() -> Self {
        Self {
            path: "/responses",
            system_instructions: SystemInstructionsPlacement::Instructions,
            system_role: "system",
            contract: ResponsesContract::OpenAi,
            strict_tools_by_default: false,
            reasoning_ciphertext: true,
        }
    }
}

#[derive(Debug)]
pub struct DialectHooks {
    pub default_endpoint: Option<fn(&str) -> Option<String>>,
    pub model_route: Option<fn(&str) -> Route>,
    pub completion_envelope: Option<CompletionEnvelope>,
    pub modality_envelope: Option<ModalityEnvelope>,
}

pub type ModalityEnvelope =
    fn(&OpenAIConfig, &mut http::Request<crate::codecs::wire::Body>) -> Result<(), http::Error>;

pub type CompletionEnvelope = fn(
    &OpenAIConfig,
    &crate::codecs::completion::CompletionRequest,
    http::request::Builder,
) -> http::request::Builder;

impl PartialEq for DialectHooks {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for DialectHooks {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Quirks {
    pub hooks: Option<&'static DialectHooks>,
    pub auth: Auth,
    pub routing: Routing,
    pub completion_route: Route,
    pub routes_by_catalog: bool,
    pub completion_path: &'static str,
    pub embeddings_path: &'static str,
    pub models_path: &'static str,
    pub verify_path: &'static str,
    pub transcription_path: &'static str,
    pub image_generation_path: &'static str,
    pub audio_generation_path: &'static str,
    pub supports_tools: bool,
    pub strict_tools_prefix: Option<&'static str>,
    pub strict_schema: StrictSchema,
    pub supports_response_format: bool,
    pub response_format_with_tools: bool,
    pub supports_image_tool_results: bool,
    pub later_system: crate::codecs::completion::LaterSystem,
    pub stream_include_usage: bool,
    pub done_without_finish_reason: bool,
    pub output_cap: OutputCap,
    pub native_finish_reason: bool,
    pub finishes: &'static [(&'static str, crate::codecs::completion::FinishReason)],
    pub reasoning_field: Option<&'static str>,
    pub reliable_reasoning_count: bool,
    pub accepts_bare_string_reply: bool,
    pub accepts_file_ids: bool,
    pub short_lived_key: bool,
    pub rewrite: BodyRewrite,
    pub root_relative_routes: &'static [&'static str],
    pub model_is_modality_path: bool,
    pub image_body: ImageBody,
    pub transcription_body: TranscriptionBody,
    pub speech_body: SpeechBody,
    pub embedding: EmbeddingQuirks,
    pub rerank: RerankQuirks,
    pub base_url_env_alias: Option<&'static str>,
    pub account_id_env: Option<&'static str>,
    pub default_instructions: Option<&'static str>,
    pub instructions_env: Option<&'static str>,
    pub identity: Option<Identity>,
    pub session_header: Option<&'static str>,
    pub responses: ResponsesQuirks,
    pub catalog_priced: bool,
}

impl Quirks {
    pub const fn openai() -> Self {
        Self {
            hooks: None,
            auth: Auth::Bearer,
            routing: Routing::Path,
            completion_route: Route::Chat,
            routes_by_catalog: false,
            completion_path: "/chat/completions",
            embeddings_path: "/embeddings",
            models_path: "/models",
            verify_path: "/models",
            transcription_path: "/audio/transcriptions",
            image_generation_path: "/images/generations",
            audio_generation_path: "/audio/speech",
            supports_tools: true,
            strict_tools_prefix: None,
            strict_schema: StrictSchema::OpenAi,
            supports_response_format: true,
            response_format_with_tools: false,
            supports_image_tool_results: false,
            later_system: crate::codecs::completion::LaterSystem::InPlace,
            stream_include_usage: true,
            done_without_finish_reason: false,
            output_cap: OutputCap::Legacy,
            native_finish_reason: false,
            finishes: &[],
            reasoning_field: None,
            reliable_reasoning_count: true,
            accepts_bare_string_reply: false,
            accepts_file_ids: true,
            short_lived_key: false,
            rewrite: BodyRewrite::None,
            embedding: EmbeddingQuirks::openai(),
            // OpenAI has no reranking endpoint, and neither does any dialect
            // on this wire but llama.cpp.
            rerank: RerankQuirks::unsupported(),
            root_relative_routes: &[],
            model_is_modality_path: false,
            image_body: ImageBody::OpenAi,
            speech_body: SpeechBody::OpenAi,
            transcription_body: TranscriptionBody::Multipart,
            base_url_env_alias: None,
            account_id_env: None,
            default_instructions: None,
            instructions_env: None,
            identity: None,
            session_header: None,
            responses: ResponsesQuirks::openai(),
            catalog_priced: true,
        }
    }

    pub const fn without_stream_usage(mut self) -> Self {
        self.stream_include_usage = false;
        self
    }

    pub const fn done_without_finish_reason(mut self) -> Self {
        self.done_without_finish_reason = true;
        self
    }

    pub const fn without_response_format(mut self) -> Self {
        self.supports_response_format = false;
        self
    }
}

impl Default for Quirks {
    fn default() -> Self {
        Self::openai()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dialect {
    pub name: &'static str,
    pub base_url: &'static str,
    pub api_key_env: &'static str,
    pub base_url_env: Option<&'static str>,
    pub request_id_header: Option<&'static str>,
    pub alternate_auth: Option<AuthAlternative>,
    pub quirks: Quirks,
}

impl Dialect {
    pub const fn gateway(
        name: &'static str,
        base_url: &'static str,
        api_key_env: &'static str,
    ) -> Self {
        Self {
            name,
            base_url,
            api_key_env,
            base_url_env: None,
            request_id_header: None,
            alternate_auth: None,
            quirks: Quirks::openai(),
        }
    }

    pub const fn with_quirks(mut self, quirks: Quirks) -> Self {
        self.quirks = quirks;
        self
    }
}

impl Serialize for Dialect {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let registered = dialects::by_name(self.name) == Some(self);
        crate::codecs::providers::internal::named_dialect::serialize(
            serializer, "OpenAI", self.name, registered,
        )
    }
}

impl<'de> Deserialize<'de> for Dialect {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        crate::codecs::providers::internal::named_dialect::deserialize(
            deserializer,
            "OpenAI",
            |name| dialects::by_name(name).copied(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenAIConfig {
    pub api_key: Secret,
    pub base_url: String,
    pub dialect: Dialect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<Route>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_api_version: Option<String>,
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_route: Option<SubRoute>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<CallerIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_instructions: Option<SystemInstructionsPlacement>,
}

impl OpenAIConfig {
    pub fn new(api_key: impl Into<Secret>) -> Self {
        Self::with_key(&OPENAI, api_key)
    }

    pub fn with_key(dialect: &Dialect, api_key: impl Into<Secret>) -> Self {
        let quirks = &dialect.quirks;
        let api_key = api_key.into();
        let base_url = quirks
            .hooks
            .and_then(|hooks| hooks.default_endpoint)
            .and_then(|endpoint| endpoint(api_key.expose()))
            .unwrap_or_else(|| dialect.base_url.to_owned());
        Self {
            api_key,
            base_url,
            dialect: *dialect,
            route: None,
            // Azure deployment URLs require an explicit API version.
            api_version: match quirks.routing {
                Routing::AzureDeployment => Some(dialects::AZURE_DEFAULT_API_VERSION.to_owned()),
                Routing::Path => None,
            },
            audio_api_version: None,
            auth: quirks.auth,
            sub_route: None,
            account_id: None,
            instructions: quirks.default_instructions.map(str::to_owned),
            identity: quirks.identity.map(|identity| CallerIdentity {
                originator: identity.originator.to_owned(),
                user_agent: default_user_agent(identity.originator),
            }),
            system_instructions: None,
        }
    }

    pub fn with_dialect(self, dialect: &Dialect) -> Self {
        Self::with_key(dialect, self.api_key)
    }

    pub fn with_sub_route(mut self, sub_route: SubRoute) -> Self {
        self.sub_route = Some(sub_route);
        self
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_api_version(mut self, api_version: impl Into<String>) -> Self {
        self.api_version = Some(api_version.into());
        self
    }

    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    pub fn with_system_instructions_placement(
        mut self,
        placement: SystemInstructionsPlacement,
    ) -> Self {
        self.system_instructions = Some(placement);
        self
    }

    pub fn with_system_instructions_as_messages(self) -> Self {
        self.with_system_instructions_placement(SystemInstructionsPlacement::InputSystemMessages)
    }

    pub fn system_instructions_placement(&self) -> SystemInstructionsPlacement {
        self.system_instructions
            .unwrap_or(self.dialect.quirks.responses.system_instructions)
    }

    pub fn with_route(mut self, route: Route) -> Self {
        self.route = Some(route);
        self
    }

    pub fn completion_route(&self) -> Route {
        self.route.unwrap_or(self.dialect.quirks.completion_route)
    }

    pub fn completion(&self, model: impl Into<String>) -> OpenAiWire {
        OpenAiWire::new(self.clone(), model)
    }

    pub fn responses(&self, model: impl Into<String>) -> Responses {
        Responses::new(self.clone(), model)
    }

    pub fn chat(&self, model: impl Into<String>) -> Chat {
        Chat::new(self.clone(), model)
    }

    pub(crate) fn completion_headers(
        &self,
        request: &crate::codecs::completion::CompletionRequest,
        builder: http::request::Builder,
    ) -> http::request::Builder {
        let builder = self.headers_in_session(builder, request.options.cache_key.as_deref());
        match self
            .dialect
            .quirks
            .hooks
            .and_then(|hooks| hooks.completion_envelope)
        {
            Some(envelope) => envelope(self, request, builder),
            None => builder,
        }
    }

    pub(crate) fn uri(&self, path: &str, model: Option<&str>) -> String {
        self.uri_versioned(path, model, self.api_version.as_deref())
    }

    pub(crate) fn uri_versioned(
        &self,
        path: &str,
        model: Option<&str>,
        api_version: Option<&str>,
    ) -> String {
        match (self.dialect.quirks.routing, model) {
            (Routing::AzureDeployment, Some(model)) => format!(
                "{}/openai/deployments/{}{}?api-version={}",
                self.base_url.trim_end_matches('/'),
                model.trim_start_matches('/'),
                path,
                api_version.unwrap_or_default(),
            ),
            _ => format!("{}{}", self.base(path), path),
        }
    }

    fn base(&self, path: &str) -> &str {
        let base = self.base_url.trim_end_matches('/');
        if self.dialect.quirks.root_relative_routes.contains(&path) {
            return base.strip_suffix("/v1").unwrap_or(base);
        }
        base
    }

    pub(crate) fn route(&self) -> std::borrow::Cow<'_, SubRoute> {
        match &self.sub_route {
            Some(route) => std::borrow::Cow::Borrowed(route),
            None => std::borrow::Cow::Owned(SubRoute::default()),
        }
    }

    pub(crate) fn deployment<'a>(&self, model: &'a str) -> Option<&'a str> {
        match self.dialect.quirks.routing {
            Routing::AzureDeployment => Some(model),
            Routing::Path => None,
        }
    }
}
