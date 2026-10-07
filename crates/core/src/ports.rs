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
    async fn prompt(&self, prompt: &Prompt) -> Result<()>;
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
    async fn configuration(&self) -> Result<Value> {
        Err(Error::Unsupported("configuration descriptors".into()))
    }
    async fn models(&self) -> Result<Value> {
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
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// None explicitly declares a snapshot-only store. Clients must not interpret missing
    /// transcript storage as an empty but complete conversation.
    fn transcript_durability(&self) -> Option<&'static str> {
        None
    }
    async fn save(&self, snapshot: &SessionSnapshot) -> Result<()>;
    async fn load(&self, id: &str) -> Result<Option<SessionSnapshot>>;
    async fn list(&self) -> Result<Vec<SessionSnapshot>>;
    /// Stores that retain only native references may leave transcript persistence disabled.
    async fn append_event(&self, _event: &RuntimeEvent) -> Result<()> {
        Err(Error::Unsupported("transcript persistence".into()))
    }
    async fn events(&self, _id: &str, _after: u64, _limit: usize) -> Result<Vec<RuntimeEvent>> {
        Err(Error::Unsupported("transcript persistence".into()))
    }
}
#[derive(Default)]
pub struct MemoryStore {
    snapshots: RwLock<BTreeMap<String, SessionSnapshot>>,
    events: RwLock<BTreeMap<String, Vec<RuntimeEvent>>>,
}
#[async_trait]
impl SessionStore for MemoryStore {
    fn transcript_durability(&self) -> Option<&'static str> {
        Some("memory")
    }
    async fn save(&self, s: &SessionSnapshot) -> Result<()> {
        self.snapshots.write().await.insert(s.id.clone(), s.clone());
        Ok(())
    }
    async fn load(&self, id: &str) -> Result<Option<SessionSnapshot>> {
        Ok(self.snapshots.read().await.get(id).cloned())
    }
    async fn list(&self) -> Result<Vec<SessionSnapshot>> {
        Ok(self.snapshots.read().await.values().cloned().collect())
    }
    async fn append_event(&self, event: &RuntimeEvent) -> Result<()> {
        self.events
            .write()
            .await
            .entry(event.session_id.clone())
            .or_default()
            .push(event.clone());
        Ok(())
    }
    async fn events(&self, id: &str, after: u64, limit: usize) -> Result<Vec<RuntimeEvent>> {
        Ok(self
            .events
            .read()
            .await
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
    fn views(&self) -> Vec<ExtensionView> {
        vec![]
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
    /// Some(response) applies a policy; None leaves the request to the client.
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
