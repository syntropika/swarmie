use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use super::completion::{CLAUDE_OPUS_4_8, CLAUDE_OPUS_5, CLAUDE_OPUS_5_5};
use super::wire::MESSAGES_API;
use crate::codecs::catalog::Sampling;
use crate::codecs::completion::{
    CompletionRequest, ExtensionOptions, ProviderExtension, ReplayTarget, ReplyExtras,
};
use crate::codecs::message::Api;

pub const FAST_MODE_BETA: &str = "fast-mode-2026-02-01";
pub const TASK_BUDGETS_BETA: &str = "task-budgets-2026-03-13";
pub const SERVER_SIDE_FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
pub const SERVER_SIDE_FALLBACK_MODELS_BETA: &str = "server-side-fallback-2026-06-01";
pub const CONTEXT_MANAGEMENT_BETA: &str = "context-management-2025-06-27";
pub const MCP_CLIENT_BETA: &str = "mcp-client-2025-11-20";
pub const CACHE_DIAGNOSIS_BETA: &str = "cache-diagnosis-2026-04-07";

const FAST_MODELS: &[&str] = &[CLAUDE_OPUS_5_5, CLAUDE_OPUS_5, CLAUDE_OPUS_4_8];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnthropicExt;

impl ProviderExtension for AnthropicExt {
    const PROVIDER: &'static str = super::PROVIDER_NAME;
    type Options = AnthropicOptions;
    type Extras = AnthropicExtras;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct AnthropicOptions {
    #[serde(rename = "*")]
    pub shared: AnthropicShared,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnthropicShared {
    pub top_k: Option<u32>,
    pub metadata_user_id: Option<String>,
    pub inference_geo: Option<InferenceGeo>,
    pub speed: Option<Speed>,
    pub task_budget: Option<TaskBudget>,
    pub fallbacks: Option<Fallbacks>,
    pub container: Option<ContainerParam>,
    pub context_management: Option<ContextManagement>,
    pub mcp_servers: Vec<McpServer>,
    pub diagnostics_previous_message_id: Option<Option<String>>,
}

impl Serialize for AnthropicShared {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Self {
            top_k,
            metadata_user_id,
            inference_geo,
            speed,
            task_budget,
            fallbacks,
            container,
            context_management,
            mcp_servers,
            diagnostics_previous_message_id,
        } = self;
        let mut map = serializer.serialize_map(None)?;
        if let Some(top_k) = top_k {
            map.serialize_entry("top_k", top_k)?;
        }
        if let Some(user_id) = metadata_user_id {
            map.serialize_entry("metadata", &Nested("user_id", user_id))?;
        }
        if let Some(geo) = inference_geo {
            map.serialize_entry("inference_geo", geo)?;
        }
        if let Some(speed) = speed {
            map.serialize_entry("speed", speed)?;
        }
        if let Some(budget) = task_budget {
            map.serialize_entry("output_config", &Nested("task_budget", budget))?;
        }
        if let Some(fallbacks) = fallbacks {
            map.serialize_entry("fallbacks", fallbacks)?;
        }
        if let Some(container) = container {
            map.serialize_entry("container", container)?;
        }
        if let Some(context_management) = context_management {
            map.serialize_entry("context_management", context_management)?;
        }
        if !mcp_servers.is_empty() {
            map.serialize_entry("mcp_servers", mcp_servers)?;
        }
        if let Some(previous) = diagnostics_previous_message_id {
            map.serialize_entry("diagnostics", &Nested("previous_message_id", previous))?;
        }
        map.end()
    }
}

pub(crate) struct Nested<'a, T>(pub(crate) &'static str, pub(crate) &'a T);

impl<T: Serialize> Serialize for Nested<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry(self.0, self.1)?;
        map.end()
    }
}

impl AnthropicOptions {
    pub fn top_k(mut self, top_k: u32) -> Self {
        self.shared.top_k = Some(top_k);
        self
    }

    pub fn metadata_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.shared.metadata_user_id = Some(user_id.into());
        self
    }

    pub fn inference_geo(mut self, geo: InferenceGeo) -> Self {
        self.shared.inference_geo = Some(geo);
        self
    }

    pub fn speed(mut self, speed: Speed) -> Self {
        self.shared.speed = Some(speed);
        self
    }

    pub fn task_budget(mut self, budget: TaskBudget) -> Self {
        self.shared.task_budget = Some(budget);
        self
    }

    pub fn fallbacks(mut self, fallbacks: Fallbacks) -> Self {
        self.shared.fallbacks = Some(fallbacks);
        self
    }

    pub fn container(mut self, container: impl Into<ContainerParam>) -> Self {
        self.shared.container = Some(container.into());
        self
    }

    pub fn context_management(mut self, context_management: ContextManagement) -> Self {
        self.shared.context_management = Some(context_management);
        self
    }

    pub fn mcp_server(mut self, server: McpServer) -> Self {
        self.shared.mcp_servers.push(server);
        self
    }

    pub fn diagnostics_previous_message_id(mut self, previous: Option<String>) -> Self {
        self.shared.diagnostics_previous_message_id = Some(previous);
        self
    }
}

impl ExtensionOptions for AnthropicOptions {
    type Ext = AnthropicExt;

    fn unsupported(
        &self,
        target: &dyn ReplayTarget,
        request: &CompletionRequest,
    ) -> Vec<(&'static str, String)> {
        let model = request
            .model
            .as_deref()
            .filter(|model| !model.is_empty())
            .unwrap_or_else(|| target.model());
        let facts = target
            .facts()
            .unwrap_or_else(|| crate::codecs::catalog::ModelFacts::builtin());
        let Some(spec) = facts.for_model(target.provider(), model) else {
            return Vec::new();
        };
        let mut refused = Vec::new();
        if self.shared.top_k.is_some() && spec.sampling == Some(Sampling::Never) {
            refused.push(("top_k", "this model does not take `top_k`".to_owned()));
        }
        if self.shared.speed == Some(Speed::Fast) && !FAST_MODELS.contains(&spec.id.as_str()) {
            refused.push((
                "speed",
                "fast mode runs on Claude Opus 5.5, Opus 5 and Opus 4.8 only".to_owned(),
            ));
        }
        refused
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InferenceGeo {
    Us,
    Global,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Speed {
    Standard,
    Fast,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskBudget {
    pub total: u32,
    pub remaining: Option<u32>,
}

impl TaskBudget {
    pub fn new(total: u32) -> Self {
        Self {
            total,
            remaining: None,
        }
    }

    pub fn remaining(mut self, remaining: u32) -> Self {
        self.remaining = Some(remaining);
        self
    }
}

impl Serialize for TaskBudget {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Self { total, remaining } = self;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", "tokens")?;
        map.serialize_entry("total", total)?;
        if let Some(remaining) = remaining {
            map.serialize_entry("remaining", remaining)?;
        }
        map.end()
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fallbacks {
    Default,
    Models(Vec<String>),
}

impl Serialize for Fallbacks {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Default => serializer.serialize_str("default"),
            Self::Models(models) => {
                serializer.collect_seq(models.iter().map(|model| Nested("model", model)))
            }
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum ContainerParam {
    Id(String),
    Skills {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        skills: Vec<Skill>,
    },
}

impl From<String> for ContainerParam {
    fn from(id: String) -> Self {
        Self::Id(id)
    }
}

impl From<&str> for ContainerParam {
    fn from(id: &str) -> Self {
        Self::Id(id.to_owned())
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Skill {
    #[serde(rename = "type")]
    pub kind: SkillKind,
    pub skill_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl Skill {
    pub fn new(kind: SkillKind, skill_id: impl Into<String>) -> Self {
        Self {
            kind,
            skill_id: skill_id.into(),
            version: None,
        }
    }

    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillKind {
    Anthropic,
    Custom,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ContextManagement {
    pub edits: Vec<Value>,
}

impl ContextManagement {
    pub fn new(edits: Vec<Value>) -> Self {
        Self { edits }
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpServer {
    pub url: String,
    pub name: String,
    pub authorization_token: Option<String>,
}

impl McpServer {
    pub fn new(url: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            name: name.into(),
            authorization_token: None,
        }
    }

    pub fn authorization_token(mut self, token: impl Into<String>) -> Self {
        self.authorization_token = Some(token.into());
        self
    }
}

impl Serialize for McpServer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Self {
            url,
            name,
            authorization_token,
        } = self;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", "url")?;
        map.serialize_entry("url", url)?;
        map.serialize_entry("name", name)?;
        if let Some(token) = authorization_token {
            map.serialize_entry("authorization_token", token)?;
        }
        map.end()
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnthropicExtras {
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub stop_details: Option<StopDetails>,
    pub cache_creation: Option<CacheCreation>,
    pub service_tier: Option<String>,
    pub inference_geo: Option<String>,
    pub speed: Option<String>,
    pub server_tool_use: Option<ServerToolUse>,
    pub container: Option<Container>,
    pub fallback_model: Option<String>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct StopDetails {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub explanation: Option<String>,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct CacheCreation {
    #[serde(default)]
    pub ephemeral_5m_input_tokens: u64,
    #[serde(default)]
    pub ephemeral_1h_input_tokens: u64,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct ServerToolUse {
    #[serde(default)]
    pub web_search_requests: u64,
    #[serde(default)]
    pub web_fetch_requests: u64,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Container {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub expires_at: String,
}

#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    stop_sequence: Option<String>,
    #[serde(default)]
    stop_details: Option<StopDetails>,
    #[serde(default)]
    usage: Option<ReplyUsage>,
    #[serde(default)]
    container: Option<Container>,
    #[serde(default, rename = "content")]
    _content: Vec<serde::de::IgnoredAny>,
}

#[derive(Default, Deserialize)]
struct ReplyUsage {
    #[serde(default)]
    cache_creation: Option<CacheCreation>,
    #[serde(default)]
    service_tier: Option<String>,
    #[serde(default)]
    inference_geo: Option<String>,
    #[serde(default)]
    speed: Option<String>,
    #[serde(default)]
    server_tool_use: Option<ServerToolUse>,
}

impl ReplyExtras for AnthropicExtras {
    fn from_reply(api: &Api, raw: &Value) -> Result<Self, serde_json::Error> {
        if api.as_str() != MESSAGES_API {
            return Err(serde::de::Error::custom(format!(
                "Anthropic extras read a Messages reply, not one from `{api}`"
            )));
        }
        let Reply {
            stop_reason,
            stop_sequence,
            stop_details,
            usage,
            container,
            _content,
        } = Reply::deserialize(raw)?;
        let usage = usage.unwrap_or_default();
        let fallback_model = raw
            .pointer("/content/0")
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("fallback"))
            .and_then(|block| block.pointer("/to/model"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        Ok(Self {
            stop_reason,
            stop_sequence,
            stop_details,
            cache_creation: usage.cache_creation,
            service_tier: usage.service_tier,
            inference_geo: usage.inference_geo,
            speed: usage.speed,
            server_tool_use: usage.server_tool_use,
            container,
            fallback_model,
        })
    }
}

#[derive(Deserialize)]
pub(crate) struct MessagesStop {
    #[serde(default)]
    pub(crate) stop_reason: Option<String>,
    #[serde(default)]
    pub(crate) stop_sequence: Option<String>,
}

impl MessagesStop {
    pub(crate) fn read(provider: &str, api: &Api, raw: &Value) -> Result<Self, serde_json::Error> {
        if api.as_str() != MESSAGES_API {
            return Err(serde::de::Error::custom(format!(
                "{provider} extras read a Messages reply, not one from `{api}`"
            )));
        }
        Self::deserialize(raw)
    }
}
