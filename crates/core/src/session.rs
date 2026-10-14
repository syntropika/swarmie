use crate::*;
use futures_util::Stream;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{RwLock, broadcast, mpsc, oneshot};

pub type EventStream = std::pin::Pin<Box<dyn Stream<Item = Result<RuntimeEvent>> + Send>>;
const HISTORY_LIMIT: usize = 4096;
#[path = "session_policy.rs"]
mod policy;
type Reply<T> = oneshot::Sender<Result<T>>;
pub(crate) enum Command {
    Prompt(String, Prompt, Reply<SubmissionReceipt>),
    Submission(String, Reply<Option<SubmissionReceipt>>),
    ReconcileSubmission(String, Reply<Option<SubmissionReceipt>>),
    Steer(Prompt, Reply<()>),
    Cancel(Option<String>, Reply<()>),
    QueueAdd(Prompt, Reply<QueuedPrompt>),
    QueueUpdate(String, Prompt, Reply<()>),
    QueueRemove(String, Reply<()>),
    QueuePause(bool, Reply<()>),
    Metadata(SessionMetadataUpdate, Reply<SessionSnapshot>),
    Permission(PermissionResponse, Reply<()>),
    Configure(Value, Reply<ConfigurationReceipt>),
    Configuration(Reply<Vec<SettingDescriptor>>),
    SlashCommands(Reply<Value>),
    Tasks(Reply<Vec<AgentTask>>),
    StopTask(String, Reply<()>),
    Quotas(Reply<Value>),
    Models(Reply<Vec<ModelDescriptor>>),
    Rollback(u64, RollbackScope, Option<u64>, Reply<()>),
    Compact(Reply<()>),
    Close(bool, Reply<()>),
}
pub(crate) struct AgentInner {
    pub commands: mpsc::UnboundedSender<Command>,
    pub events: broadcast::Sender<RuntimeEvent>,
    pub snapshot: Arc<RwLock<SessionSnapshot>>,
    pub history: Arc<RwLock<VecDeque<RuntimeEvent>>>,
    pub closed: Arc<AtomicBool>,
    // Live launch credentials never enter snapshots, public events, or journals.
    pub execution: Arc<std::sync::Mutex<Option<SessionExecutionOptions>>>,
}
impl Drop for AgentInner {
    fn drop(&mut self) {
        let (tx, _) = oneshot::channel();
        let _ = self.commands.send(Command::Close(true, tx));
    }
}
#[derive(Clone)]
pub struct Agent(pub(crate) Arc<AgentInner>);
impl Agent {
    /// The live owner's home override, without exposing launch credentials or other environment values.
    pub fn execution_home(&self) -> Option<std::path::PathBuf> {
        let execution = self.0.execution.lock().unwrap();
        let execution = execution.as_ref()?;
        execution
            .env
            .get("HOME")
            .or_else(|| execution.env.get("USERPROFILE"))
            .map(std::path::PathBuf::from)
    }

    pub async fn snapshot(&self) -> SessionSnapshot {
        self.0.snapshot.read().await.clone()
    }
    pub async fn capabilities(&self) -> Capabilities {
        self.0.snapshot.read().await.capabilities.clone()
    }
    pub fn events(&self) -> EventStream {
        let receiver = self.0.events.subscribe();
        let closed = self.0.closed.load(Ordering::SeqCst);
        Box::pin(futures_util::stream::unfold(
            (receiver, closed),
            |(mut receiver, done)| async move {
                if done {
                    return None;
                }
                match receiver.recv().await {
                    Ok(event) => {
                        let done = matches!(event.kind, EventKind::SessionEnded);
                        Some((Ok(event), (receiver, done)))
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        Some((Err(Error::Lagged(n)), (receiver, false)))
                    }
                    Err(broadcast::error::RecvError::Closed) => None,
                }
            },
        ))
    }
    /// Replay retained live events. A gap is an error, never silent truncation.
    pub async fn replay(&self, after: u64) -> Result<Vec<RuntimeEvent>> {
        let h = self.0.history.read().await;
        if h.front()
            .is_some_and(|e| after.saturating_add(1) < e.sequence)
        {
            return Err(Error::Invalid(
                "requested events are no longer retained".into(),
            ));
        }
        Ok(h.iter().filter(|e| e.sequence > after).cloned().collect())
    }
    async fn request<T>(&self, make: impl FnOnce(Reply<T>) -> Command) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.0.commands.send(make(tx)).map_err(|_| Error::Closed)?;
        rx.await.map_err(|_| Error::Closed)?
    }
    pub async fn prompt(&self, prompt: impl Into<Prompt>) -> Result<()> {
        let receipt = self
            .submit(uuid::Uuid::new_v4().to_string(), prompt)
            .await?;
        if receipt.delivery == DeliveryKnowledge::Accepted {
            if receipt.error.as_deref() == Some("session closed after native acceptance") {
                Err(Error::Closed)
            } else {
                Ok(())
            }
        } else if receipt.delivery == DeliveryKnowledge::Rejected {
            Err(Error::Rejected(receipt.error.unwrap_or_else(|| {
                "native input rejected without effects".into()
            })))
        } else if self.0.closed.load(Ordering::SeqCst) {
            Err(Error::Closed)
        } else {
            Err(Error::Provider(receipt.error.unwrap_or_else(|| {
                "submission delivery is uncertain".into()
            })))
        }
    }
    pub async fn submit(
        &self,
        key: impl Into<String>,
        prompt: impl Into<Prompt>,
    ) -> Result<SubmissionReceipt> {
        let key = key.into();
        crate::submission::validate_client_submission_key(&key)?;
        self.request(|r| Command::Prompt(key, prompt.into(), r))
            .await
    }
    pub async fn submission(&self, key: impl Into<String>) -> Result<Option<SubmissionReceipt>> {
        self.request(|r| Command::Submission(key.into(), r)).await
    }
    pub async fn reconcile_submission(
        &self,
        key: impl Into<String>,
    ) -> Result<Option<SubmissionReceipt>> {
        let key = key.into();
        crate::submission::validate_submission_key(&key)?;
        self.request(|r| Command::ReconcileSubmission(key, r)).await
    }
    pub async fn steer(&self, prompt: impl Into<Prompt>) -> Result<()> {
        self.request(|r| Command::Steer(prompt.into(), r)).await
    }
    pub async fn cancel(&self) -> Result<()> {
        self.request(|r| Command::Cancel(None, r)).await
    }
    pub async fn cancel_turn(&self, turn: impl Into<String>) -> Result<()> {
        self.request(|r| Command::Cancel(Some(turn.into()), r))
            .await
    }
    pub async fn enqueue(&self, prompt: impl Into<Prompt>) -> Result<QueuedPrompt> {
        self.request(|r| Command::QueueAdd(prompt.into(), r)).await
    }
    pub async fn update_queued(
        &self,
        id: impl Into<String>,
        prompt: impl Into<Prompt>,
    ) -> Result<()> {
        self.request(|r| Command::QueueUpdate(id.into(), prompt.into(), r))
            .await
    }
    pub async fn remove_queued(&self, id: impl Into<String>) -> Result<()> {
        self.request(|r| Command::QueueRemove(id.into(), r)).await
    }
    pub async fn pause_queue(&self, paused: bool) -> Result<()> {
        self.request(|r| Command::QueuePause(paused, r)).await
    }
    pub async fn update_metadata(&self, update: SessionMetadataUpdate) -> Result<SessionSnapshot> {
        self.request(|r| Command::Metadata(update, r)).await
    }
    pub async fn respond_permission(&self, response: PermissionResponse) -> Result<()> {
        self.request(|r| Command::Permission(response, r)).await
    }
    pub async fn configure(&self, config: Value) -> Result<ConfigurationReceipt> {
        self.request(|r| Command::Configure(config, r)).await
    }
    pub async fn models(&self) -> Result<Vec<ModelDescriptor>> {
        self.request(Command::Models).await
    }
    pub async fn configuration(&self) -> Result<Vec<SettingDescriptor>> {
        self.request(Command::Configuration).await
    }
    pub async fn slash_commands(&self) -> Result<Value> {
        self.request(Command::SlashCommands).await
    }
    pub async fn tasks(&self) -> Result<Vec<AgentTask>> {
        self.request(Command::Tasks).await
    }
    pub async fn stop_task(&self, task_id: &str) -> Result<()> {
        if task_id.is_empty() || task_id.len() > 4096 || task_id.chars().any(char::is_control) {
            return Err(Error::Invalid("invalid background task identifier".into()));
        }
        self.request(|reply| Command::StopTask(task_id.into(), reply))
            .await
    }
    pub async fn quotas(&self) -> Result<Value> {
        self.request(Command::Quotas).await
    }
    pub async fn rollback(&self, turns: u64) -> Result<()> {
        self.rollback_with(turns, RollbackScope::ConversationOnly)
            .await
    }
    pub async fn rollback_with(&self, turns: u64, scope: RollbackScope) -> Result<()> {
        self.rollback_at(turns, scope, None).await
    }
    /// Apply a rollback only to the history used to prepare it.
    pub async fn rollback_at(
        &self,
        turns: u64,
        scope: RollbackScope,
        sequence: Option<u64>,
    ) -> Result<()> {
        self.request(|reply| Command::Rollback(turns, scope, sequence, reply))
            .await
    }
    pub async fn compact(&self) -> Result<()> {
        self.request(Command::Compact).await
    }
    pub async fn close(&self) -> Result<()> {
        self.request(|r| Command::Close(false, r)).await
    }
    pub async fn kill(&self) -> Result<()> {
        self.request(|r| Command::Close(true, r)).await
    }
}
pub(crate) struct Engine {
    pub host_tools: Option<Arc<crate::tools::HostToolScope>>,
    pub submissions: BTreeMap<String, SubmissionRecord>,
    pub recovered_turn: Option<String>,
    pub inflight_queue: Option<String>,
    pub storage_error: Option<String>,
    pub backend: Arc<dyn AgentBackend>,
    pub input: mpsc::Receiver<BackendEvent>,
    pub commands: mpsc::UnboundedReceiver<Command>,
    pub task_controls: tokio::task::JoinSet<()>,
    pub snapshot: SessionSnapshot,
    pub shared: Arc<RwLock<SessionSnapshot>>,
    pub events: broadcast::Sender<RuntimeEvent>,
    pub global: broadcast::Sender<RuntimeEvent>,
    pub history: Arc<RwLock<VecDeque<RuntimeEvent>>>,
    pub store: SharedStore,
    pub extensions: Vec<Arc<dyn RuntimeExtension>>,
    pub permissions: BTreeMap<String, PermissionRequest>,
    pub closed: Arc<AtomicBool>,
    pub execution: Arc<std::sync::Mutex<Option<SessionExecutionOptions>>>,
}
impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(tools) = &self.host_tools {
            tools.revoke();
        }
        self.execution.lock().unwrap().take();
    }
}
impl Engine {
    async fn stop_task_controls(&mut self) {
        self.task_controls.abort_all();
        while self.task_controls.join_next().await.is_some() {}
    }
    pub(crate) async fn emit(&mut self, kind: EventKind, raw: Option<Value>) {
        if self.storage_error.is_some() {
            return;
        }
        self.snapshot.sequence += 1;
        let event = RuntimeEvent {
            session_id: self.snapshot.id.clone(),
            sequence: self.snapshot.sequence,
            turn_id: self.snapshot.turn.clone(),
            source: self.snapshot.options.harness.clone(),
            timestamp_ms: crate::runtime::timestamp_ms(),
            kind,
            raw,
        };
        let persistence_error = if self.store.transcript_durability().is_some() {
            self.store
                .commit(SessionWrite {
                    snapshot: Some(&self.snapshot),
                    events: std::slice::from_ref(&event),
                    ..Default::default()
                })
                .await
                .err()
        } else {
            None
        };
        if persistence_error.is_none() {
            self.publish(&event).await;
        }
        if let Some(error) = persistence_error {
            // Report storage failure without recursively attempting to journal that error.
            self.snapshot.sequence += 1;
            self.storage_error = Some(error.to_string());
            self.snapshot.state = SessionState::Failed;
            self.closed.store(true, Ordering::SeqCst);
            self.stop_task_controls().await;
            if let Some(tools) = &self.host_tools {
                tools.revoke();
            }
            let diagnostic = RuntimeEvent {
                sequence: self.snapshot.sequence,
                kind: EventKind::Error {
                    message: format!("transcript persistence failed: {error}"),
                },
                raw: None,
                ..event
            };
            let mut history = self.history.write().await;
            if history.len() == HISTORY_LIMIT {
                history.pop_front();
            }
            history.push_back(diagnostic.clone());
            drop(history);
            *self.shared.write().await = self.snapshot.clone();
            let _ = self.events.send(diagnostic.clone());
            let _ = self.global.send(diagnostic.clone());
            for extension in &self.extensions {
                extension.on_event(&diagnostic).await;
            }
            self.snapshot.sequence += 1;
            let ended = RuntimeEvent {
                sequence: self.snapshot.sequence,
                kind: EventKind::SessionEnded,
                ..diagnostic
            };
            let mut history = self.history.write().await;
            if history.len() == HISTORY_LIMIT {
                history.pop_front();
            }
            history.push_back(ended.clone());
            drop(history);
            *self.shared.write().await = self.snapshot.clone();
            let _ = self.events.send(ended.clone());
            let _ = self.global.send(ended.clone());
            for extension in &self.extensions {
                extension.on_event(&ended).await;
            }
        }
    }
    async fn publish(&self, event: &RuntimeEvent) {
        if let Some(tools) = &self.host_tools {
            tools.observe(&self.snapshot);
        }
        *self.shared.write().await = self.snapshot.clone();
        let mut history = self.history.write().await;
        if history.len() == HISTORY_LIMIT {
            history.pop_front();
        }
        history.push_back(event.clone());
        drop(history);
        let _ = self.events.send(event.clone());
        let _ = self.global.send(event.clone());
        for extension in &self.extensions {
            extension.on_event(event).await;
        }
    }
    pub(crate) async fn persist(&mut self) {
        if let Err(e) = self.store.save(&self.snapshot).await {
            self.emit(
                EventKind::Error {
                    message: e.to_string(),
                },
                None,
            )
            .await;
        }
    }
    pub(crate) async fn state(&mut self, state: SessionState) {
        if self.snapshot.state != state {
            self.snapshot.state = state.clone();
            self.emit(EventKind::StateChanged(state), None).await;
            self.persist().await;
        }
    }

    async fn finish(
        &mut self,
        outcome: TurnOutcome,
        native_reason: Option<String>,
        raw: Option<Value>,
    ) {
        if self.snapshot.turn.is_none() {
            if raw.is_some() {
                self.emit(EventKind::Raw, raw).await;
            }
            return;
        }
        let pending = std::mem::take(&mut self.permissions);
        for (id, _) in pending {
            self.emit(
                EventKind::PermissionResolved {
                    id,
                    option: "cancelled".into(),
                },
                None,
            )
            .await;
        }
        if let Some(tools) = &self.host_tools {
            tools.cancel_turn();
        }
        self.snapshot.turns_completed += 1;
        self.emit(
            EventKind::TurnEnded {
                outcome,
                native_reason,
            },
            raw,
        )
        .await;
        self.snapshot.turn = None;
        self.snapshot.metadata.updated_at_ms = crate::runtime::timestamp_ms();
        self.state(SessionState::Idle).await;
    }
    async fn commit_change(
        &mut self,
        mut snapshot: SessionSnapshot,
        kind: EventKind,
    ) -> Result<()> {
        snapshot.sequence += 1;
        let event = RuntimeEvent {
            session_id: snapshot.id.clone(),
            sequence: snapshot.sequence,
            turn_id: snapshot.turn.clone(),
            source: snapshot.options.harness.clone(),
            timestamp_ms: crate::runtime::timestamp_ms(),
            kind,
            raw: None,
        };
        self.store
            .commit(SessionWrite {
                snapshot: Some(&snapshot),
                events: if self.store.transcript_durability().is_some() {
                    std::slice::from_ref(&event)
                } else {
                    &[]
                },
                ..Default::default()
            })
            .await?;
        self.snapshot = snapshot;
        self.publish(&event).await;
        Ok(())
    }
    async fn save_queue(&mut self, queue: PromptQueue) -> Result<()> {
        let mut snapshot = self.snapshot.clone();
        snapshot.queue = queue;
        let kind = EventKind::QueueChanged(snapshot.queue.clone());
        self.commit_change(snapshot, kind).await
    }
    async fn queue_command(&mut self, command: Command) {
        match command {
            Command::QueueAdd(prompt, reply) => {
                let result = async {
                    if prompt.content.is_empty() {
                        return Err(Error::Invalid("prompt content must not be empty".into()));
                    }
                    if self.snapshot.queue.items.len() >= 128 {
                        return Err(Error::Invalid("queue is limited to 128 prompts".into()));
                    }
                    let item = QueuedPrompt {
                        id: uuid::Uuid::new_v4().to_string(),
                        prompt,
                        created_at_ms: crate::runtime::timestamp_ms(),
                    };
                    let mut queue = self.snapshot.queue.clone();
                    queue.items.push(item.clone());
                    self.save_queue(queue).await?;
                    Ok(item)
                }
                .await;
                let _ = reply.send(result);
            }
            Command::QueueUpdate(id, prompt, reply) => {
                let result = async {
                    if self.inflight_queue.as_ref() == Some(&id) {
                        return Err(Error::Busy);
                    }
                    if prompt.content.is_empty() {
                        return Err(Error::Invalid("prompt content must not be empty".into()));
                    }
                    let mut queue = self.snapshot.queue.clone();
                    let item = queue
                        .items
                        .iter_mut()
                        .find(|item| item.id == id)
                        .ok_or_else(|| Error::Invalid("prompt is no longer queued".into()))?;
                    item.prompt = prompt;
                    if self.submissions.contains_key(&format!("queue:{}", id)) {
                        item.id = uuid::Uuid::new_v4().to_string();
                    }
                    self.save_queue(queue).await
                }
                .await;
                let _ = reply.send(result);
            }
            Command::QueueRemove(id, reply) => {
                if self.inflight_queue.as_ref() == Some(&id) {
                    let _ = reply.send(Err(Error::Busy));
                    return;
                }
                let mut queue = self.snapshot.queue.clone();
                let before = queue.items.len();
                queue.items.retain(|item| item.id != id);
                let result = if before == queue.items.len() {
                    Err(Error::Invalid("prompt is no longer queued".into()))
                } else {
                    self.save_queue(queue).await
                };
                let _ = reply.send(result);
            }
            Command::QueuePause(paused, reply) => {
                if !paused && self.dispatch_blocked() {
                    let _ = reply.send(Err(Error::Busy));
                    return;
                }
                let mut queue = self.snapshot.queue.clone();
                queue.paused = paused;
                let _ = reply.send(self.save_queue(queue).await);
            }
            Command::Metadata(update, reply) => {
                let result = async {
                    let mut snapshot = self.snapshot.clone();
                    crate::runtime::apply_metadata(&mut snapshot, update)?;
                    let kind = EventKind::SessionUpdated(json!({"metadata":snapshot.metadata}));
                    self.commit_change(snapshot, kind).await?;
                    Ok(self.snapshot.clone())
                }
                .await;
                let _ = reply.send(result);
            }
            _ => unreachable!("queue and metadata commands only"),
        }
    }
    async fn promote_queue(&mut self) {
        if self.dispatch_blocked()
            || self.snapshot.turn.is_some()
            || !self.permissions.is_empty()
            || self.snapshot.queue.paused
            || self.snapshot.queue.items.is_empty()
        {
            return;
        }
        let item = self.snapshot.queue.items[0].clone();
        self.inflight_queue = Some(item.id.clone());
        let result = self
            .submit_prompt(format!("queue:{}", item.id), item.prompt.clone())
            .await
            .and_then(|receipt| {
                if receipt.delivery == DeliveryKnowledge::Accepted {
                    Ok(())
                } else {
                    Err(Error::Provider(
                        "queued submission delivery is uncertain".into(),
                    ))
                }
            });
        self.inflight_queue = None;
        if self.closed.load(Ordering::SeqCst) {
            self.snapshot.queue.paused = true;
            let _ = self.store.save(&self.snapshot).await;
            return;
        }
        if let Err(error) = result {
            let mut queue = self.snapshot.queue.clone();
            queue.paused = true;
            let _ = self.save_queue(queue).await;
            self.emit(
                EventKind::Error {
                    message: format!("queued prompt delivery did not complete: {error}"),
                },
                None,
            )
            .await;
        } else {
            let mut queue = self.snapshot.queue.clone();
            queue.items.retain(|queued| queued.id != item.id);
            if let Err(error) = self.save_queue(queue).await {
                self.snapshot.queue.paused = true;
                self.emit(
                    EventKind::Error {
                        message: format!(
                            "delivered prompt could not be removed from queue: {error}"
                        ),
                    },
                    None,
                )
                .await;
            }
        }
    }
    async fn answer(&mut self, mut response: PermissionResponse) -> Result<()> {
        let request = self
            .permissions
            .get(&response.id)
            .ok_or_else(|| Error::Invalid("unknown or resolved permission id".into()))?;
        response.normalize_answers(request.kind == "question" && response.option == "answer")?;
        if !request.options.iter().any(|o| o.id == response.option) {
            return Err(Error::Invalid("permission option was not offered".into()));
        }
        if request.kind == "question" && response.option == "answer" {
            validate_question_answers(request, &response.data)?;
        }
        let declined = request.options.iter().any(|o| {
            o.id == response.option
                && matches!(
                    o.kind.as_str(),
                    "deny" | "deny_once" | "deny_session" | "cancel"
                )
        });
        if !declined {
            if self.snapshot.execution_policy.dispatch_blocked {
                return Err(Error::Busy);
            }
            let context = self
                .snapshot
                .working_context
                .as_ref()
                .ok_or_else(|| Error::Storage("missing working context".into()))?;
            verify_context(context).await?;
        }
        let backend = self.backend.clone();
        let submitted = response.clone();
        self.controlled(
            Box::pin(async move { backend.respond_permission(&submitted).await }),
            false,
        )
        .await?;
        self.permissions.remove(&response.id);
        self.emit(
            EventKind::PermissionResolved {
                id: response.id,
                option: response.option,
            },
            None,
        )
        .await;
        self.settle_permission_state().await;
        Ok(())
    }
    async fn settle_permission_state(&mut self) {
        if self.permissions.is_empty()
            && self.snapshot.state != SessionState::Cancelling
            && !self.closed.load(Ordering::SeqCst)
        {
            let state = if self.snapshot.turn.is_some() {
                SessionState::Running
            } else {
                SessionState::Idle
            };
            self.state(state).await;
        }
    }
    async fn clear_permissions(&mut self) {
        for (id, _) in std::mem::take(&mut self.permissions) {
            self.emit(
                EventKind::PermissionResolved {
                    id,
                    option: "cancelled".into(),
                },
                None,
            )
            .await;
        }
    }
    async fn cancel_permissions(&mut self) {
        let pending: Vec<_> = self.permissions.values().cloned().collect();
        for p in pending {
            if let Some(option) = p.options.iter().find(|o| {
                matches!(
                    o.kind.as_str(),
                    "cancel" | "deny" | "deny_once" | "deny_session"
                )
            }) {
                let _ = self
                    .answer(PermissionResponse {
                        answers: Default::default(),
                        id: p.id,
                        option: option.id.clone(),
                        data: Value::Null,
                    })
                    .await;
            }
        }
    }
    async fn handle_event(&mut self, event: BackendEvent) -> bool {
        match event {
            BackendEvent::Content { kind, raw } => {
                let mut kind = *kind;
                if let EventKind::PermissionRequested(request) = &mut kind
                    && let Err(error) = request.normalize_questions()
                {
                    self.emit(
                        EventKind::Error {
                            message: error.to_string(),
                        },
                        None,
                    )
                    .await;
                }
                if let EventKind::PermissionRequested(p) = &kind {
                    self.permissions.insert(p.id.clone(), p.clone());
                    self.state(SessionState::NeedsInput).await;
                    self.emit(kind.clone(), raw).await;
                    if let Some(authority) = self
                        .extensions
                        .iter()
                        .find(|extension| extension.permission_authority())
                        .cloned()
                    {
                        let decision = authority.on_permission(&self.snapshot, p).await;
                        let result = match decision {
                            Ok(Some(response)) => {
                                let declined = p.options.iter().any(|option| {
                                    option.id == response.option
                                        && matches!(
                                            option.kind.as_str(),
                                            "deny" | "deny_once" | "deny_session" | "cancel"
                                        )
                                });
                                if !declined
                                    && matches!(
                                        self.snapshot.execution_policy.requested,
                                        Some(ExecutionProfile::Ask | ExecutionProfile::Review)
                                    )
                                {
                                    Err(Error::Unsupported("The selected execution profile leaves unresolved native requests to the user".into()))
                                } else {
                                    self.answer(response).await
                                }
                            }
                            Ok(None) => Ok(()),
                            Err(error) => Err(error),
                        };
                        if let Err(error) = result {
                            self.emit(
                                EventKind::Error {
                                    message: error.to_string(),
                                },
                                None,
                            )
                            .await;
                        }
                    }
                } else if let EventKind::ConfigurationUpdated(configuration) = &kind {
                    if let Some(object) = configuration.as_object() {
                        let mut saved = self
                            .snapshot
                            .options
                            .config
                            .as_object()
                            .cloned()
                            .unwrap_or_default();
                        saved.extend(object.clone());
                        self.snapshot.options.config = Value::Object(saved);
                        if let Some(model) = configuration["model"].as_str() {
                            self.snapshot.options.model = Some(model.into());
                        }
                    }
                    self.emit(kind, raw).await;
                    if let Err(error) = self.refresh_execution_policy().await {
                        self.snapshot.execution_policy.dispatch_blocked =
                            self.snapshot.execution_policy.requested.is_some();
                        self.snapshot.execution_policy.status = PolicyStatus::Unknown;
                        self.snapshot.execution_policy.reason = Some(error.to_string());
                    }
                    self.persist().await;
                } else if let EventKind::PermissionResolved { id, .. } = &kind {
                    self.permissions.remove(id);
                    self.emit(kind, raw).await;
                    self.settle_permission_state().await;
                } else if !matches!(
                    kind,
                    EventKind::TurnStarted
                        | EventKind::TurnEnded { .. }
                        | EventKind::StateChanged(_)
                        | EventKind::SessionEnded
                        | EventKind::SessionStarted(_)
                ) {
                    self.emit(kind, raw).await;
                }
            }
            BackendEvent::TurnFinished {
                outcome,
                native_reason,
                raw,
            } => {
                self.settle_submission(outcome, native_reason.clone(), raw.clone())
                    .await;
                self.finish(outcome, native_reason, raw).await;
            }
            BackendEvent::NativeSessionChanged(native) => {
                self.snapshot.native = native;
                self.clear_permissions().await;
                self.snapshot.execution_policy.native_session = Some(self.snapshot.native.clone());
                self.snapshot.execution_policy.observed = None;
                self.snapshot.execution_policy.status = PolicyStatus::Unknown;
                self.snapshot.execution_policy.dispatch_blocked =
                    self.snapshot.execution_policy.requested.is_some();
                self.emit(
                    EventKind::SessionUpdated(json!({"native":self.snapshot.native,"executionPolicy":self.snapshot.execution_policy})),
                    None,
                )
                .await;
                self.persist().await;
                if let Some(profile) = self.snapshot.execution_policy.requested
                    && let Err(error) = self
                        .apply_configuration(json!({"executionProfile":profile}))
                        .await
                {
                    self.snapshot.execution_policy.reason = Some(error.to_string());
                    self.emit(
                        EventKind::SessionUpdated(
                            json!({"executionPolicy":self.snapshot.execution_policy}),
                        ),
                        None,
                    )
                    .await;
                    self.persist().await;
                }
            }
            BackendEvent::Exited { message } => {
                self.stop_task_controls().await;
                self.emit(EventKind::Error { message }, None).await;
                self.clear_permissions().await;
                self.finish(TurnOutcome::Failed, None, None).await;
                self.snapshot.queue.paused = true;
                self.state(SessionState::Failed).await;
                self.closed.store(true, Ordering::SeqCst);
                self.emit(EventKind::SessionEnded, None).await;
                self.persist().await;
                return true;
            }
        }
        false
    }
    /// Lifecycle requests preempt slow provider acceptance and configuration calls.
    pub(crate) async fn controlled<T>(
        &mut self,
        mut operation: std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send>>,
        defer_cancel: bool,
    ) -> Result<T> {
        let mut cancel_replies: Vec<Reply<()>> = vec![];
        let cancel_deadline = tokio::time::sleep(std::time::Duration::from_secs(5));
        tokio::pin!(cancel_deadline);
        loop {
            tokio::select! {
                result = &mut operation => {
                    if !cancel_replies.is_empty() {
                        let backend = self.backend.clone();
                        let cancellation = if result.is_ok() { Box::pin(self.controlled(Box::pin(async move { backend.cancel().await }),false)).await } else { Ok(()) };
                        for reply in cancel_replies {
                            let value = match &cancellation { Ok(()) => Ok(()), Err(Error::Closed) => Err(Error::Closed), Err(e) => Err(Error::Provider(e.to_string())) };
                            let _ = reply.send(value);
                        }
                    }
                    // Nested cancellation can close the session after native acceptance.
                    // Never allow an accepted prompt to reopen that terminal session.
                    if self.closed.load(Ordering::SeqCst) && !defer_cancel { return Err(Error::Closed); }
                    return result;
                }
                command = self.commands.recv() => match command {
                    Some(Command::Close(_, _)) | None => {
                        if let Some(tools) = &self.host_tools { tools.revoke(); }
                        self.stop_task_controls().await;
                        let result = self.backend.kill().await;
                        self.snapshot.queue.paused = true; self.clear_permissions().await; self.finish(TurnOutcome::Cancelled,None,None).await; self.state(SessionState::Closed).await;
                        self.closed.store(true,Ordering::SeqCst); self.emit(EventKind::SessionEnded,None).await; self.persist().await;
                        if let Some(Command::Close(_,reply)) = command { let _ = reply.send(result); }
                        for reply in cancel_replies { let _ = reply.send(Err(Error::Closed)); }
                        return Err(Error::Closed);
                    }
                    Some(Command::Cancel(expected, reply)) => {
                        if expected.as_ref().is_some_and(|turn| self.snapshot.turn.as_ref() != Some(turn)) { let _ = reply.send(Err(Error::Invalid("turn is no longer active".into()))); continue; }
                        if let Some(tools) = &self.host_tools { tools.cancel_turn(); }
                        let mut queue = self.snapshot.queue.clone(); queue.paused = true;
                        if let Err(error) = self.save_queue(queue).await { let _ = reply.send(Err(error)); continue; }
                        if cancel_replies.is_empty() { cancel_deadline.as_mut().reset(tokio::time::Instant::now()+std::time::Duration::from_secs(5)); }
                        if defer_cancel { cancel_replies.push(reply); }
                        else {
                            let backend = self.backend.clone();
                            tokio::spawn(async move { let result = tokio::time::timeout(std::time::Duration::from_secs(5),backend.cancel()).await.unwrap_or_else(|_| Err(Error::Timeout("cancel acknowledgement".into()))); let _ = reply.send(result); });
                        }
                    }
                    Some(Command::Prompt(key,prompt,reply)) => {
                        let result = self.retained_submission(&key,&prompt).and_then(|receipt| receipt.ok_or(Error::Busy));
                        let _ = reply.send(result);
                    }
                    Some(Command::Submission(key,reply)) => { let _ = reply.send(Ok(self.submissions.get(&key).map(|r| r.receipt.clone()))); }
                    Some(Command::ReconcileSubmission(_,r)) => { let _ = r.send(Err(Error::Busy)); }
                    Some(Command::Steer(_,r) | Command::Rollback(_,_,_,r) | Command::Compact(r)) => { let _ = r.send(Err(Error::Busy)); }
                    Some(Command::Permission(_,r)) => { let _ = r.send(Err(Error::Busy)); }
                    Some(Command::Models(r)) => { let _ = r.send(Err(Error::Busy)); }
                    Some(Command::Tasks(reply)) => {
                        let backend = self.backend.clone(); let capability = self.snapshot.capabilities.background_tasks.clone();
                        self.task_controls.spawn(async move {
                            let result = tokio::time::timeout(std::time::Duration::from_secs(5), async { capability.require("background tasks")?; backend.tasks().await }).await.unwrap_or_else(|_| Err(Error::Timeout("background task status".into())));
                            let _ = reply.send(result);
                        });
                    }
                    Some(Command::StopTask(task_id,reply)) => {
                        let backend = self.backend.clone(); let capability = self.snapshot.capabilities.background_tasks.clone();
                        self.task_controls.spawn(async move {
                            let result = tokio::time::timeout(std::time::Duration::from_secs(5), async { capability.require("background task control")?; backend.stop_task(&task_id).await }).await.unwrap_or_else(|_| Err(Error::Timeout("background task stop acknowledgement".into())));
                            let _ = reply.send(result);
                        });
                    }
                    Some(Command::Configuration(r)) => { let _ = r.send(Err(Error::Busy)); }
                    Some(Command::Configure(_,r)) => { let _ = r.send(Err(Error::Busy)); }
                    Some(Command::SlashCommands(r) | Command::Quotas(r)) => { let _ = r.send(Err(Error::Busy)); }
                    Some(command @ (Command::QueueAdd(..) | Command::QueueUpdate(..) | Command::QueueRemove(..) | Command::QueuePause(..) | Command::Metadata(..))) => self.queue_command(command).await,
                },
                _ = &mut cancel_deadline, if !cancel_replies.is_empty() => {
                    self.stop_task_controls().await;
                    let _ = self.backend.kill().await;
                    self.snapshot.queue.paused = true; self.clear_permissions().await; self.finish(TurnOutcome::Cancelled,None,None).await; self.state(SessionState::Closed).await;
                    self.closed.store(true,Ordering::SeqCst); self.emit(EventKind::SessionEnded,None).await; self.persist().await;
                    for reply in cancel_replies { let _ = reply.send(Err(Error::Timeout("prompt acceptance during cancellation; session terminated".into()))); }
                    return Err(Error::Closed);
                }
                _ = self.task_controls.join_next(), if !self.task_controls.is_empty() => {}
            }
        }
    }
    pub async fn run(mut self) {
        if let Some(turn) = self.recovered_turn.take() {
            self.snapshot.turn = Some(turn);
            self.emit(
                EventKind::TurnEnded {
                    outcome: TurnOutcome::Interrupted,
                    native_reason: None,
                },
                None,
            )
            .await;
            self.snapshot.turn = None;
            self.snapshot.turns_completed += 1;
        }
        self.emit(
            EventKind::SessionStarted(Box::new(self.snapshot.clone())),
            None,
        )
        .await;
        self.persist().await;
        loop {
            if self.closed.load(Ordering::SeqCst) {
                break;
            }
            // Drain trailing provider frames and pending client controls before a queued turn.
            if self.commands.is_empty() {
                // A provider's idle heartbeat cannot starve client follow-ups forever.
                if self.snapshot.turn.is_none()
                    && !self.snapshot.queue.paused
                    && !self.snapshot.queue.items.is_empty()
                {
                    for _ in 0..64 {
                        match self.input.try_recv() {
                            Ok(event) => {
                                if self.handle_event(event).await {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
                if self.closed.load(Ordering::SeqCst) {
                    break;
                }
                self.promote_queue().await;
                if self.closed.load(Ordering::SeqCst) {
                    break;
                }
            }
            tokio::select! {
                biased;
                cmd = self.commands.recv() => {
                    let Some(cmd) = cmd else { break };
                    match cmd {
                        Command::Prompt(key,p, reply) => { let result = self.submit_prompt(key,p).await; let _ = reply.send(result); }
                        Command::Submission(key,reply) => { let _ = reply.send(Ok(self.submissions.get(&key).map(|r| r.receipt.clone()))); }
                        Command::ReconcileSubmission(key,reply) => { let result = self.reconcile_retained_submission(&key).await; let _ = reply.send(result); }
                        Command::Steer(mut p, reply) => {
                            let result = async {
                                if self.dispatch_blocked() { return Err(Error::Busy); }
                                if !self.snapshot.capabilities.steering.is_available() { return Err(Error::Unsupported("steering".into())); }
                                if self.snapshot.turn.is_none() { return Err(Error::Invalid("steering requires an active turn".into())); }
                                let context = self.snapshot.working_context.as_ref().ok_or_else(|| Error::Storage("session has no retained working context".into()))?;
                                verify_context(context).await?;
                                for ext in &self.extensions { ext.before_prompt(&self.snapshot, &mut p).await?; }
                                let backend = self.backend.clone(); let submitted = p.clone();
                                self.controlled(Box::pin(async move { backend.steer(&submitted).await }),false).await?;
                                self.emit(EventKind::UserMessage(p), None).await; Ok(())
                            }.await; let _ = reply.send(result);
                        }
                        Command::Cancel(expected, reply) => {
                            let result = async {
                                if expected.as_ref().is_some_and(|turn| self.snapshot.turn.as_ref() != Some(turn)) { return Err(Error::Invalid("turn is no longer active".into())); }
                                if let Some(tools) = &self.host_tools { tools.cancel_turn(); }
                                let mut queue = self.snapshot.queue.clone(); queue.paused = true; self.save_queue(queue).await?;
                                let active_turn = self.snapshot.turn.is_some();
                                self.cancel_permissions().await;
                                if active_turn || !self.permissions.is_empty() {
                                    if active_turn { self.state(SessionState::Cancelling).await; }
                                    let backend = self.backend.clone();
                                    self.controlled(Box::pin(async move { backend.cancel().await }),false).await?;
                                    if !active_turn {
                                        self.clear_permissions().await;
                                        self.settle_permission_state().await;
                                    }
                                }
                                Ok(())
                            }.await;
                            let _ = reply.send(result);
                        }
                        Command::Permission(r, reply) => { let result = self.answer(r).await; let _ = reply.send(result); }
                        Command::Models(reply) => { let backend = self.backend.clone(); let capability = self.snapshot.capabilities.models.clone(); let result = self.controlled(Box::pin(async move { capability.require("models")?; backend.models().await }),false).await; let _ = reply.send(result); }
                        Command::Configuration(reply) => { let result = self.describe_configuration().await; let _ = reply.send(result); }
                        Command::SlashCommands(reply) => { let backend = self.backend.clone(); let result = self.controlled(Box::pin(async move { backend.commands().await }),false).await; let _ = reply.send(result); }
                        Command::Tasks(reply) => { let backend = self.backend.clone(); let capability = self.snapshot.capabilities.background_tasks.clone(); let result = self.controlled(Box::pin(async move { capability.require("background tasks")?; backend.tasks().await }),false).await; let _ = reply.send(result); }
                        Command::StopTask(task_id, reply) => { let backend = self.backend.clone(); let capability = self.snapshot.capabilities.background_tasks.clone(); let result = self.controlled(Box::pin(async move { capability.require("background task control")?; backend.stop_task(&task_id).await }),false).await; let _ = reply.send(result); }
                        Command::Quotas(reply) => { let backend = self.backend.clone(); let result = self.controlled(Box::pin(async move { backend.quotas().await }),false).await; let _ = reply.send(result); }
                        Command::Configure(v, reply) => {
                            let result = self.apply_configuration(v).await;
                            let _ = reply.send(result);
                        }
                        command @ (Command::QueueAdd(..) | Command::QueueUpdate(..) | Command::QueueRemove(..) | Command::QueuePause(..) | Command::Metadata(..)) => self.queue_command(command).await,
                        Command::Rollback(n, scope, sequence, reply) => {
                            let result = async {
                                if sequence.is_some_and(|seq| seq != self.snapshot.sequence) { return Err(Error::Invalid("history changed before rollback".into())); }
                                if self.dispatch_blocked() || self.snapshot.turn.is_some() || !self.permissions.is_empty() { return Err(Error::Busy); }
                                if n == 0 { return Err(Error::Invalid("turns must be positive".into())); }
                                if scope == RollbackScope::ConversationAndFiles {
                                    let context = self.snapshot.working_context.as_ref().ok_or_else(|| Error::Storage("missing working context".into()))?;
                                    verify_context(context).await?;
                                }
                                let backend = self.backend.clone();
                                self.controlled(Box::pin(async move { backend.rollback_with(n, scope).await }), false).await?;
                                self.snapshot.turns_completed = self.snapshot.turns_completed.saturating_sub(n);
                                self.emit(EventKind::ConversationRolledBack { turns: n }, None).await;
                                self.store.save(&self.snapshot).await
                            }.await;
                            let _ = reply.send(result);
                        }
                        Command::Compact(reply) => { let result = if self.dispatch_blocked() || self.snapshot.turn.is_some() || !self.permissions.is_empty() { Err(Error::Busy) } else { { let backend = self.backend.clone(); self.controlled(Box::pin(async move { backend.compact().await }),false).await } }; let _ = reply.send(result); }
                        Command::Close(kill, reply) => {
                            if let Some(tools) = &self.host_tools { tools.revoke(); }
                            self.stop_task_controls().await;
                            // Native teardown settles control requests without waiting for an
                            // approval acknowledgement that the provider may never complete.
                            let result = if kill { self.backend.kill().await } else { self.backend.close().await };
                            self.clear_permissions().await;
                            self.finish(TurnOutcome::Cancelled, None, None).await;
                            self.snapshot.queue.paused = true; self.state(SessionState::Closed).await; self.closed.store(true,Ordering::SeqCst); self.emit(EventKind::SessionEnded, None).await; self.persist().await;
                            let _ = reply.send(result); break;
                        }
                    }
                }
                event = self.input.recv() => {
                    let event = event.unwrap_or(BackendEvent::Exited { message: "provider event stream closed".into() });
                    if self.handle_event(event).await { break; }
                }
                _ = self.task_controls.join_next(), if !self.task_controls.is_empty() => {}
            }
        }
        if let Some(tools) = &self.host_tools {
            tools.revoke();
        }
        self.stop_task_controls().await;
        let _ = self.backend.kill().await;
        for ext in &self.extensions {
            ext.on_session_closed(&self.snapshot).await;
        }
    }
}

fn validate_question_answers(request: &PermissionRequest, data: &Value) -> Result<()> {
    let Some(questions) = request.metadata["questions"].as_array() else {
        return Ok(());
    };
    let mut ids = std::collections::BTreeSet::new();
    for question in questions {
        let id = question["id"]
            .as_str()
            .ok_or_else(|| Error::Invalid("question has no id".into()))?;
        if !ids.insert(id) {
            return Err(Error::Invalid("question ids must be unique".into()));
        }
    }
    let answers = data["answers"]
        .as_object()
        .ok_or_else(|| Error::Invalid("question answers must be an object".into()))?;
    for id in answers.keys() {
        if !questions
            .iter()
            .any(|question| question["id"].as_str() == Some(id))
        {
            return Err(Error::Invalid(format!("unknown question: {id}")));
        }
    }
    for question in questions {
        let id = question["id"]
            .as_str()
            .ok_or_else(|| Error::Invalid("question has no id".into()))?;
        let answer = answers
            .get(id)
            .ok_or_else(|| Error::Invalid(format!("answer required for question: {id}")))?;
        let values = if question["multiple"].as_bool().unwrap_or(false) {
            answer
                .as_array()
                .ok_or_else(|| Error::Invalid(format!("question {id} requires an array")))?
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| Error::Invalid("answers must be strings".into()))
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            vec![
                answer
                    .as_str()
                    .ok_or_else(|| Error::Invalid(format!("question {id} requires a string")))?,
            ]
        };
        if values.is_empty() || values.iter().any(|value| value.trim().is_empty()) {
            return Err(Error::Invalid(format!(
                "answer required for question: {id}"
            )));
        }
        if !question["allowText"].as_bool().unwrap_or(true) {
            for value in values {
                if !question["options"].as_array().is_some_and(|options| {
                    options
                        .iter()
                        .any(|option| option["id"].as_str() == Some(value))
                }) {
                    return Err(Error::Invalid(format!(
                        "question {id} did not offer answer: {value}"
                    )));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn questions_validate_ids_choices_multiplicity_and_free_text() {
        let mut request = PermissionRequest {
            questions: Vec::new(),
            id: "dialog".into(),
            kind: "question".into(),
            title: "Configuration".into(),
            description: None,
            command: None,
            paths: vec![],
            options: vec![],
            metadata: json!({"questions":[
                {"id":"selection","multiple":false,"allowText":false,"options":[{"id":"a","label":"A"}]},
                {"id":"selections","multiple":true,"allowText":false,"options":[{"id":"a"},{"id":"b"}]},
                {"id":"notes","multiple":false,"allowText":true,"options":[]}
            ]}),
        };
        let valid =
            json!({"answers":{"selection":"a","selections":["a","b"],"notes":"custom text"}});
        validate_question_answers(&request, &valid).unwrap();
        for invalid in [
            json!({"answers":{"selection":"unoffered","selections":["a"],"notes":"text"}}),
            json!({"answers":{"selection":["a"],"selections":["a"],"notes":"text"}}),
            json!({"answers":{"selection":"a","selections":"a","notes":"text"}}),
            json!({"answers":{"selection":"a","selections":[],"notes":"text"}}),
            json!({"answers":{"selection":"a","selections":["a"],"notes":""}}),
            json!({"answers":{"selection":"a","selections":["a"],"notes":"text","unknown":"extra"}}),
        ] {
            assert!(matches!(
                validate_question_answers(&request, &invalid),
                Err(Error::Invalid(_))
            ));
        }
        request.metadata["questions"][2]["id"] = json!("selection");
        assert!(validate_question_answers(&request, &valid).is_err());
    }
}
