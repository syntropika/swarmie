use crate::{Extension, Message, ToolDefinition};
use tembra::prelude::*;

/// Connection identity selects a private configuration, never a URL or credential.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ModelIdentity {
    pub connection: String,
    pub model: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct GenerationOptions {
    pub temperature: Option<f64>,
    pub max_output_tokens: Option<u32>,
    pub top_p: Option<f64>,
    pub reasoning_effort: Option<String>,
    pub response_format: Option<ResponseFormat>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "tembra::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "tembra::schemars", transform = crate::tagged_union)]
pub enum ResponseFormat {
    Json,
    JsonSchema {
        name: String,
        schema: serde_json::Value,
    },
}

/// Caller-owned correlation; request_id is not a provider idempotency key. Use a fresh ID
/// for each physical attempt. Reacquiring an expired generation can execute another attempt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ModelRequest {
    pub request_id: String,
    pub model: ModelIdentity,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
    #[serde(default)]
    pub options: GenerationOptions,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    StopSequence,
    Other,
    Unknown,
}

/// No inferred prices: cost is absent unless an authoritative provider supplies it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ModelResponse {
    pub request_id: String,
    pub model: ModelIdentity,
    pub message: Message,
    pub finish_reason: FinishReason,
    pub raw_finish_reason: Option<String>,
    pub usage: TokenUsage,
    pub provider_response_id: Option<String>,
    pub extensions: Vec<Extension>,
}

/// Supported means the adapter path is implemented; provider/model acceptance remains remote.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum Support {
    Supported,
    Unsupported,
    ModelDependent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct Capabilities {
    pub streaming: Support,
    pub tools: Support,
    pub json: Support,
    pub json_schema: Support,
    pub reasoning: Support,
    pub images: Support,
    pub developer_role: Support,
    pub model_discovery: Support,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum ProviderErrorKind {
    InvalidRequest,
    Unsupported,
    Busy,
    Authentication,
    RateLimited,
    Http,
    Parsing,
    Timeout,
    Cancelled,
    PayloadLimit,
    Transport,
    DataLoss,
}

/// SafeToRetry is restricted to known local refusal before any remote attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum Submission {
    NotSent,
    MayHaveBeenSent,
    ResponseReceived,
}

/// A sanitized category, never a vendor error string, response body, URL or request payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub submission: Submission,
    pub http_status: Option<u16>,
    pub retry_after_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "tembra::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "tembra::schemars", transform = crate::tagged_union)]
pub enum GenerateResult {
    Response { response: Box<ModelResponse> },
    Error { error: ProviderError },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "tembra::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "tembra::schemars", transform = crate::tagged_union)]
pub enum ModelList {
    Models { models: Vec<ModelIdentity> },
    Error { error: ProviderError },
}
