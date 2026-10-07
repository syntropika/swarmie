use async_trait::async_trait;
use futures_util::StreamExt;
use mpai::{
    mock::{MockHarness, MockScript, MockStep},
    *,
};
use mpai_resources::ResourcesExtension;
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
async fn call(runtime: &Runtime, command: &str, args: Value) -> Value {
    runtime
        .invoke_extension("resources", command, args)
        .await
        .unwrap()
}
async fn idle(agent: &Agent) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while agent.snapshot().await.state != SessionState::Idle {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}
fn end() -> MockStep {
    MockStep::End {
        reason: "completed".into(),
    }
}

#[tokio::test]
async fn templates_resources_and_history_are_real_and_durable() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let sessions = tempfile::tempdir().unwrap();
    let skill = project.path().join(".agents/skills/check");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "# Check\nUse the test runner.").unwrap();
    let runtime = Runtime::builder()
        .harness(MockHarness::new(
            MockScript::default().expect("Inspect source", vec![MockStep::text("done"), end()]),
        ))
        .store(
            storage::FileSessionStore::new(sessions.path())
                .await
                .unwrap(),
        )
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    call(
        &runtime,
        "templates.put",
        json!({"id":"inspect","name":"Inspect","text":"Inspect {{target}}"}),
    )
    .await;
    assert!(
        runtime
            .invoke_extension(
                "resources",
                "templates.render",
                json!({"id":"inspect","variables":{}})
            )
            .await
            .is_err()
    );
    assert_eq!(
        call(
            &runtime,
            "templates.render",
            json!({"id":"inspect","variables":{"target":"{{source}}"}})
        )
        .await["text"],
        "Inspect {{source}}"
    );
    let discovered = call(&runtime, "resources.list", json!({"cwd":project.path()})).await;
    assert!(
        discovered["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["name"] == "check" && r["kind"] == "skill")
    );
    assert_eq!(
        call(
            &runtime,
            "resources.read",
            json!({"cwd":project.path(),"path":skill.join("SKILL.md")})
        )
        .await["text"],
        "# Check\nUse the test runner."
    );
    assert!(
        runtime
            .invoke_extension(
                "resources",
                "resources.read",
                json!({"cwd":project.path(),"path":"/etc/passwd"})
            )
            .await
            .is_err()
    );
    let peer = call(
        &runtime,
        "peers.spawn",
        json!({"cwd":project.path(),"harness":"mock","text":"Inspect source"}),
    )
    .await;
    let id = peer["session"]["id"].as_str().unwrap();
    idle(&runtime.agent(id).await.unwrap()).await;
    let events = call(&runtime, "peers.read", json!({"sessionId":id})).await;
    assert!(
        events["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "text.delta")
    );
    assert_eq!(
        call(
            &runtime,
            "history.list",
            json!({"cwd":project.path(),"query":"SOURCE"})
        )
        .await["items"][0]["text"],
        "Inspect source"
    );
    runtime.shutdown().await.unwrap();
    let runtime = Runtime::builder()
        .store(
            storage::FileSessionStore::new(sessions.path())
                .await
                .unwrap(),
        )
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    assert_eq!(
        call(&runtime, "history.list", json!({})).await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        call(&runtime, "templates.list", json!({})).await["templates"][0]["id"],
        "inspect"
    );
    runtime.shutdown().await.unwrap();
}
#[tokio::test]
async fn schedules_queue_busy_work_without_answering_permissions_and_stop_at_shutdown() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let request = PermissionRequest {
        id: "approval".into(),
        kind: "tool".into(),
        title: "Run tests".into(),
        description: None,
        command: None,
        paths: vec![],
        options: vec![PermissionOption {
            id: "allow".into(),
            kind: "approve_once".into(),
            label: "Allow".into(),
        }],
        questions: vec![],
        metadata: json!({}),
    };
    let mock = MockHarness::new(
        MockScript::default()
            .expect(
                "first",
                vec![
                    MockStep::Request { request },
                    MockStep::AwaitAnswer {
                        id: "approval".into(),
                        option: Some("allow".into()),
                    },
                    end(),
                ],
            )
            .expect("scheduled", vec![MockStep::text("second"), end()]),
    );
    let runtime = Runtime::builder()
        .harness(mock.clone())
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    let peer = call(
        &runtime,
        "peers.spawn",
        json!({"cwd":project.path(),"harness":"mock","text":"first"}),
    )
    .await;
    let id = peer["session"]["id"].as_str().unwrap();
    let agent = runtime.agent(id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while agent.snapshot().await.state != SessionState::NeedsInput {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    call(
        &runtime,
        "schedules.put",
        json!({"id":"daily","sessionId":id,"text":"scheduled","nextRunAtMs":0}),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while call(&runtime, "schedules.list", json!({})).await["schedules"][0]["status"]
            != "delivered"
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(mock.answers().await.is_empty());
    assert_eq!(mock.prompts().await.len(), 1);
    assert_eq!(agent.snapshot().await.queue.items.len(), 1);
    agent
        .respond_permission(PermissionResponse {
            id: "approval".into(),
            option: "allow".into(),
            answers: BTreeMap::new(),
            data: json!({}),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while mock.prompts().await.len() != 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    idle(&agent).await;
    call(
        &runtime,
        "schedules.put",
        json!({"id":"future","sessionId":id,"text":"never","nextRunAtMs":u64::MAX}),
    )
    .await;
    runtime.shutdown().await.unwrap();
    assert!(
        runtime
            .invoke_extension(
                "resources",
                "peers.spawn",
                json!({"cwd":project.path(),"harness":"mock"})
            )
            .await
            .is_err()
    );
    assert_eq!(mock.prompts().await.len(), 2);
}
#[tokio::test]
async fn fanout_is_independent_streams_actual_text_and_cancels_native_turns() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let alternate = tempfile::tempdir().unwrap();
    let fast = MockHarness::new(
        MockScript::default().expect("compare", vec![MockStep::text("answer-one"), end()]),
    )
    .named("fast");
    let slow = MockHarness::new(MockScript::default().expect(
        "compare",
        vec![
            MockStep::Sleep { milliseconds: 3000 },
            MockStep::text("late"),
            end(),
        ],
    ))
    .named("slow");
    let runtime = Runtime::builder()
        .harness(fast.clone())
        .harness(slow.clone())
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    let job=call(&runtime,"fanout.start",json!({"cwd":project.path(),"text":"compare","targets":[{"harness":"fast","model":"a"},{"harness":"slow","model":"b","cwd":alternate.path()}]})).await;
    let id = job["id"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let result = call(&runtime, "fanout.status", json!({"id":id})).await;
            if result["targets"][0]["status"] == "completed"
                && result["targets"][1]["sessionId"].is_string()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        slow.launches().await[0].cwd,
        alternate.path().canonicalize().unwrap()
    );
    let result = call(&runtime, "fanout.cancel", json!({"id":id})).await;
    assert_eq!(result["job"]["status"], "cancelled");
    assert_eq!(result["job"]["targets"][0]["text"], "answer-one");
    let slow_id = result["job"]["targets"][1]["sessionId"].as_str().unwrap();
    let slow_agent = runtime.agent(slow_id).await.unwrap();
    idle(&slow_agent).await;
    let events = slow_agent.replay(0).await.unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind,EventKind::TurnEnded{reason} if reason=="cancelled"))
    );
    runtime.shutdown().await.unwrap();
}
fn usage(total: u64, cost: Option<f64>) -> Usage {
    Usage {
        scope: "session".into(),
        input_tokens: Some(total),
        output_tokens: Some(0),
        cached_input_tokens: None,
        cache_write_tokens: None,
        total_tokens: Some(total),
        cost_usd: cost,
    }
}
#[tokio::test]
async fn historical_usage_backfills_and_counts_cumulative_increments_once() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let sessions = tempfile::tempdir().unwrap();
    let steps = vec![
        MockStep::Emit {
            kind: Box::new(EventKind::UsageUpdated(usage(10, Some(0.1)))),
            raw: None,
        },
        MockStep::Emit {
            kind: Box::new(EventKind::ModelRerouted(ModelReroute {
                from_model: "a".into(),
                to_model: "b".into(),
                reason: None,
            })),
            raw: None,
        },
        MockStep::Emit {
            kind: Box::new(EventKind::UsageUpdated(usage(25, None))),
            raw: None,
        },
        end(),
    ];
    // Historical events existed before installing the optional extension.
    let runtime = Runtime::builder()
        .harness(MockHarness::new(MockScript::default().turn(steps)))
        .store(
            storage::FileSessionStore::new(sessions.path())
                .await
                .unwrap(),
        )
        .build();
    let mut options = SpawnOptions::new("mock", project.path());
    options.model = Some("a".into());
    let agent = runtime.spawn(options).await.unwrap();
    let mut events = agent.events();
    agent.prompt("old prompt").await.unwrap();
    while let Some(event) = events.next().await {
        if matches!(event.unwrap().kind, EventKind::TurnEnded { .. }) {
            break;
        }
    }
    runtime.shutdown().await.unwrap();
    let runtime = Runtime::builder()
        .store(
            storage::FileSessionStore::new(sessions.path())
                .await
                .unwrap(),
        )
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    for _ in 0..2 {
        let report = call(&runtime, "usage.dashboard", json!({})).await;
        assert_eq!(report["totals"]["totalTokens"], 25);
        assert_eq!(report["byModel"]["a"]["totalTokens"], 10);
        assert_eq!(report["byModel"]["b"]["totalTokens"], 15);
        assert_eq!(report["unknownCosts"], 1);
        assert_eq!(report["totals"]["knownCostUsd"], 0.1);
    }
    assert_eq!(
        call(&runtime, "history.list", json!({})).await["items"][0]["text"],
        "old prompt"
    );
    runtime.shutdown().await.unwrap();
    assert_eq!(ResourcesExtension::tool_definitions().len(), 6);
    assert!(
        !ResourcesExtension::tool_definitions()
            .iter()
            .any(|t| t["command"].as_str().unwrap().starts_with("credentials"))
    );
}

#[tokio::test]
async fn recurrence_skips_missed_intervals_and_shutdown_stops_clock_delivery() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let mock = MockHarness::new(
        MockScript::default()
            .expect("repeat", vec![end()])
            .expect("repeat", vec![end()])
            .expect("repeat", vec![end()]),
    );
    let runtime = Runtime::builder()
        .harness(mock.clone())
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    let session = call(
        &runtime,
        "session.create",
        json!({"cwd":project.path(),"harness":"mock"}),
    )
    .await;
    call(&runtime,"schedules.put",json!({"id":"repeat","sessionId":session["id"],"text":"repeat","nextRunAtMs":0,"intervalSeconds":1})).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while mock.prompts().await.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let receipt = call(&runtime, "schedules.list", json!({})).await;
    assert!(
        receipt["schedules"][0]["nextRunAtMs"].as_u64().unwrap()
            > receipt["schedules"][0]["lastRunAtMs"].as_u64().unwrap()
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(mock.prompts().await.len(), 1);
    tokio::time::timeout(Duration::from_secs(2), async {
        while mock.prompts().await.len() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    runtime.shutdown().await.unwrap();
    let count = mock.prompts().await.len();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(mock.prompts().await.len(), count);
}
#[tokio::test]
async fn incomplete_comparisons_are_interrupted_durably_and_weak_runtime_does_not_leak() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let runtime = Runtime::builder()
        .harness(MockHarness::new(MockScript::default().turn(vec![
            MockStep::Sleep {
                milliseconds: 10000,
            },
            end(),
        ])))
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    let job = call(
        &runtime,
        "fanout.start",
        json!({"cwd":project.path(),"text":"wait","targets":[{"harness":"mock"}]}),
    )
    .await;
    let id = job["id"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while call(&runtime, "fanout.status", json!({"id":id})).await["targets"][0]["status"]
            != "running"
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let weak = runtime.downgrade();
    runtime.shutdown().await.unwrap();
    drop(runtime);
    assert!(weak.upgrade().is_none());
    let runtime = Runtime::builder()
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    let status = call(&runtime, "fanout.status", json!({"id":id})).await;
    assert_eq!(status["status"], "interrupted");
    assert_eq!(status["targets"][0]["status"], "interrupted");
    runtime.shutdown().await.unwrap();
}
#[tokio::test]
async fn sparse_cumulative_cost_keeps_known_baseline_and_scans_new_events_incrementally() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let script = MockScript::default()
        .turn(vec![
            MockStep::Emit {
                kind: Box::new(EventKind::UsageUpdated(usage(10, Some(0.1)))),
                raw: None,
            },
            end(),
        ])
        .turn(vec![
            MockStep::Emit {
                kind: Box::new(EventKind::UsageUpdated(usage(20, None))),
                raw: None,
            },
            MockStep::Emit {
                kind: Box::new(EventKind::UsageUpdated(usage(30, Some(0.3)))),
                raw: None,
            },
            end(),
        ]);
    let runtime = Runtime::builder()
        .harness(MockHarness::new(script))
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    let peer = call(
        &runtime,
        "peers.spawn",
        json!({"cwd":project.path(),"harness":"mock","text":"first"}),
    )
    .await;
    let agent = runtime
        .agent(peer["session"]["id"].as_str().unwrap())
        .await
        .unwrap();
    idle(&agent).await;
    assert_eq!(
        call(&runtime, "usage.dashboard", json!({})).await["totals"]["totalTokens"],
        10
    );
    agent.prompt("second").await.unwrap();
    idle(&agent).await;
    let report = call(&runtime, "usage.dashboard", json!({})).await;
    assert_eq!(report["totals"]["totalTokens"], 30);
    assert!((report["totals"]["knownCostUsd"].as_f64().unwrap() - 0.3).abs() < 1e-9);
    assert_eq!(report["unknownCosts"], 1);
    runtime.shutdown().await.unwrap();
}

#[derive(Clone)]
struct ForkMock(MockHarness);
#[async_trait]
impl Harness for ForkMock {
    fn id(&self) -> &str {
        "mock"
    }
    async fn detect(&self) -> Result<Detection> {
        let mut value = self.0.detect().await?;
        value.capabilities.fork = true;
        Ok(value)
    }
    async fn connect(
        &self,
        options: &SpawnOptions,
        _native: Option<&NativeSessionRef>,
        _fork: bool,
    ) -> Result<BackendConnection> {
        let mut connection = self.0.connect(options, None, false).await?;
        connection.capabilities.fork = true;
        Ok(connection)
    }
}
#[tokio::test]
async fn inherited_fork_usage_is_not_charged_twice_in_live_or_backfilled_dashboard() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let sessions = tempfile::tempdir().unwrap();
    let harness = ForkMock(MockHarness::new(MockScript::default().turn(vec![
        MockStep::Emit {
            kind: Box::new(EventKind::UsageUpdated(usage(10, Some(0.1)))),
            raw: None,
        },
        end(),
    ])));
    let runtime = Runtime::builder()
        .harness(harness)
        .store(
            storage::FileSessionStore::new(sessions.path())
                .await
                .unwrap(),
        )
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    let agent = runtime
        .spawn(SpawnOptions::new("mock", project.path()))
        .await
        .unwrap();
    agent.prompt("parent").await.unwrap();
    idle(&agent).await;
    let child = runtime.fork(&agent.snapshot().await.id).await.unwrap();
    child.prompt("child").await.unwrap();
    idle(&child).await;
    let report = call(&runtime, "usage.dashboard", json!({})).await;
    assert_eq!(
        call(&runtime, "history.list", json!({})).await["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // Inherited journal events are excluded; each native session's actual new counter is counted.
    assert_eq!(report["totals"]["totalTokens"], 20);
    assert_eq!(report["totals"]["knownCostUsd"], 0.2);
    runtime.shutdown().await.unwrap();
    let fresh = tempfile::tempdir().unwrap();
    let runtime = Runtime::builder()
        .store(
            storage::FileSessionStore::new(sessions.path())
                .await
                .unwrap(),
        )
        .extension(ResourcesExtension::open(fresh.path()).unwrap())
        .build();
    let report = call(&runtime, "usage.dashboard", json!({})).await;
    assert_eq!(report["totals"]["totalTokens"], 20);
    runtime.shutdown().await.unwrap();
}

#[derive(Clone)]
struct ResumeMock(MockHarness);
#[async_trait]
impl Harness for ResumeMock {
    fn id(&self) -> &str {
        "mock"
    }
    async fn detect(&self) -> Result<Detection> {
        let mut value = self.0.detect().await?;
        value.capabilities.resume = true;
        Ok(value)
    }
    async fn connect(
        &self,
        options: &SpawnOptions,
        native: Option<&NativeSessionRef>,
        _fork: bool,
    ) -> Result<BackendConnection> {
        let mut connection = self.0.connect(options, None, false).await?;
        connection.capabilities.resume = true;
        if let Some(native) = native {
            connection.native = native.clone();
        }
        Ok(connection)
    }
}

#[derive(Clone, Default)]
struct AccountMock(std::sync::Arc<tokio::sync::Mutex<Vec<AccountQuery>>>);
#[async_trait]
impl Harness for AccountMock {
    fn id(&self) -> &str {
        "account"
    }
    async fn detect(&self) -> Result<Detection> {
        Ok(Detection {
            harness: "account".into(),
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
        panic!("Account query must not open a conversation")
    }
    async fn account_quotas(&self, query: &AccountQuery) -> Result<Value> {
        self.0.lock().await.push(query.clone());
        Ok(json!({"windows":[{"usedPercent":23}]}))
    }
}

#[tokio::test]
async fn aliases_and_project_native_defaults_are_durable_and_have_explicit_precedence() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let mock = MockHarness::new(MockScript::default());
    let runtime = Runtime::builder()
        .harness(mock.clone())
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    call(&runtime,"templates.put",json!({"id":"review","name":"Review","text":"Review {{file}}","aliases":["inspect","check"]})).await;
    for (id, aliases) in [
        ("other", json!(["inspect"])),
        ("inspect", json!([])),
        ("other", json!(["review"])),
        ("other", json!(["duplicate", "duplicate"])),
    ] {
        assert!(
            runtime
                .invoke_extension(
                    "resources",
                    "templates.put",
                    json!({"id":id,"name":"Other","text":"Other","aliases":aliases})
                )
                .await
                .is_err()
        );
    }
    let rendered = call(
        &runtime,
        "templates.render",
        json!({"id":"inspect","variables":{"file":"main.rs"}}),
    )
    .await;
    assert_eq!(rendered, json!({"id":"review","text":"Review main.rs"}));
    let defaults=call(&runtime,"project.set",json!({"cwd":project.path(),"harness":"mock","workspace":workspace.path(),"config":{"permissionMode":"plan","nested":{"keep":true,"mode":"project"}}})).await;
    assert_eq!(
        defaults["workspace"],
        json!(std::fs::canonicalize(workspace.path()).unwrap())
    );
    call(
        &runtime,
        "project.set",
        json!({"cwd":project.path(),"config":{"nested":{"added":42}}}),
    )
    .await;
    assert_eq!(
        call(&runtime, "project.get", json!({"cwd":project.path()})).await["workspace"],
        defaults["workspace"]
    );
    call(
        &runtime,
        "session.create",
        json!({"cwd":project.path(),"config":{"nested":{"mode":"explicit"}}}),
    )
    .await;
    let launch = &mock.launches().await[0];
    assert_eq!(launch.cwd, project.path());
    assert_eq!(
        launch.config,
        json!({"permissionMode":"plan","nested":{"keep":true,"mode":"explicit","added":42}})
    );
    assert!(
        runtime
            .invoke_extension(
                "resources",
                "project.set",
                json!({"cwd":project.path(),"config":{"apiToken":"secret"}})
            )
            .await
            .is_err()
    );
    assert!(
        runtime
            .invoke_extension(
                "resources",
                "project.set",
                json!({"cwd":project.path(),"workspace":workspace.path().join("missing")})
            )
            .await
            .is_err()
    );
    runtime.shutdown().await.unwrap();
    let extension = ResourcesExtension::open(root.path()).unwrap();
    assert_eq!(
        extension
            .invoke(
                "templates.render",
                json!({"id":"check","variables":{"file":"lib.rs"}})
            )
            .await
            .unwrap()["text"],
        "Review lib.rs"
    );
    assert_eq!(
        extension
            .invoke("project.get", json!({"cwd":project.path()}))
            .await
            .unwrap()["config"]["permissionMode"],
        "plan"
    );
}

#[tokio::test]
async fn comparison_text_is_coalesced_but_final_status_and_shutdown_are_durable() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let mock = MockHarness::new(MockScript::default().expect(
        "finish",
        vec![
            MockStep::text("A"),
            MockStep::text("B"),
            MockStep::text("C"),
            MockStep::Sleep { milliseconds: 500 },
            end(),
        ],
    ));
    let pending_mock = MockHarness::new(MockScript::default().expect(
        "shutdown",
        vec![
            MockStep::text("retained"),
            MockStep::Sleep { milliseconds: 5000 },
            end(),
        ],
    ))
    .named("pending");
    let runtime = Runtime::builder()
        .harness(mock)
        .harness(pending_mock)
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    let job = call(
        &runtime,
        "fanout.start",
        json!({"cwd":project.path(),"text":"finish","targets":[{"harness":"mock"}]}),
    )
    .await;
    let id = job["id"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while call(&runtime, "fanout.status", json!({"id":id})).await["targets"][0]["text"] != "ABC"
        {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        runtime
            .invoke_extension("resources", "fanout.remove", json!({"id":id}))
            .await,
        Err(Error::Busy)
    ));
    let state: Value =
        serde_json::from_slice(&std::fs::read(root.path().join("resources.json")).unwrap())
            .unwrap();
    assert_ne!(state["jobs"][id]["targets"][0]["text"], "ABC");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let state: Value =
                serde_json::from_slice(&std::fs::read(root.path().join("resources.json")).unwrap())
                    .unwrap();
            if state["jobs"][id]["targets"][0]["text"] == "ABC" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while call(&runtime, "fanout.status", json!({"id":id})).await["status"] != "completed" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let saved: Value =
        serde_json::from_slice(&std::fs::read(root.path().join("resources.json")).unwrap())
            .unwrap();
    assert_eq!(saved["jobs"][id]["status"], "completed");
    assert_eq!(saved["jobs"][id]["targets"][0]["text"], "ABC");
    let list = call(&runtime, "fanout.list", json!({"status":"completed"})).await;
    assert_eq!(list["jobs"][0]["id"], id);
    assert!(list["jobs"][0]["targets"][0].get("text").is_none());
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match runtime
                .invoke_extension("resources", "fanout.remove", json!({"id":id}))
                .await
            {
                Ok(_) => break,
                Err(Error::Busy) => tokio::time::sleep(Duration::from_millis(5)).await,
                Err(error) => panic!("{error}"),
            }
        }
    })
    .await
    .unwrap();
    assert!(
        call(&runtime, "fanout.list", json!({})).await["jobs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        runtime
            .invoke_extension("resources", "fanout.status", json!({"id":id}))
            .await
            .is_err()
    );
    let pending = call(
        &runtime,
        "fanout.start",
        json!({"cwd":project.path(),"text":"shutdown","targets":[{"harness":"pending"}]}),
    )
    .await;
    let pending_id = pending["id"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while call(&runtime, "fanout.status", json!({"id":pending_id})).await["targets"][0]["text"]
            != "retained"
        {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    runtime.shutdown().await.unwrap();
    let extension = ResourcesExtension::open(root.path()).unwrap();
    let saved = extension
        .invoke("fanout.status", json!({"id":pending_id}))
        .await
        .unwrap();
    assert_eq!(saved["status"], "interrupted");
    assert_eq!(saved["targets"][0]["text"], "retained");
}

#[tokio::test]
async fn private_credentials_and_project_defaults_remain_durable() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let mock = MockHarness::new(MockScript::default());
    let runtime = Runtime::builder()
        .harness(mock.clone())
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    call(
        &runtime,
        "credentials.set",
        json!({"id":"key","value":"private-value"}),
    )
    .await;
    assert!(
        !call(&runtime, "credentials.status", json!({}))
            .await
            .to_string()
            .contains("private-value")
    );
    call(&runtime, "project.set", json!({"cwd":project.path(),"harness":"mock","model":"saved-model","config":{"nested":{"keep":true,"mode":"saved"}}})).await;
    runtime.shutdown().await.unwrap();
    let runtime = Runtime::builder()
        .harness(mock.clone())
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    let snapshot = call(&runtime, "session.create", json!({"cwd":project.path(),"model":"explicit-model","config":{"nested":{"mode":"explicit"}}})).await;
    assert_eq!(snapshot["options"]["model"], "explicit-model");
    let launch = &mock.launches().await[0];
    assert_eq!(
        launch.config,
        json!({"nested":{"keep":true,"mode":"explicit"}})
    );
    assert!(launch.execution.env.is_empty());
    assert_eq!(
        call(&runtime, "credentials.status", json!({})).await["credentials"][0]["id"],
        "key"
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn project_and_explicit_native_controls_reject_sensitive_fields() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let runtime = Runtime::builder()
        .harness(MockHarness::new(MockScript::default()))
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    for config in [
        json!({"apiToken":"private"}),
        json!({"nested":{"headers":{"Authorization":"private"}}}),
        json!({"env":{"API_KEY":"private"}}),
    ] {
        for command in [
            "project.set",
            "session.create",
            "text.generate",
            "account.quotas",
        ] {
            assert!(runtime.invoke_extension("resources",command,json!({"cwd":project.path(),"harness":"mock","config":config,"text":"unused"})).await.is_err());
        }
    }
    call(&runtime,"project.set",json!({"cwd":project.path(),"harness":"mock","config":{"maxTokens":512,"tokenBudget":true}})).await;
    call(
        &runtime,
        "project.set",
        json!({"cwd":project.path(),"config":null}),
    )
    .await;
    assert!(call(&runtime, "project.get", json!({"cwd":project.path()})).await["config"].is_null());
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn direct_generation_uses_native_controls_without_retaining_a_session() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let mock = MockHarness::new(MockScript::default().expect(
        "commit description",
        vec![MockStep::text("Improve queue delivery"), end()],
    ));
    let runtime = Runtime::builder()
        .harness(mock.clone())
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    let result=call(&runtime,"text.generate",json!({"cwd":project.path(),"harness":"mock","model":"small","config":{"mode":"readonly"},"text":"commit description"})).await;
    assert_eq!(result["text"], "Improve queue delivery");
    assert_eq!(mock.launches().await[0].model.as_deref(), Some("small"));
    assert!(runtime.sessions().await.unwrap().is_empty());
    assert!(
        call(&runtime, "history.list", json!({})).await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn managed_peer_resume_retains_saved_native_model_and_controls() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let sessions = tempfile::tempdir().unwrap();
    let mock = MockHarness::new(MockScript::default().turn(vec![end()]));
    let runtime = Runtime::builder()
        .harness(ResumeMock(mock.clone()))
        .store(
            storage::FileSessionStore::new(sessions.path())
                .await
                .unwrap(),
        )
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    let snapshot=call(&runtime,"session.create",json!({"cwd":project.path(),"harness":"mock","model":"chosen","config":{"mode":"readonly"}})).await;
    let id = snapshot["id"].as_str().unwrap();
    runtime.agent(id).await.unwrap().close().await.unwrap();
    runtime.shutdown().await.unwrap();
    let runtime = Runtime::builder()
        .harness(ResumeMock(mock.clone()))
        .store(
            storage::FileSessionStore::new(sessions.path())
                .await
                .unwrap(),
        )
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    let resumed = call(&runtime, "session.resume", json!({"sessionId":id})).await;
    assert_eq!(resumed["native"]["id"], snapshot["native"]["id"]);
    call(
        &runtime,
        "peers.send",
        json!({"sessionId":id,"text":"resume work"}),
    )
    .await;
    idle(&runtime.agent(id).await.unwrap()).await;
    let launch = &mock.launches().await[1];
    assert_eq!(launch.model.as_deref(), Some("chosen"));
    assert_eq!(launch.config, json!({"mode":"readonly"}));
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_account_query_uses_project_defaults_without_a_conversation() {
    let root = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let harness = AccountMock::default();
    let runtime = Runtime::builder()
        .harness(harness.clone())
        .extension(ResourcesExtension::open(root.path()).unwrap())
        .build();
    call(
        &runtime,
        "project.set",
        json!({"cwd":project.path(),"harness":"account","config":{"endpoint":"http://localhost"}}),
    )
    .await;
    let quotas = call(
        &runtime,
        "account.quotas",
        json!({"cwd":project.path(),"config":{"mode":"readonly"}}),
    )
    .await;
    assert_eq!(quotas["windows"][0]["usedPercent"], 23);
    let captured = harness.0.lock().await;
    assert_eq!(
        captured[0].config,
        json!({"endpoint":"http://localhost","mode":"readonly"})
    );
    assert!(captured[0].execution.env.is_empty());
    drop(captured);
    assert!(runtime.sessions().await.unwrap().is_empty());
    runtime.shutdown().await.unwrap();
}
