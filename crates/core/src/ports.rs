use crate::*;
use async_trait::async_trait;
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::{RwLock, mpsc};

pub struct BackendConnection {
    pub backend: Box<dyn AgentBackend>,
    pub native: NativeSessionRef,
    pub capabilities: Capabilities,
    pub events: mpsc::Receiver<BackendEvent>,
}
#[async_trait]
pub trait Harness: Send + Sync {
    fn id(&self) -> &str;
    async fn detect(&self) -> Result<Detection>;
    /// A probe must not create or mutate a conversational session.
    async fn probe(&self) -> Result<HarnessProbe> {
        let detection = self.detect().await?;
        Ok(HarnessProbe {
            harness: detection.harness,
            available: detection.available,
            executable: detection.executable,
            version: None,
            auth_state: "unknown".into(),
            message: None,
            capabilities: detection.capabilities,
        })
    }
    /// Read account quotas without creating a conversation, turn, or runtime session.
    async fn account_quotas(&self, _query: &AccountQuery) -> Result<Value> {
        Err(Error::Unsupported("standalone account quotas".into()))
    }
    /// Compile a launch intent without creating native execution.
    fn compile_execution_profile(
        &self,
        _options: &SpawnOptions,
        _profile: ExecutionProfile,
    ) -> Result<ExecutionPolicyPlan> {
        Err(Error::Unsupported(
            "common execution profile for this harness".into(),
        ))
    }
    async fn connect(
        &self,
        options: &SpawnOptions,
        native: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection>;
    async fn native_history(
        &self,
        _query: &NativeHistoryQuery,
    ) -> Result<Vec<NativeHistorySession>> {
        Err(Error::Unsupported("native history discovery".into()))
    }
    async fn read_native_history(
        &self,
        _query: &NativeHistoryQuery,
        _native_id: &str,
    ) -> Result<NativeHistoryDocument> {
        Err(Error::Unsupported("native history import".into()))
    }
    async fn fork_before_user(
        &self,
        _options: &SpawnOptions,
        _native: &NativeSessionRef,
        _message_id: &str,
    ) -> Result<BackendConnection> {
        Err(Error::Unsupported("fork before user message".into()))
    }
    async fn fork_at(
        &self,
        _options: &SpawnOptions,
        _native: &NativeSessionRef,
        _message_id: &str,
    ) -> Result<BackendConnection> {
        Err(Error::Unsupported("fork at message".into()))
    }
}
#[async_trait]
pub trait AgentBackend: Send + Sync {
    /// Return when accepted, never wait for the model to finish.
    /// Only Error::Rejected certifies no native effects. Other errors retain uncertainty.
    async fn prompt(&self, prompt: &Prompt) -> Result<()>;
    /// The runtime commits this opaque identity before the native call.
    async fn submit(&self, _submission_id: &str, prompt: &Prompt) -> Result<()> {
        self.prompt(prompt).await
    }
    /// Read correlated native facts without submitting or repeating work.
    async fn reconcile_submission(
        &self,
        _submission_id: &str,
    ) -> Result<Option<NativeSubmissionEvidence>> {
        Err(Error::Unsupported(
            "qualified native submission reconciliation".into(),
        ))
    }
    async fn steer(&self, _prompt: &Prompt) -> Result<()> {
        Err(Error::Unsupported("steering".into()))
    }
    async fn cancel(&self) -> Result<()>;
    async fn respond_permission(&self, response: &PermissionResponse) -> Result<()>;
    /// Flat scalar settings in a successful receipt are authoritative, including native resets.
    /// Descriptor arrays or nested provider receipts remain available to the caller.
    async fn configure(&self, _config: Value) -> Result<Value> {
        Err(Error::Unsupported("configure".into()))
    }
    /// Report declared choices separately from acknowledged or observed native state.
    async fn execution_policy(&self) -> Result<ExecutionPolicyReport> {
        Ok(ExecutionPolicyReport::default())
    }
    async fn compile_execution_profile(
        &self,
        _profile: ExecutionProfile,
    ) -> Result<ExecutionPolicyPlan> {
        Err(Error::Unsupported(
            "common execution profile for this backend".into(),
        ))
    }
    async fn configuration(&self) -> Result<Vec<SettingDescriptor>> {
        Err(Error::Unsupported("configuration descriptors".into()))
    }
    async fn models(&self) -> Result<Vec<ModelDescriptor>> {
        Err(Error::Unsupported("models".into()))
    }
    async fn commands(&self) -> Result<Value> {
        Err(Error::Unsupported("slash commands".into()))
    }
    async fn quotas(&self) -> Result<Value> {
        Err(Error::Unsupported("account quotas".into()))
    }
    async fn rollback(&self, _turns: u64) -> Result<()> {
        Err(Error::Unsupported("rollback".into()))
    }
    async fn rollback_with(&self, turns: u64, scope: RollbackScope) -> Result<()> {
        match scope {
            RollbackScope::ConversationOnly => self.rollback(turns).await,
            RollbackScope::ConversationAndFiles => {
                Err(Error::Unsupported("filesystem rollback".into()))
            }
        }
    }
    async fn compact(&self) -> Result<()> {
        Err(Error::Unsupported("compact".into()))
    }
    async fn close(&self) -> Result<()>;
    async fn kill(&self) -> Result<()>;
}
/// One atomic host mutation. No native call may run inside this transaction.
#[derive(Default)]
pub struct SessionWrite<'a> {
    pub snapshot: Option<&'a SessionSnapshot>,
    pub submission: Option<&'a SubmissionRecord>,
    pub events: &'a [RuntimeEvent],
}
impl SessionWrite<'_> {
    pub fn session_id(&self) -> Result<&str> {
        let id = self
            .snapshot
            .map(|s| s.id.as_str())
            .or_else(|| self.submission.map(|s| s.receipt.session_id.as_str()))
            .or_else(|| self.events.first().map(|e| e.session_id.as_str()))
            .filter(|id| !id.is_empty())
            .ok_or_else(|| Error::Invalid("empty storage transaction".into()))?;
        if self.submission.is_some_and(|s| s.receipt.session_id != id)
            || self.events.iter().any(|e| e.session_id != id)
            || self
                .events
                .windows(2)
                .any(|pair| pair[0].sequence >= pair[1].sequence)
            || self.events.first().is_some_and(|e| e.sequence == 0)
            || self
                .snapshot
                .is_some_and(|s| self.events.last().is_some_and(|e| s.sequence < e.sequence))
        {
            return Err(Error::Storage("inconsistent session transaction".into()));
        }
        Ok(id)
    }
}
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// None declares snapshot-only storage, not an empty complete transcript.
    fn transcript_durability(&self) -> Option<&'static str> {
        None
    }
    /// Commit all supplied records or leave the store unchanged.
    async fn commit(&self, write: SessionWrite<'_>) -> Result<()>;
    async fn local_catalog(&self) -> Result<LocalCatalog>;
    async fn update_catalog(&self, update: CatalogUpdate) -> Result<CatalogRecord>;
    async fn prepare_context(
        &self,
        session_id: &str,
        binding_id: &str,
        selection: ContextSelection,
    ) -> Result<WorkingContext>;
    async fn working_context(&self, id: &str) -> Result<Option<WorkingContext>>;
    async fn extension_state(&self, _id: &str) -> Result<Option<Value>> {
        Err(Error::Unsupported("host extension storage".into()))
    }
    async fn save_extension_state(&self, _id: &str, _value: Value) -> Result<()> {
        Err(Error::Unsupported("host extension storage".into()))
    }
    async fn save_submission(&self, record: &SubmissionRecord) -> Result<()> {
        self.commit(SessionWrite {
            submission: Some(record),
            ..Default::default()
        })
        .await
    }
    async fn submissions(&self, _session_id: &str) -> Result<Vec<SubmissionRecord>> {
        Ok(vec![])
    }
    async fn save(&self, snapshot: &SessionSnapshot) -> Result<()> {
        self.commit(SessionWrite {
            snapshot: Some(snapshot),
            ..Default::default()
        })
        .await
    }
    async fn load(&self, id: &str) -> Result<Option<SessionSnapshot>>;
    async fn list(&self) -> Result<Vec<SessionSnapshot>>;
    async fn append_event(&self, event: &RuntimeEvent) -> Result<()> {
        self.commit(SessionWrite {
            events: std::slice::from_ref(event),
            ..Default::default()
        })
        .await
    }
    async fn events(&self, _id: &str, _after: u64, _limit: usize) -> Result<Vec<RuntimeEvent>> {
        Err(Error::Unsupported("transcript persistence".into()))
    }
}
#[derive(Default)]
struct MemoryState {
    catalog: LocalCatalog,
    contexts: BTreeMap<String, WorkingContext>,
    extension_state: BTreeMap<String, Value>,
    submissions: BTreeMap<(String, String), SubmissionRecord>,
    snapshots: BTreeMap<String, SessionSnapshot>,
    events: BTreeMap<String, Vec<RuntimeEvent>>,
}
#[derive(Default)]
pub struct MemoryStore {
    state: RwLock<MemoryState>,
}
#[async_trait]
impl SessionStore for MemoryStore {
    async fn local_catalog(&self) -> Result<LocalCatalog> {
        Ok(self.state.read().await.catalog.clone())
    }
    async fn update_catalog(&self, update: CatalogUpdate) -> Result<CatalogRecord> {
        self.state.write().await.catalog.apply(update)
    }
    async fn prepare_context(
        &self,
        session_id: &str,
        binding_id: &str,
        selection: ContextSelection,
    ) -> Result<WorkingContext> {
        let mut state = self.state.write().await;
        if state
            .contexts
            .values()
            .any(|c| c.session_id == session_id || c.binding_id == binding_id)
        {
            return Err(Error::Storage("working context already prepared".into()));
        }
        let context = if let ContextSelection::Inherited { context_id } = &selection {
            let parent = state
                .contexts
                .get(context_id)
                .ok_or_else(|| Error::Storage("parent context not found".into()))?;
            WorkingContext {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: session_id.into(),
                binding_id: binding_id.into(),
                ..parent.clone()
            }
        } else {
            state
                .catalog
                .prepare(session_id.into(), binding_id.into(), &selection)?
        };
        state.contexts.insert(context.id.clone(), context.clone());
        Ok(context)
    }
    async fn working_context(&self, id: &str) -> Result<Option<WorkingContext>> {
        Ok(self.state.read().await.contexts.get(id).cloned())
    }
    async fn extension_state(&self, id: &str) -> Result<Option<Value>> {
        Ok(self.state.read().await.extension_state.get(id).cloned())
    }
    async fn save_extension_state(&self, id: &str, value: Value) -> Result<()> {
        self.state
            .write()
            .await
            .extension_state
            .insert(id.into(), value);
        Ok(())
    }
    fn transcript_durability(&self) -> Option<&'static str> {
        Some("memory")
    }
    async fn commit(&self, write: SessionWrite<'_>) -> Result<()> {
        let id = write.session_id()?;
        let mut state = self.state.write().await;
        if let Some(snapshot) = write.snapshot {
            if state.contexts.values().find(|c| c.session_id == id)
                != snapshot.working_context.as_ref()
            {
                return Err(Error::Storage(
                    "snapshot differs from its retained working context".into(),
                ));
            }
            if let Some(context) = &snapshot.working_context
                && (state.contexts.get(&context.id) != Some(context)
                    || context.session_id != id
                    || snapshot.options.cwd != context.directory.path)
            {
                return Err(Error::Storage(
                    "snapshot differs from its retained working context".into(),
                ));
            }
        }
        if let Some(first) = write.events.first()
            && state
                .events
                .get(id)
                .and_then(|events| events.last())
                .is_some_and(|last| last.sequence >= first.sequence)
        {
            return Err(Error::Storage("transcript sequences must increase".into()));
        }
        if let Some(record) = write.submission
            && let Some(existing) = state
                .submissions
                .get(&(id.into(), record.receipt.idempotency_key.clone()))
            && (existing.receipt.id != record.receipt.id
                || serde_json::to_value(&existing.authored)?
                    != serde_json::to_value(&record.authored)?
                || serde_json::to_value(&existing.projected)?
                    != serde_json::to_value(&record.projected)?)
        {
            return Err(Error::Storage(
                "submission identity and input are immutable".into(),
            ));
        }
        if let Some(snapshot) = write.snapshot {
            state.snapshots.insert(id.into(), snapshot.clone());
        }
        if let Some(record) = write.submission {
            state.submissions.insert(
                (id.into(), record.receipt.idempotency_key.clone()),
                record.clone(),
            );
        }
        state
            .events
            .entry(id.into())
            .or_default()
            .extend_from_slice(write.events);
        Ok(())
    }
    async fn submissions(&self, id: &str) -> Result<Vec<SubmissionRecord>> {
        Ok(self
            .state
            .read()
            .await
            .submissions
            .values()
            .filter(|r| r.receipt.session_id == id)
            .cloned()
            .collect())
    }
    async fn load(&self, id: &str) -> Result<Option<SessionSnapshot>> {
        Ok(self.state.read().await.snapshots.get(id).cloned())
    }
    async fn list(&self) -> Result<Vec<SessionSnapshot>> {
        Ok(self
            .state
            .read()
            .await
            .snapshots
            .values()
            .cloned()
            .collect())
    }
    async fn events(&self, id: &str, after: u64, limit: usize) -> Result<Vec<RuntimeEvent>> {
        Ok(self
            .state
            .read()
            .await
            .events
            .get(id)
            .into_iter()
            .flatten()
            .filter(|e| e.sequence > after)
            .take(limit)
            .cloned()
            .collect())
    }
}
/// Hooks use the same public types as external harnesses. No built-in privilege.
#[async_trait]
pub trait RuntimeExtension: Send + Sync {
    fn id(&self) -> &str;
    /// Extension commands may access public runtime operations without retaining the runtime.
    fn initialize(&self, _runtime: WeakRuntime) {}
    /// Stop extension-owned background work before provider sessions are closed.
    async fn shutdown(&self) {}
    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![]
    }
    /// Agent-callable tools use their own schemas and cancellable execution contract.
    fn tools(&self) -> Vec<HostTool> {
        vec![]
    }
    async fn execute_tool(
        &self,
        tool: &str,
        _arguments: Value,
        _context: &HostToolContext,
    ) -> Result<Value> {
        Err(Error::Unsupported(format!("host tool {tool}")))
    }
    /// Optional versioned client presentation, independent of harness capabilities.
    fn ui(&self) -> Option<UiManifest> {
        None
    }
    fn views(&self) -> Vec<ExtensionView> {
        vec![]
    }
    /// Receive the retained host context when arguments select an existing session.
    /// History commands can remain available even when the directory is missing.
    async fn before_invoke(
        &self,
        _command: &str,
        _params: &Value,
        _context: Option<&ContextStatus>,
    ) -> Result<()> {
        Ok(())
    }
    /// Commands execute outside the session actor's ordered hook pipeline.
    async fn invoke(&self, command: &str, _params: Value) -> Result<Value> {
        Err(Error::Unsupported(format!("extension command {command}")))
    }
    async fn before_spawn(&self, _options: &mut SpawnOptions) -> Result<()> {
        Ok(())
    }
    async fn after_spawn(&self, _snapshot: &SessionSnapshot) {}
    async fn before_prompt(&self, _session: &SessionSnapshot, _prompt: &mut Prompt) -> Result<()> {
        Ok(())
    }
    async fn on_event(&self, _event: &RuntimeEvent) {}
    /// Declare the single host authority for offered permission requests.
    /// Other extensions observe requests through on_event.
    fn permission_authority(&self) -> bool {
        false
    }
    /// Only the declared authority is called; None or an error leaves the request open.
    async fn on_permission(
        &self,
        _session: &SessionSnapshot,
        _request: &PermissionRequest,
    ) -> Result<Option<PermissionResponse>> {
        Ok(None)
    }
    async fn on_session_closed(&self, _session: &SessionSnapshot) {}
}
pub type SharedStore = Arc<dyn SessionStore>;
