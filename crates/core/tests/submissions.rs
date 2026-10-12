use async_trait::async_trait;
use mpai::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::{Mutex, Semaphore, mpsc};

struct Control {
    calls: AtomicUsize,
    recovery: Mutex<Option<NativeSubmissionEvidence>>,
    identities: Mutex<Vec<String>>,
    gate: Semaphore,
    events: Mutex<Option<mpsc::Sender<BackendEvent>>>,
}
impl Default for Control {
    fn default() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            recovery: Mutex::new(None),
            identities: Mutex::new(vec![]),
            gate: Semaphore::new(0),
            events: Mutex::new(None),
        }
    }
}
#[derive(Clone)]
struct Peer(Arc<Control>);
#[async_trait]
impl Harness for Peer {
    fn id(&self) -> &str {
        "submission-peer"
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
        let (sender, events) = mpsc::channel(16);
        *self.0.events.lock().await = Some(sender);
        Ok(BackendConnection {
            backend: Box::new(self.clone()),
            events,
            native: NativeSessionRef {
                provider: self.id().into(),
                id: "native".into(),
            },
            capabilities: Capabilities {
                resume: true.into(),
                submission_reconciliation: true.into(),
                steering: true.into(),
                ..Default::default()
            },
        })
    }
}
#[async_trait]
impl AgentBackend for Peer {
    async fn submit(&self, id: &str, prompt: &Prompt) -> Result<()> {
        self.0.identities.lock().await.push(id.into());
        self.prompt(prompt).await
    }
    async fn reconcile_submission(&self, _: &str) -> Result<Option<NativeSubmissionEvidence>> {
        Ok(self.0.recovery.lock().await.clone())
    }

    async fn prompt(&self, prompt: &Prompt) -> Result<()> {
        self.0.calls.fetch_add(1, Ordering::SeqCst);
        self.0.gate.acquire().await.unwrap().forget();
        match prompt.text()?.as_str() {
            "uncertain" => Err(Error::Timeout("native acknowledgement lost".into())),
            "reject" => Err(Error::Rejected("qualified no-effect rejection".into())),
            _ => Ok(()),
        }
    }
    async fn cancel(&self) -> Result<()> {
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
async fn wait_calls(control: &Control, expected: usize) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while control.calls.load(Ordering::SeqCst) < expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
fn options() -> SpawnOptions {
    SpawnOptions::new("submission-peer", std::env::current_dir().unwrap())
}
async fn completed(agent: &Agent, control: &Control) {
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
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while agent.snapshot().await.turn.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn concurrent_duplicate_and_lost_client_response_dispatch_once_even_after_completion() {
    let control = Arc::new(Control::default());
    let runtime = Runtime::builder().harness(Peer(control.clone())).build();
    let agent = runtime.spawn(options()).await.unwrap();
    let caller = agent.clone();
    let lost = tokio::spawn(async move { caller.submit("original", "hello").await });
    wait_calls(&control, 1).await;
    let inflight = agent.submit("original", "hello").await.unwrap();
    assert_eq!(inflight.delivery, DeliveryKnowledge::InFlight);
    assert!(matches!(
        agent.submit("original", "changed").await,
        Err(Error::IdempotencyConflict)
    ));
    assert!(matches!(
        agent.submit("different", "hello").await,
        Err(Error::Busy)
    ));
    lost.abort();
    control.gate.add_permits(1);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while agent
            .submission("original")
            .await
            .unwrap()
            .unwrap()
            .delivery
            != DeliveryKnowledge::Accepted
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    completed(&agent, &control).await;
    let duplicate = agent.submit("original", "hello").await.unwrap();
    assert_eq!(duplicate.id, inflight.id);
    assert_eq!(duplicate.outcome, Some(TurnOutcome::Completed));
    assert_eq!(control.calls.load(Ordering::SeqCst), 1);
    control.gate.add_permits(1);
    agent
        .submit("intentional-second-message", "hello")
        .await
        .unwrap();
    completed(&agent, &control).await;
    assert_eq!(control.calls.load(Ordering::SeqCst), 2);
    agent.close().await.unwrap();
    let id = agent.snapshot().await.id;
    let retained = runtime
        .submit(&id, "original", "hello".into())
        .await
        .unwrap();
    assert_eq!(retained.id, duplicate.id);
    assert_eq!(control.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn ambiguous_native_failure_blocks_followups_queue_resume_and_steering() {
    let control = Arc::new(Control::default());
    control.gate.add_permits(1);
    let runtime = Runtime::builder().harness(Peer(control.clone())).build();
    let agent = runtime.spawn(options()).await.unwrap();
    let receipt = agent.submit("uncertain-key", "uncertain").await.unwrap();
    assert_eq!(receipt.delivery, DeliveryKnowledge::Unknown);
    assert!(receipt.recovery_required);
    assert!(agent.snapshot().await.turn.is_none());
    assert_eq!(agent.snapshot().await.state, SessionState::NeedsInput);
    assert!(agent.snapshot().await.queue.paused);
    agent.enqueue("follow-up").await.unwrap();
    assert!(matches!(agent.pause_queue(false).await, Err(Error::Busy)));
    assert!(matches!(
        agent.submit("new-key", "hello").await,
        Err(Error::Busy)
    ));
    assert!(matches!(agent.steer("hello").await, Err(Error::Busy)));
    assert_eq!(
        agent.submit("uncertain-key", "uncertain").await.unwrap().id,
        receipt.id
    );
    assert_eq!(control.calls.load(Ordering::SeqCst), 1);
    agent.close().await.unwrap();
}

#[tokio::test]
async fn qualified_rejection_is_retained_and_allows_a_deliberate_replacement() {
    let control = Arc::new(Control::default());
    control.gate.add_permits(2);
    let runtime = Runtime::builder().harness(Peer(control.clone())).build();
    let agent = runtime.spawn(options()).await.unwrap();
    let receipt = agent.submit("rejected-key", "reject").await.unwrap();
    assert_eq!(receipt.delivery, DeliveryKnowledge::Rejected);
    assert!(!receipt.recovery_required);
    assert!(agent.snapshot().await.turn.is_none());
    assert_eq!(
        agent.submit("rejected-key", "reject").await.unwrap().id,
        receipt.id
    );
    agent.submit("replacement-key", "hello").await.unwrap();
    assert_eq!(control.calls.load(Ordering::SeqCst), 2);
    completed(&agent, &control).await;
    agent.close().await.unwrap();
}

struct FaultStore {
    memory: Arc<MemoryStore>,
    writes: AtomicUsize,
    fail_at: usize,
}
#[async_trait]
impl SessionStore for FaultStore {
    async fn local_catalog(&self) -> Result<LocalCatalog> {
        self.memory.local_catalog().await
    }
    async fn update_catalog(&self, update: CatalogUpdate) -> Result<CatalogRecord> {
        self.memory.update_catalog(update).await
    }
    async fn prepare_context(
        &self,
        session_id: &str,
        binding_id: &str,
        selection: ContextSelection,
    ) -> Result<WorkingContext> {
        self.memory
            .prepare_context(session_id, binding_id, selection)
            .await
    }
    async fn working_context(&self, id: &str) -> Result<Option<WorkingContext>> {
        self.memory.working_context(id).await
    }

    async fn commit(&self, write: SessionWrite<'_>) -> Result<()> {
        if write.submission.is_some()
            && self.writes.fetch_add(1, Ordering::SeqCst) + 1 == self.fail_at
        {
            return Err(Error::Storage("injected delivery write failure".into()));
        }
        self.memory.commit(write).await
    }
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
        self.memory.append_event(event).await
    }
    async fn events(&self, id: &str, after: u64, limit: usize) -> Result<Vec<RuntimeEvent>> {
        self.memory.events(id, after, limit).await
    }
    async fn submissions(&self, id: &str) -> Result<Vec<SubmissionRecord>> {
        self.memory.submissions(id).await
    }
    async fn save_submission(&self, record: &SubmissionRecord) -> Result<()> {
        if self.writes.fetch_add(1, Ordering::SeqCst) + 1 == self.fail_at {
            return Err(Error::Storage("injected delivery write failure".into()));
        }
        self.memory.save_submission(record).await
    }
}
#[tokio::test]
async fn storage_failures_before_dispatch_never_call_native_and_after_dispatch_keep_evidence() {
    for fail_at in 1..=3 {
        let control = Arc::new(Control::default());
        control.gate.add_permits(1);
        let memory = Arc::new(MemoryStore::default());
        let runtime = Runtime::builder()
            .harness(Peer(control.clone()))
            .store(FaultStore {
                memory: memory.clone(),
                writes: AtomicUsize::new(0),
                fail_at,
            })
            .build();
        let agent = runtime.spawn(options()).await.unwrap();
        assert!(matches!(
            agent.submit("original", "hello").await,
            Err(Error::Storage(_))
        ));
        let receipt = agent.submission("original").await.unwrap().unwrap();
        assert_eq!(
            control.calls.load(Ordering::SeqCst),
            usize::from(fail_at == 3)
        );
        if fail_at == 3 {
            assert_eq!(receipt.delivery, DeliveryKnowledge::Accepted);
            let records = memory
                .submissions(&agent.snapshot().await.id)
                .await
                .unwrap();
            assert_eq!(records[0].receipt.delivery, DeliveryKnowledge::InFlight);
            completed(&agent, &control).await;
        } else {
            assert_eq!(receipt.delivery, DeliveryKnowledge::NotSent);
        }
        assert_eq!(
            agent.submit("original", "hello").await.unwrap().id,
            receipt.id
        );
        agent.close().await.unwrap();
    }
}

#[tokio::test]
async fn correlated_completion_recovers_an_uncertain_attempt_and_keeps_the_queue_paused() {
    let control = Arc::new(Control::default());
    let store = Arc::new(MemoryStore::default());
    let runtime = Runtime::builder()
        .harness(Peer(control.clone()))
        .store(FaultStore {
            memory: store.clone(),
            writes: AtomicUsize::new(0),
            fail_at: usize::MAX,
        })
        .build();
    let agent = runtime.spawn(options()).await.unwrap();
    control.gate.add_permits(1);
    let original = agent.submit("original", "uncertain").await.unwrap();
    assert_eq!(original.delivery, DeliveryKnowledge::Unknown);
    assert_eq!(
        control.identities.lock().await.as_slice(),
        std::slice::from_ref(&original.id)
    );
    agent.enqueue("queued work").await.unwrap();
    let absent = agent
        .reconcile_submission("original")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(absent.delivery, DeliveryKnowledge::Unknown);
    assert!(matches!(
        agent.submit("different", "new work").await,
        Err(Error::Busy)
    ));
    *control.recovery.lock().await = Some(NativeSubmissionEvidence {
        submission_id: "wrong-attempt".into(),
        native_turn_id: "native-turn".into(),
        outcome: Some(TurnOutcome::Completed),
        native_reason: Some("completed".into()),
        raw: serde_json::json!({"confirmed":true}),
        messages: vec![],
    });
    assert!(matches!(
        agent.reconcile_submission("original").await,
        Err(Error::Provider(_))
    ));
    let mut evidence = control.recovery.lock().await.clone().unwrap();
    evidence.submission_id = original.id.clone();
    evidence.outcome = None;
    *control.recovery.lock().await = Some(evidence.clone());
    let accepted = agent
        .reconcile_submission("original")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(accepted.delivery, DeliveryKnowledge::Accepted);
    assert!(accepted.recovery_required);
    assert!(matches!(agent.pause_queue(false).await, Err(Error::Busy)));
    // An uncorrelated terminal event cannot settle an accepted, unresolved submission.
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
    let still_pending = agent.submission("original").await.unwrap().unwrap();
    assert!(still_pending.recovery_required);
    evidence.outcome = Some(TurnOutcome::Completed);
    evidence.messages = vec![serde_json::json!({"id":"native-answer","text":"Recovered answer"})];
    *control.recovery.lock().await = Some(evidence);
    let recovered = agent
        .reconcile_submission("original")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.outcome, Some(TurnOutcome::Completed));
    assert!(!recovered.recovery_required);
    assert_eq!(agent.snapshot().await.state, SessionState::Idle);
    assert!(agent.snapshot().await.queue.paused);
    assert_eq!(agent.snapshot().await.turns_completed, 1);
    let id = agent.snapshot().await.id;
    let events = store.events(&id, 0, 4096).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::TurnStarted))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::UserMessage(_)))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::AssistantMessage(_)))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::TurnEnded { .. }))
            .count(),
        1
    );
    assert_eq!(
        agent
            .reconcile_submission("original")
            .await
            .unwrap()
            .unwrap()
            .outcome,
        recovered.outcome
    );
    assert_eq!(
        agent.submit("original", "uncertain").await.unwrap().id,
        original.id
    );
    assert_eq!(control.calls.load(Ordering::SeqCst), 1);
    assert_eq!(control.identities.lock().await.len(), 1);
    assert!(
        agent
            .reconcile_submission("missing")
            .await
            .unwrap()
            .is_none()
    );
    agent.close().await.unwrap();
}

#[tokio::test]
async fn reconciliation_write_failure_preserves_native_facts_and_can_repair_without_another_native_read()
 {
    let control = Arc::new(Control::default());
    let store = Arc::new(MemoryStore::default());
    let runtime = Runtime::builder()
        .harness(Peer(control.clone()))
        .store(FaultStore {
            memory: store.clone(),
            writes: AtomicUsize::new(0),
            fail_at: 4,
        })
        .build();
    let agent = runtime.spawn(options()).await.unwrap();
    control.gate.add_permits(1);
    let original = agent.submit("original", "uncertain").await.unwrap();
    *control.recovery.lock().await = Some(NativeSubmissionEvidence {
        submission_id: original.id,
        native_turn_id: "native-turn".into(),
        outcome: Some(TurnOutcome::Completed),
        native_reason: Some("completed".into()),
        raw: serde_json::json!({"confirmed":true}),
        messages: vec![serde_json::json!({"id":"answer","text":"retained result"})],
    });
    assert!(matches!(
        agent.reconcile_submission("original").await,
        Err(Error::Storage(_))
    ));
    assert_eq!(
        agent.submission("original").await.unwrap().unwrap().outcome,
        Some(TurnOutcome::Completed)
    );
    *control.recovery.lock().await = None;
    let repaired = agent
        .reconcile_submission("original")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(repaired.outcome, Some(TurnOutcome::Completed));
    assert_eq!(agent.snapshot().await.state, SessionState::Idle);
    let events = store
        .events(&agent.snapshot().await.id, 0, 4096)
        .await
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::AssistantMessage(_)))
            .count(),
        1
    );
    assert_eq!(control.calls.load(Ordering::SeqCst), 1);
    agent.close().await.unwrap();
}
