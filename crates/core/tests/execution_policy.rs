use async_trait::async_trait;
use swarmie::{
    mock::{MockHarness, MockScript, MockStep},
    *,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::{Mutex, RwLock, mpsc};

#[derive(Clone)]
struct PolicyHarness {
    mock: MockHarness,
    values: Arc<RwLock<Value>>,
    fail_second: Arc<AtomicBool>,
    writes: Arc<AtomicUsize>,
    events: Arc<Mutex<Option<mpsc::Sender<BackendEvent>>>>,
}
fn plan(profile: ExecutionProfile) -> Result<ExecutionPolicyPlan> {
    let native = match profile {
        ExecutionProfile::Ask => json!({"approval":"ask","restriction":"restricted"}),
        ExecutionProfile::Full => json!({"approval":"never","restriction":"full"}),
        _ => return Err(Error::Unsupported("fixture profile".into())),
    };
    Ok(ExecutionPolicyPlan {
        profile,
        native,
        application: SettingApplication::NextTurn,
    })
}
#[async_trait]
impl Harness for PolicyHarness {
    fn id(&self) -> &str {
        "policy"
    }
    async fn detect(&self) -> Result<Detection> {
        let mut detection = self.mock.detect().await?;
        detection.harness = "policy".into();
        detection.capabilities.resume = true.into();
        Ok(detection)
    }
    fn compile_execution_profile(
        &self,
        _: &SpawnOptions,
        profile: ExecutionProfile,
    ) -> Result<ExecutionPolicyPlan> {
        plan(profile)
    }
    async fn connect(
        &self,
        options: &SpawnOptions,
        native: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection> {
        assert!(!fork);
        let mut connection = self.mock.connect(options, None, false).await?;
        if let Some(native) = native {
            connection.native = native.clone();
        }
        connection.capabilities.resume = true.into();
        let (sender, receiver) = mpsc::channel(64);
        *self.events.lock().await = Some(sender.clone());
        let mut source = connection.events;
        tokio::spawn(async move {
            while let Some(event) = source.recv().await {
                if sender.send(event).await.is_err() {
                    break;
                }
            }
        });
        connection.events = receiver;
        connection.backend = Box::new(PolicyBackend {
            inner: connection.backend,
            harness: self.clone(),
        });
        Ok(connection)
    }
}
struct PolicyBackend {
    inner: Box<dyn AgentBackend>,
    harness: PolicyHarness,
}
#[async_trait]
impl AgentBackend for PolicyBackend {
    async fn prompt(&self, prompt: &Prompt) -> Result<()> {
        self.inner.prompt(prompt).await
    }
    async fn cancel(&self) -> Result<()> {
        self.inner.cancel().await
    }
    async fn close(&self) -> Result<()> {
        self.inner.close().await
    }
    async fn kill(&self) -> Result<()> {
        self.inner.kill().await
    }
    async fn respond_permission(&self, response: &PermissionResponse) -> Result<()> {
        self.inner.respond_permission(response).await
    }
    async fn configure(&self, patch: Value) -> Result<Value> {
        let mut values = self.harness.values.write().await;
        for (id, value) in patch.as_object().unwrap() {
            if id == "restriction" && self.harness.fail_second.swap(false, Ordering::SeqCst) {
                return Err(Error::Provider("second native field failed".into()));
            }
            self.harness.writes.fetch_add(1, Ordering::SeqCst);
            values[id] = value.clone();
        }
        Ok(values.clone())
    }
    async fn compile_execution_profile(
        &self,
        profile: ExecutionProfile,
    ) -> Result<ExecutionPolicyPlan> {
        plan(profile)
    }
    async fn configuration(&self) -> Result<Vec<SettingDescriptor>> {
        let values = self.harness.values.read().await;
        serde_json::from_value(json!([
            {"id":"approval","label":"Approval","type":"select","value":values["approval"],"choices":[{"id":"ask","label":"Ask"},{"id":"never","label":"Never"}],"application":"next_turn"},
            {"id":"restriction","label":"Restriction","type":"select","value":values["restriction"],"choices":[{"id":"restricted","label":"Restricted"},{"id":"full","label":"Full"}],"application":"next_turn"}
        ])).map_err(Error::from)
    }
    async fn execution_policy(&self) -> Result<ExecutionPolicyReport> {
        let native = self.harness.values.read().await.clone();
        let profile = if native == plan(ExecutionProfile::Ask)?.native {
            Some(ExecutionProfile::Ask)
        } else if native == plan(ExecutionProfile::Full)?.native {
            Some(ExecutionProfile::Full)
        } else {
            None
        };
        let mut choices = unavailable_profile_choices("fixture reviewer is unavailable");
        for choice in &mut choices {
            if choice.profile != ExecutionProfile::Review {
                choice.capability = true.into();
                choice.mapping = PolicyMapping::Equivalent;
            }
        }
        Ok(ExecutionPolicyReport {
            choices,
            controlled_settings: vec!["approval".into(), "restriction".into()],
            planned: native.clone(),
            planned_profile: profile,
            observed: Some(PolicyObservation {
                profile,
                native,
                evidence: PolicyEvidence::NativeAcknowledgement,
                coverage: vec!["fixture controls".into()],
                limitations: vec![],
                native_version: Some("fixture/1".into()),
            }),
            status: PolicyStatus::Acknowledged,
        })
    }
}
fn harness() -> PolicyHarness {
    PolicyHarness {
        mock: MockHarness::new(
            MockScript::default().turn(vec![MockStep::Sleep { milliseconds: 2000 }]),
        ),
        values: Arc::new(RwLock::new(plan(ExecutionProfile::Ask).unwrap().native)),
        fail_second: Arc::new(AtomicBool::new(false)),
        writes: Arc::new(AtomicUsize::new(0)),
        events: Arc::default(),
    }
}
#[derive(Default)]
struct Store {
    memory: MemoryStore,
    fail_pending: AtomicBool,
    fail_result: AtomicBool,
}
#[async_trait]
impl SessionStore for Store {
    async fn local_catalog(&self) -> Result<LocalCatalog> {
        self.memory.local_catalog().await
    }
    async fn update_catalog(&self, update: CatalogUpdate) -> Result<CatalogRecord> {
        self.memory.update_catalog(update).await
    }
    async fn prepare_context(
        &self,
        session: &str,
        binding: &str,
        selection: ContextSelection,
    ) -> Result<WorkingContext> {
        self.memory
            .prepare_context(session, binding, selection)
            .await
    }
    async fn working_context(&self, id: &str) -> Result<Option<WorkingContext>> {
        self.memory.working_context(id).await
    }
    async fn commit(&self, write: SessionWrite<'_>) -> Result<()> {
        if let Some(snapshot) = write.snapshot {
            let policy = &snapshot.execution_policy;
            if policy.revision > 0
                && policy.dispatch_blocked
                && self.fail_pending.swap(false, Ordering::SeqCst)
            {
                return Err(Error::Storage("pending intent write failed".into()));
            }
            if policy.revision > 0
                && !policy.dispatch_blocked
                && self.fail_result.swap(false, Ordering::SeqCst)
            {
                return Err(Error::Storage("native result write failed".into()));
            }
        }
        self.memory.commit(write).await
    }
    async fn load(&self, id: &str) -> Result<Option<SessionSnapshot>> {
        self.memory.load(id).await
    }
    async fn list(&self) -> Result<Vec<SessionSnapshot>> {
        self.memory.list().await
    }
}
#[tokio::test]
async fn partial_native_policy_survives_readback_blocks_dispatch_and_accepts_explicit_repair() {
    let harness = harness();
    let store = Arc::new(Store::default());
    let runtime = Runtime::builder()
        .harness(harness.clone())
        .shared_store(store.clone())
        .build();
    let agent = runtime
        .spawn(SpawnOptions::new(
            "policy",
            std::env::current_dir().unwrap(),
        ))
        .await
        .unwrap();
    harness.fail_second.store(true, Ordering::SeqCst);
    assert!(matches!(
        agent.configure(json!({"executionProfile":"full"})).await,
        Err(Error::Provider(_))
    ));
    let failed = agent.snapshot().await;
    assert_eq!(failed.execution_policy.status, PolicyStatus::Partial);
    assert!(failed.execution_policy.dispatch_blocked);
    assert_eq!(
        failed.execution_policy.observed.unwrap().native,
        json!({"approval":"never","restriction":"restricted"})
    );
    agent.configuration().await.unwrap();
    assert!(agent.snapshot().await.execution_policy.dispatch_blocked);
    assert!(
        store
            .load(&failed.id)
            .await
            .unwrap()
            .unwrap()
            .execution_policy
            .dispatch_blocked
    );
    assert!(matches!(
        agent.prompt("must not run").await,
        Err(Error::Busy)
    ));
    let receipt = agent
        .configure(json!({"executionProfile":"ask"}))
        .await
        .unwrap();
    assert!(!receipt.execution_policy.dispatch_blocked);
    assert_eq!(receipt.execution_policy.revision, 2);
    agent.prompt("now permitted").await.unwrap();
    assert!(matches!(
        agent.configure(json!({"executionProfile":"full"})).await,
        Err(Error::Busy)
    ));
    agent.close().await.unwrap();
}
#[tokio::test]
async fn intent_write_failure_has_no_native_effects_and_result_write_failure_retains_durable_uncertainty()
 {
    for fail_pending in [true, false] {
        let harness = harness();
        let store = Arc::new(Store::default());
        let runtime = Runtime::builder()
            .harness(harness.clone())
            .shared_store(store.clone())
            .build();
        let agent = runtime
            .spawn(SpawnOptions::new(
                "policy",
                std::env::current_dir().unwrap(),
            ))
            .await
            .unwrap();
        if fail_pending {
            store.fail_pending.store(true, Ordering::SeqCst);
        } else {
            store.fail_result.store(true, Ordering::SeqCst);
        }
        assert!(matches!(
            agent.configure(json!({"executionProfile":"full"})).await,
            Err(Error::Storage(_))
        ));
        assert_eq!(
            harness.writes.load(Ordering::SeqCst),
            if fail_pending { 0 } else { 2 }
        );
        assert!(agent.snapshot().await.execution_policy.dispatch_blocked);
        assert!(matches!(agent.prompt("blocked").await, Err(Error::Busy)));
        if !fail_pending {
            let retained = store
                .load(&agent.snapshot().await.id)
                .await
                .unwrap()
                .unwrap();
            assert!(retained.execution_policy.dispatch_blocked);
            assert_eq!(
                retained.execution_policy.requested,
                Some(ExecutionProfile::Full)
            );
        }
        agent.close().await.unwrap();
    }
}
#[tokio::test]
async fn successful_model_controls_cannot_release_a_previous_policy_failure() {
    let harness = harness();
    let runtime = Runtime::builder().harness(harness.clone()).build();
    let agent = runtime
        .spawn(SpawnOptions::new(
            "policy",
            std::env::current_dir().unwrap(),
        ))
        .await
        .unwrap();
    harness.fail_second.store(true, Ordering::SeqCst);
    assert!(
        agent
            .configure(json!({"executionProfile":"full"}))
            .await
            .is_err()
    );
    // Empty unrelated configuration refreshes the backend successfully but does not repair policy.
    let receipt = agent.configure(json!({})).await.unwrap();
    assert!(receipt.execution_policy.dispatch_blocked);
    assert!(matches!(agent.prompt("blocked").await, Err(Error::Busy)));
    agent.close().await.unwrap();
}
#[test]
fn missing_native_observations_and_partial_mappings_do_not_create_a_profile() {
    let mut policy = ExecutionPolicy::default();
    policy.observe(ExecutionPolicyReport::default());
    assert!(policy.descriptor().value.is_none());
    for profile in [
        ExecutionProfile::Ask,
        ExecutionProfile::Review,
        ExecutionProfile::Full,
    ] {
        assert!(ExecutionPolicyReport::default().require(profile).is_err());
    }
    let encoded = serde_json::to_value(policy).unwrap();
    assert_eq!(encoded["dispatchBlocked"], false);
    assert!(encoded["observed"].is_null());
}

#[tokio::test]
async fn a_new_runtime_retains_uncertainty_until_explicit_repair() {
    let harness = harness();
    let store = Arc::new(Store::default());
    let runtime = Runtime::builder()
        .harness(harness.clone())
        .shared_store(store.clone())
        .build();
    let agent = runtime
        .spawn(SpawnOptions::new(
            "policy",
            std::env::current_dir().unwrap(),
        ))
        .await
        .unwrap();
    harness.fail_second.store(true, Ordering::SeqCst);
    assert!(
        agent
            .configure(json!({"executionProfile":"full"}))
            .await
            .is_err()
    );
    let id = agent.snapshot().await.id;
    agent.close().await.unwrap();
    drop(runtime);
    let restarted = Runtime::builder()
        .harness(harness)
        .shared_store(store)
        .build();
    let resumed = restarted.resume(&id).await.unwrap();
    // Reapplying and acknowledging controls cannot erase an unresolved durable mutation.
    assert!(resumed.snapshot().await.execution_policy.dispatch_blocked);
    assert!(matches!(resumed.prompt("blocked").await, Err(Error::Busy)));
    resumed
        .configure(json!({"executionProfile":"ask"}))
        .await
        .unwrap();
    assert!(!resumed.snapshot().await.execution_policy.dispatch_blocked);
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn external_native_drift_cannot_satisfy_a_retained_common_profile() {
    let harness = harness();
    let runtime = Runtime::builder().harness(harness.clone()).build();
    let agent = runtime
        .spawn(SpawnOptions::new(
            "policy",
            std::env::current_dir().unwrap(),
        ))
        .await
        .unwrap();
    agent
        .configure(json!({"executionProfile":"ask"}))
        .await
        .unwrap();
    *harness.values.write().await = plan(ExecutionProfile::Full).unwrap().native;
    agent.configuration().await.unwrap();
    let policy = agent.snapshot().await.execution_policy;
    assert_eq!(policy.requested, Some(ExecutionProfile::Ask));
    assert_eq!(
        policy.observed.unwrap().profile,
        Some(ExecutionProfile::Full)
    );
    assert!(policy.dispatch_blocked);
    assert!(matches!(agent.prompt("blocked").await, Err(Error::Busy)));
    agent.close().await.unwrap();
}

#[derive(Clone)]
struct Authority {
    id: &'static str,
    option: Option<&'static str>,
    fail: bool,
}
#[async_trait]
impl RuntimeExtension for Authority {
    fn id(&self) -> &str {
        self.id
    }
    fn permission_authority(&self) -> bool {
        true
    }
    async fn on_permission(
        &self,
        _: &SessionSnapshot,
        request: &PermissionRequest,
    ) -> Result<Option<PermissionResponse>> {
        if self.fail {
            return Err(Error::Provider("authority failed".into()));
        }
        Ok(self.option.map(|option| PermissionResponse {
            answers: Default::default(),
            id: request.id.clone(),
            option: option.into(),
            data: Value::Null,
        }))
    }
}
fn request() -> PermissionRequest {
    serde_json::from_value(json!({"id":"operation","kind":"command","title":"Operation","options":[{"id":"approve","label":"Approve","kind":"approve_once"},{"id":"deny","label":"Deny","kind":"deny_once"}]})).unwrap()
}
#[tokio::test]
async fn one_host_authority_preserves_denials_and_leaves_unresolved_common_requests_to_the_user() {
    use futures_util::StreamExt;
    for (option, fail, expect_answer) in [
        (Some("approve"), false, false),
        (Some("deny"), false, true),
        (None, true, false),
    ] {
        let mut harness = harness();
        harness.mock = MockHarness::new(MockScript::default().turn(vec![
            MockStep::Request { request: request() },
            MockStep::AwaitAnswer {
                id: "operation".into(),
                option: None,
            },
        ]));
        let runtime = Runtime::builder()
            .harness(harness.clone())
            .extension(Authority {
                id: "authority",
                option,
                fail,
            })
            .build();
        let agent = runtime
            .spawn(SpawnOptions::new(
                "policy",
                std::env::current_dir().unwrap(),
            ))
            .await
            .unwrap();
        agent
            .configure(json!({"executionProfile":"ask"}))
            .await
            .unwrap();
        let mut events = agent.events();
        agent.prompt("request operation").await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while let Some(event) = events.next().await {
                if matches!(
                    event.unwrap().kind,
                    EventKind::PermissionResolved { .. } | EventKind::Error { .. }
                ) {
                    break;
                }
            }
        })
        .await
        .unwrap();
        let answers = harness.mock.answers().await;
        assert_eq!(answers.len(), usize::from(expect_answer));
        if expect_answer {
            assert_eq!(answers[0].option, "deny");
        } else {
            assert_eq!(agent.snapshot().await.state, SessionState::NeedsInput);
            agent
                .respond_permission(PermissionResponse {
                    answers: Default::default(),
                    id: "operation".into(),
                    option: "approve".into(),
                    data: Value::Null,
                })
                .await
                .unwrap();
            assert_eq!(harness.mock.answers().await.len(), 1);
        }
        agent.close().await.unwrap();
    }
    let harness = harness();
    let runtime = Runtime::builder()
        .harness(harness.clone())
        .extension(Authority {
            id: "first",
            option: None,
            fail: true,
        })
        .extension(Authority {
            id: "second",
            option: Some("approve"),
            fail: false,
        })
        .build();
    assert!(matches!(
        runtime
            .spawn(SpawnOptions::new(
                "policy",
                std::env::current_dir().unwrap()
            ))
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        runtime
            .preview_configuration(SpawnOptions::new(
                "policy",
                std::env::current_dir().unwrap()
            ))
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(harness.mock.launches().await.is_empty());
}

#[tokio::test]
async fn native_replacement_invalidates_old_requests_and_requires_policy_requalification() {
    use futures_util::StreamExt;
    let mut harness = harness();
    harness.mock = MockHarness::new(MockScript::default().turn(vec![
        MockStep::Request { request: request() },
        MockStep::AwaitAnswer {
            id: "operation".into(),
            option: None,
        },
    ]));
    let runtime = Runtime::builder().harness(harness.clone()).build();
    let agent = runtime
        .spawn(SpawnOptions::new(
            "policy",
            std::env::current_dir().unwrap(),
        ))
        .await
        .unwrap();
    agent
        .configure(json!({"executionProfile":"ask"}))
        .await
        .unwrap();
    let mut events = agent.events();
    agent.prompt("pending request").await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if matches!(event.unwrap().kind, EventKind::PermissionRequested(_)) {
                break;
            }
        }
    })
    .await
    .unwrap();
    harness
        .events
        .lock()
        .await
        .as_ref()
        .unwrap()
        .send(BackendEvent::NativeSessionChanged(NativeSessionRef {
            provider: "policy".into(),
            id: "replacement".into(),
        }))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if let EventKind::SessionUpdated(value) = event.unwrap().kind
                && value["native"]["id"] == "replacement"
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    let policy = agent.snapshot().await.execution_policy;
    assert!(policy.observed.is_none());
    assert!(policy.dispatch_blocked);
    assert_eq!(policy.native_session.unwrap().id, "replacement");
    assert!(matches!(
        agent
            .respond_permission(PermissionResponse {
                answers: Default::default(),
                id: "operation".into(),
                option: "approve".into(),
                data: Value::Null
            })
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(harness.mock.answers().await.is_empty());
    agent.cancel().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if matches!(event.unwrap().kind, EventKind::TurnEnded { .. }) {
                break;
            }
        }
    })
    .await
    .unwrap();
    agent
        .configure(json!({"executionProfile":"ask"}))
        .await
        .unwrap();
    assert!(!agent.snapshot().await.execution_policy.dispatch_blocked);
    agent.close().await.unwrap();
}
