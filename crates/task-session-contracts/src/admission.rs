use swarmie_task_contracts::TaskSpec;
use tembra::prelude::*;

/// Identity is scoped to the durable source database, never a mutable provider name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct BindingKey {
    pub task_store_id: String,
    pub task_id: String,
}

/// Complete immutable destination. All four identifiers are required, even for one conversation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct SessionDestination {
    pub session_store_id: String,
    pub session_id: String,
    pub conversation_id: String,
    pub turn_id: String,
}

/// Root intent, committed before any attempt to call DurableTasks.create.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PrepareBinding {
    pub task_store_id: String,
    pub task: TaskSpec,
    pub destination: SessionDestination,
    pub routing_version: u32,
}

/// Child intent precedes the owning task's Wait transition. None inherits the conversation;
/// Some explicitly freezes a fork. Store, session, turn and root are always inherited.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PrepareChildBinding {
    pub parent: BindingKey,
    pub task: TaskSpec,
    pub conversation_id: Option<String>,
}

/// Pending is a committed complete intent; active additionally confirms a matching task commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum BindingAdmission {
    Pending,
    Active,
}

/// Historical routing identity and original submission are immutable; only admission advances.
/// The retained exact TaskSpec permits recovery from an uncertain create response.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct TaskSessionBinding {
    pub key: BindingKey,
    pub destination: SessionDestination,
    pub root_task_id: String,
    pub parent_task_id: Option<String>,
    pub routing_version: u32,
    pub task: TaskSpec,
    pub admission: BindingAdmission,
}

/// Explicit unrelated-task policy. It never replaces a route or treats missing data as excluded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ExcludedBinding {
    pub key: BindingKey,
    pub reason: String,
}

/// A durable disposition is either a route or an explicit exclusion, never an implicit skip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "tembra::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "tembra::schemars", transform = tagged_union)]
pub enum BindingDisposition {
    Routed { binding: TaskSessionBinding },
    Excluded { exclusion: ExcludedBinding },
}

fn tagged_union(schema: &mut tembra::schemars::Schema) {
    if let Some(object) = schema.as_object_mut()
        && let Some(branches) = object.remove("oneOf")
    {
        object.insert("anyOf".into(), branches);
    }
}

/// Trusted application admission authority, consumed directly through a selected Reference.
/// No hook can substitute for this boundary. Failed/aborted calls may have persisted intent
/// or created a task; retry the same immutable identity and reconcile instead of inventing IDs.
#[tembra::contract(local, name = "swarmie.task-session-bindings", version = 1)]
pub trait TaskSessionBindings {
    /// Persist an exact root intent; identical retries are idempotent and conflicts change nothing.
    #[portable]
    async fn prepare(&self, request: PrepareBinding) -> Result<TaskSessionBinding>;

    /// Persist inherited routing before child submission by its task owner.
    #[portable]
    async fn prepare_child(&self, request: PrepareChildBinding) -> Result<TaskSessionBinding>;

    /// Create/reconcile a root using the retained spec. Never creates a child as a root.
    /// Errors retain the intent, including definite source capacity refusal.
    #[portable]
    async fn admit(&self, key: BindingKey) -> Result<TaskSessionBinding>;

    /// Check both current stores and the authoritative task, then durably confirm association.
    /// Can activate a pending intent before admit returns; never creates absent work. Call at
    /// every recovered phase and immediately before an effect, after middleware/policy awaits.
    /// Compare the returned destination with persisted application identity. Definition changes,
    /// missing tasks/routes, exclusion, mismatches and unavailable providers fail closed.
    #[portable]
    async fn require_confirmed_binding(&self, key: BindingKey) -> Result<TaskSessionBinding>;

    /// Read a historical disposition without asserting current provider identity or admission.
    #[portable]
    async fn lookup(&self, key: BindingKey) -> Result<Option<BindingDisposition>>;

    /// Explicit immutable exclusion for unrelated work; refuses replacement of any routed intent.
    #[portable]
    async fn exclude(&self, request: ExcludedBinding) -> Result<ExcludedBinding>;
}
