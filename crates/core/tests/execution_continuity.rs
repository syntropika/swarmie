use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use swarmie::*;
use tokio::sync::{Mutex, mpsc};

#[derive(Clone, Default)]
struct Store(Arc<MemoryStore>);
#[async_trait]
impl SessionStore for Store {
    async fn local_catalog(&self) -> Result<LocalCatalog> {
        self.0.local_catalog().await
    }
    async fn update_catalog(&self, update: CatalogUpdate) -> Result<CatalogRecord> {
        self.0.update_catalog(update).await
    }
    async fn prepare_context(
        &self,
        session_id: &str,
        binding_id: &str,
        selection: ContextSelection,
    ) -> Result<WorkingContext> {
        self.0
            .prepare_context(session_id, binding_id, selection)
            .await
    }
    async fn working_context(&self, id: &str) -> Result<Option<WorkingContext>> {
        self.0.working_context(id).await
    }

    async fn commit(&self, write: SessionWrite<'_>) -> Result<()> {
        self.0.commit(write).await
    }
    fn transcript_durability(&self) -> Option<&'static str> {
        Some("memory")
    }
    async fn save_submission(&self, record: &SubmissionRecord) -> Result<()> {
        self.0.save_submission(record).await
    }
    async fn submissions(&self, id: &str) -> Result<Vec<SubmissionRecord>> {
        self.0.submissions(id).await
    }
    async fn save(&self, s: &SessionSnapshot) -> Result<()> {
        self.0.save(s).await
    }
    async fn load(&self, id: &str) -> Result<Option<SessionSnapshot>> {
        self.0.load(id).await
    }
    async fn list(&self) -> Result<Vec<SessionSnapshot>> {
        self.0.list().await
    }
    async fn append_event(&self, e: &RuntimeEvent) -> Result<()> {
        self.0.append_event(e).await
    }
    async fn events(&self, id: &str, after: u64, limit: usize) -> Result<Vec<RuntimeEvent>> {
        self.0.events(id, after, limit).await
    }
}
#[derive(Clone)]
struct Launch {
    execution: SessionExecutionOptions,
    fork: bool,
    anchor: Option<String>,
}
#[derive(Clone, Default)]
struct NativeHarness {
    launches: Arc<Mutex<Vec<Launch>>>,
    transcripts: Arc<Mutex<BTreeMap<String, Vec<String>>>>,
    rewrites_ids: bool,
}
fn capabilities() -> Capabilities {
    Capabilities {
        ephemeral: true.into(),
        output_schema: true.into(),
        tools_disabled: true.into(),
        fork_at_message: true.into(),

        resume: true.into(),
        fork: true.into(),
        metadata: BTreeMap::from([("mcpTransports".into(), json!(["stdio", "http", "sse"]))]),
        ..Default::default()
    }
}
impl NativeHarness {
    async fn launch(
        &self,
        options: &SpawnOptions,
        saved: Option<&NativeSessionRef>,
        fork: bool,
        anchor: Option<&str>,
    ) -> Result<BackendConnection> {
        self.launches.lock().await.push(Launch {
            execution: options.execution.clone(),
            fork,
            anchor: anchor.map(String::from),
        });
        if !options.execution.env.contains_key("SESSION_TOKEN") {
            return Err(Error::Provider(
                "private launch credentials must be supplied anew".into(),
            ));
        }
        let mut transcripts = self.transcripts.lock().await;
        let mut transcript = if let Some(native) = saved {
            transcripts
                .get(&native.id)
                .cloned()
                .ok_or_else(|| Error::Provider("native transcript missing".into()))?
        } else {
            vec![]
        };
        if let Some(anchor) = anchor {
            let cut = transcript
                .iter()
                .position(|id| id == anchor)
                .ok_or_else(|| Error::Provider("native anchor missing".into()))?;
            transcript.truncate(cut + 1);
        }
        let id = match saved {
            Some(saved) if !fork => saved.id.clone(),
            _ => uuid::Uuid::new_v4().to_string(),
        };
        let mut caps = capabilities();
        if fork && self.rewrites_ids {
            let mapping: BTreeMap<String, String> = transcript
                .iter()
                .map(|source| (source.clone(), uuid::Uuid::new_v4().to_string()))
                .collect();
            let mut aliases = mapping.clone();
            for (canonical, source) in options.config["_runtimeMessageIdAliases"]
                .as_object()
                .into_iter()
                .flatten()
            {
                if let Some(child) = source.as_str().and_then(|source| mapping.get(source)) {
                    aliases.insert(canonical.clone(), child.clone());
                }
            }
            transcript = transcript
                .iter()
                .map(|source| mapping[source].clone())
                .collect();
            caps.metadata
                .insert("messageIdAliases".into(), json!(aliases));
        } else if options.config["_runtimeMessageIdAliases"].is_object() {
            caps.metadata.insert(
                "messageIdAliases".into(),
                options.config["_runtimeMessageIdAliases"].clone(),
            );
        }
        transcripts.insert(id.clone(), transcript);
        let (tx, events) = mpsc::channel(16);
        Ok(BackendConnection {
            native: NativeSessionRef {
                provider: self.id().into(),
                id: id.clone(),
            },
            capabilities: caps,
            events,
            backend: Box::new(NativeBackend {
                tx,
                id,
                transcripts: self.transcripts.clone(),
            }),
        })
    }
}
#[async_trait]
impl Harness for NativeHarness {
    fn id(&self) -> &str {
        "credential-peer"
    }
    async fn detect(&self) -> Result<Detection> {
        Ok(Detection {
            harness: self.id().into(),
            available: true,
            executable: None,
            capabilities: capabilities(),
        })
    }
    async fn connect(
        &self,
        o: &SpawnOptions,
        saved: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection> {
        self.launch(o, saved, fork, None).await
    }
    async fn fork_at(
        &self,
        o: &SpawnOptions,
        native: &NativeSessionRef,
        anchor: &str,
    ) -> Result<BackendConnection> {
        self.launch(o, Some(native), true, Some(anchor)).await
    }
}
struct NativeBackend {
    tx: mpsc::Sender<BackendEvent>,
    id: String,
    transcripts: Arc<Mutex<BTreeMap<String, Vec<String>>>>,
}
#[async_trait]
impl AgentBackend for NativeBackend {
    async fn prompt(&self, p: &Prompt) -> Result<()> {
        if p.text()? == "crash" {
            self.tx
                .send(BackendEvent::Exited {
                    message: "native peer stopped".into(),
                })
                .await
                .map_err(|_| Error::Closed)?;
            return Ok(());
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.transcripts
            .lock()
            .await
            .get_mut(&self.id)
            .unwrap()
            .push(id.clone());
        self.tx
            .send(BackendEvent::content(
                EventKind::AssistantMessage(
                    json!({"id":id,"content":[{"type":"text","text":"authenticated native turn"}]}),
                ),
                json!({"native":true}),
            ))
            .await
            .map_err(|_| Error::Closed)?;
        self.tx
            .send(BackendEvent::TurnFinished {
                outcome: TurnOutcome::Completed,

                native_reason: None,
                raw: None,
            })
            .await
            .map_err(|_| Error::Closed)?;
        Ok(())
    }
    async fn cancel(&self) -> Result<()> {
        Ok(())
    }
    async fn respond_permission(&self, _: &PermissionResponse) -> Result<()> {
        Err(Error::Unsupported("permissions".into()))
    }
    async fn close(&self) -> Result<()> {
        Ok(())
    }
    async fn kill(&self) -> Result<()> {
        Ok(())
    }
}
fn execution(prefix: &str) -> SessionExecutionOptions {
    SessionExecutionOptions {
        env: BTreeMap::from([("SESSION_TOKEN".into(), format!("{prefix}-process-secret"))]),
        mcp_servers: vec![
            McpServer {
                name: "stdio".into(),
                transport: McpTransport::Stdio {
                    command: "tool-server".into(),
                    args: vec!["serve".into()],
                    env: BTreeMap::from([("TOOL_TOKEN".into(), format!("{prefix}-stdio-secret"))]),
                },
            },
            McpServer {
                name: "http".into(),
                transport: McpTransport::Http {
                    url: "https://example.test/mcp".into(),
                    headers: BTreeMap::from([(
                        "Authorization".into(),
                        format!("Bearer {prefix}-http-secret"),
                    )]),
                },
            },
            McpServer {
                name: "sse".into(),
                transport: McpTransport::Sse {
                    url: "https://example.test/events".into(),
                    headers: BTreeMap::from([("X-Token".into(), format!("{prefix}-sse-secret"))]),
                },
            },
        ],
        ..Default::default()
    }
}
fn options(execution: SessionExecutionOptions) -> SpawnOptions {
    let mut options = SpawnOptions::new("credential-peer", std::env::current_dir().unwrap());
    options.execution = execution;
    options
}
fn assert_credentials(actual: &SessionExecutionOptions, expected: &SessionExecutionOptions) {
    assert_eq!(actual.env, expected.env);
    assert_eq!(actual.mcp_servers.len(), expected.mcp_servers.len());
    for (actual, expected) in actual.mcp_servers.iter().zip(&expected.mcp_servers) {
        assert_eq!(actual.name, expected.name);
        match (&actual.transport, &expected.transport) {
            (McpTransport::Stdio { env: a, .. }, McpTransport::Stdio { env: b, .. }) => {
                assert_eq!(a, b)
            }
            (McpTransport::Http { headers: a, .. }, McpTransport::Http { headers: b, .. })
            | (McpTransport::Sse { headers: a, .. }, McpTransport::Sse { headers: b, .. }) => {
                assert_eq!(a, b)
            }
            _ => panic!("transport changed"),
        }
    }
}
fn assert_sanitized(snapshot: &SessionSnapshot) {
    assert!(snapshot.options.execution.env.is_empty());
    for server in &snapshot.options.execution.mcp_servers {
        match &server.transport {
            McpTransport::Stdio { env, .. } => assert!(env.is_empty()),
            McpTransport::Http { headers, .. } | McpTransport::Sse { headers, .. } => {
                assert!(headers.is_empty())
            }
        }
    }
}
async fn assert_private(store: &Store) {
    for snapshot in store.list().await.unwrap() {
        assert_sanitized(&snapshot);
        let journal = store.events(&snapshot.id, 0, 4096).await.unwrap();
        for event in &journal {
            if let EventKind::SessionStarted(snapshot) = &event.kind {
                assert_sanitized(snapshot);
            }
        }
        let encoded = format!(
            "{}{}",
            serde_json::to_string(&snapshot).unwrap(),
            serde_json::to_string(&journal).unwrap()
        );
        for secret in [
            "-process-secret",
            "-stdio-secret",
            "-http-secret",
            "-sse-secret",
            "source-only-secret",
        ] {
            assert!(!encoded.contains(secret));
        }
    }
}
async fn turn(agent: &Agent) -> String {
    let mut events = agent.events();
    agent.prompt("turn").await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut anchor = None;
        loop {
            match events.next().await.unwrap().unwrap().kind {
                EventKind::AssistantMessage(message) => {
                    anchor = message["id"].as_str().map(String::from)
                }
                EventKind::TurnEnded { .. } => return anchor.unwrap(),
                _ => (),
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn live_fork_and_message_fork_inherit_private_process_and_mcp_credentials_without_public_leaks()
 {
    let store = Store::default();
    let harness = NativeHarness::default();
    let runtime = Runtime::builder()
        .store(store.clone())
        .harness(harness.clone())
        .build();
    let original = execution("original");
    let source = runtime.spawn(options(original.clone())).await.unwrap();
    let anchor = turn(&source).await;
    let id = source.snapshot().await.id;
    let fork = runtime.fork(&id).await.unwrap();
    let cut = runtime.fork_at(&id, &anchor).await.unwrap();
    let launches = harness.launches.lock().await.clone();
    assert_eq!(launches.len(), 3);
    for launch in &launches {
        assert_credentials(&launch.execution, &original);
    }
    assert!(launches[1].fork && launches[2].fork);
    assert_eq!(launches[2].anchor.as_deref(), Some(anchor.as_str()));
    turn(&fork).await;
    turn(&cut).await;
    source.close().await.unwrap();
    fork.close().await.unwrap();
    cut.close().await.unwrap();
    assert_private(&store).await;
}

#[tokio::test]
async fn rewritten_native_ids_do_not_break_canonical_message_anchors_across_fork_and_resume() {
    let store = Store::default();
    let harness = NativeHarness {
        rewrites_ids: true,
        ..Default::default()
    };
    let runtime = Runtime::builder()
        .store(store.clone())
        .harness(harness.clone())
        .build();
    let source = runtime.spawn(options(execution("original"))).await.unwrap();
    let anchor = turn(&source).await;
    turn(&source).await;
    let child = runtime.fork(&source.snapshot().await.id).await.unwrap();
    let child_snapshot = child.snapshot().await;
    let translated = child_snapshot.capabilities.metadata["messageIdAliases"][&anchor]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(translated, anchor);
    assert!(
        child_snapshot
            .options
            .config
            .get("_runtimeMessageIdAliases")
            .is_none()
    );
    let grandchild = runtime.fork_at(&child_snapshot.id, &anchor).await.unwrap();
    assert_eq!(grandchild.snapshot().await.turns_completed, 1);
    assert_eq!(
        harness
            .launches
            .lock()
            .await
            .last()
            .unwrap()
            .anchor
            .as_deref(),
        Some(translated.as_str())
    );
    let third = runtime
        .fork_at(&grandchild.snapshot().await.id, &anchor)
        .await
        .unwrap();
    assert_eq!(third.snapshot().await.turns_completed, 1);
    child.close().await.unwrap();
    let resumed = runtime
        .resume_with(&child_snapshot.id, execution("fresh"))
        .await
        .unwrap();
    let restored = runtime.fork_at(&child_snapshot.id, &anchor).await.unwrap();
    assert_eq!(restored.snapshot().await.turns_completed, 1);
    let transcript = runtime
        .transcript(&restored.snapshot().await.id, 0, 4096)
        .await
        .unwrap();
    assert!(
        transcript.iter().any(
            |e| matches!(&e.kind,EventKind::AssistantMessage(message) if message["id"]==anchor)
        )
    );
    for agent in [source, grandchild, third, resumed, restored] {
        agent.close().await.unwrap();
    }
    assert_private(&store).await;
}

#[tokio::test]
async fn explicit_live_fork_overrides_replace_credentials_and_cascade_without_mutating_source() {
    let store = Store::default();
    let harness = NativeHarness::default();
    let runtime = Runtime::builder()
        .store(store.clone())
        .harness(harness.clone())
        .build();
    let mut original = execution("original");
    original
        .env
        .insert("SOURCE_ONLY".into(), "source-only-secret".into());
    let source = runtime.spawn(options(original.clone())).await.unwrap();
    let anchor = turn(&source).await;
    let id = source.snapshot().await.id;
    let replacement = execution("replacement");
    let child = runtime.fork_with(&id, replacement.clone()).await.unwrap();
    let child_id = child.snapshot().await.id;
    let cascade = runtime.fork(&child_id).await.unwrap();
    let at = runtime
        .fork_at_with(&id, &anchor, replacement.clone())
        .await
        .unwrap();
    let unchanged = runtime.fork(&id).await.unwrap();
    let launches = harness.launches.lock().await.clone();
    for launch in &launches[1..4] {
        assert_credentials(&launch.execution, &replacement);
        assert!(!launch.execution.env.contains_key("SOURCE_ONLY"));
    }
    assert_credentials(&launches[4].execution, &original);
    for agent in [source, child, cascade, at, unchanged] {
        agent.close().await.unwrap();
    }
    assert_private(&store).await;
}

#[tokio::test]
async fn closed_restarted_and_resumed_sources_require_explicit_fresh_credentials() {
    let store = Store::default();
    let harness = NativeHarness::default();
    let runtime = Runtime::builder()
        .store(store.clone())
        .harness(harness.clone())
        .build();
    let source = runtime.spawn(options(execution("original"))).await.unwrap();
    let anchor = turn(&source).await;
    let id = source.snapshot().await.id;
    source.close().await.unwrap();
    assert!(matches!(runtime.fork(&id).await, Err(Error::Provider(_))));
    assert!(matches!(
        runtime.fork_at(&id, &anchor).await,
        Err(Error::Provider(_))
    ));
    let fresh = execution("fresh");
    let closed_fork = runtime.fork_with(&id, fresh.clone()).await.unwrap();
    closed_fork.close().await.unwrap();
    let restarted = Runtime::builder()
        .store(store.clone())
        .harness(harness.clone())
        .build();
    assert!(matches!(restarted.fork(&id).await, Err(Error::Provider(_))));
    let cut = restarted
        .fork_at_with(&id, &anchor, fresh.clone())
        .await
        .unwrap();
    cut.close().await.unwrap();
    let resumed = restarted.resume_with(&id, fresh.clone()).await.unwrap();
    let child = restarted.fork(&id).await.unwrap();
    let launches = harness.launches.lock().await.clone();
    assert_credentials(&launches.last().unwrap().execution, &fresh);
    resumed.close().await.unwrap();
    child.close().await.unwrap();
    // Keeping the original closed Agent alive must not make its old credentials reusable.
    assert!(matches!(runtime.fork(&id).await, Err(Error::Provider(_))));
    assert_private(&store).await;
}

#[tokio::test]
async fn failed_source_does_not_reuse_private_credentials_while_an_old_handle_remains() {
    let store = Store::default();
    let harness = NativeHarness::default();
    let runtime = Runtime::builder()
        .store(store.clone())
        .harness(harness)
        .build();
    let source = runtime.spawn(options(execution("original"))).await.unwrap();
    let id = source.snapshot().await.id;
    let mut events = source.events();
    source.prompt("crash").await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !matches!(
            events.next().await.unwrap().unwrap().kind,
            EventKind::SessionEnded
        ) {}
    })
    .await
    .unwrap();
    assert!(runtime.agent(&id).await.is_none());
    assert!(matches!(runtime.fork(&id).await, Err(Error::Provider(_))));
    let replacement = runtime
        .fork_with(&id, execution("replacement"))
        .await
        .unwrap();
    replacement.close().await.unwrap();
    assert_private(&store).await;
}
