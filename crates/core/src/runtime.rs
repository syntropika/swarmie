#[path = "local_runtime.rs"]
mod local_runtime;
use crate::session::{AgentInner, Engine};
use crate::*;
use futures_util::{StreamExt, stream};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak, atomic::AtomicBool},
};
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};

pub struct RuntimeBuilder {
    harnesses: BTreeMap<String, Arc<dyn Harness>>,
    store: SharedStore,
    extensions: Vec<Arc<dyn RuntimeExtension>>,
}
impl Default for RuntimeBuilder {
    fn default() -> Self {
        Self {
            harnesses: BTreeMap::new(),
            store: Arc::new(MemoryStore::default()),
            extensions: vec![],
        }
    }
}
impl RuntimeBuilder {
    pub fn harness(mut self, harness: impl Harness + 'static) -> Self {
        self.harnesses
            .insert(harness.id().into(), Arc::new(harness));
        self
    }
    pub fn store(mut self, store: impl SessionStore + 'static) -> Self {
        self.store = Arc::new(store);
        self
    }
    pub fn extension(mut self, ext: impl RuntimeExtension + 'static) -> Self {
        self.extensions.push(Arc::new(ext));
        self
    }
    pub fn build(self) -> Runtime {
        let (events, _) = broadcast::channel(4096);
        let runtime = Runtime(Arc::new(RuntimeInner {
            incarnation_id: uuid::Uuid::new_v4().to_string(),
            harnesses: self.harnesses,
            store: self.store,
            extensions: self.extensions,
            agents: RwLock::new(BTreeMap::new()),
            open_lock: Mutex::new(()),
            events,
            host_tools: Default::default(),
        }));
        for extension in &runtime.0.extensions {
            extension.initialize(runtime.downgrade());
        }
        runtime
    }
}
struct RuntimeInner {
    incarnation_id: String,
    host_tools: crate::tools::HostTools,
    harnesses: BTreeMap<String, Arc<dyn Harness>>,
    store: SharedStore,
    extensions: Vec<Arc<dyn RuntimeExtension>>,
    agents: RwLock<BTreeMap<String, Weak<AgentInner>>>,
    open_lock: Mutex<()>,
    events: broadcast::Sender<RuntimeEvent>,
}
#[derive(Clone)]
pub struct Runtime(Arc<RuntimeInner>);
/// Access to a runtime without extending its lifetime.
#[derive(Clone)]
pub struct WeakRuntime(Weak<RuntimeInner>);
impl WeakRuntime {
    pub fn upgrade(&self) -> Option<Runtime> {
        self.0.upgrade().map(Runtime)
    }
}
impl Default for Runtime {
    fn default() -> Self {
        RuntimeBuilder::default().build()
    }
}
impl Runtime {
    fn validate_permission_authority(&self) -> Result<()> {
        if self
            .0
            .extensions
            .iter()
            .filter(|extension| extension.permission_authority())
            .count()
            > 1
        {
            return Err(Error::Invalid(
                "register only one permission authority".into(),
            ));
        }
        Ok(())
    }
    /// Install node-local delivery configuration; never exposed as model authority.
    pub fn install_host_tool_transport(&self, server: McpServer) -> Result<()> {
        self.0.host_tools.transport(server)
    }
    pub fn host_tool_channel(&self, token: &str) -> bool {
        self.0.host_tools.channel(token)
    }
    pub async fn invoke_host_tool_channel(
        &self,
        token: &str,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        self.0.host_tools.dispatch(token, method, params).await
    }
    /// Clones share an incarnation; a new execution owner receives a new one.
    pub async fn identity(&self) -> Result<RuntimeIdentity> {
        Ok(RuntimeIdentity {
            identity: self.local_catalog().await?.identity,
            incarnation_id: self.0.incarnation_id.clone(),
        })
    }
    pub fn downgrade(&self) -> WeakRuntime {
        WeakRuntime(Arc::downgrade(&self.0))
    }
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::default()
    }
    pub fn events(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.0.events.subscribe()
    }
    pub fn transcript_durability(&self) -> Option<&'static str> {
        self.0.store.transcript_durability()
    }
    pub fn extensions(&self) -> Vec<ExtensionInfo> {
        self.0
            .extensions
            .iter()
            .map(|e| ExtensionInfo {
                id: e.id().into(),
                commands: e.commands(),
                views: e.views(),
                ui: e.ui(),
            })
            .collect()
    }
    pub async fn invoke_extension(
        &self,
        id: &str,
        command: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let extension = self
            .0
            .extensions
            .iter()
            .find(|e| e.id() == id)
            .ok_or_else(|| Error::Invalid(format!("unknown extension: {id}")))?;
        if !extension.commands().iter().any(|c| c.name == command) {
            return Err(Error::Unsupported(format!(
                "extension command {id}/{command}"
            )));
        }
        let context = if let Some(session_id) = params.get("sessionId").and_then(Value::as_str) {
            Some(self.session_context(session_id).await?)
        } else {
            None
        };
        extension
            .before_invoke(command, &params, context.as_ref())
            .await?;
        extension.invoke(command, params).await
    }
    pub async fn discover(&self) -> Result<Vec<Detection>> {
        let mut result = Vec::new();
        for h in self.0.harnesses.values() {
            result.push(h.detect().await?);
        }
        Ok(result)
    }
    pub async fn probe(&self, harness: &str) -> Result<HarnessProbe> {
        self.0
            .harnesses
            .get(harness)
            .ok_or_else(|| Error::Invalid(format!("unregistered harness: {harness}")))?
            .probe()
            .await
    }
    /// Read native settings through a temporary connection without saving a chat or sending a prompt.
    pub async fn preview_configuration(
        &self,
        mut options: SpawnOptions,
    ) -> Result<ConfigurationPreview> {
        self.validate_permission_authority()?;
        options = self.resolve_launch_options(options).await?;
        let harness =
            self.0.harnesses.get(&options.harness).ok_or_else(|| {
                Error::Invalid(format!("unregistered harness: {}", options.harness))
            })?;
        let detection = harness.detect().await?;
        if !detection.capabilities.ephemeral.is_available() {
            return Err(Error::Unsupported(
                "This harness requires a session to discover its model settings".into(),
            ));
        }
        options.execution.ephemeral = true;
        compile_launch_profile(harness.as_ref(), &mut options)?;
        let connection = harness
            .connect(&native_launch_options(&options), None, false)
            .await?;
        let execution_policy =
            match launch_policy(connection.backend.as_ref(), &options, None).await {
                Ok(policy) => policy,
                Err(error) => {
                    let _ = connection.backend.kill().await;
                    return Err(error);
                }
            };
        let mut events = connection.events;
        let drain = tokio::spawn(async move { while events.recv().await.is_some() {} });
        let result = async {
            let mut configuration = if connection.capabilities.configure.is_available() {
                connection.backend.configuration().await?
            } else {
                vec![]
            };
            configuration.push(execution_policy.descriptor());
            let configuration = Some(configuration);
            let models = if connection.capabilities.models.is_available() {
                Some(connection.backend.models().await?)
            } else {
                None
            };
            Ok(ConfigurationPreview {
                capabilities: connection.capabilities.clone(),
                execution_policy,
                configuration,
                models,
            })
        }
        .await;
        let closed = connection.backend.close().await;
        drain.abort();
        closed?;
        result
    }
    pub async fn native_history(
        &self,
        query: NativeHistoryQuery,
    ) -> Result<Vec<NativeHistorySession>> {
        let harness = self
            .0
            .harnesses
            .get(&query.harness)
            .ok_or_else(|| Error::Invalid("unregistered history harness".into()))?;
        harness.native_history(&query).await
    }
    pub async fn read_native_history(
        &self,
        query: NativeHistoryQuery,
        native_id: &str,
    ) -> Result<NativeHistoryDocument> {
        let harness = self
            .0
            .harnesses
            .get(&query.harness)
            .ok_or_else(|| Error::Invalid("unregistered history harness".into()))?;
        harness.read_native_history(&query, native_id).await
    }
    /// Import observed native history without creating or executing a provider session.
    pub async fn import_native_history(
        &self,
        query: NativeHistoryQuery,
        native_id: &str,
    ) -> Result<SessionSnapshot> {
        let _lock = self.0.open_lock.lock().await;
        if self.transcript_durability().is_none() {
            return Err(Error::Unsupported(
                "native history import requires transcript persistence".into(),
            ));
        }
        let document = self.read_native_history(query, native_id).await?;
        for saved in self.0.store.list().await? {
            if saved.native.provider == document.session.native.provider
                && saved.native.id == document.session.native.id
            {
                return Ok(saved);
            }
        }
        if !document.capabilities.resume.is_available() {
            return Err(Error::Unsupported("imported native session resume".into()));
        }
        let now = timestamp_ms();
        let mut options = sanitized_options(document.options);
        if !options.config.is_object() {
            options.config = serde_json::json!({});
        }
        options.config["nativeHistory"] = serde_json::to_value(&document.session)?;
        let id = uuid::Uuid::new_v4().to_string();
        let directory = self.register_observed_directory(&options.cwd, true).await?;
        options.cwd = directory.path.clone();
        let context = self
            .0
            .store
            .prepare_context(
                &id,
                &uuid::Uuid::new_v4().to_string(),
                ContextSelection::Directory {
                    directory: directory.reference(),
                    native_existing: true,
                },
            )
            .await?;
        let mut snapshot = SessionSnapshot {
            id,
            working_context: Some(context),
            execution_policy: ExecutionPolicy::default(),
            native: document.session.native,
            options,
            capabilities: document.capabilities,
            state: SessionState::Closed,
            sequence: 0,
            turn: None,
            turns_completed: 0,
            metadata: SessionMetadata {
                title: Some(document.session.title),
                created_at_ms: now,
                updated_at_ms: now,
                ..Default::default()
            },
            queue: Default::default(),
        };
        let mut events = Vec::new();
        for record in document.records {
            snapshot.sequence += 1;
            if matches!(record.kind, EventKind::TurnEnded { .. }) {
                snapshot.turns_completed += 1;
            }
            events.push(RuntimeEvent {
                session_id: snapshot.id.clone(),
                sequence: snapshot.sequence,
                turn_id: record.turn_id,
                source: snapshot.options.harness.clone(),
                timestamp_ms: document.session.updated_at_ms,
                kind: record.kind,
                raw: record.raw,
            });
        }
        self.0
            .store
            .commit(SessionWrite {
                snapshot: Some(&snapshot),
                events: &events,
                ..Default::default()
            })
            .await?;
        Ok(snapshot)
    }
    pub async fn account_quotas(&self, query: AccountQuery) -> Result<Value> {
        query.validate()?;
        let harness =
            self.0.harnesses.get(&query.harness).ok_or_else(|| {
                Error::Invalid(format!("unregistered harness: {}", query.harness))
            })?;
        tokio::time::timeout(
            std::time::Duration::from_secs(60),
            harness.account_quotas(&query),
        )
        .await
        .map_err(|_| Error::Timeout("account quotas".into()))?
    }
    /// An explicit, bounded list of independent account queries. No sessions are opened.
    pub async fn account_quotas_all(
        &self,
        queries: Vec<AccountQuery>,
    ) -> Result<Vec<AccountQuotaResult>> {
        if queries.is_empty() || queries.len() > 32 {
            return Err(Error::Invalid(
                "provide between 1 and 32 account queries".into(),
            ));
        }
        Ok(stream::iter(queries)
            .map(|query| async move {
                let harness = query.harness.clone();
                match self.account_quotas(query).await {
                    Ok(quotas) => AccountQuotaResult::Available { harness, quotas },
                    Err(Error::Unsupported(message)) => {
                        AccountQuotaResult::Unsupported { harness, message }
                    }
                    Err(error) => AccountQuotaResult::Error {
                        harness,
                        message: error.to_string(),
                    },
                }
            })
            .buffered(4)
            .collect()
            .await)
    }
    /// Read a retained receipt without creating or resuming provider work.
    pub async fn submission(&self, id: &str, key: &str) -> Result<Option<SubmissionReceipt>> {
        crate::submission::validate_submission_key(key)?;
        if let Some(agent) = self.agent(id).await {
            return agent.submission(key).await;
        }
        Ok(self
            .0
            .store
            .submissions(id)
            .await?
            .into_iter()
            .find(|r| r.receipt.idempotency_key == key)
            .map(|mut record| {
                record.recover();
                record.receipt
            }))
    }
    /// Inspect an existing native connection. This never resumes a closed session.
    pub async fn reconcile_submission(
        &self,
        id: &str,
        key: &str,
    ) -> Result<Option<SubmissionReceipt>> {
        crate::submission::validate_submission_key(key)?;
        self.agent(id)
            .await
            .ok_or(Error::Closed)?
            .reconcile_submission(key)
            .await
    }
    pub async fn submit(&self, id: &str, key: &str, prompt: Prompt) -> Result<SubmissionReceipt> {
        crate::submission::validate_client_submission_key(key)?;
        if let Some(agent) = self.agent(id).await {
            return agent.submit(key, prompt).await;
        }
        if let Some(mut record) = self
            .0
            .store
            .submissions(id)
            .await?
            .into_iter()
            .find(|r| r.receipt.idempotency_key == key)
        {
            record.recover();
            return record.duplicate(&prompt);
        }
        Err(Error::Closed)
    }
    pub async fn snapshot(&self, id: &str) -> Result<SessionSnapshot> {
        if let Some(agent) = self.agent(id).await {
            return Ok(agent.snapshot().await);
        }
        self.0
            .store
            .load(id)
            .await?
            .ok_or_else(|| Error::Invalid("unknown saved session".into()))
    }
    pub async fn transcript(
        &self,
        id: &str,
        after: u64,
        limit: usize,
    ) -> Result<Vec<RuntimeEvent>> {
        if self.transcript_durability().is_none() {
            return Err(Error::Unsupported("transcript persistence".into()));
        }
        if limit == 0 || limit > 4096 {
            return Err(Error::Invalid("limit must be between 1 and 4096".into()));
        }
        self.snapshot(id).await?;
        self.0.store.events(id, after, limit).await
    }
    pub async fn update_metadata(
        &self,
        id: &str,
        update: SessionMetadataUpdate,
    ) -> Result<SessionSnapshot> {
        let _lock = self.0.open_lock.lock().await;
        if let Some(agent) = self.agent(id).await {
            return agent.update_metadata(update).await;
        }
        let mut snapshot = self.snapshot(id).await?;
        apply_metadata(&mut snapshot, update)?;
        self.0.store.save(&snapshot).await?;
        Ok(snapshot)
    }
    pub async fn spawn(&self, options: SpawnOptions) -> Result<Agent> {
        self.open(options, None, false, None, false, false).await
    }
    /// A tool-disabled native turn with no session or events written to this runtime's store.
    pub async fn generate(
        &self,
        options: SpawnOptions,
        prompt: impl Into<Prompt>,
    ) -> Result<String> {
        self.generate_with_timeout(options, prompt.into(), std::time::Duration::from_secs(120))
            .await
    }
    pub async fn generate_with_timeout(
        &self,
        mut options: SpawnOptions,
        prompt: Prompt,
        timeout: std::time::Duration,
    ) -> Result<String> {
        if timeout.is_zero() {
            return Err(Error::Invalid("generate timeout must be positive".into()));
        }
        options = self.resolve_launch_options(options).await?;
        options.project_id = None;
        options.project_revision = None;
        options.execution.tools = ToolPolicy::Disabled;
        options.execution.ephemeral = true;
        let schema = options.execution.output_schema.is_some();
        let (events, _) = broadcast::channel(4096);
        let isolated = Runtime(Arc::new(RuntimeInner {
            incarnation_id: uuid::Uuid::new_v4().to_string(),
            host_tools: Default::default(),
            harnesses: self.0.harnesses.clone(),
            store: Arc::new(MemoryStore::default()),
            extensions: vec![],
            agents: RwLock::new(BTreeMap::new()),
            open_lock: Mutex::new(()),
            events,
        }));
        let deadline = tokio::time::Instant::now() + timeout;
        let agent = tokio::time::timeout_at(deadline, isolated.spawn(options))
            .await
            .map_err(|_| Error::Timeout("generate handshake".into()))??;
        let mut stream = agent.events();
        let result = tokio::time::timeout_at(
            deadline,
            crate::generate::collect(&agent, &mut stream, prompt, schema),
        )
        .await
        .unwrap_or_else(|_| Err(Error::Timeout("generate turn".into())));
        let cleanup = tokio::time::timeout(std::time::Duration::from_secs(5), agent.close())
            .await
            .unwrap_or_else(|_| Err(Error::Timeout("generate cleanup".into())));
        match (result, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(text), Ok(())) => Ok(text),
        }
    }
    pub async fn resume(&self, id: &str) -> Result<Agent> {
        // Attaching another client to a live binding does not require native restart support.
        if let Some(agent) = self.agent(id).await {
            return Ok(agent);
        }
        let s = self
            .0
            .store
            .load(id)
            .await?
            .ok_or_else(|| Error::Invalid("unknown saved session".into()))?;
        if !s.capabilities.resume.is_available() {
            return Err(Error::Unsupported("resume".into()));
        }
        self.open(s.options.clone(), Some(s), false, None, false, false)
            .await
    }
    /// Execution overrides replace the saved declaration; credentials must be supplied anew.
    pub async fn resume_with(&self, id: &str, execution: SessionExecutionOptions) -> Result<Agent> {
        let mut saved = self.snapshot(id).await?;
        if !saved.capabilities.resume.is_available() {
            return Err(Error::Unsupported("resume".into()));
        }
        saved.options.execution = execution;
        self.open(saved.options.clone(), Some(saved), false, None, true, false)
            .await
    }
    pub async fn fork(&self, id: &str) -> Result<Agent> {
        self.fork_with_execution(id, None, None, false).await
    }
    /// Replaces the execution declaration for the new fork without changing its source.
    /// Closed or restarted sources require credentials to be supplied anew.
    pub async fn fork_with(&self, id: &str, execution: SessionExecutionOptions) -> Result<Agent> {
        self.fork_with_execution(id, None, Some(execution), false)
            .await
    }
    pub async fn fork_at(&self, id: &str, message_id: &str) -> Result<Agent> {
        self.fork_with_execution(id, Some(message_id), None, false)
            .await
    }
    /// Replaces the execution declaration for a fork at a completed message boundary.
    pub async fn fork_at_with(
        &self,
        id: &str,
        message_id: &str,
        execution: SessionExecutionOptions,
    ) -> Result<Agent> {
        self.fork_with_execution(id, Some(message_id), Some(execution), false)
            .await
    }
    pub async fn session_tree(&self, id: &str) -> Result<Vec<NativeMessageNode>> {
        let snapshot = self.snapshot(id).await?;
        let query = NativeHistoryQuery {
            harness: snapshot.options.harness.clone(),
            cwd: None,
            config_home: snapshot.options.execution.config_home.clone(),
            limit: 1000,
        };
        let mut tree = self
            .read_native_history(query, &snapshot.native.id)
            .await?
            .tree;
        if let Some(aliases) = snapshot
            .capabilities
            .metadata
            .get("messageIdAliases")
            .and_then(Value::as_object)
        {
            let reverse: BTreeMap<&str, &str> = aliases
                .iter()
                .filter_map(|(canonical, native)| {
                    native.as_str().map(|native| (native, canonical.as_str()))
                })
                .collect();
            for node in &mut tree {
                if let Some(canonical) = reverse.get(node.id.as_str()) {
                    if !node.metadata.is_object() {
                        node.metadata = serde_json::json!({});
                    }
                    node.metadata["nativeId"] = serde_json::json!(node.id);
                    node.id = (*canonical).into();
                }
                if let Some(parent) = &mut node.parent_id
                    && let Some(canonical) = reverse.get(parent.as_str())
                {
                    *parent = (*canonical).into();
                }
            }
        }
        Ok(tree)
    }
    pub async fn fork_before_user(
        &self,
        id: &str,
        message_id: &str,
        execution: Option<SessionExecutionOptions>,
    ) -> Result<Agent> {
        self.fork_with_execution(id, Some(message_id), execution, true)
            .await
    }
    async fn fork_with_execution(
        &self,
        id: &str,
        message_id: Option<&str>,
        execution: Option<SessionExecutionOptions>,
        before_user: bool,
    ) -> Result<Agent> {
        if message_id.is_some_and(|id| id.trim().is_empty() || id.len() > 4096) {
            return Err(Error::Invalid(
                "messageId must contain 1 to 4096 bytes".into(),
            ));
        }
        let live = self.agent(id).await;
        let mut snapshot = if let Some(agent) = &live {
            agent.snapshot().await
        } else {
            self.snapshot(id).await?
        };
        if snapshot.turn.is_some() || snapshot.state == SessionState::NeedsInput {
            return Err(Error::Busy);
        }
        let supported = if message_id.is_some() {
            if before_user {
                snapshot.capabilities.fork_before_user.is_available()
            } else {
                snapshot.capabilities.fork_at_message.is_available()
            }
        } else {
            snapshot.capabilities.fork.is_available()
        };
        if !supported {
            return Err(Error::Unsupported(
                if message_id.is_some() {
                    "fork at message"
                } else {
                    "fork"
                }
                .into(),
            ));
        }
        let live_execution = live
            .as_ref()
            .filter(|_| !matches!(snapshot.state, SessionState::Closed | SessionState::Failed))
            .and_then(|agent| agent.0.execution.lock().unwrap().clone());
        if let Some(execution) = execution.or(live_execution) {
            snapshot.options.execution = execution;
        }
        self.open(
            snapshot.options.clone(),
            Some(snapshot),
            true,
            message_id.map(String::from),
            false,
            before_user,
        )
        .await
    }
    async fn open(
        &self,
        mut options: SpawnOptions,
        saved: Option<SessionSnapshot>,
        fork: bool,
        message_id: Option<String>,
        execution_override: bool,
        before_user: bool,
    ) -> Result<Agent> {
        self.validate_permission_authority()?;
        let _lock = self.0.open_lock.lock().await;
        if before_user && self.transcript_durability().is_none() {
            return Err(Error::Unsupported(
                "before-user fork requires transcript persistence".into(),
            ));
        }
        if let Some(s) = &saved
            && !fork
            && let Some(a) = self.agent(&s.id).await
        {
            if execution_override {
                return Err(Error::Invalid(
                    "execution overrides require a closed session".into(),
                ));
            }
            return Ok(a);
        }
        let session_id = saved
            .as_ref()
            .filter(|_| !fork)
            .map(|s| s.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let context = if let Some(snapshot) = &saved {
            let retained = self.retained_context(snapshot).await?;
            verify_context(&retained).await?;
            options.cwd = retained.directory.path.clone();
            if fork {
                self.0
                    .store
                    .prepare_context(
                        &session_id,
                        &uuid::Uuid::new_v4().to_string(),
                        ContextSelection::Inherited {
                            context_id: retained.id,
                        },
                    )
                    .await?
            } else {
                retained
            }
        } else {
            let selection = self.resolve_new_selection(&mut options).await?;
            let context = self
                .0
                .store
                .prepare_context(&session_id, &uuid::Uuid::new_v4().to_string(), selection)
                .await?;
            options.cwd = context.directory.path.clone();
            context
        };
        let selected_project = options.project_id.clone();
        let selected_revision = options.project_revision;
        for ext in &self.0.extensions {
            ext.before_spawn(&mut options).await?;
        }
        if options.cwd != context.directory.path
            || options.project_id != selected_project
            || options.project_revision != selected_revision
        {
            return Err(Error::Invalid(
                "extension changed a retained working selection".into(),
            ));
        }
        verify_context(&context).await?;
        let h =
            self.0.harnesses.get(&options.harness).ok_or_else(|| {
                Error::Invalid(format!("unregistered harness: {}", options.harness))
            })?;
        let mut retained = std::collections::VecDeque::new();
        let mut submission_history = crate::submission::SubmissionHistory::default();
        let mut last_sequence = saved.as_ref().map_or(0, |s| s.sequence);
        let mut turns_completed = saved.as_ref().map_or(0, |s| s.turns_completed);
        let mut recovered_turn = saved
            .as_ref()
            .filter(|_| !fork)
            .and_then(|s| s.turn.clone());
        if let Some(s) = &saved
            && self.transcript_durability().is_some()
        {
            let mut after = 0;
            loop {
                let page = self.0.store.events(&s.id, after, 4096).await?;
                if page.is_empty() {
                    break;
                }
                for event in page {
                    submission_history.observe(&event);
                    after = event.sequence;
                    last_sequence = last_sequence.max(after);
                    if matches!(&event.kind, EventKind::TurnEnded { .. })
                        && event.turn_id == recovered_turn
                    {
                        recovered_turn = None;
                    }
                    if event.sequence > s.sequence {
                        match event.kind {
                            EventKind::TurnEnded { .. } => turns_completed += 1,
                            EventKind::ConversationRolledBack { turns } => {
                                turns_completed = turns_completed.saturating_sub(turns)
                            }
                            _ => (),
                        }
                    }
                    if retained.len() == 4096 {
                        retained.pop_front();
                    }
                    retained.push_back(event);
                }
            }
        }
        let fork_origin = if fork {
            let parent = saved.as_ref().expect("fork requires source");
            if before_user {
                let query = NativeHistoryQuery {
                    harness: options.harness.clone(),
                    cwd: Some(options.cwd.clone()),
                    config_home: options.execution.config_home.clone(),
                    limit: 1000,
                };
                let document = h.read_native_history(&query, &parent.native.id).await?;
                let anchor = message_id
                    .as_deref()
                    .expect("before-user fork requires message");
                let native_anchor = parent
                    .capabilities
                    .metadata
                    .get("messageIdAliases")
                    .and_then(|aliases| aliases.get(anchor))
                    .and_then(Value::as_str)
                    .unwrap_or(anchor);
                if !document.tree.iter().any(|node| {
                    node.id == native_anchor
                        && node.role == "user"
                        && node.metadata["nativeAnchor"] == true
                }) {
                    return Err(Error::Invalid(
                        "native user message anchor was not found".into(),
                    ));
                }
                last_sequence = 0;
                turns_completed = 0;
                Some(ForkOrigin {
                    session_id: parent.id.clone(),
                    message_id: message_id.clone(),
                    sequence: 0,
                    turn_id: None,
                })
            } else if let Some(message_id) = &message_id {
                let (origin, completed) = self
                    .fork_boundary(parent, message_id, turns_completed)
                    .await?;
                last_sequence = origin.sequence;
                turns_completed = completed;
                Some(origin)
            } else {
                Some(ForkOrigin {
                    session_id: parent.id.clone(),
                    message_id: None,
                    sequence: last_sequence,
                    turn_id: None,
                })
            }
        } else {
            None
        };
        // Providers that rewrite IDs on native fork can retain canonical journal anchors.
        compile_launch_profile(h.as_ref(), &mut options)?;
        let mut connect_options = native_launch_options(&options);
        if let Some(config) = connect_options.config.as_object_mut() {
            config.remove("_runtimeMessageIdAliases");
        }
        let aliases = saved
            .as_ref()
            .and_then(|snapshot| snapshot.capabilities.metadata.get("messageIdAliases"))
            .filter(|value| value.is_object());
        if let Some(aliases) = aliases {
            if !connect_options.config.is_object() {
                connect_options.config = serde_json::json!({});
            }
            connect_options.config["_runtimeMessageIdAliases"] = aliases.clone();
        }
        let mut submissions = BTreeMap::new();
        if let Some(saved) = &saved
            && !fork
        {
            for mut record in self.0.store.submissions(&saved.id).await? {
                record.recover();
                self.0.store.save_submission(&record).await?;
                submissions.insert(record.receipt.idempotency_key.clone(), record);
            }
        }
        let host_tools =
            self.0
                .host_tools
                .launch(&mut connect_options, &context, &self.0.extensions)?;
        let connection = if let Some(message_id) = &message_id {
            let native_message_id = aliases
                .and_then(|map| map.get(message_id))
                .and_then(Value::as_str)
                .unwrap_or(message_id);
            if before_user {
                h.fork_before_user(
                    &connect_options,
                    &saved.as_ref().expect("message fork requires source").native,
                    native_message_id,
                )
                .await?
            } else {
                h.fork_at(
                    &connect_options,
                    &saved.as_ref().expect("message fork requires source").native,
                    native_message_id,
                )
                .await?
            }
        } else {
            h.connect(&connect_options, saved.as_ref().map(|s| &s.native), fork)
                .await?
        };
        let mut execution_policy = match launch_policy(
            connection.backend.as_ref(),
            &options,
            saved.as_ref().map(|saved| &saved.execution_policy),
        )
        .await
        {
            Ok(policy) => policy,
            Err(error) => {
                let _ = connection.backend.kill().await;
                return Err(error);
            }
        };
        execution_policy.binding_id = Some(context.binding_id.clone());
        execution_policy.native_session = Some(connection.native.clone());
        if let Err(error) = validate_execution(&connect_options.execution, &connection.capabilities)
        {
            let _ = connection.backend.kill().await;
            return Err(error);
        }
        if !fork {
            for record in submissions
                .values_mut()
                .filter(|r| r.receipt.recovery_required)
            {
                if record.resolve_not_sent() {
                    if let Err(error) = self.0.store.save_submission(record).await {
                        let _ = connection.backend.kill().await;
                        return Err(error);
                    }
                    continue;
                }
                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    connection.backend.reconcile_submission(&record.receipt.id),
                )
                .await;
                if let Ok(Ok(Some(evidence))) = result {
                    if let Err(error) = record.reconcile(evidence) {
                        let _ = connection.backend.kill().await;
                        return Err(error);
                    }
                    // Retain native facts before repairing any journal or snapshot.
                    record.terminal_sequence =
                        record.receipt.outcome.as_ref().map(|_| {
                            last_sequence + submission_history.missing(record).len() as u64
                        });
                    if let Err(error) = self.0.store.save_submission(record).await {
                        let _ = connection.backend.kill().await;
                        return Err(error);
                    }
                }
            }
        }
        if self.transcript_durability().is_some() && !fork {
            for record in submissions.values() {
                if submission_history.open_turn(record) {
                    recovered_turn = Some(record.receipt.turn_id.clone());
                }
                for (kind, raw) in submission_history.missing(record) {
                    last_sequence += 1;
                    let terminal = matches!(kind, EventKind::TurnEnded { .. });
                    let event = RuntimeEvent {
                        session_id: record.receipt.session_id.clone(),
                        sequence: last_sequence,
                        turn_id: Some(record.receipt.turn_id.clone()),
                        source: options.harness.clone(),
                        timestamp_ms: timestamp_ms(),
                        kind,
                        raw,
                    };
                    if let Err(error) = self.0.store.append_event(&event).await {
                        let _ = connection.backend.kill().await;
                        return Err(error);
                    }
                    if terminal {
                        if record.terminal_sequence.is_some_and(|sequence| {
                            saved.as_ref().is_some_and(|s| s.sequence < sequence)
                        }) {
                            turns_completed += 1;
                        }
                        if recovered_turn.as_ref() == Some(&record.receipt.turn_id) {
                            recovered_turn = None;
                        }
                    } else if record.receipt.outcome.is_none() {
                        recovered_turn = Some(record.receipt.turn_id.clone());
                    }
                    if retained.len() == 4096 {
                        retained.pop_front();
                    }
                    retained.push_back(event);
                }
            }
        }
        if submissions.values().any(|record| {
            record.receipt.outcome.is_none()
                && recovered_turn.as_ref() == Some(&record.receipt.turn_id)
        }) {
            recovered_turn = None;
        }
        let now = timestamp_ms();
        let mut metadata = saved
            .as_ref()
            .map_or_else(SessionMetadata::default, |s| s.metadata.clone());
        if metadata.created_at_ms == 0 || fork {
            metadata.created_at_ms = now;
        }
        metadata.updated_at_ms = now;
        if fork {
            metadata.archived = false;
            metadata.fork_origin = fork_origin;
        }
        let mut queue = saved
            .as_ref()
            .filter(|_| !fork)
            .map_or_else(PromptQueue::default, |s| s.queue.clone());
        // Resuming the daemon never executes old queued edits without a client decision.
        if !queue.items.is_empty() {
            queue.paused = true;
        }
        let execution = Arc::new(std::sync::Mutex::new(Some(options.execution.clone())));
        let mut snapshot = SessionSnapshot {
            id: session_id,
            working_context: Some(context),
            execution_policy,
            native: connection.native,
            options: sanitized_options(options),
            capabilities: connection.capabilities,
            state: SessionState::Idle,
            sequence: last_sequence,
            turn: None,
            turns_completed,
            metadata,
            queue,
        };
        if let Some(tools) = &host_tools {
            tools.observe(&snapshot);
        }
        let mut initial_events = Vec::new();
        if before_user {
            let query = NativeHistoryQuery {
                harness: snapshot.options.harness.clone(),
                cwd: Some(snapshot.options.cwd.clone()),
                config_home: snapshot.options.execution.config_home.clone(),
                limit: 1000,
            };
            let document = match h.read_native_history(&query, &snapshot.native.id).await {
                Ok(document) => document,
                Err(error) => {
                    let _ = connection.backend.kill().await;
                    return Err(error);
                }
            };
            retained.clear();
            for record in document.records {
                snapshot.sequence += 1;
                if matches!(record.kind, EventKind::TurnEnded { .. }) {
                    snapshot.turns_completed += 1;
                }
                let event = RuntimeEvent {
                    session_id: snapshot.id.clone(),
                    sequence: snapshot.sequence,
                    turn_id: record.turn_id,
                    source: snapshot.options.harness.clone(),
                    timestamp_ms: document.session.updated_at_ms,
                    kind: record.kind,
                    raw: record.raw,
                };
                initial_events.push(event.clone());
                if retained.len() == 4096 {
                    retained.pop_front();
                }
                retained.push_back(event);
            }
        } else if fork && self.transcript_durability().is_some() {
            // A fork has its own journal while retaining the conversation shown to the user.
            let parent = saved.as_ref().expect("fork requires a saved source");
            retained.clear();
            let mut after = 0;
            'inherit: loop {
                let page = self.0.store.events(&parent.id, after, 4096).await?;
                if page.is_empty() {
                    break;
                }
                for mut event in page {
                    after = event.sequence;
                    if event.sequence > snapshot.sequence {
                        break 'inherit;
                    }
                    event.session_id = snapshot.id.clone();
                    if let EventKind::SessionStarted(started) = &mut event.kind {
                        started.id = snapshot.id.clone();
                    }
                    initial_events.push(event.clone());
                    if retained.len() == 4096 {
                        retained.pop_front();
                    }
                    retained.push_back(event);
                }
            }
            for event in &mut retained {
                event.session_id = snapshot.id.clone();
            }
        }
        if submissions.values().any(SubmissionRecord::blocks_dispatch) {
            snapshot.queue.paused = true;
            snapshot.state = SessionState::NeedsInput;
        }
        if let Err(error) = self
            .0
            .store
            .commit(SessionWrite {
                snapshot: Some(&snapshot),
                events: &initial_events,
                ..Default::default()
            })
            .await
        {
            let _ = connection.backend.kill().await;
            return Err(error);
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(2048);
        let shared = Arc::new(RwLock::new(snapshot.clone()));
        let history = Arc::new(RwLock::new(retained));
        let closed = Arc::new(AtomicBool::new(false));
        let inner = Arc::new(AgentInner {
            execution: execution.clone(),
            closed: closed.clone(),
            commands: tx,
            events: events.clone(),
            snapshot: shared.clone(),
            history: history.clone(),
        });
        self.0
            .agents
            .write()
            .await
            .insert(snapshot.id.clone(), Arc::downgrade(&inner));
        for ext in &self.0.extensions {
            ext.after_spawn(&snapshot).await;
        }
        let engine = Engine {
            host_tools,
            submissions,
            execution,
            backend: Arc::from(connection.backend),
            input: connection.events,
            commands: rx,
            snapshot,
            shared,
            events,
            global: self.0.events.clone(),
            history,
            store: self.0.store.clone(),
            extensions: self.0.extensions.clone(),
            permissions: Default::default(),
            closed,
            inflight_queue: None,
            storage_error: None,
            recovered_turn,
        };
        tokio::spawn(engine.run());
        Ok(Agent(inner))
    }
    pub async fn agent(&self, id: &str) -> Option<Agent> {
        let a = self
            .0
            .agents
            .read()
            .await
            .get(id)
            .and_then(Weak::upgrade)
            .map(Agent)?;
        if matches!(
            a.snapshot().await.state,
            SessionState::Closed | SessionState::Failed
        ) {
            None
        } else {
            Some(a)
        }
    }
    pub async fn sessions(&self) -> Result<Vec<SessionSnapshot>> {
        self.0.store.list().await
    }
    async fn fork_boundary(
        &self,
        source: &SessionSnapshot,
        message_id: &str,
        current_completed: u64,
    ) -> Result<(ForkOrigin, u64)> {
        if self.transcript_durability().is_none() {
            return Err(Error::Unsupported(
                "message fork requires a retained transcript".into(),
            ));
        }
        let mut after = 0;
        let mut matches = std::collections::BTreeSet::new();
        let mut anchor_aliases = std::collections::BTreeSet::from([message_id.to_string()]);
        let mut last_messages: BTreeMap<String, std::collections::BTreeSet<String>> =
            BTreeMap::new();
        let mut terminals: Vec<(String, u64)> = vec![];
        let mut changes: Vec<(u64, i128)> = vec![];
        let mut conversation_turns: Vec<String> = vec![];
        loop {
            let page = self.0.store.events(&source.id, after, 4096).await?;
            if page.is_empty() {
                break;
            }
            for event in page {
                after = event.sequence;
                let message_matches = match &event.kind {
                    EventKind::TextDelta { item_id, .. } => item_id.as_deref() == Some(message_id),
                    EventKind::AssistantMessage(value) => ["id", "uuid", "messageId"]
                        .iter()
                        .any(|key| value[*key].as_str() == Some(message_id)),
                    _ => false,
                };
                let aliases = match &event.kind {
                    EventKind::TextDelta {
                        item_id: Some(id), ..
                    } => std::collections::BTreeSet::from([id.clone()]),
                    EventKind::AssistantMessage(value) => ["id", "uuid", "messageId"]
                        .iter()
                        .filter_map(|key| value[*key].as_str().map(str::to_string))
                        .collect(),
                    _ => std::collections::BTreeSet::new(),
                };
                if !aliases.is_empty()
                    && let Some(turn) = &event.turn_id
                {
                    last_messages.insert(turn.clone(), aliases.clone());
                }
                if message_matches {
                    anchor_aliases.extend(aliases);
                    matches.insert(event.turn_id.clone().ok_or_else(|| {
                        Error::Invalid("message anchor has no runtime turn".into())
                    })?);
                }
                match &event.kind {
                    EventKind::TurnStarted => {
                        if let Some(turn) = &event.turn_id {
                            conversation_turns.push(turn.clone());
                        }
                    }
                    EventKind::TurnEnded { .. } => {
                        if let Some(turn) = &event.turn_id {
                            terminals.push((turn.clone(), event.sequence));
                            changes.push((event.sequence, 1));
                        }
                    }
                    EventKind::ConversationRolledBack { turns } => {
                        changes.push((event.sequence, -i128::from(*turns)));
                        conversation_turns
                            .truncate(conversation_turns.len().saturating_sub(*turns as usize));
                    }
                    _ => (),
                }
            }
        }
        if matches.len() != 1 {
            return Err(Error::Invalid(
                "message anchor is unknown or ambiguous".into(),
            ));
        }
        let turn = matches.pop_first().expect("one match");
        if last_messages
            .get(&turn)
            .is_some_and(|aliases| aliases.is_disjoint(&anchor_aliases))
        {
            return Err(Error::Unsupported(
                "fork anchor must be the final assistant message of its turn".into(),
            ));
        }
        if !conversation_turns.contains(&turn) {
            return Err(Error::Invalid("message anchor was rolled back".into()));
        }
        let terminal = terminals
            .iter()
            .filter(|(id, _)| *id == turn)
            .collect::<Vec<_>>();
        if terminal.len() != 1 {
            return Err(Error::Invalid(
                "message anchor has no unique terminal turn".into(),
            ));
        }
        let sequence = terminal[0].1;
        let mut completed = i128::from(current_completed);
        for (_, change) in changes.into_iter().rev().filter(|(at, _)| *at > sequence) {
            completed -= change;
        }
        let completed = u64::try_from(completed)
            .map_err(|_| Error::Invalid("message anchor has inconsistent turn history".into()))?;
        Ok((
            ForkOrigin {
                session_id: source.id.clone(),
                message_id: Some(message_id.into()),
                sequence,
                turn_id: Some(turn),
            },
            completed,
        ))
    }
    pub async fn shutdown(&self) -> Result<()> {
        for extension in &self.0.extensions {
            extension.shutdown().await;
        }
        let agents: Vec<_> = self
            .0
            .agents
            .read()
            .await
            .values()
            .filter_map(Weak::upgrade)
            .map(Agent)
            .collect();
        let mut error = None;
        for a in agents {
            if let Err(e) = a.close().await
                && !matches!(e, Error::Closed)
            {
                error = Some(e);
            }
        }
        error.map_or(Ok(()), Err)
    }
}

fn sanitized_options(mut options: SpawnOptions) -> SpawnOptions {
    options.execution.env.clear();
    for server in &mut options.execution.mcp_servers {
        match &mut server.transport {
            McpTransport::Stdio { env, .. } => env.clear(),
            McpTransport::Http { headers, .. } | McpTransport::Sse { headers, .. } => {
                headers.clear()
            }
        }
    }
    options
}
fn validate_execution(
    execution: &SessionExecutionOptions,
    capabilities: &Capabilities,
) -> Result<()> {
    if execution.tools == ToolPolicy::Disabled && !capabilities.tools_disabled.is_available() {
        return Err(Error::Unsupported("launch-time tool disabling".into()));
    }
    if execution.output_schema.is_some() && !capabilities.output_schema.is_available() {
        return Err(Error::Unsupported("output schema".into()));
    }
    if execution.ephemeral && !capabilities.ephemeral.is_available() {
        return Err(Error::Unsupported("ephemeral native session".into()));
    }
    let mut names = std::collections::BTreeSet::new();
    for server in &execution.mcp_servers {
        if server.name.trim().is_empty() || !names.insert(&server.name) {
            return Err(Error::Invalid(
                "MCP server names must be nonempty and unique".into(),
            ));
        }
        let transport = match &server.transport {
            McpTransport::Stdio { command, .. } => {
                if command.as_os_str().is_empty() {
                    return Err(Error::Invalid("MCP command must not be empty".into()));
                }
                "stdio"
            }
            McpTransport::Http { url, .. } => {
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return Err(Error::Invalid("MCP URL must use HTTP or HTTPS".into()));
                }
                "http"
            }
            McpTransport::Sse { url, .. } => {
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return Err(Error::Invalid("MCP URL must use HTTP or HTTPS".into()));
                }
                "sse"
            }
        };
        if !capabilities
            .metadata
            .get("mcpTransports")
            .and_then(Value::as_array)
            .is_some_and(|transports| {
                transports
                    .iter()
                    .any(|value| value.as_str() == Some(transport))
            })
        {
            return Err(Error::Unsupported(format!("MCP transport {transport}")));
        }
    }
    if execution
        .output_schema
        .as_ref()
        .is_some_and(|schema| !schema.is_object() && !schema.is_boolean())
    {
        return Err(Error::Invalid(
            "outputSchema must be a JSON Schema object or boolean".into(),
        ));
    }
    if execution
        .env
        .keys()
        .any(|key| key.is_empty() || key.contains('=') || key.contains('\0'))
        || execution.env.values().any(|value| value.contains('\0'))
    {
        return Err(Error::Invalid(
            "environment contains an invalid key or value".into(),
        ));
    }
    Ok(())
}

pub(crate) fn timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub(crate) fn apply_metadata(
    snapshot: &mut SessionSnapshot,
    update: SessionMetadataUpdate,
) -> Result<()> {
    if let Some(title) = update.title {
        let title = title.trim();
        if title.is_empty() || title.chars().count() > 256 {
            return Err(Error::Invalid(
                "title must contain between 1 and 256 characters".into(),
            ));
        }
        snapshot.metadata.title = Some(title.into());
    }
    if let Some(archived) = update.archived {
        snapshot.metadata.archived = archived;
    }
    snapshot.metadata.updated_at_ms = timestamp_ms();
    Ok(())
}

fn compile_launch_profile(harness: &dyn Harness, options: &mut SpawnOptions) -> Result<()> {
    if let Some(profile) = requested_execution_profile(&options.config)? {
        let plan = harness.compile_execution_profile(options, profile)?;
        let native = plan
            .native
            .as_object()
            .ok_or_else(|| Error::Provider("execution policy plan must be an object".into()))?;
        let config = options
            .config
            .as_object_mut()
            .ok_or_else(|| Error::Invalid("configuration must be an object".into()))?;
        config.extend(native.clone());
    }
    Ok(())
}
fn native_launch_options(options: &SpawnOptions) -> SpawnOptions {
    let mut options = options.clone();
    if let Some(config) = options.config.as_object_mut() {
        config.remove(EXECUTION_PROFILE_SETTING);
    }
    options
}
async fn launch_policy(
    backend: &dyn AgentBackend,
    options: &SpawnOptions,
    saved: Option<&ExecutionPolicy>,
) -> Result<ExecutionPolicy> {
    let requested = requested_execution_profile(&options.config)?;
    let mut policy = ExecutionPolicy {
        requested,
        revision: saved.map_or(u64::from(requested.is_some()), |saved| saved.revision),
        ..Default::default()
    };
    if let Some(profile) = requested {
        let plan = backend.compile_execution_profile(profile).await?;
        backend.configure(plan.native).await?;
    }
    let report = backend.execution_policy().await?;
    if saved.is_some_and(|saved| saved.dispatch_blocked) {
        let reconciled = report.observed.as_ref().is_some_and(|observed| {
            observed.evidence == PolicyEvidence::NativeReceipt
                && report.status == PolicyStatus::Effective
                && requested.is_some()
                && observed.profile == requested
        });
        if !reconciled {
            policy.dispatch_blocked = true;
            policy.status = PolicyStatus::Unknown;
            policy.reason =
                Some("Retained policy uncertainty requires an explicit supported selection".into());
        }
    }
    policy.observe(report);
    if requested.is_some()
        && matches!(
            policy.status,
            PolicyStatus::Unknown | PolicyStatus::Partial | PolicyStatus::Rejected
        )
    {
        policy.dispatch_blocked = true;
        policy
            .reason
            .get_or_insert_with(|| "Native policy application has not been confirmed".into());
    }
    Ok(policy)
}
