use serde_json::{Value, json};
use swarmie::{
    mock::{MockHarness, MockScript, MockStep},
    *,
};
use swarmie_resources::ResourcesExtension;

async fn call(runtime: &Runtime, command: &str, args: Value) -> Value {
    runtime
        .invoke_extension("resources", command, args)
        .await
        .unwrap()
}

#[tokio::test]
async fn schedule_pause_resume_and_dispatch_history_are_durable() {
    let root = tempfile::tempdir().unwrap();
    let resources = tempfile::tempdir().unwrap();
    let runtime = Runtime::builder()
        .store(storage::SqliteSessionStore::new(root.path()).await.unwrap())
        .harness(MockHarness::new(MockScript::default().expect(
            "Check status",
            vec![
                MockStep::text("Checked"),
                MockStep::End {
                    outcome: TurnOutcome::Completed,
                },
            ],
        )))
        .extension(ResourcesExtension::open(resources.path()).unwrap())
        .build();
    let agent = runtime.spawn(SpawnOptions::new("mock", ".")).await.unwrap();
    let session = agent.snapshot().await.id;
    call(&runtime, "schedules.put", json!({"id":"routine-a", "sessionId":session, "text":"Check status", "nextRunAtMs":4_102_444_800_000_u64})).await;
    call(
        &runtime,
        "schedules.enabled",
        json!({"id":"routine-a", "enabled":false}),
    )
    .await;
    assert_eq!(
        call(&runtime, "schedules.list", json!({})).await["schedules"][0]["enabled"],
        false
    );
    call(
        &runtime,
        "schedules.enabled",
        json!({"id":"routine-a", "enabled":true}),
    )
    .await;
    call(&runtime, "schedules.run", json!({"id":"routine-a"})).await;
    let history = call(&runtime, "schedules.list", json!({})).await;
    let attempts = &history["schedules"][0]["attempts"];
    assert_eq!(attempts[0]["status"], "delivered");
    assert_eq!(attempts[0]["result"]["delivery"], "started");
    assert!(attempts[0]["finishedAtMs"].is_u64());
    assert_eq!(history["schedules"][0]["enabled"], false);
    call(&runtime, "schedules.cancel", json!({"id":"routine-a"})).await;
    assert!(
        runtime
            .invoke_extension(
                "resources",
                "schedules.enabled",
                json!({"id":"routine-a", "enabled":true})
            )
            .await
            .is_err()
    );
    runtime.shutdown().await.unwrap();
    drop(agent);
    drop(runtime);
    let restored = Runtime::builder()
        .store(storage::SqliteSessionStore::new(root.path()).await.unwrap())
        .extension(ResourcesExtension::open(resources.path()).unwrap())
        .build();
    assert_eq!(
        call(&restored, "schedules.list", json!({})).await["schedules"][0]["attempts"],
        *attempts
    );
    restored.shutdown().await.unwrap();
}

#[tokio::test]
async fn interrupted_schedule_dispatch_is_retained_and_never_replayed_on_restart() {
    let root = tempfile::tempdir().unwrap();
    let resources = tempfile::tempdir().unwrap();
    let store = storage::SqliteSessionStore::new(root.path()).await.unwrap();
    store.save_extension_state("resources", json!({
        "schedules":{"routine-a":{"id":"routine-a", "sessionId":"session-a", "text":"Inspect", "nextRunAtMs":0,
            "intervalSeconds":null, "enabled":true, "status":"running", "lastRunAtMs":null, "lastResult":null, "error":null,
            "attempts":[{"id":"attempt-a", "startedAtMs":1, "finishedAtMs":null, "status":"dispatching", "result":null}]
        }}
    })).await.unwrap();
    let runtime = Runtime::builder()
        .store(store)
        .extension(ResourcesExtension::open(resources.path()).unwrap())
        .build();
    let routine = call(&runtime, "schedules.list", json!({})).await;
    assert_eq!(routine["schedules"][0]["status"], "interrupted");
    assert_eq!(routine["schedules"][0]["enabled"], false);
    assert_eq!(
        routine["schedules"][0]["attempts"][0]["status"],
        "interrupted"
    );
    assert!(
        runtime
            .invoke_extension(
                "resources",
                "schedules.enabled",
                json!({"id":"routine-a", "enabled":true})
            )
            .await
            .is_err()
    );
    runtime.shutdown().await.unwrap();
}
