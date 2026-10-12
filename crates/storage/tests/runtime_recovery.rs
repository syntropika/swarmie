use async_trait::async_trait;
use swarmie::*;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use storage::SqliteSessionStore;
use tokio::sync::{Mutex, mpsc};

#[derive(Default)]
struct Control {
    events: Mutex<Option<mpsc::Sender<BackendEvent>>>,
    delivered: Mutex<Vec<String>>,
    connected: Mutex<Vec<(Option<String>, bool)>>,
    cancels: AtomicUsize,
    fork_anchors: Mutex<Vec<String>>,
    executions: Mutex<Vec<SessionExecutionOptions>>,
    configuration: Mutex<Value>,
}
struct ControlledHarness(Arc<Control>);
struct ControlledBackend {
    control: Arc<Control>,
    events: mpsc::Sender<BackendEvent>,
}
#[async_trait]
impl Harness for ControlledHarness {
    fn id(&self) -> &str {
        "controlled"
    }
    async fn detect(&self) -> Result<Detection> {
        unreachable!()
    }
    async fn fork_at(
        &self,
        options: &SpawnOptions,
        native: &NativeSessionRef,
        message_id: &str,
    ) -> Result<BackendConnection> {
        self.0.fork_anchors.lock().await.push(message_id.into());
        self.connect(options, Some(native), true).await
    }
    async fn connect(
        &self,
        options: &SpawnOptions,
        native: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection> {
        self.0
            .executions
            .lock()
            .await
            .push(options.execution.clone());
        self.0
            .connected
            .lock()
            .await
            .push((native.map(|native| native.id.clone()), fork));
        *self.0.configuration.lock().await = options.config.clone();
        let (events, receiver) = mpsc::channel(32);
        *self.0.events.lock().await = Some(events.clone());
        Ok(BackendConnection {
            backend: Box::new(ControlledBackend {
                control: self.0.clone(),
                events,
            }),
            events: receiver,
            native: NativeSessionRef {
                provider: self.id().into(),
                id: if fork { "native-fork" } else { "native-source" }.into(),
            },
            capabilities: Capabilities {
                fork_at_message: true.into(),

                resume: true.into(),
                fork: true.into(),
                cancel: true.into(),
                rollback: true.into(),
                configure: true.into(),
                metadata: std::collections::BTreeMap::from([(
                    "mcpTransports".into(),
                    json!(["stdio", "http"]),
                )]),
                ..Default::default()
            },
        })
    }
}
#[async_trait]
impl AgentBackend for ControlledBackend {
    async fn prompt(&self, prompt: &Prompt) -> Result<()> {
        let text = prompt.text()?;
        self.control.delivered.lock().await.push(text.clone());
        self.events
            .send(BackendEvent::content(
                EventKind::TextDelta {
                    item_id: Some(format!("item-{text}")),
                    text,
                },
                json!({"providerDetail":"preserved"}),
            ))
            .await
            .unwrap();
        Ok(())
    }
    async fn cancel(&self) -> Result<()> {
        self.control.cancels.fetch_add(1, Ordering::SeqCst);
        self.events
            .send(BackendEvent::TurnFinished {
                outcome: TurnOutcome::Cancelled,

                native_reason: None,
                raw: None,
            })
            .await
            .unwrap();
        Ok(())
    }
    async fn respond_permission(&self, _: &PermissionResponse) -> Result<()> {
        unreachable!()
    }
    async fn configuration(&self) -> Result<Vec<SettingDescriptor>> {
        let state = self.control.configuration.lock().await;
        Ok([
            ("model", SettingType::String),
            ("thinkingLevel", SettingType::String),
            ("effort", SettingType::String),
            ("fast", SettingType::Boolean),
            ("rejectLater", SettingType::Boolean),
        ]
        .into_iter()
        .map(|(id, kind)| SettingDescriptor {
            id: id.into(),
            label: id.into(),
            kind,
            value: state.get(id).cloned(),
            choices: vec![],
            application: SettingApplication::NextTurn,
            description: None,
            metadata: Default::default(),
        })
        .collect())
    }
    async fn configure(&self, config: Value) -> Result<Value> {
        let mut state = self.control.configuration.lock().await;
        if !state.is_object() {
            *state = json!({});
        }
        if config["model"] == "native-reset" {
            *state = json!({"model":"native-reset","effort":"low","fast":false});
            return Ok(state.clone());
        }
        if config["rejectLater"] == true {
            state["model"] = json!("partially-accepted");
            state["thinkingLevel"] = json!("high");
            self.events
                .send(BackendEvent::content(
                    EventKind::ConfigurationUpdated(state.clone()),
                    json!({"nativeAck":true}),
                ))
                .await
                .unwrap();
            return Err(Error::Invalid("later configuration field rejected".into()));
        }
        for (id, value) in config.as_object().unwrap() {
            state[id] = value.clone();
        }
        Ok(state.clone())
    }
    async fn rollback(&self, _: u64) -> Result<()> {
        Ok(())
    }
    async fn close(&self) -> Result<()> {
        Ok(())
    }
    async fn kill(&self) -> Result<()> {
        Ok(())
    }
}
struct Prefix;
#[async_trait]
impl RuntimeExtension for Prefix {
    fn id(&self) -> &str {
        "prefix"
    }
    async fn before_prompt(&self, _: &SessionSnapshot, prompt: &mut Prompt) -> Result<()> {
        *prompt = format!("accepted: {}", prompt.text()?).as_str().into();
        Ok(())
    }
}
async fn runtime(root: &std::path::Path, control: Arc<Control>) -> Runtime {
    Runtime::builder()
        .store(SqliteSessionStore::new(root).await.unwrap())
        .harness(ControlledHarness(control))
        .extension(Prefix)
        .build()
}
async fn wait_until_idle(agent: &Agent) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if agent.snapshot().await.state == SessionState::Idle {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn native_resume_retains_transcript_metadata_settings_and_editable_queue() {
    let directory = tempfile::tempdir().unwrap();
    let control = Arc::new(Control::default());
    let first = runtime(directory.path(), control.clone()).await;
    let agent = first
        .spawn(SpawnOptions::new("controlled", directory.path()))
        .await
        .unwrap();
    let id = agent.snapshot().await.id;
    agent.pause_queue(true).await.unwrap();
    let queued = agent.enqueue("first draft").await.unwrap();
    agent
        .update_queued(&queued.id, "edited follow-up")
        .await
        .unwrap();
    let removed = agent.enqueue("discarded").await.unwrap();
    agent.remove_queued(&removed.id).await.unwrap();
    agent.prompt("active").await.unwrap();
    assert!(matches!(
        agent.prompt("ordinary prompt").await,
        Err(Error::Busy)
    ));
    assert!(matches!(
        agent.cancel_turn("stale-turn").await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(control.cancels.load(Ordering::SeqCst), 0);
    agent
        .cancel_turn(agent.snapshot().await.turn.unwrap())
        .await
        .unwrap();
    wait_until_idle(&agent).await;
    assert!(agent.snapshot().await.queue.paused);
    assert_eq!(control.delivered.lock().await.len(), 1);
    agent
        .configure(json!({"model":"chosen-model","thinkingLevel":"high"}))
        .await
        .unwrap();
    first
        .update_metadata(
            &id,
            SessionMetadataUpdate {
                title: Some("Durable task".into()),
                archived: Some(true),
            },
        )
        .await
        .unwrap();
    agent.close().await.unwrap();
    let before = first.transcript(&id, 0, 4096).await.unwrap();
    assert!(before.iter().any(|event| {
        event
            .raw
            .as_ref()
            .is_some_and(|raw| raw["providerDetail"] == "preserved")
    }));
    let old_sequence = before.last().unwrap().sequence;
    // Simulate a crash after journaling fragments but before the next snapshot checkpoint.
    let lagging_store = SqliteSessionStore::new(directory.path()).await.unwrap();
    let mut lagging_snapshot = lagging_store.load(&id).await.unwrap().unwrap();
    lagging_snapshot.sequence = 1;
    lagging_snapshot.turns_completed = 0;
    lagging_store.save(&lagging_snapshot).await.unwrap();
    drop(agent);
    drop(first);

    let second = runtime(directory.path(), control.clone()).await;
    let closed = second.snapshot(&id).await.unwrap();
    assert_eq!(closed.metadata.title.as_deref(), Some("Durable task"));
    assert!(closed.metadata.archived);
    assert_eq!(closed.options.model.as_deref(), Some("chosen-model"));
    assert_eq!(closed.options.config["thinkingLevel"], "high");
    assert_eq!(
        closed.queue.items[0].prompt.text().unwrap(),
        "edited follow-up"
    );
    let agent = second.resume(&id).await.unwrap();
    assert_eq!(agent.snapshot().await.id, id);
    assert!(agent.snapshot().await.sequence >= old_sequence);
    assert_eq!(
        control.connected.lock().await[1],
        (Some("native-source".into()), false)
    );
    assert!(agent.snapshot().await.queue.paused);
    agent.pause_queue(false).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while control.delivered.lock().await.len() < 2 {
            tokio::task::yield_now().await;
        }
        while agent.snapshot().await.queue.items.len() == 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        control.delivered.lock().await.as_slice(),
        ["accepted: active", "accepted: edited follow-up"]
    );
    control
        .events
        .lock()
        .await
        .as_ref()
        .unwrap()
        .send(BackendEvent::TurnFinished {
            outcome: TurnOutcome::Completed,

            native_reason: None,
            raw: None,
        })
        .await
        .unwrap();
    wait_until_idle(&agent).await;
    assert!(
        second
            .transcript(&id, old_sequence, 4096)
            .await
            .unwrap()
            .iter()
            .all(|event| event.sequence > old_sequence)
    );
    agent.rollback(1).await.unwrap();
    assert_eq!(agent.snapshot().await.turns_completed, 1);
    assert!(
        second
            .transcript(&id, old_sequence, 4096)
            .await
            .unwrap()
            .iter()
            .any(|event| matches!(event.kind, EventKind::ConversationRolledBack { turns: 1 }))
    );
    let fork = second.fork(&id).await.unwrap();
    let fork_id = fork.snapshot().await.id;
    assert_ne!(fork_id, id);
    assert!(!fork.snapshot().await.metadata.archived);
    let fork_events = second.transcript(&fork_id, 0, 4096).await.unwrap();
    assert!(fork_events.iter().all(|event| event.session_id == fork_id));
    assert!(fork_events.iter().any(|event| matches!(&event.kind, EventKind::UserMessage(prompt) if prompt.text().unwrap() == "accepted: active")));
    fork.close().await.unwrap();
    agent.close().await.unwrap();
}

#[tokio::test]
async fn abrupt_running_resume_closes_the_old_turn_once_and_preserves_its_identity() {
    let directory = tempfile::tempdir().unwrap();
    let store = SqliteSessionStore::new(directory.path()).await.unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let old_turn = "interrupted-turn";
    let (path, fingerprint) = observe_directory(directory.path(), false).await.unwrap();
    let CatalogRecord::Directory(resource) = store
        .update_catalog(CatalogUpdate::RegisterDirectory {
            path,
            fingerprint,
            ownership: DirectoryOwnership::User,
        })
        .await
        .unwrap()
    else {
        unreachable!()
    };
    let context = store
        .prepare_context(
            &id,
            &uuid::Uuid::new_v4().to_string(),
            ContextSelection::Directory {
                directory: resource.reference(),
                native_existing: false,
            },
        )
        .await
        .unwrap();
    let snapshot = SessionSnapshot {
        working_context: Some(context),
        execution_policy: Default::default(),
        id: id.clone(),
        native: NativeSessionRef {
            provider: "controlled".into(),
            id: "native-source".into(),
        },
        options: SpawnOptions::new("controlled", directory.path().canonicalize().unwrap()),
        capabilities: Capabilities {
            resume: true.into(),
            ..Default::default()
        },
        state: SessionState::Running,
        sequence: 2,
        turn: Some(old_turn.into()),
        turns_completed: 3,
        metadata: Default::default(),
        queue: Default::default(),
    };
    store.save(&snapshot).await.unwrap();
    for (sequence, kind) in [
        (1, EventKind::TurnStarted),
        (2, EventKind::UserMessage("unfinished".into())),
    ] {
        store
            .append_event(&RuntimeEvent {
                session_id: id.clone(),
                sequence,
                turn_id: Some(old_turn.into()),
                source: "controlled".into(),
                timestamp_ms: 1,
                kind,
                raw: None,
            })
            .await
            .unwrap();
    }
    let runtime = runtime(directory.path(), Arc::new(Control::default())).await;
    let agent = runtime.resume(&id).await.unwrap();
    agent.pause_queue(true).await.unwrap();
    let history = runtime.transcript(&id, 0, 4096).await.unwrap();
    let endings = history
        .iter()
        .filter(|event| {
            event.turn_id.as_deref() == Some(old_turn)
                && matches!(event.kind, EventKind::TurnEnded { .. })
        })
        .collect::<Vec<_>>();
    assert_eq!(endings.len(), 1);
    assert!(
        matches!(&endings[0].kind, EventKind::TurnEnded { outcome, .. } if outcome.as_str() == "interrupted")
    );
    assert_eq!(agent.snapshot().await.turns_completed, 4);
    assert_eq!(agent.snapshot().await.state, SessionState::Idle);
    agent.close().await.unwrap();
    let resumed = runtime.resume(&id).await.unwrap();
    resumed.pause_queue(true).await.unwrap();
    assert_eq!(
        runtime
            .transcript(&id, 0, 4096)
            .await
            .unwrap()
            .iter()
            .filter(|event| event.turn_id.as_deref() == Some(old_turn)
                && matches!(event.kind, EventKind::TurnEnded { .. }))
            .count(),
        1
    );
    assert_eq!(resumed.snapshot().await.turns_completed, 4);
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn anchored_native_fork_cuts_later_turns_and_rejects_unknown_anchors_before_native_call() {
    let directory = tempfile::tempdir().unwrap();
    let control = Arc::new(Control::default());
    let runtime = runtime(directory.path(), control.clone()).await;
    let agent = runtime
        .spawn(SpawnOptions::new("controlled", directory.path()))
        .await
        .unwrap();
    let parent_id = agent.snapshot().await.id;
    for prompt in ["first", "second"] {
        agent.prompt(prompt).await.unwrap();
        control
            .events
            .lock()
            .await
            .as_ref()
            .unwrap()
            .send(BackendEvent::TurnFinished {
                outcome: TurnOutcome::Completed,

                native_reason: None,
                raw: None,
            })
            .await
            .unwrap();
        wait_until_idle(&agent).await;
    }
    assert!(matches!(
        runtime.fork_at(&parent_id, "unknown-message").await,
        Err(Error::Invalid(_))
    ));
    assert!(control.fork_anchors.lock().await.is_empty());
    let child = runtime
        .fork_at(&parent_id, "item-accepted: first")
        .await
        .unwrap();
    child.pause_queue(true).await.unwrap();
    let child_snapshot = child.snapshot().await;
    assert_eq!(child_snapshot.turns_completed, 1);
    let origin = child_snapshot.metadata.fork_origin.as_ref().unwrap();
    assert_eq!(origin.session_id, parent_id);
    assert_eq!(origin.message_id.as_deref(), Some("item-accepted: first"));
    let history = runtime
        .transcript(&child_snapshot.id, 0, 4096)
        .await
        .unwrap();
    assert!(history.iter().any(|event| matches!(&event.kind, EventKind::TextDelta { text, .. } if text == "accepted: first")));
    assert!(!history.iter().any(|event| matches!(&event.kind, EventKind::TextDelta { text, .. } if text == "accepted: second")));
    assert_eq!(
        control.fork_anchors.lock().await.as_slice(),
        ["item-accepted: first"]
    );
    child.close().await.unwrap();
    agent.rollback(1).await.unwrap();
    assert!(matches!(
        runtime.fork_at(&parent_id, "item-accepted: second").await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(control.fork_anchors.lock().await.len(), 1);
    agent.close().await.unwrap();
}

#[tokio::test]
async fn successful_native_configuration_resets_are_persisted_before_returning() {
    let directory = tempfile::tempdir().unwrap();
    let control = Arc::new(Control::default());
    let runtime = runtime(directory.path(), control).await;
    let agent = runtime
        .spawn(SpawnOptions::new("controlled", directory.path()))
        .await
        .unwrap();
    agent
        .configure(json!({"effort":"high","fast":true}))
        .await
        .unwrap();
    agent
        .configure(json!({"model":"native-reset"}))
        .await
        .unwrap();
    let saved = agent.snapshot().await;
    assert_eq!(saved.options.model.as_deref(), Some("native-reset"));
    assert_eq!(saved.options.config["effort"], "low");
    assert_eq!(saved.options.config["fast"], false);
    agent.close().await.unwrap();
    let resumed = runtime.resume(&saved.id).await.unwrap();
    let resumed = resumed.snapshot().await;
    assert_eq!(resumed.options.config["effort"], "low");
    assert_eq!(resumed.options.config["fast"], false);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_configuration_ack_survives_a_later_configure_error_and_resume() {
    let directory = tempfile::tempdir().unwrap();
    let control = Arc::new(Control::default());
    let runtime = runtime(directory.path(), control).await;
    let agent = runtime
        .spawn(SpawnOptions::new("controlled", directory.path()))
        .await
        .unwrap();
    let id = agent.snapshot().await.id;
    assert!(matches!(
        runtime
            .resume_with(&id, SessionExecutionOptions::default())
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        agent.configure(json!({"rejectLater":true})).await,
        Err(Error::Invalid(_))
    ));
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while agent.snapshot().await.options.model.as_deref() != Some("partially-accepted") {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        agent.snapshot().await.options.config["thinkingLevel"],
        "high"
    );
    assert!(agent.snapshot().await.options.config["rejectLater"].is_null());
    agent.close().await.unwrap();
    let saved = SqliteSessionStore::new(directory.path())
        .await
        .unwrap()
        .load(&id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.options.model.as_deref(), Some("partially-accepted"));
    let resumed = runtime.resume(&id).await.unwrap();
    assert_eq!(
        resumed.snapshot().await.options.config["thinkingLevel"],
        "high"
    );
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn queued_turn_progresses_under_continuous_idle_provider_events() {
    let directory = tempfile::tempdir().unwrap();
    let control = Arc::new(Control::default());
    let runtime = runtime(directory.path(), control.clone()).await;
    let agent = runtime
        .spawn(SpawnOptions::new("controlled", directory.path()))
        .await
        .unwrap();
    agent.pause_queue(true).await.unwrap();
    agent.enqueue("follow-up").await.unwrap();
    let sender = control.events.lock().await.clone().unwrap();
    let flood = tokio::spawn(async move {
        loop {
            if sender
                .send(BackendEvent::content(
                    EventKind::Diagnostic(json!({"code":"heartbeat"})),
                    json!({"heartbeat":true}),
                ))
                .await
                .is_err()
            {
                break;
            }
        }
    });
    agent.pause_queue(false).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while control.delivered.lock().await.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        control.delivered.lock().await.as_slice(),
        ["accepted: follow-up"]
    );
    assert_ne!(agent.snapshot().await.state, SessionState::Failed);
    flood.abort();
    agent.close().await.unwrap();
}

#[tokio::test]
async fn legacy_rollback_never_accepts_implicit_file_rewind_and_intermediate_fork_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let control = Arc::new(Control::default());
    let runtime = runtime(directory.path(), control.clone()).await;
    let agent = runtime
        .spawn(SpawnOptions::new("controlled", directory.path()))
        .await
        .unwrap();
    agent.prompt("first-message").await.unwrap();
    let sender = control.events.lock().await.clone().unwrap();
    sender
        .send(BackendEvent::content(
            EventKind::TextDelta {
                item_id: Some("later-assistant-message".into()),
                text: "later".into(),
            },
            json!({}),
        ))
        .await
        .unwrap();
    sender
        .send(BackendEvent::TurnFinished {
            outcome: TurnOutcome::Completed,

            native_reason: None,
            raw: None,
        })
        .await
        .unwrap();
    wait_until_idle(&agent).await;
    let id = agent.snapshot().await.id;
    assert!(matches!(
        runtime.fork_at(&id, "item-accepted: first-message").await,
        Err(Error::Unsupported(_))
    ));
    assert!(control.fork_anchors.lock().await.is_empty());
    assert!(matches!(
        agent
            .rollback_with(1, RollbackScope::ConversationAndFiles)
            .await,
        Err(Error::Unsupported(_))
    ));
    assert_eq!(agent.snapshot().await.turns_completed, 1);
    assert!(
        !runtime
            .transcript(&id, 0, 4096)
            .await
            .unwrap()
            .iter()
            .any(|event| matches!(event.kind, EventKind::ConversationRolledBack { .. }))
    );
    agent.rollback(1).await.unwrap();
    assert_eq!(agent.snapshot().await.turns_completed, 0);
    agent.close().await.unwrap();
}

#[tokio::test]
async fn execution_credentials_reach_native_connect_but_require_explicit_resupply_on_resume() {
    let directory = tempfile::tempdir().unwrap();
    let control = Arc::new(Control::default());
    let runtime = runtime(directory.path(), control.clone()).await;
    let mut options = SpawnOptions::new("controlled", directory.path());
    options
        .execution
        .env
        .insert("TEST_KEY".into(), "process-secret".into());
    options.execution.mcp_servers = vec![
        McpServer {
            name: "stdio-test".into(),
            transport: McpTransport::Stdio {
                command: "fixture-mcp".into(),
                args: vec![],
                env: std::collections::BTreeMap::from([("TOKEN".into(), "stdio-secret".into())]),
            },
        },
        McpServer {
            name: "http-test".into(),
            transport: McpTransport::Http {
                url: "http://127.0.0.1:1234/mcp".into(),
                headers: std::collections::BTreeMap::from([(
                    "Authorization".into(),
                    "header-secret".into(),
                )]),
            },
        },
    ];
    let credentials = options.execution.clone();
    let agent = runtime.spawn(options).await.unwrap();
    let id = agent.snapshot().await.id;
    assert_eq!(
        control.executions.lock().await[0].env["TEST_KEY"],
        "process-secret"
    );
    agent.close().await.unwrap();
    let store = SqliteSessionStore::new(directory.path()).await.unwrap();
    let retained = serde_json::json!({
        "snapshot": store.load(&id).await.unwrap(),
        "events": store.events(&id, 0, 4096).await.unwrap(),
        "submissions": store.submissions(&id).await.unwrap(),
    })
    .to_string();
    for secret in ["process-secret", "stdio-secret", "header-secret"] {
        assert!(!retained.contains(secret));
        for path in [
            store.database_path().to_path_buf(),
            store.database_path().with_file_name("host.sqlite-wal"),
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                assert!(
                    !bytes
                        .windows(secret.len())
                        .any(|window| window == secret.as_bytes())
                );
            }
        }
    }
    let resumed = runtime.resume_with(&id, credentials).await.unwrap();
    resumed.pause_queue(true).await.unwrap();
    assert_eq!(
        control.executions.lock().await[1].env["TEST_KEY"],
        "process-secret"
    );
    assert!(resumed.snapshot().await.options.execution.env.is_empty());
    match &resumed.snapshot().await.options.execution.mcp_servers[1].transport {
        McpTransport::Http { headers, .. } => assert!(headers.is_empty()),
        _ => panic!("expected HTTP transport"),
    }
    resumed.close().await.unwrap();
    let ordinary = runtime.resume(&id).await.unwrap();
    assert!(control.executions.lock().await[2].env.is_empty());
    ordinary.close().await.unwrap();
}

#[tokio::test]
async fn retained_delivery_ledger_recovers_without_dispatch_or_native_read_side_effects() {
    for delivery in [
        DeliveryKnowledge::NotSent,
        DeliveryKnowledge::InFlight,
        DeliveryKnowledge::Accepted,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let control = Arc::new(Control::default());
        let first = runtime(directory.path(), control.clone()).await;
        let agent = first
            .spawn(SpawnOptions::new("controlled", directory.path()))
            .await
            .unwrap();
        agent.pause_queue(true).await.unwrap();
        agent.enqueue("pending input").await.unwrap();
        let id = agent.snapshot().await.id;
        agent.close().await.unwrap();
        let store = SqliteSessionStore::new(directory.path()).await.unwrap();
        let record = SubmissionRecord {
            receipt: SubmissionReceipt {
                id: "retained-submission".into(),
                session_id: id.clone(),
                idempotency_key: "original-key".into(),
                turn_id: "original-turn".into(),
                delivery: delivery.clone(),
                recovery_required: delivery != DeliveryKnowledge::Accepted,
                outcome: None,
                error: None,
            },
            authored: "original input".into(),
            projected: "accepted: original input".into(),
            terminal_native_reason: None,
            terminal_raw: None,
            terminal_sequence: None,
            recovered_messages: vec![],
            reconciliation_raw: None,
        };
        store.save_submission(&record).await.unwrap();
        drop(store);
        let second = runtime(directory.path(), control.clone()).await;
        let before = control.connected.lock().await.len();
        let receipt = second
            .submission(&id, "original-key")
            .await
            .unwrap()
            .unwrap();
        assert!(receipt.recovery_required);
        assert_eq!(
            receipt.delivery,
            if delivery == DeliveryKnowledge::InFlight {
                DeliveryKnowledge::Unknown
            } else {
                delivery.clone()
            }
        );
        assert_eq!(control.connected.lock().await.len(), before);
        let duplicate = second
            .submit(&id, "original-key", "original input".into())
            .await
            .unwrap();
        assert_eq!(duplicate.id, receipt.id);
        assert!(matches!(
            second
                .submit(&id, "original-key", "different input".into())
                .await,
            Err(Error::IdempotencyConflict)
        ));
        assert!(
            second
                .submission(&id, "never-admitted")
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(control.connected.lock().await.len(), before);
        let resumed = second.resume(&id).await.unwrap();
        assert!(resumed.snapshot().await.queue.paused);
        assert_eq!(resumed.snapshot().await.queue.items.len(), 1);
        if delivery == DeliveryKnowledge::NotSent {
            assert_eq!(resumed.snapshot().await.state, SessionState::Idle);
            let receipt = resumed.submission("original-key").await.unwrap().unwrap();
            assert_eq!(receipt.delivery, DeliveryKnowledge::Rejected);
            assert!(!receipt.recovery_required);
        } else {
            assert_eq!(resumed.snapshot().await.state, SessionState::NeedsInput);
            assert!(matches!(resumed.pause_queue(false).await, Err(Error::Busy)));
            assert!(matches!(
                resumed.submit("next-key", "new work").await,
                Err(Error::Busy)
            ));
        }
        assert!(control.delivered.lock().await.is_empty());
        resumed.close().await.unwrap();
        // Delivery records remain separate from the session listing.
        assert_eq!(
            SqliteSessionStore::new(directory.path())
                .await
                .unwrap()
                .list()
                .await
                .unwrap()
                .len(),
            1
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file = directory.path().join("state/host.sqlite");
            assert_eq!(
                std::fs::metadata(file).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}

#[tokio::test]
async fn durable_native_completion_repairs_missing_journal_events_once() {
    let directory = tempfile::tempdir().unwrap();
    let control = Arc::new(Control::default());
    let first = runtime(directory.path(), control.clone()).await;
    let agent = first
        .spawn(SpawnOptions::new("controlled", directory.path()))
        .await
        .unwrap();
    let id = agent.snapshot().await.id;
    agent.close().await.unwrap();
    let store = SqliteSessionStore::new(directory.path()).await.unwrap();
    let mut snapshot = store.load(&id).await.unwrap().unwrap();
    snapshot.turn = Some("native-completion-turn".into());
    snapshot.state = SessionState::Running;
    store.save(&snapshot).await.unwrap();
    store
        .save_submission(&SubmissionRecord {
            receipt: SubmissionReceipt {
                id: "native-completion-submission".into(),
                session_id: id.clone(),
                idempotency_key: "completed-key".into(),
                turn_id: "native-completion-turn".into(),
                delivery: DeliveryKnowledge::Accepted,
                recovery_required: false,
                outcome: Some(TurnOutcome::Completed),
                error: None,
            },
            authored: "original input".into(),
            projected: "accepted: original input".into(),
            terminal_native_reason: Some("native_success".into()),
            terminal_raw: Some(json!({"completionEvidence":true})),
            terminal_sequence: Some(snapshot.sequence + 3),
            recovered_messages: vec![],
            reconciliation_raw: None,
        })
        .await
        .unwrap();
    let second = runtime(directory.path(), control.clone()).await;
    let resumed = second.resume(&id).await.unwrap();
    assert_eq!(resumed.snapshot().await.turns_completed, 1);
    assert_eq!(resumed.snapshot().await.state, SessionState::Idle);
    assert!(control.delivered.lock().await.is_empty());
    resumed.close().await.unwrap();
    let third = runtime(directory.path(), control.clone()).await;
    let resumed = third.resume(&id).await.unwrap();
    assert_eq!(resumed.snapshot().await.turns_completed, 1);
    resumed.close().await.unwrap();
    let events = store.events(&id, 0, 4096).await.unwrap();
    let related: Vec<_> = events
        .iter()
        .filter(|event| event.turn_id.as_deref() == Some("native-completion-turn"))
        .collect();
    assert_eq!(
        related
            .iter()
            .filter(|event| matches!(event.kind, EventKind::UserMessage(_)))
            .count(),
        1
    );
    let ended: Vec<_> = related
        .iter()
        .filter(|event| matches!(event.kind, EventKind::TurnEnded { .. }))
        .collect();
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].raw, Some(json!({"completionEvidence":true})));
    assert!(
        matches!(&ended[0].kind, EventKind::TurnEnded { outcome: TurnOutcome::Completed, native_reason: Some(reason) } if reason == "native_success")
    );
}

#[tokio::test]
async fn failed_queue_event_rolls_back_queue_and_publishes_no_change() {
    let directory = tempfile::tempdir().unwrap();
    let control = Arc::new(Control::default());
    let runtime = runtime(directory.path(), control.clone()).await;
    let agent = runtime
        .spawn(SpawnOptions::new("controlled", directory.path()))
        .await
        .unwrap();
    agent.pause_queue(true).await.unwrap();
    let before = agent.snapshot().await;
    let store = SqliteSessionStore::new(directory.path()).await.unwrap();
    let connection = rusqlite::Connection::open(store.database_path()).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_queue_event BEFORE INSERT ON session_events
        WHEN json_extract(NEW.event_json, '$.type') = 'queue.changed'
        BEGIN SELECT RAISE(ABORT, 'injected queue event failure'); END;",
        )
        .unwrap();
    assert!(agent.enqueue("must not be admitted").await.is_err());
    assert!(agent.snapshot().await.queue.items.is_empty());
    assert!(
        store
            .load(&before.id)
            .await
            .unwrap()
            .unwrap()
            .queue
            .items
            .is_empty()
    );
    assert_eq!(
        store
            .events(&before.id, before.sequence, 4096)
            .await
            .unwrap()
            .len(),
        0
    );
    assert!(agent.replay(before.sequence).await.unwrap().is_empty());
    assert!(control.delivered.lock().await.is_empty());
    connection
        .execute_batch("DROP TRIGGER reject_queue_event")
        .unwrap();
    agent.enqueue("admitted").await.unwrap();
    assert_eq!(
        store
            .load(&before.id)
            .await
            .unwrap()
            .unwrap()
            .queue
            .items
            .len(),
        1
    );
    assert_eq!(
        store
            .events(&before.id, before.sequence, 4096)
            .await
            .unwrap()
            .len(),
        1
    );
    agent.close().await.unwrap();
}

#[tokio::test]
async fn terminal_checkpoint_keeps_completed_count_when_following_idle_commit_fails() {
    let directory = tempfile::tempdir().unwrap();
    let control = Arc::new(Control::default());
    let first = runtime(directory.path(), control.clone()).await;
    let agent = first
        .spawn(SpawnOptions::new("controlled", directory.path()))
        .await
        .unwrap();
    agent
        .submit("terminal-key", "one native dispatch")
        .await
        .unwrap();
    let id = agent.snapshot().await.id;
    let store = SqliteSessionStore::new(directory.path()).await.unwrap();
    let connection = rusqlite::Connection::open(store.database_path()).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_idle_event BEFORE INSERT ON session_events
        WHEN json_extract(NEW.event_json, '$.type') = 'state.changed'
            AND json_extract(NEW.event_json, '$.data') = 'idle'
        BEGIN SELECT RAISE(ABORT, 'injected idle event failure'); END;",
        )
        .unwrap();
    control
        .events
        .lock()
        .await
        .clone()
        .unwrap()
        .send(BackendEvent::TurnFinished {
            outcome: TurnOutcome::Completed,
            native_reason: None,
            raw: None,
        })
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if store.load(&id).await.unwrap().unwrap().state == SessionState::Failed {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(agent);
    drop(first);
    assert_eq!(store.load(&id).await.unwrap().unwrap().turns_completed, 1);
    connection
        .execute_batch("DROP TRIGGER reject_idle_event")
        .unwrap();
    let second = runtime(directory.path(), control.clone()).await;
    let resumed = second.resume(&id).await.unwrap();
    assert_eq!(resumed.snapshot().await.turns_completed, 1);
    assert_eq!(control.delivered.lock().await.len(), 1);
    resumed.close().await.unwrap();
}
