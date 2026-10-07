use async_trait::async_trait;
use futures_util::StreamExt;
use mpai::*;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::mpsc;

struct TestHarness {
    killed: Arc<AtomicBool>,
}
struct Backend {
    tx: mpsc::Sender<BackendEvent>,
    killed: Arc<AtomicBool>,
}
#[async_trait]
impl Harness for TestHarness {
    fn id(&self) -> &str {
        "external"
    }
    async fn detect(&self) -> Result<Detection> {
        Ok(Detection {
            harness: self.id().into(),
            available: true,
            executable: None,
            capabilities: Capabilities::default(),
        })
    }
    async fn connect(
        &self,
        _: &SpawnOptions,
        _: Option<&NativeSessionRef>,
        _: bool,
    ) -> Result<BackendConnection> {
        let (tx, events) = mpsc::channel(16);
        Ok(BackendConnection {
            backend: Box::new(Backend {
                tx,
                killed: self.killed.clone(),
            }),
            native: NativeSessionRef {
                provider: "external".into(),
                id: "native".into(),
            },
            capabilities: Capabilities {
                cancel: true,
                compact: true,
                permissions: true,
                ..Default::default()
            },
            events,
        })
    }
}
#[async_trait]
impl AgentBackend for Backend {
    async fn prompt(&self, p: &Prompt) -> Result<()> {
        if p.text()? == "expired-dialog" {
            self.tx
                .send(BackendEvent::content(
                    EventKind::PermissionRequested(PermissionRequest {
                        questions: Vec::new(),
                        id: "dialog".into(),
                        kind: "question".into(),
                        title: "Expired input".into(),
                        description: None,
                        command: None,
                        paths: vec![],
                        options: vec![PermissionOption {
                            id: "answer".into(),
                            label: "Submit".into(),
                            kind: "answer".into(),
                        }],
                        metadata: json!({"questions":[{"id":"input","allowText":true}]}),
                    }),
                    json!({"dialog":true}),
                ))
                .await
                .unwrap();
            self.tx
                .send(BackendEvent::content(
                    EventKind::PermissionResolved {
                        id: "dialog".into(),
                        option: "expired".into(),
                    },
                    json!({"timeout":true}),
                ))
                .await
                .unwrap();
            return Ok(());
        }
        if p.text()? == "slow" {
            tokio::time::sleep(Duration::from_secs(20)).await;
        }
        self.tx
            .send(BackendEvent::content(
                EventKind::TextDelta {
                    text: p.text()?,
                    item_id: None,
                },
                json!({"native":true}),
            ))
            .await
            .unwrap();
        self.tx
            .send(BackendEvent::TurnFinished {
                reason: "completed".into(),
                raw: None,
            })
            .await
            .unwrap();
        Ok(())
    }
    async fn cancel(&self) -> Result<()> {
        Ok(())
    }
    async fn respond_permission(&self, _: &PermissionResponse) -> Result<()> {
        Err(Error::Unsupported("permissions".into()))
    }
    async fn compact(&self) -> Result<()> {
        tokio::time::sleep(Duration::from_secs(20)).await;
        Ok(())
    }
    async fn close(&self) -> Result<()> {
        self.killed.store(true, Ordering::SeqCst);
        Ok(())
    }
    async fn kill(&self) -> Result<()> {
        self.close().await
    }
}
struct Prefix;
#[async_trait]
impl RuntimeExtension for Prefix {
    fn id(&self) -> &str {
        "prefix"
    }
    async fn before_prompt(&self, _: &SessionSnapshot, p: &mut Prompt) -> Result<()> {
        *p = format!("Modified {}", p.text()?).as_str().into();
        Ok(())
    }
}
fn options() -> SpawnOptions {
    SpawnOptions::new("external", std::env::current_dir().unwrap())
}
#[tokio::test]
async fn kill_preempts_slow_acceptance_and_compaction() {
    for compact in [false, true] {
        let killed = Arc::new(AtomicBool::new(false));
        let r = Runtime::builder()
            .harness(TestHarness {
                killed: killed.clone(),
            })
            .build();
        let a = r.spawn(options()).await.unwrap();
        let background = a.clone();
        let task = tokio::spawn(async move {
            if compact {
                background.compact().await
            } else {
                background.prompt("slow").await
            }
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        tokio::time::timeout(Duration::from_millis(300), a.kill())
            .await
            .expect("kill must preempt")
            .unwrap();
        assert!(matches!(task.await.unwrap(), Err(Error::Closed)));
        assert!(killed.load(Ordering::SeqCst));
    }
}
#[tokio::test]
async fn stream_ends_after_session_end_and_hooks_use_public_api() {
    let r = Runtime::builder()
        .harness(TestHarness {
            killed: Arc::default(),
        })
        .extension(Prefix)
        .build();
    let a = r.spawn(options()).await.unwrap();
    let mut events = a.events();
    a.prompt("prompt").await.unwrap();
    loop {
        let e = events.next().await.unwrap().unwrap();
        if let EventKind::TextDelta { text, .. } = e.kind {
            assert_eq!(text, "Modified prompt");
            break;
        }
    }
    a.close().await.unwrap();
    tokio::time::timeout(Duration::from_millis(300), async {
        while events.next().await.is_some() {}
    })
    .await
    .expect("closed stream must end");
    assert!(a.events().next().await.is_none());
}
#[tokio::test]
async fn dropping_last_handle_cleans_up_without_runtime_shutdown() {
    let killed = Arc::new(AtomicBool::new(false));
    let r = Runtime::builder()
        .harness(TestHarness {
            killed: killed.clone(),
        })
        .build();
    let a = r.spawn(options()).await.unwrap();
    drop(a);
    tokio::time::timeout(Duration::from_millis(500), async {
        while !killed.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
#[allow(dead_code)]
fn json_is_opaque(_: Value) {}

#[tokio::test]
async fn provider_dialog_expiry_clears_pending_requests_and_restores_running_state() {
    let runtime = Runtime::builder()
        .harness(TestHarness {
            killed: Arc::default(),
        })
        .build();
    let agent = runtime.spawn(options()).await.unwrap();
    let mut events = agent.events();
    agent.prompt("expired-dialog").await.unwrap();
    let mut expired = false;
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let event = events.next().await.unwrap().unwrap();
            if matches!(event.kind, EventKind::PermissionResolved { ref option, .. } if option == "expired") { expired = true; }
            if expired && matches!(event.kind, EventKind::StateChanged(SessionState::Running)) { break; }
        }
    }).await.unwrap();
    assert_eq!(agent.snapshot().await.state, SessionState::Running);
    assert!(matches!(
        agent
            .respond_permission(PermissionResponse {
                answers: Default::default(),
                id: "dialog".into(),
                option: "answer".into(),
                data: json!({"answers":{"input":"too late"}})
            })
            .await,
        Err(Error::Invalid(_))
    ));
    agent.close().await.unwrap();
}

#[tokio::test]
async fn kill_preempts_a_slow_native_cancel_acknowledgement() {
    struct SlowCancel {
        tx: mpsc::Sender<BackendEvent>,
        prompt_started: Arc<tokio::sync::Notify>,
        cancel_started: Arc<tokio::sync::Notify>,
    }
    struct HarnessWithSlowCancel {
        prompt_started: Arc<tokio::sync::Notify>,
        cancel_started: Arc<tokio::sync::Notify>,
    }
    #[async_trait]
    impl Harness for HarnessWithSlowCancel {
        fn id(&self) -> &str {
            "slow-cancel"
        }
        async fn detect(&self) -> Result<Detection> {
            unreachable!()
        }
        async fn connect(
            &self,
            _: &SpawnOptions,
            _: Option<&NativeSessionRef>,
            _: bool,
        ) -> Result<BackendConnection> {
            let (tx, events) = mpsc::channel(8);
            Ok(BackendConnection {
                backend: Box::new(SlowCancel {
                    tx,
                    prompt_started: self.prompt_started.clone(),
                    cancel_started: self.cancel_started.clone(),
                }),
                native: NativeSessionRef {
                    provider: "slow-cancel".into(),
                    id: "native".into(),
                },
                capabilities: Capabilities {
                    cancel: true,
                    ..Default::default()
                },
                events,
            })
        }
    }
    #[async_trait]
    impl AgentBackend for SlowCancel {
        async fn prompt(&self, prompt: &Prompt) -> Result<()> {
            self.prompt_started.notify_one();
            if prompt.text()? == "deferred" {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            self.tx
                .send(BackendEvent::content(
                    EventKind::Raw,
                    json!({"accepted":true}),
                ))
                .await
                .unwrap();
            Ok(())
        }
        async fn cancel(&self) -> Result<()> {
            self.cancel_started.notify_one();
            tokio::time::sleep(Duration::from_secs(20)).await;
            Ok(())
        }
        async fn respond_permission(&self, _: &PermissionResponse) -> Result<()> {
            unreachable!()
        }
        async fn close(&self) -> Result<()> {
            Ok(())
        }
        async fn kill(&self) -> Result<()> {
            Ok(())
        }
    }
    for deferred in [false, true] {
        let prompt_started = Arc::new(tokio::sync::Notify::new());
        let cancel_started = Arc::new(tokio::sync::Notify::new());
        let r = Runtime::builder()
            .harness(HarnessWithSlowCancel {
                prompt_started: prompt_started.clone(),
                cancel_started: cancel_started.clone(),
            })
            .build();
        let a = r
            .spawn(SpawnOptions::new(
                "slow-cancel",
                std::env::current_dir().unwrap(),
            ))
            .await
            .unwrap();
        let mut events = a.events();
        let pending_prompt = if deferred {
            let copy = a.clone();
            let task = tokio::spawn(async move { copy.prompt("deferred").await });
            prompt_started.notified().await;
            Some(task)
        } else {
            a.prompt("hold").await.unwrap();
            None
        };
        let copy = a.clone();
        let cancel = tokio::spawn(async move { copy.cancel().await });
        tokio::time::timeout(Duration::from_secs(1), cancel_started.notified())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_millis(300), a.kill())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(cancel.await.unwrap(), Err(Error::Closed)));
        if let Some(prompt) = pending_prompt {
            assert!(matches!(prompt.await.unwrap(), Err(Error::Closed)));
        }
        assert_eq!(a.snapshot().await.state, SessionState::Closed);
        let last = tokio::time::timeout(Duration::from_millis(300), async {
            let mut last = None;
            while let Some(event) = events.next().await {
                last = Some(event.unwrap().kind);
            }
            last
        })
        .await
        .unwrap();
        assert!(matches!(last, Some(EventKind::SessionEnded)));
    }
}

#[tokio::test]
async fn closing_during_queued_acceptance_preserves_the_queue_without_events_after_end() {
    let r = Runtime::builder()
        .harness(TestHarness {
            killed: Arc::default(),
        })
        .build();
    let agent = r.spawn(options()).await.unwrap();
    let mut events = agent.events();
    agent.pause_queue(true).await.unwrap();
    agent.enqueue("slow").await.unwrap();
    agent.pause_queue(false).await.unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    agent.kill().await.unwrap();
    let snapshot = agent.snapshot().await;
    assert_eq!(snapshot.state, SessionState::Closed);
    assert_eq!(snapshot.queue.items.len(), 1);
    assert!(snapshot.queue.paused);
    let mut last = None;
    while let Some(event) = events.next().await {
        last = Some(event.unwrap().kind);
    }
    assert!(matches!(last, Some(EventKind::SessionEnded)));
}

#[tokio::test]
async fn failed_snapshot_storage_rejects_new_provider_work() {
    struct FailingStore {
        fail: Arc<AtomicBool>,
        memory: MemoryStore,
    }
    #[async_trait]
    impl SessionStore for FailingStore {
        async fn save(&self, snapshot: &SessionSnapshot) -> Result<()> {
            if self.fail.load(Ordering::SeqCst) {
                Err(Error::Storage("injected write failure".into()))
            } else {
                self.memory.save(snapshot).await
            }
        }
        async fn load(&self, id: &str) -> Result<Option<SessionSnapshot>> {
            self.memory.load(id).await
        }
        async fn list(&self) -> Result<Vec<SessionSnapshot>> {
            self.memory.list().await
        }
    }
    let fail = Arc::new(AtomicBool::new(false));
    let runtime = Runtime::builder()
        .harness(TestHarness {
            killed: Arc::default(),
        })
        .store(FailingStore {
            fail: fail.clone(),
            memory: MemoryStore::default(),
        })
        .build();
    let agent = runtime.spawn(options()).await.unwrap();
    // Synchronize after the session actor's initial save before injecting the failure.
    agent.pause_queue(true).await.unwrap();
    fail.store(true, Ordering::SeqCst);
    assert!(matches!(
        agent.prompt("must not execute").await,
        Err(Error::Storage(_))
    ));
    assert!(agent.snapshot().await.turn.is_none());
    assert!(!agent.replay(0).await.unwrap().iter().any(|event| matches!(
        event.kind,
        EventKind::UserMessage(_) | EventKind::TextDelta { .. }
    )));
    assert!(
        runtime
            .transcript(&agent.snapshot().await.id, 0, 4096)
            .await
            .is_err(),
        "snapshot-only store must not promise a complete transcript"
    );
    fail.store(false, Ordering::SeqCst);
    agent.close().await.unwrap();
}

#[tokio::test]
async fn failed_journal_ends_the_session_and_reports_acceptance_failure() {
    struct JournalStore {
        fail: Arc<AtomicBool>,
        memory: MemoryStore,
    }
    #[async_trait]
    impl SessionStore for JournalStore {
        fn transcript_durability(&self) -> Option<&'static str> {
            Some("memory")
        }
        async fn save(&self, snapshot: &SessionSnapshot) -> Result<()> {
            self.memory.save(snapshot).await
        }
        async fn load(&self, id: &str) -> Result<Option<SessionSnapshot>> {
            self.memory.load(id).await
        }
        async fn list(&self) -> Result<Vec<SessionSnapshot>> {
            self.memory.list().await
        }
        async fn append_event(&self, event: &RuntimeEvent) -> Result<()> {
            if self.fail.load(Ordering::SeqCst) {
                Err(Error::Storage("injected journal failure".into()))
            } else {
                self.memory.append_event(event).await
            }
        }
        async fn events(&self, id: &str, after: u64, limit: usize) -> Result<Vec<RuntimeEvent>> {
            self.memory.events(id, after, limit).await
        }
    }
    let fail = Arc::new(AtomicBool::new(false));
    let killed = Arc::new(AtomicBool::new(false));
    let runtime = Runtime::builder()
        .harness(TestHarness {
            killed: killed.clone(),
        })
        .store(JournalStore {
            fail: fail.clone(),
            memory: MemoryStore::default(),
        })
        .build();
    let agent = runtime.spawn(options()).await.unwrap();
    let mut events = agent.events();
    agent.pause_queue(true).await.unwrap();
    fail.store(true, Ordering::SeqCst);
    assert!(matches!(
        agent
            .prompt("accepted natively but cannot be journaled")
            .await,
        Err(Error::Storage(_))
    ));
    assert_eq!(agent.snapshot().await.state, SessionState::Failed);
    tokio::time::timeout(Duration::from_secs(1), async {
        while !killed.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut kinds = vec![];
    while let Some(event) = events.next().await {
        kinds.push(event.unwrap().kind);
    }
    assert!(kinds.iter().any(
        |kind| matches!(kind, EventKind::Error { message } if message.contains("journal failure"))
    ));
    assert!(matches!(kinds.last(), Some(EventKind::SessionEnded)));
}

#[tokio::test]
async fn generate_requires_strict_tools_disabled_capability_and_kills_rejected_connection() {
    let killed = Arc::new(AtomicBool::new(false));
    let runtime = Runtime::builder()
        .harness(TestHarness {
            killed: killed.clone(),
        })
        .build();
    let result = runtime
        .generate(
            SpawnOptions::new("external", std::env::current_dir().unwrap()),
            "never submitted",
        )
        .await;
    assert!(
        matches!(result, Err(Error::Unsupported(message)) if message.contains("tool disabling"))
    );
    assert!(killed.load(Ordering::SeqCst));
    assert!(runtime.sessions().await.unwrap().is_empty());
}
