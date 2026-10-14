use async_trait::async_trait;
use protocol::RuntimeService;
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use swarmie::*;
use tokio::sync::{Notify, mpsc};

#[derive(Default)]
struct Observation {
    entered: Notify,
    release: Notify,
    stopped: AtomicUsize,
    cancelled: AtomicUsize,
    listed: AtomicUsize,
    slow_tasks: AtomicBool,
    slow_stop: AtomicBool,
    tasks_entered: Notify,
    stop_entered: Notify,
    controls_release: Notify,
    controls_active: AtomicUsize,
    active_at_close: AtomicUsize,
}
struct ActiveControl(Arc<Observation>);
impl ActiveControl {
    fn new(observation: &Arc<Observation>) -> Self {
        observation.controls_active.fetch_add(1, Ordering::SeqCst);
        Self(observation.clone())
    }
}
impl Drop for ActiveControl {
    fn drop(&mut self) {
        self.0.controls_active.fetch_sub(1, Ordering::SeqCst);
    }
}
struct Peer {
    observation: Arc<Observation>,
    supported: bool,
}
struct Backend {
    observation: Arc<Observation>,
    _events: mpsc::Sender<BackendEvent>,
}
fn task() -> AgentTask {
    AgentTask {
        id: "task-1".into(),
        description: "Check the fixture".into(),
        status: AgentTaskStatus::Running,
        kind: Some("agent".into()),
        summary: None,
    }
}
#[async_trait]
impl Harness for Peer {
    fn id(&self) -> &str {
        "tasks-peer"
    }
    async fn detect(&self) -> Result<Detection> {
        Ok(Detection {
            harness: self.id().into(),
            available: true,
            executable: None,
            capabilities: Capabilities {
                background_tasks: self.supported.into(),
                ..Default::default()
            },
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
                observation: self.observation.clone(),
                _events: tx,
            }),
            native: NativeSessionRef {
                provider: self.id().into(),
                id: "native-session".into(),
            },
            capabilities: self.detect().await?.capabilities,
            events,
        })
    }
}
#[async_trait]
impl AgentBackend for Backend {
    async fn prompt(&self, _: &Prompt) -> Result<()> {
        self.observation.entered.notify_one();
        self.observation.release.notified().await;
        Ok(())
    }
    async fn cancel(&self) -> Result<()> {
        self.observation.cancelled.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn respond_permission(&self, _: &PermissionResponse) -> Result<()> {
        Ok(())
    }
    async fn tasks(&self) -> Result<Vec<AgentTask>> {
        let _active = ActiveControl::new(&self.observation);
        if self.observation.slow_tasks.load(Ordering::SeqCst) {
            self.observation.tasks_entered.notify_one();
            self.observation.controls_release.notified().await;
        }
        self.observation.listed.fetch_add(1, Ordering::SeqCst);
        Ok(vec![task()])
    }
    async fn stop_task(&self, id: &str) -> Result<()> {
        if id != "task-1" {
            return Err(Error::Invalid("Unknown task in this session".into()));
        }
        let _active = ActiveControl::new(&self.observation);
        if self.observation.slow_stop.load(Ordering::SeqCst) {
            self.observation.stop_entered.notify_one();
            self.observation.controls_release.notified().await;
        }
        self.observation.stopped.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn close(&self) -> Result<()> {
        self.observation.active_at_close.store(
            self.observation.controls_active.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );
        self.observation.release.notify_waiters();
        Ok(())
    }
    async fn kill(&self) -> Result<()> {
        self.close().await
    }
}

#[tokio::test]
async fn close_drains_pending_task_controls_before_native_teardown() {
    let (service, observation, id) = fixture(true).await;
    observation.slow_tasks.store(true, Ordering::SeqCst);
    observation.slow_stop.store(true, Ordering::SeqCst);
    let pending = service.clone();
    let session = id.clone();
    let prompt = tokio::spawn(async move {
        pending.invoke("session/prompt", json!({"sessionId":session,"text":"pending fixture","idempotencyKey":"close-parent"})).await
    });
    tokio::time::timeout(Duration::from_secs(2), observation.entered.notified())
        .await
        .unwrap();
    let pending = service.clone();
    let session = id.clone();
    let list = tokio::spawn(async move {
        pending
            .invoke("session/tasks", json!({"sessionId":session}))
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), observation.tasks_entered.notified())
        .await
        .unwrap();
    let pending = service.clone();
    let session = id.clone();
    let stop = tokio::spawn(async move {
        pending
            .invoke(
                "session/task/stop",
                json!({"sessionId":session,"taskId":"task-1"}),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), observation.stop_entered.notified())
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        service.invoke("session/close", json!({"sessionId":id})),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(list.await.unwrap(), Err(Error::Closed)));
    assert!(matches!(stop.await.unwrap(), Err(Error::Closed)));
    let _ = prompt.await.unwrap();
    assert_eq!(observation.controls_active.load(Ordering::SeqCst), 0);
    assert_eq!(observation.active_at_close.load(Ordering::SeqCst), 0);
    // Released native controls cannot resume after session/close acknowledges.
    observation.controls_release.notify_waiters();
    assert_eq!(observation.listed.load(Ordering::SeqCst), 0);
    assert_eq!(observation.stopped.load(Ordering::SeqCst), 0);
    assert_eq!(observation.cancelled.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn pending_stop_survives_prompt_acceptance_and_keeps_its_native_receipt() {
    let (service, observation, id) = fixture(true).await;
    observation.slow_stop.store(true, Ordering::SeqCst);
    let pending = service.clone();
    let session = id.clone();
    let prompt = tokio::spawn(async move {
        pending.invoke("session/prompt", json!({"sessionId":session,"text":"pending fixture","idempotencyKey":"accept-parent"})).await
    });
    tokio::time::timeout(Duration::from_secs(2), observation.entered.notified())
        .await
        .unwrap();
    let pending = service.clone();
    let session = id.clone();
    let stop = tokio::spawn(async move {
        pending
            .invoke(
                "session/task/stop",
                json!({"sessionId":session,"taskId":"task-1"}),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), observation.stop_entered.notified())
        .await
        .unwrap();
    observation.release.notify_one();
    tokio::time::timeout(Duration::from_secs(1), prompt)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!stop.is_finished());
    observation.controls_release.notify_one();
    tokio::time::timeout(Duration::from_secs(1), stop)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(observation.stopped.load(Ordering::SeqCst), 1);
    assert_eq!(observation.cancelled.load(Ordering::SeqCst), 0);
    service
        .invoke("session/close", json!({"sessionId":id}))
        .await
        .unwrap();
}

async fn fixture(supported: bool) -> (RuntimeService, Arc<Observation>, String) {
    let observation = Arc::new(Observation::default());
    let service = RuntimeService::new(
        Runtime::builder()
            .harness(Peer {
                observation: observation.clone(),
                supported,
            })
            .build(),
    );
    let snapshot = service
        .invoke(
            "session/create",
            json!({"harness":"tasks-peer", "cwd":std::env::current_dir().unwrap()}),
        )
        .await
        .unwrap();
    (
        service,
        observation,
        snapshot["id"].as_str().unwrap().into(),
    )
}

#[tokio::test]
async fn task_control_preempts_pending_prompt_acceptance_without_cancelling_the_parent() {
    let (service, observation, id) = fixture(true).await;
    let pending = service.clone();
    let session = id.clone();
    let prompt = tokio::spawn(async move {
        pending.invoke("session/prompt", json!({"sessionId":session,"text":"pending fixture","idempotencyKey":"task-parent"})).await
    });
    tokio::time::timeout(Duration::from_secs(2), observation.entered.notified())
        .await
        .unwrap();
    let tasks = tokio::time::timeout(
        Duration::from_secs(1),
        service.invoke("session/tasks", json!({"sessionId":id})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(tasks[0]["status"], "running");
    tokio::time::timeout(
        Duration::from_secs(1),
        service.invoke(
            "session/task/stop",
            json!({"sessionId":id,"taskId":"task-1"}),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(observation.stopped.load(Ordering::SeqCst), 1);
    assert_eq!(observation.cancelled.load(Ordering::SeqCst), 0);
    // A request acknowledgement is not a fabricated native terminal outcome.
    assert_eq!(
        service
            .invoke("session/tasks", json!({"sessionId":id}))
            .await
            .unwrap()[0]["status"],
        "running"
    );
    observation.release.notify_one();
    prompt.await.unwrap().unwrap();
    service
        .invoke("session/close", json!({"sessionId":id}))
        .await
        .unwrap();
}

#[tokio::test]
async fn unavailable_tasks_and_foreign_runtime_targets_have_no_native_effects() {
    let (service, observation, id) = fixture(false).await;
    assert!(matches!(
        service
            .invoke("session/tasks", json!({"sessionId":id}))
            .await,
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        service
            .invoke(
                "session/task/stop",
                json!({"sessionId":id,"taskId":"task-1"})
            )
            .await,
        Err(Error::Unsupported(_))
    ));
    let foreign = Runtime::default().identity().await.unwrap();
    let response = service.handle(json!({"jsonrpc":"2.0","id":1,"method":"session/task/stop","params":{"sessionId":id,"taskId":"task-1"},"expectedRuntime":foreign})).await.unwrap();
    assert_eq!(response["error"]["code"], -32005);
    assert_eq!(observation.listed.load(Ordering::SeqCst), 0);
    assert_eq!(observation.stopped.load(Ordering::SeqCst), 0);
    service
        .invoke("session/close", json!({"sessionId":id}))
        .await
        .unwrap();
}

#[tokio::test]
async fn invalid_or_unowned_task_ids_fail_and_schema_exposes_typed_results() {
    let (service, observation, id) = fixture(true).await;
    for task_id in ["", "task\n1", "another-session-task"] {
        assert!(matches!(
            service
                .invoke(
                    "session/task/stop",
                    json!({"sessionId":id,"taskId":task_id})
                )
                .await,
            Err(Error::Invalid(_))
        ));
    }
    assert_eq!(observation.stopped.load(Ordering::SeqCst), 0);
    let schema = service.invoke("runtime/schema", json!({})).await.unwrap();
    assert_eq!(
        schema["x-methods"]["session/tasks"]["result"]["items"]["$ref"],
        "#/$defs/AgentTask"
    );
    assert!(schema["$defs"]["Capabilities"]["properties"]["backgroundTasks"].is_object());
    service
        .invoke("session/close", json!({"sessionId":id}))
        .await
        .unwrap();
}
