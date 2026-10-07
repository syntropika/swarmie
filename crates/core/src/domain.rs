use crate::semantic::{PlanUpdate, Question, QuestionAnswer, SubagentInfo, ToolDetails};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCommand {
    pub name: String,
    pub title: String,
    pub mutates: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionInfo {
    pub id: String,
    pub commands: Vec<ExtensionCommand>,
    #[serde(default)]
    pub views: Vec<ExtensionView>,
}
/// An optional client presentation of a headless extension command.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionView {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub command: String,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported capability: {0}")]
    Unsupported(String),
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("session is closed")]
    Closed,
    #[error("session is busy")]
    Busy,
    #[error("provider error: {0}")]
    Provider(String),
    #[error("request timed out: {0}")]
    Timeout(String),
    #[error("event consumer lagged by {0} events; use session/events to recover")]
    Lagged(u64),
    #[error("storage error: {0}")]
    Storage(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub resume: bool,
    pub fork: bool,
    pub steering: bool,
    pub cancel: bool,
    pub permissions: bool,
    pub reasoning: bool,
    pub usage: bool,
    pub models: bool,
    pub configure: bool,
    pub rollback: bool,
    pub compact: bool,
    pub modes: bool,
    pub images: bool,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpawnOptions {
    pub harness: String,
    pub cwd: PathBuf,
    #[serde(default)]
    pub model: Option<String>,
    /// Provider-specific configuration remains opaque to the core.
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub execution: SessionExecutionOptions,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionExecutionOptions {
    /// Process overrides are supplied anew on resume and never included in saved snapshots.
    #[serde(default, skip_serializing)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub config_home: Option<PathBuf>,
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
    #[serde(default)]
    pub tools: ToolPolicy,
    #[serde(default)]
    pub output_schema: Option<Value>,
    #[serde(default)]
    pub ephemeral: bool,
}
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolPolicy {
    #[default]
    Enabled,
    Disabled,
}
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum RollbackScope {
    #[default]
    ConversationOnly,
    ConversationAndFiles,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub name: String,
    pub transport: McpTransport,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransport {
    Stdio {
        command: PathBuf,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default, skip_serializing)]
        env: BTreeMap<String, String>,
    },
    Http {
        url: String,
        #[serde(default, skip_serializing)]
        headers: BTreeMap<String, String>,
    },
    Sse {
        url: String,
        #[serde(default, skip_serializing)]
        headers: BTreeMap<String, String>,
    },
}
impl SpawnOptions {
    pub fn new(harness: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            harness: harness.into(),
            cwd: cwd.into(),
            model: None,
            config: Value::Null,
            execution: SessionExecutionOptions::default(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NativeSessionRef {
    pub provider: String,
    pub id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Idle,
    Running,
    NeedsInput,
    Cancelling,
    Closed,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub id: String,
    pub native: NativeSessionRef,
    pub options: SpawnOptions,
    pub capabilities: Capabilities,
    pub state: SessionState,
    pub sequence: u64,
    pub turn: Option<String>,
    pub turns_completed: u64,
    #[serde(default)]
    pub metadata: SessionMetadata,
    #[serde(default)]
    pub queue: PromptQueue,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadata {
    pub title: Option<String>,
    pub archived: bool,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub fork_origin: Option<ForkOrigin>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForkOrigin {
    pub session_id: String,
    pub message_id: Option<String>,
    pub sequence: u64,
    pub turn_id: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadataUpdate {
    pub title: Option<String>,
    pub archived: Option<bool>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PromptQueue {
    pub items: Vec<QueuedPrompt>,
    pub paused: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueuedPrompt {
    pub id: String,
    pub prompt: Prompt,
    pub created_at_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub harness: String,
    pub available: bool,
    pub executable: Option<PathBuf>,
    pub capabilities: Capabilities,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HarnessProbe {
    pub harness: String,
    pub available: bool,
    pub executable: Option<PathBuf>,
    pub version: Option<String>,
    /// `authenticated`, `unauthenticated`, `unknown`, or `unsupported`.
    pub auth_state: String,
    pub message: Option<String>,
    pub capabilities: Capabilities,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub id: String,
    pub label: String,
    pub kind: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub command: Option<Value>,
    #[serde(default)]
    pub paths: Vec<String>,
    pub options: Vec<PermissionOption>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<Question>,
    #[serde(default)]
    pub metadata: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponse {
    pub id: String,
    pub option: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub answers: BTreeMap<String, QuestionAnswer>,
    #[serde(default)]
    pub data: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum PromptContent {
    Text(String),
    Image { data: String, mime_type: String },
    Resource { uri: String },
    Skill { name: String, path: String },
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Prompt {
    pub content: Vec<PromptContent>,
}
impl From<&str> for Prompt {
    fn from(value: &str) -> Self {
        Self {
            content: vec![PromptContent::Text(value.into())],
        }
    }
}
impl Prompt {
    pub fn text(&self) -> Result<String> {
        self.content
            .iter()
            .map(|c| match c {
                PromptContent::Text(t) => Ok(t.as_str()),
                _ => Err(Error::Unsupported("non-text content".into())),
            })
            .collect::<Result<Vec<_>>>()
            .map(|p| p.join("\n"))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelReroute {
    pub from_model: String,
    pub to_model: String,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolEvent {
    pub id: String,
    pub name: Option<String>,
    pub status: String,
    pub input: Value,
    pub output: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<ToolDetails>,
    #[serde(default)]
    pub metadata: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub scope: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub used_tokens: Option<u64>,
    pub context_window_tokens: Option<u64>,
    pub remaining_tokens: Option<u64>,
    pub percent: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type", content = "data")]
pub enum EventKind {
    #[serde(rename = "session.started")]
    SessionStarted(Box<SessionSnapshot>),
    #[serde(rename = "session.updated")]
    SessionUpdated(Value),
    #[serde(rename = "configuration.updated")]
    ConfigurationUpdated(Value),
    #[serde(rename = "model.rerouted")]
    ModelRerouted(ModelReroute),
    #[serde(rename = "commands.updated")]
    CommandsUpdated(Value),
    #[serde(rename = "quotas.updated")]
    QuotasUpdated(Value),
    #[serde(rename = "queue.changed")]
    QueueChanged(PromptQueue),
    #[serde(rename = "conversation.rolled_back")]
    ConversationRolledBack { turns: u64 },
    #[serde(rename = "session.ended")]
    SessionEnded,
    #[serde(rename = "turn.started")]
    TurnStarted,
    #[serde(rename = "turn.ended")]
    TurnEnded { reason: String },
    #[serde(rename = "message.user")]
    UserMessage(Prompt),
    #[serde(rename = "message.assistant")]
    AssistantMessage(Value),
    #[serde(rename = "message.ended")]
    MessageEnded { item_id: Option<String> },
    #[serde(rename = "output.structured")]
    StructuredOutput(Value),
    #[serde(rename = "text.delta")]
    TextDelta {
        text: String,
        item_id: Option<String>,
    },
    #[serde(rename = "reasoning.delta")]
    ReasoningDelta { text: String },
    #[serde(rename = "tool.started")]
    ToolStarted(ToolEvent),
    #[serde(rename = "tool.updated")]
    ToolUpdated(ToolEvent),
    #[serde(rename = "tool.completed")]
    ToolCompleted(ToolEvent),
    #[serde(rename = "permission.requested")]
    PermissionRequested(PermissionRequest),
    #[serde(rename = "permission.resolved")]
    PermissionResolved { id: String, option: String },
    #[serde(rename = "file.changed")]
    FileChanged(Value),
    #[serde(rename = "diff")]
    Diff(Value),
    #[serde(rename = "plan.updated")]
    PlanUpdated(PlanUpdate),
    #[serde(rename = "subagent.updated")]
    SubagentUpdated(SubagentInfo),
    #[serde(rename = "usage.updated")]
    UsageUpdated(Usage),
    #[serde(rename = "context.updated")]
    ContextUpdated(ContextUsage),
    #[serde(rename = "state.changed")]
    StateChanged(SessionState),
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "diagnostic")]
    Diagnostic(Value),
    #[serde(rename = "provider.raw")]
    Raw,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEvent {
    pub session_id: String,
    pub sequence: u64,
    pub turn_id: Option<String>,
    pub source: String,
    pub timestamp_ms: u64,
    #[serde(flatten)]
    pub kind: EventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}
/// Adapters report facts. Session transitions and turn identifiers belong to the engine.
#[derive(Debug)]
pub enum BackendEvent {
    Content {
        kind: Box<EventKind>,
        raw: Option<Value>,
    },
    TurnFinished {
        reason: String,
        raw: Option<Value>,
    },
    NativeSessionChanged(NativeSessionRef),
    Exited {
        message: String,
    },
}
impl BackendEvent {
    pub fn content(kind: EventKind, raw: Value) -> Self {
        Self::Content {
            kind: Box::new(kind),
            raw: Some(raw),
        }
    }
}
