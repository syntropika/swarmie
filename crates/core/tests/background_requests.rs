use async_trait::async_trait;
use futures_util::StreamExt;
use swarmie::*;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::sync::{Mutex, Notify, mpsc};

#[derive(Default)]
struct Observations {
    input: Mutex<Option<mpsc::Sender<BackendEvent>>>,
    prompts: Mutex<Vec<String>>,
    answers: Mutex<Vec<PermissionResponse>>,
    answer_started: Notify,
    block_answers: AtomicBool,
    fail_cancel: AtomicBool,
    cancellations: AtomicUsize,
    closes: AtomicUsize,
    kills: AtomicUsize,
}
#[derive(Clone)]
struct IndependentHarness(Arc<Observations>);
struct IndependentBackend {
    observations: Arc<Observations>,
    input: mpsc::Sender<BackendEvent>,
}
#[async_trait]
impl Harness for IndependentHarness {
    fn id(&self) -> &str {
        "independent-background"
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
        let (input, events) = mpsc::channel(32);
        *self.0.input.lock().await = Some(input.clone());
        Ok(BackendConnection {
            backend: Box::new(IndependentBackend {
                observations: self.0.clone(),
                input,
            }),
            native: NativeSessionRef {
                provider: self.id().into(),
                id: "parent-with-background-child".into(),
            },
            capabilities: Capabilities {
                cancel: true.into(),
                compact: true.into(),
                rollback: true.into(),
                fork: true.into(),
                permissions: true.into(),
                ..Default::default()
            },
            events,
        })
    }
}
#[async_trait]
impl AgentBackend for IndependentBackend {
    async fn prompt(&self, prompt: &Prompt) -> Result<()> {
        self.observations.prompts.lock().await.push(prompt.text()?);
        self.input
            .send(BackendEvent::TurnFinished {
                outcome: TurnOutcome::Completed,

                native_reason: None,
                raw: None,
            })
            .await
            .map_err(|_| Error::Closed)
    }
    async fn cancel(&self) -> Result<()> {
        self.observations
            .cancellations
            .fetch_add(1, Ordering::SeqCst);
        if self.observations.fail_cancel.load(Ordering::SeqCst) {
            return Err(Error::Provider("native cancellation failed".into()));
        }
        Ok(())
    }
    async fn respond_permission(&self, response: &PermissionResponse) -> Result<()> {
        self.observations
            .answers
            .lock()
            .await
            .push(response.clone());
        self.observations.answer_started.notify_one();
        if self.observations.block_answers.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        Ok(())
    }
    async fn rollback(&self, _: u64) -> Result<()> {
        Ok(())
    }
    async fn compact(&self) -> Result<()> {
        Ok(())
    }
    async fn close(&self) -> Result<()> {
        self.observations.closes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn kill(&self) -> Result<()> {
        self.observations.kills.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
type PolicyObservations = Arc<Mutex<Vec<(Option<String>, SessionState)>>>;
#[derive(Clone)]
struct BackgroundPolicy(PolicyObservations);
#[async_trait]
impl RuntimeExtension for BackgroundPolicy {
    fn id(&self) -> &str {
        "background-policy"
    }
    fn permission_authority(&self) -> bool {
        true
    }
    async fn on_permission(
        &self,
        session: &SessionSnapshot,
        request: &PermissionRequest,
    ) -> Result<Option<PermissionResponse>> {
        self.0
            .lock()
            .await
            .push((session.turn.clone(), session.state.clone()));
        Ok(Some(PermissionResponse {
            answers: Default::default(),
            id: request.id.clone(),
            option: "approve-once".into(),
            data: Value::Null,
        }))
    }
}
fn permission(id: &str) -> PermissionRequest {
    PermissionRequest {
        questions: Vec::new(),
        id: id.into(),
        kind: "command".into(),
        title: "Background operation".into(),
        description: None,
        command: Some(json!(["echo", "background"])),
        paths: vec![],
        options: vec![
            PermissionOption {
                id: "approve-once".into(),
                label: "Approve once".into(),
                kind: "approve_once".into(),
            },
            PermissionOption {
                id: "reject-once".into(),
                label: "Reject".into(),
                kind: "deny_once".into(),
            },
        ],
        metadata: json!({"nativeChild":"child-1"}),
    }
}
async fn next(events: &mut EventStream, predicate: impl Fn(&RuntimeEvent) -> bool) -> RuntimeEvent {
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            let event = event.unwrap();
            if predicate(&event) {
                return event;
            }
        }
        panic!("event stream ended unexpectedly");
    })
    .await
    .unwrap()
}
async fn parent(
    policy: Option<BackgroundPolicy>,
) -> (Runtime, Agent, Arc<Observations>, EventStream) {
    let observations = Arc::new(Observations::default());
    let builder = Runtime::builder().harness(IndependentHarness(observations.clone()));
    let runtime = if let Some(policy) = policy {
        builder.extension(policy).build()
    } else {
        builder.build()
    };
    let agent = runtime
        .spawn(SpawnOptions::new(
            "independent-background",
            std::env::current_dir().unwrap(),
        ))
        .await
        .unwrap();
    let mut events = agent.events();
    agent.prompt("parent").await.unwrap();
    let ended = next(&mut events, |event| {
        matches!(event.kind, EventKind::TurnEnded { .. })
    })
    .await;
    assert!(ended.turn_id.is_some());
    next(&mut events, |event| {
        matches!(event.kind, EventKind::StateChanged(SessionState::Idle))
    })
    .await;
    (runtime, agent, observations, events)
}
async fn emit(observations: &Observations, kind: EventKind) {
    observations
        .input
        .lock()
        .await
        .as_ref()
        .unwrap()
        .send(BackendEvent::content(kind, json!({"background":true})))
        .await
        .unwrap();
}
async fn requested(
    observations: &Observations,
    events: &mut EventStream,
    request: PermissionRequest,
) -> RuntimeEvent {
    emit(observations, EventKind::PermissionRequested(request)).await;
    let event = next(events, |event| {
        matches!(event.kind, EventKind::PermissionRequested(_))
    })
    .await;
    assert!(event.turn_id.is_none());
    assert_eq!(event.raw, Some(json!({"background":true})));
    event
}
async fn answer(agent: &Agent, id: &str) -> Result<()> {
    agent
        .respond_permission(PermissionResponse {
            answers: Default::default(),
            id: id.into(),
            option: "approve-once".into(),
            data: Value::Null,
        })
        .await
}
async fn idle_without_extra_turn(events: &mut EventStream) {
    let event = next(events, |event| {
        assert!(!matches!(event.kind, EventKind::TurnEnded { .. }));
        matches!(event.kind, EventKind::StateChanged(SessionState::Idle))
    })
    .await;
    assert!(event.turn_id.is_none());
}

#[tokio::test]
async fn post_parent_request_is_interactive_exact_answers_route_native_without_a_new_turn() {
    let (runtime, agent, observations, mut events) = parent(None).await;
    let event = requested(&observations, &mut events, permission("late-command")).await;
    assert!(event.turn_id.is_none());
    assert_eq!(agent.snapshot().await.state, SessionState::NeedsInput);
    assert!(agent.snapshot().await.turn.is_none());
    assert!(matches!(agent.prompt("premature").await, Err(Error::Busy)));
    assert!(matches!(agent.compact().await, Err(Error::Busy)));
    assert!(matches!(agent.rollback(1).await, Err(Error::Busy)));
    assert!(matches!(
        runtime.fork(&agent.snapshot().await.id).await,
        Err(Error::Busy)
    ));
    assert!(matches!(
        agent
            .respond_permission(PermissionResponse {
                answers: Default::default(),
                id: "late-command".into(),
                option: "invented-approval".into(),
                data: Value::Null
            })
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(observations.answers.lock().await.is_empty());
    assert_eq!(agent.snapshot().await.state, SessionState::NeedsInput);
    answer(&agent, "late-command").await.unwrap();
    idle_without_extra_turn(&mut events).await;
    assert_eq!(observations.answers.lock().await.len(), 1);
    assert!(matches!(
        answer(&agent, "late-command").await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(agent.snapshot().await.turns_completed, 1);
    agent.close().await.unwrap();
}
#[tokio::test]
async fn typed_only_background_questions_route_answers_and_keep_conflicts_pending() {
    let (_runtime, agent, observations, mut events) = parent(None).await;
    let mut request = permission("typed-question");
    request.kind = "question".into();
    request.metadata = Value::Null;
    request.questions = serde_json::from_value(json!([{"id":"target","prompt":"Target platform?","options":[{"id":"linux","label":"Linux"}],"multiple":false,"allowText":false}])).unwrap();
    request.options = vec![PermissionOption {
        id: "answer".into(),
        label: "Answer".into(),
        kind: "answer".into(),
    }];
    let event = requested(&observations, &mut events, request).await;
    let EventKind::PermissionRequested(request) = event.kind else {
        panic!("missing question")
    };
    assert_eq!(request.questions[0].id, "target");
    assert_eq!(request.metadata["questions"][0]["id"], "target");
    let answers =
        std::collections::BTreeMap::from([("target".into(), QuestionAnswer::Text("linux".into()))]);
    for data in [
        json!({"answers":{"target":"other"}}),
        json!({"answers":{"unknown":"linux"}}),
    ] {
        assert!(matches!(
            agent
                .respond_permission(PermissionResponse {
                    id: request.id.clone(),
                    option: "answer".into(),
                    answers: answers.clone(),
                    data
                })
                .await,
            Err(Error::Invalid(_))
        ));
        assert_eq!(agent.snapshot().await.state, SessionState::NeedsInput);
        assert!(observations.answers.lock().await.is_empty());
    }
    agent
        .respond_permission(PermissionResponse {
            id: request.id,
            option: "answer".into(),
            answers,
            data: Value::Null,
        })
        .await
        .unwrap();
    idle_without_extra_turn(&mut events).await;
    let received = observations.answers.lock().await;
    assert_eq!(received[0].data["answers"]["target"], "linux");
    assert_eq!(
        received[0].answers["target"],
        QuestionAnswer::Text("linux".into())
    );
    drop(received);
    assert_eq!(agent.snapshot().await.turns_completed, 1);
    agent.close().await.unwrap();
}

#[tokio::test]
async fn queue_waits_for_background_request_then_starts_its_own_real_turn() {
    let (_runtime, agent, observations, mut events) = parent(None).await;
    requested(&observations, &mut events, permission("late-command")).await;
    agent.enqueue("queued").await.unwrap();
    next(&mut events, |event| {
        matches!(event.kind, EventKind::QueueChanged(_))
    })
    .await;
    assert!(
        tokio::time::timeout(Duration::from_millis(30), events.next())
            .await
            .is_err()
    );
    assert_eq!(*observations.prompts.lock().await, vec!["parent"]);
    assert_eq!(agent.snapshot().await.queue.items.len(), 1);
    assert!(!agent.snapshot().await.queue.paused);
    answer(&agent, "late-command").await.unwrap();
    idle_without_extra_turn(&mut events).await;
    let started = next(&mut events, |event| {
        matches!(event.kind, EventKind::TurnStarted)
    })
    .await;
    assert!(started.turn_id.is_some());
    let ended = next(&mut events, |event| {
        matches!(event.kind, EventKind::TurnEnded { .. })
    })
    .await;
    assert_eq!(started.turn_id, ended.turn_id);
    assert_eq!(*observations.prompts.lock().await, vec!["parent", "queued"]);
    agent.close().await.unwrap();
}
#[tokio::test]
async fn native_expiry_restores_idle_only_after_all_background_requests_are_resolved() {
    let (_runtime, agent, observations, mut events) = parent(None).await;
    requested(&observations, &mut events, permission("first")).await;
    requested(&observations, &mut events, permission("second")).await;
    answer(&agent, "first").await.unwrap();
    next(
        &mut events,
        |event| matches!(event.kind, EventKind::PermissionResolved { ref id, .. } if id == "first"),
    )
    .await;
    assert_eq!(agent.snapshot().await.state, SessionState::NeedsInput);
    emit(
        &observations,
        EventKind::PermissionResolved {
            id: "second".into(),
            option: "expired".into(),
        },
    )
    .await;
    idle_without_extra_turn(&mut events).await;
    assert_eq!(observations.answers.lock().await.len(), 1);
    assert!(matches!(
        answer(&agent, "second").await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(agent.snapshot().await.turns_completed, 1);
    agent.close().await.unwrap();
}
#[tokio::test]
async fn automatic_public_policy_handles_background_requests_with_no_parent_turn() {
    let hook_seen = Arc::new(Mutex::new(vec![]));
    let (_runtime, agent, observations, mut events) =
        parent(Some(BackgroundPolicy(hook_seen.clone()))).await;
    requested(&observations, &mut events, permission("automatic")).await;
    idle_without_extra_turn(&mut events).await;
    assert_eq!(
        *hook_seen.lock().await,
        vec![(None, SessionState::NeedsInput)]
    );
    assert_eq!(observations.answers.lock().await[0].option, "approve-once");
    assert_eq!(agent.snapshot().await.turns_completed, 1);
    agent.close().await.unwrap();
}
#[tokio::test]
async fn cancel_settles_offered_denials_and_unanswerable_background_controls() {
    for offers_denial in [true, false] {
        let (_runtime, agent, observations, mut events) = parent(None).await;
        let mut request = permission("cancel-me");
        if !offers_denial {
            request
                .options
                .retain(|option| option.kind == "approve_once");
        }
        requested(&observations, &mut events, request).await;
        agent.cancel().await.unwrap();
        idle_without_extra_turn(&mut events).await;
        assert!(agent.snapshot().await.queue.paused);
        assert_eq!(
            observations.cancellations.load(Ordering::SeqCst),
            usize::from(!offers_denial)
        );
        let answers = observations.answers.lock().await;
        assert_eq!(answers.len(), usize::from(offers_denial));
        if offers_denial {
            assert_eq!(answers[0].option, "reject-once");
        }
        drop(answers);
        assert!(matches!(
            answer(&agent, "cancel-me").await,
            Err(Error::Invalid(_))
        ));
        assert_eq!(agent.snapshot().await.turns_completed, 1);
        agent.close().await.unwrap();
    }
}
#[tokio::test]
async fn failed_native_cancellation_keeps_background_request_available_to_the_client() {
    let (_runtime, agent, observations, mut events) = parent(None).await;
    let mut request = permission("not-cancelled");
    request
        .options
        .retain(|option| option.kind == "approve_once");
    requested(&observations, &mut events, request).await;
    observations.fail_cancel.store(true, Ordering::SeqCst);
    assert!(matches!(agent.cancel().await, Err(Error::Provider(_))));
    assert_eq!(agent.snapshot().await.state, SessionState::NeedsInput);
    answer(&agent, "not-cancelled").await.unwrap();
    idle_without_extra_turn(&mut events).await;
    agent.close().await.unwrap();
}
#[tokio::test]
async fn close_does_not_wait_for_a_background_permission_acknowledgement() {
    let (_runtime, agent, observations, mut events) = parent(None).await;
    requested(&observations, &mut events, permission("close-me")).await;
    observations.block_answers.store(true, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_millis(300), agent.close())
        .await
        .unwrap()
        .unwrap();
    let ended = next(&mut events, |event| {
        assert!(!matches!(event.kind, EventKind::TurnEnded { .. }));
        matches!(event.kind, EventKind::SessionEnded)
    })
    .await;
    assert!(ended.turn_id.is_none());
    assert!(observations.answers.lock().await.is_empty());
    assert_eq!(observations.closes.load(Ordering::SeqCst), 1);
    assert_eq!(agent.snapshot().await.state, SessionState::Closed);
}
#[tokio::test]
async fn close_preempts_an_inflight_background_answer_and_does_not_reopen_the_session() {
    let (_runtime, agent, observations, mut events) = parent(None).await;
    requested(&observations, &mut events, permission("blocked-answer")).await;
    observations.block_answers.store(true, Ordering::SeqCst);
    let responding = {
        let agent = agent.clone();
        tokio::spawn(async move { answer(&agent, "blocked-answer").await })
    };
    tokio::time::timeout(
        Duration::from_secs(2),
        observations.answer_started.notified(),
    )
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_millis(300), agent.close())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(responding.await.unwrap(), Err(Error::Closed)));
    next(&mut events, |event| {
        assert!(!matches!(event.kind, EventKind::TurnEnded { .. }));
        matches!(event.kind, EventKind::SessionEnded)
    })
    .await;
    assert!(observations.kills.load(Ordering::SeqCst) >= 1);
    assert_eq!(agent.snapshot().await.state, SessionState::Closed);
    assert!(agent.snapshot().await.turn.is_none());
}
