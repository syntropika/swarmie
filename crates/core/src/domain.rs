use crate::WorkingContext;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<UiManifest>,
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
/// Versioned declarative client contributions. Runtime extensions never receive host DOM access.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiManifest {
    pub version: u32,
    #[serde(default)]
    pub commands: Vec<UiCommand>,
    #[serde(default)]
    pub slots: Vec<UiContribution>,
    #[serde(default)]
    pub screens: Vec<UiSurface>,
    #[serde(default)]
    pub panels: Vec<UiSurface>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub enum UiSlot {
    #[serde(rename = "sidebar.primary")]
    SidebarPrimary,
    #[serde(rename = "sidebar.footer")]
    SidebarFooter,
    #[serde(rename = "app.header.actions")]
    AppHeaderActions,
    #[serde(rename = "conversation.header.actions")]
    ConversationHeaderActions,
    #[serde(rename = "composer.actions")]
    ComposerActions,
    #[serde(rename = "composer.context")]
    ComposerContext,
    #[serde(rename = "composer.status")]
    ComposerStatus,
    #[serde(rename = "message.actions")]
    MessageActions,
    #[serde(rename = "message.details")]
    MessageDetails,
    #[serde(rename = "project.actions")]
    ProjectActions,
    #[serde(rename = "project.contextMenu")]
    ProjectContextMenu,
    #[serde(rename = "conversation.contextMenu")]
    ConversationContextMenu,
    #[serde(rename = "panel.toolbar")]
    PanelToolbar,
    #[serde(rename = "panel.status")]
    PanelStatus,
    #[serde(rename = "settings.sections")]
    SettingsSections,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiCommand {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
    #[serde(default)]
    pub when: Vec<UiCondition>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiContribution {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<UiIcon>,
    pub slot: UiSlot,
    pub kind: UiContributionKind,
    pub title: String,
    #[serde(default)]
    pub order: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    pub when: Vec<UiCondition>,
    #[serde(default)]
    pub enabled_when: Vec<UiCondition>,
    #[serde(default)]
    pub clients: Vec<UiClient>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum UiContributionKind {
    Action,
    Text,
    Section,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum UiClient {
    Browser,
    Desktop,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum UiIcon {
    Microphone,
    Stop,
    Spinner,
    Extension,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum UiCondition {
    Connected,
    Conversation,
    Project,
    Message,
    Panel,
    ComposerReady,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiSurface {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub clients: Vec<UiClient>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported capability: {0}")]
    Unsupported(String),
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("idempotency key already belongs to a different submission")]
    IdempotencyConflict,
    #[error("session is closed")]
    Closed,
    #[error("session is busy")]
    Busy,
    /// An adapter may use this only with authoritative evidence of no native effects.
    #[error("native input rejected without effects: {0}")]
    Rejected(String),
    #[error("operation cancelled: {0}")]
    Cancelled(String),
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

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    #[serde(default = "queue_capability")]
    pub queue: crate::Capability,
    pub commands: crate::Capability,
    pub quotas: crate::Capability,
    pub ephemeral: crate::Capability,
    pub output_schema: crate::Capability,
    pub tools_disabled: crate::Capability,
    pub fork_at_message: crate::Capability,
    pub fork_before_user: crate::Capability,
    pub native_skill_inputs: crate::Capability,
    pub submission_reconciliation: crate::Capability,

    pub resume: crate::Capability,
    pub fork: crate::Capability,
    pub steering: crate::Capability,
    pub cancel: crate::Capability,
    pub permissions: crate::Capability,
    pub operation_approvals: crate::Capability,
    pub reasoning: crate::Capability,
    pub usage: crate::Capability,
    pub models: crate::Capability,
    pub configure: crate::Capability,
    pub rollback: crate::Capability,
    pub compact: crate::Capability,
    pub modes: crate::Capability,
    pub images: crate::Capability,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}
fn queue_capability() -> crate::Capability {
    crate::Capability {
        source: crate::CapabilitySource::Runtime,
        ..true.into()
    }
}
impl Default for Capabilities {
    fn default() -> Self {
        Self {
            queue: queue_capability(),
            commands: Default::default(),
            quotas: Default::default(),
            ephemeral: Default::default(),
            output_schema: Default::default(),
            tools_disabled: Default::default(),
            fork_at_message: Default::default(),
            fork_before_user: Default::default(),
            native_skill_inputs: Default::default(),
            submission_reconciliation: Default::default(),

            resume: Default::default(),
            fork: Default::default(),
            steering: Default::default(),
            cancel: Default::default(),
            permissions: Default::default(),
            operation_approvals: Default::default(),
            reasoning: Default::default(),
            usage: Default::default(),
            models: Default::default(),
            configure: Default::default(),
            rollback: Default::default(),
            compact: Default::default(),
            modes: Default::default(),
            images: Default::default(),
            metadata: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpawnOptions {
    pub harness: String,
    #[serde(default)]
    pub cwd: PathBuf,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub project_revision: Option<u64>,
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
    /// Explicit host tool grants; native tool permissions remain independent.
    #[serde(default)]
    pub host_tools: Vec<crate::HostToolGrant>,
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
            project_id: None,
            project_revision: None,
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
    pub working_context: Option<WorkingContext>,
    #[serde(default)]
    pub execution_policy: crate::ExecutionPolicy,
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
/// A turn's terminal execution result, independent of native provider vocabulary.
/// Completion describes the execution cycle, not achievement of a user goal.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcome {
    Completed,
    Cancelled,
    Interrupted,
    Failed,
    Limited,
    Refused,
    Handled,
    #[default]
    Unknown,
}
impl TurnOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
            Self::Limited => "limited",
            Self::Refused => "refused",
            Self::Handled => "handled",
            Self::Unknown => "unknown",
        }
    }
}
impl std::fmt::Display for TurnOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
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
    TurnEnded {
        outcome: TurnOutcome,
        #[serde(rename = "nativeReason", skip_serializing_if = "Option::is_none")]
        native_reason: Option<String>,
    },
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
        outcome: TurnOutcome,
        native_reason: Option<String>,
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

#[cfg(test)]
mod contribution_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_extension_catalogs_keep_commands_and_views_without_ui() {
        let legacy = json!({"id":"example","commands":[{"name":"inspect","title":"Inspect","mutates":false}],"views":[{"id":"inspect","title":"Inspect","kind":"web","command":"inspect"}]});
        let extension: ExtensionInfo = serde_json::from_value(legacy.clone()).unwrap();
        assert!(extension.ui.is_none());
        assert_eq!(serde_json::to_value(extension).unwrap(), legacy);
    }

    #[test]
    fn contribution_manifest_round_trips_named_slots_and_separate_surfaces() {
        let manifest: UiManifest = serde_json::from_value(json!({
            "version":1,
            "commands":[{"id":"inspect","title":"Inspect","runtimeCommand":"inspect"}],
            "slots":[{"id":"inspect","slot":"composer.actions","kind":"action","title":"Inspect","command":"inspect","enabledWhen":["composerReady"]}],
            "screens":[{"id":"screen","title":"Screen","command":"inspect"}],
            "panels":[{"id":"panel","title":"Panel","command":"inspect"}]
        })).unwrap();
        let encoded = serde_json::to_value(manifest).unwrap();
        assert_eq!(encoded["slots"][0]["slot"], "composer.actions");
        assert_eq!(encoded["slots"][0]["enabledWhen"][0], "composerReady");
        assert_eq!(encoded["screens"][0]["id"], "screen");
        assert_eq!(encoded["panels"][0]["id"], "panel");
        assert!(
            serde_json::from_value::<UiManifest>(json!({"version":1,"html":"unrestricted"}))
                .is_err()
        );
    }
}
