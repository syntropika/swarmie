use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Phase {
    Idle,
    Working,
    Interrupted,
    Stopped,
    Completed,
    Failed,
    Unknown,
}

/// Immutable execution binding. Identity is a canonical root/Git-directory tuple.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ExecutionContext {
    pub version: u32,
    pub node_id: String,
    pub principal: String,
    pub path: String,
    pub owner: String,
    pub identity: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Draft {
    pub text: String,
    pub attachments: Vec<String>,
    pub account: Option<String>,
    pub model: String,
    pub effort: String,
    pub node: String,
    #[serde(default)]
    pub execution_context: Option<ExecutionContext>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Submission {
    pub conversation_id: String,
    pub submission_id: String,
    pub create_conversation: bool,
    pub draft: Draft,
    pub mode: String,
    #[serde(default)]
    pub execution_context: Option<ExecutionContext>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Message {
    pub id: String,
    pub role: String,
    pub text: String,
    pub truncated: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Snapshot {
    pub conversation_id: String,
    pub messages: Vec<Message>,
    pub phase: Phase,
    pub label: String,
    pub admitted: Vec<String>,
    pub omitted_messages: usize,
    pub can_acknowledge: bool,
    pub execution_context: Option<ExecutionContext>,
    pub lifecycle_warning: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Receipt {
    pub id: String,
    pub digest: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Metadata {
    pub id: String,
    pub title: String,
    pub principal: String,
    pub node: String,
    pub account: String,
    pub model: String,
    pub effort: String,
    pub archived: bool,
    pub pinned: bool,
    pub receipts: Vec<Receipt>,
    pub phase: Phase,
    #[serde(default)]
    pub execution_context: Option<ExecutionContext>,
    #[serde(default)]
    pub checkpoint: Option<swarmie_agent_loop::conversation::Checkpoint>,
    #[serde(default)]
    pub task: Option<swarmie_durable_tasks::TaskId>,
    #[serde(default)]
    pub stop_requested: bool,
    #[serde(default)]
    pub lifecycle_warning: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub(super) enum RunResult {
    Settled,
    Refused { reason: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CatalogPage {
    pub entries: Vec<CatalogEntry>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CatalogEntry {
    pub id: String,
    pub title: String,
    pub account: String,
    pub model: String,
    pub effort: String,
    pub archived: bool,
    pub pinned: bool,
    pub execution_context: Option<ExecutionContext>,
    pub lifecycle_warning: Option<String>,
}

impl From<Metadata> for CatalogEntry {
    fn from(value: Metadata) -> Self {
        Self {
            id: value.id,
            title: value.title,
            account: value.account,
            model: value.model,
            effort: value.effort,
            archived: value.archived,
            pinned: value.pinned,
            execution_context: value.execution_context,
            lifecycle_warning: value
                .lifecycle_warning
                .map(super::checkout::bounded_warning),
        }
    }
}
